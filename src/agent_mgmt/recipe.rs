use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    pub source: RecipeSource,
    #[serde(default)]
    pub entry_points: Vec<EntryPoint>,
    #[serde(default)]
    pub version_check: Option<VersionCheck>,
    #[serde(default)]
    pub post_install: Vec<RecipeCommand>,
    #[serde(default)]
    pub pre_remove: Vec<RecipeCommand>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPoint {
    pub relative_path: String,
    pub symlink_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum RecipeSource {
    Url {
        template: String,
        #[serde(default)]
        archive: ArchiveKind,
        #[serde(default)]
        checksum: Option<String>,
    },
    Script {
        install: ShellCommand,
        #[serde(default)]
        update: Option<ShellCommand>,
        #[serde(default)]
        remove: Option<ShellCommand>,
        #[serde(default)]
        latest_version: Option<ShellCommand>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum ArchiveKind {
    #[default]
    TarGz,
    Zip,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommand {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionCheck {
    #[serde(default = "default_version_args")]
    pub args: Vec<String>,
    #[serde(default)]
    pub regex: Option<String>,
}

fn default_version_args() -> Vec<String> {
    vec!["--version".to_string()]
}

/// Detect the host OS in install.sh's vocabulary.
pub fn host_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

/// Detect the host architecture in install.sh's vocabulary.
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    }
}

/// Substitute `{version}`, `{os}`, `{arch}`, `{payload_dir}`, `{bin_dir}` into a string.
pub fn substitute(
    template: &str,
    version: &str,
    payload_dir: &Path,
    bin_dir: &Path,
) -> String {
    template
        .replace("{version}", version)
        .replace("{os}", host_os())
        .replace("{arch}", host_arch())
        .replace("{payload_dir}", &payload_dir.display().to_string())
        .replace("{bin_dir}", &bin_dir.display().to_string())
}

impl Recipe {
    /// Returns true when the source is a placeholder stub that the operator
    /// hasn't filled in yet — used by built-in recipes that ship without a
    /// concrete URL or install script.
    pub fn is_stub(&self) -> bool {
        match &self.source {
            RecipeSource::Url { template, .. } => template.trim().is_empty(),
            RecipeSource::Script { install, .. } => install.program.trim().is_empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn substitute_replaces_all_variables() {
        let payload = PathBuf::from("/data/agents/hermes-agent");
        let bin = PathBuf::from("/home/me/.local/bin");
        let s = substitute(
            "https://example.com/hermes-{version}-{os}-{arch}.tar.gz#payload={payload_dir}&bin={bin_dir}",
            "v1.2.3",
            &payload,
            &bin,
        );
        assert!(s.contains("hermes-v1.2.3-"));
        assert!(s.contains("/data/agents/hermes-agent"));
        assert!(s.contains("/home/me/.local/bin"));
    }

    #[test]
    fn stub_recipe_detected() {
        let stub = Recipe {
            name: "x".to_string(),
            display_name: "x".to_string(),
            source: RecipeSource::Url {
                template: String::new(),
                archive: ArchiveKind::TarGz,
                checksum: None,
            },
            entry_points: vec![],
            version_check: None,
            post_install: vec![],
            pre_remove: vec![],
        };
        assert!(stub.is_stub());
    }
}
