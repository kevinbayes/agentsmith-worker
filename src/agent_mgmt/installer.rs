use std::collections::HashSet;
use std::fs::File;
use std::io::Write as _;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::agent_mgmt::recipe::{
    substitute, ArchiveKind, Recipe, RecipeCommand, RecipeSource, ShellCommand, VersionCheck,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstallOutcome {
    pub version: String,
    pub payload_path: PathBuf,
    pub symlinks: Vec<PathBuf>,
    pub detected_version: Option<String>,
    pub post_install_output: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RemoveOutcome {
    pub removed_payload: bool,
    pub removed_symlinks: Vec<PathBuf>,
}

/// Install (or reinstall) an agent. Stages into a sibling of `payload_dir`,
/// swaps atomically, runs post_install, and symlinks entry points.
pub async fn install(
    recipe: &Recipe,
    version: &str,
    payload_dir: &Path,
    bin_dir: &Path,
) -> anyhow::Result<InstallOutcome> {
    if recipe.is_stub() {
        bail!(
            "recipe '{}' is not yet configured — fill in the URL template or install script before installing",
            recipe.name
        );
    }
    if recipe.entry_points.is_empty() {
        bail!(
            "recipe '{}' has no entry_points — nothing to symlink into bin_dir",
            recipe.name
        );
    }

    let payload_parent = payload_dir
        .parent()
        .ok_or_else(|| anyhow!("payload_dir has no parent: {}", payload_dir.display()))?;
    std::fs::create_dir_all(payload_parent)
        .with_context(|| format!("creating payload parent {}", payload_parent.display()))?;
    std::fs::create_dir_all(bin_dir)
        .with_context(|| format!("creating bin_dir {}", bin_dir.display()))?;

    let staging = payload_parent.join(format!(
        ".staging-{}-{}",
        recipe.name,
        Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("creating staging dir {}", staging.display()))?;

    // Stage the new payload.
    let staging_result =
        stage_into(recipe, version, payload_dir, bin_dir, &staging).await;
    if let Err(e) = staging_result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Atomic swap.
    let backup = if payload_dir.exists() {
        let b = payload_parent.join(format!(
            ".old-{}-{}",
            recipe.name,
            Uuid::new_v4().simple()
        ));
        std::fs::rename(payload_dir, &b)
            .with_context(|| format!("moving old payload {} aside", payload_dir.display()))?;
        Some(b)
    } else {
        None
    };

    if let Err(e) = std::fs::rename(&staging, payload_dir) {
        if let Some(b) = &backup {
            let _ = std::fs::rename(b, payload_dir);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e).with_context(|| format!("promoting staging to {}", payload_dir.display()));
    }

    // Post-install. On failure, roll back to backup so we don't leave a
    // half-configured payload exposed via symlinks.
    let mut post_output = Vec::new();
    let post_result: anyhow::Result<()> = (|| {
        for cmd in &recipe.post_install {
            post_output.push(run_recipe_command(cmd, payload_dir)?);
        }
        Ok(())
    })();

    if let Err(e) = post_result {
        let _ = std::fs::remove_dir_all(payload_dir);
        if let Some(b) = &backup {
            let _ = std::fs::rename(b, payload_dir);
        }
        return Err(e.context("post_install failed; payload rolled back"));
    }

    // Symlinks.
    let mut symlinks = Vec::new();
    for ep in &recipe.entry_points {
        let target = payload_dir.join(&ep.relative_path);
        ensure_executable(&target)?;
        let link = bin_dir.join(&ep.symlink_name);
        place_symlink(&link, &target).with_context(|| {
            format!("linking {} -> {}", link.display(), target.display())
        })?;
        symlinks.push(link);
    }

    let detected_version = recipe
        .version_check
        .as_ref()
        .and_then(|vc| run_version_check(&symlinks[0], vc));

    if let Some(b) = backup {
        let _ = std::fs::remove_dir_all(&b);
    }

    Ok(InstallOutcome {
        version: version.to_string(),
        payload_path: payload_dir.to_path_buf(),
        symlinks,
        detected_version,
        post_install_output: post_output,
    })
}

async fn stage_into(
    recipe: &Recipe,
    version: &str,
    payload_dir: &Path,
    bin_dir: &Path,
    staging: &Path,
) -> anyhow::Result<()> {
    match &recipe.source {
        RecipeSource::Url {
            template,
            archive,
            checksum,
        } => {
            let url = substitute(template, version, payload_dir, bin_dir);
            tracing::info!(recipe = %recipe.name, version, url = %url, "Downloading agent payload");
            let bytes = download(&url).await?;
            if let Some(expected) = checksum {
                verify_sha256(&bytes, expected)?;
            }
            extract(&bytes, *archive, staging, &recipe.name)?;
        }
        RecipeSource::Script { install, .. } => {
            run_shell(install, version, payload_dir, bin_dir, staging)?;
        }
    }
    Ok(())
}

pub fn remove(
    recipe: &Recipe,
    payload_dir: &Path,
    bin_dir: &Path,
) -> anyhow::Result<RemoveOutcome> {
    let mut removed_symlinks = Vec::new();

    if payload_dir.exists() {
        for cmd in &recipe.pre_remove {
            if let Err(e) = run_recipe_command(cmd, payload_dir) {
                tracing::warn!(recipe = %recipe.name, error = ?e, "pre_remove command failed");
            }
        }
    }

    for ep in &recipe.entry_points {
        let link = bin_dir.join(&ep.symlink_name);
        match std::fs::symlink_metadata(&link) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if let Ok(t) = std::fs::read_link(&link) {
                    if t.starts_with(payload_dir) {
                        let _ = std::fs::remove_file(&link);
                        removed_symlinks.push(link);
                    } else {
                        tracing::warn!(
                            link = %link.display(),
                            target = %t.display(),
                            "symlink no longer points into payload — leaving alone"
                        );
                    }
                }
            }
            _ => {}
        }
    }

    let removed_payload = if payload_dir.exists() {
        std::fs::remove_dir_all(payload_dir)
            .with_context(|| format!("removing payload {}", payload_dir.display()))?;
        true
    } else {
        false
    };

    Ok(RemoveOutcome {
        removed_payload,
        removed_symlinks,
    })
}

pub fn check_bin_dir_on_path(bin_dir: &Path) {
    let canon = std::fs::canonicalize(bin_dir).unwrap_or_else(|_| bin_dir.to_path_buf());
    let path_env = std::env::var("PATH").unwrap_or_default();
    let entries: HashSet<PathBuf> = path_env
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .map(|p| std::fs::canonicalize(&p).unwrap_or(p))
        .collect();
    if !entries.contains(&canon) {
        tracing::warn!(
            bin_dir = %bin_dir.display(),
            "agent_management.bin_dir is not in $PATH — installed agents won't be callable from new shells. PATH={}",
            path_env
        );
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

async fn download(url: &str) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if !status.is_success() {
        bail!("HTTP {} downloading {}", status, url);
    }
    Ok(resp.bytes().await?.to_vec())
}

fn verify_sha256(bytes: &[u8], expected: &str) -> anyhow::Result<()> {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = hex::encode(hasher.finalize());
    let expected = expected.trim().to_lowercase();
    if actual != expected {
        bail!("checksum mismatch: expected {}, got {}", expected, actual);
    }
    Ok(())
}

fn extract(bytes: &[u8], kind: ArchiveKind, dest: &Path, recipe_name: &str) -> anyhow::Result<()> {
    match kind {
        ArchiveKind::TarGz => {
            let gz = flate2::read::GzDecoder::new(bytes);
            let mut tar = tar::Archive::new(gz);
            tar.unpack(dest)
                .with_context(|| format!("extracting tar.gz into {}", dest.display()))?;
        }
        ArchiveKind::Zip => {
            let reader = std::io::Cursor::new(bytes);
            let mut zip = zip::ZipArchive::new(reader).context("opening zip archive")?;
            zip.extract(dest)
                .with_context(|| format!("extracting zip into {}", dest.display()))?;
        }
        ArchiveKind::Raw => {
            let file_path = dest.join(recipe_name);
            let mut f = File::create(&file_path)
                .with_context(|| format!("creating {}", file_path.display()))?;
            f.write_all(bytes)?;
            let mut perms = std::fs::metadata(&file_path)?.permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&file_path, perms)?;
        }
    }
    Ok(())
}

fn run_shell(
    cmd: &ShellCommand,
    version: &str,
    payload_dir: &Path,
    bin_dir: &Path,
    cwd: &Path,
) -> anyhow::Result<()> {
    let program = substitute(&cmd.program, version, payload_dir, bin_dir);
    let args: Vec<String> = cmd
        .args
        .iter()
        .map(|a| substitute(a, version, payload_dir, bin_dir))
        .collect();

    let mut command = Command::new(&program);
    command.args(&args).current_dir(cwd);
    for (k, v) in &cmd.env {
        let v = substitute(v, version, payload_dir, bin_dir);
        command.env(k, v);
    }
    command
        .env("AGENTSMITH_VERSION", version)
        .env("AGENTSMITH_PAYLOAD_DIR", payload_dir)
        .env("AGENTSMITH_BIN_DIR", bin_dir);

    let output = command
        .output()
        .with_context(|| format!("spawning shell command {}", program))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "install script '{}' failed with status {}: {}",
            program,
            output.status,
            stderr.trim()
        );
    }
    Ok(())
}

fn run_recipe_command(cmd: &RecipeCommand, payload_dir: &Path) -> anyhow::Result<String> {
    let mut path_env = std::env::var("PATH").unwrap_or_default();
    path_env = format!(
        "{}:{}:{}",
        payload_dir.display(),
        payload_dir.join("bin").display(),
        path_env
    );

    let output = Command::new(&cmd.program)
        .args(&cmd.args)
        .current_dir(payload_dir)
        .env("PATH", &path_env)
        .output()
        .with_context(|| format!("spawning post_install command {}", cmd.program))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "post_install '{}' failed with status {}: {}",
            cmd.program,
            output.status,
            stderr.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn ensure_executable(path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        bail!(
            "entry_point target {} does not exist after install",
            path.display()
        );
    }
    let mut perms = std::fs::metadata(path)?.permissions();
    if perms.mode() & 0o111 == 0 {
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn place_symlink(link: &Path, target: &Path) -> anyhow::Result<()> {
    if let Ok(meta) = std::fs::symlink_metadata(link) {
        if !meta.file_type().is_symlink() {
            bail!(
                "{} already exists and is not a symlink — refusing to overwrite",
                link.display()
            );
        }
        std::fs::remove_file(link)
            .with_context(|| format!("removing old symlink {}", link.display()))?;
    }
    std::os::unix::fs::symlink(target, link)
        .with_context(|| format!("creating symlink {} -> {}", link.display(), target.display()))?;
    Ok(())
}

fn run_version_check(link: &Path, vc: &VersionCheck) -> Option<String> {
    let output = Command::new(link).args(&vc.args).output().ok()?;
    let text = if !output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };
    text.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .map(|l| l.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_tarball(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_data = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_data);
            for (name, data) in files {
                let mut hdr = tar::Header::new_gnu();
                hdr.set_size(data.len() as u64);
                hdr.set_mode(0o755);
                hdr.set_cksum();
                builder
                    .append_data(&mut hdr, name, *data)
                    .expect("tar append");
            }
            builder.finish().expect("tar finish");
        }
        let mut gz_out = Vec::new();
        {
            let mut enc =
                flate2::write::GzEncoder::new(&mut gz_out, flate2::Compression::default());
            enc.write_all(&tar_data).expect("gz write");
            enc.finish().expect("gz finish");
        }
        gz_out
    }

    #[test]
    fn extract_tar_gz_unpacks_files() {
        let bytes = make_tarball(&[("hermes-agent", b"#!/bin/sh\necho hi\n" as &[u8])]);
        let dir = TempDir::new().unwrap();
        extract(&bytes, ArchiveKind::TarGz, dir.path(), "hermes-agent").unwrap();
        let f = dir.path().join("hermes-agent");
        assert!(f.exists());
    }

    #[test]
    fn place_symlink_refuses_to_clobber_regular_file() {
        let dir = TempDir::new().unwrap();
        let link = dir.path().join("hermes-agent");
        std::fs::write(&link, b"manual install").unwrap();
        let target = dir.path().join("payload-target");
        std::fs::write(&target, b"#!/bin/sh").unwrap();
        let res = place_symlink(&link, &target);
        assert!(res.is_err());
        assert_eq!(std::fs::read(&link).unwrap(), b"manual install");
    }

    #[test]
    fn place_symlink_replaces_existing_symlink() {
        let dir = TempDir::new().unwrap();
        let target1 = dir.path().join("t1");
        let target2 = dir.path().join("t2");
        std::fs::write(&target1, b"a").unwrap();
        std::fs::write(&target2, b"b").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target1, &link).unwrap();
        place_symlink(&link, &target2).unwrap();
        let resolved = std::fs::read_link(&link).unwrap();
        assert_eq!(resolved, target2);
    }
}
