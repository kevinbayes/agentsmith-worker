//! Runtime introspection of an installed Hermes binary.
//!
//! Right now this is just `list_profiles`, which shells out to
//! `hermes profile list` and parses the unicode-decorated table. Hermes
//! doesn't expose a JSON output flag (as of the version verified locally),
//! so we parse the rendered text. The parser uses the `─` separator row to
//! derive column boundaries, which is more robust than splitting on
//! whitespace runs.
//!
//! Results are cached in-process for 60 seconds so dashboard polling
//! doesn't fork hermes every refresh.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::Serialize;
use tokio::process::Command;

const HERMES_FALLBACK_PATHS: &[&str] =
    &["~/.hermes/hermes-agent/hermes", "~/.hermes/bin/hermes"];

const PROFILE_CACHE_TTL: Duration = Duration::from_secs(60);

/// A single row from `hermes profile list`.
#[derive(Debug, Clone, Serialize)]
pub struct HermesProfile {
    pub name: String,
    #[serde(default)]
    pub is_default: bool,
    pub model: Option<String>,
    pub gateway: Option<String>,
    pub alias: Option<String>,
    pub distribution: Option<String>,
}

/// Result of locating the hermes binary on the host. Shared with
/// `session::hermes_prompt`.
pub fn resolve_binary(configured: &str) -> Option<PathBuf> {
    let direct = Path::new(configured);
    if direct.is_absolute() && direct.exists() {
        return Some(direct.to_path_buf());
    }
    if let Ok(p) = which::which(configured) {
        return Some(p);
    }
    for hint in HERMES_FALLBACK_PATHS {
        let expanded = expand_home(hint);
        if expanded.exists() {
            return Some(expanded);
        }
    }
    None
}

fn expand_home(s: &str) -> PathBuf {
    if let Some(stripped) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(s)
}

/// Process-wide cache so dashboard polling doesn't fork hermes for every
/// refresh. Keyed by (binary path, ttl).
#[derive(Default)]
struct ProfileCache {
    entries: Option<(Instant, Vec<HermesProfile>)>,
    key: Option<PathBuf>,
}

fn cache() -> &'static Mutex<ProfileCache> {
    static CACHE: std::sync::OnceLock<Mutex<ProfileCache>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ProfileCache::default()))
}

/// Return the cached profiles if fresh AND keyed against the same binary
/// path; otherwise None.
fn cached_for(binary: &Path) -> Option<Vec<HermesProfile>> {
    let guard = cache().lock().ok()?;
    let (when, profiles) = guard.entries.as_ref()?;
    if guard.key.as_deref() != Some(binary) {
        return None;
    }
    if when.elapsed() >= PROFILE_CACHE_TTL {
        return None;
    }
    Some(profiles.clone())
}

fn store(binary: &Path, profiles: &[HermesProfile]) {
    if let Ok(mut guard) = cache().lock() {
        guard.entries = Some((Instant::now(), profiles.to_vec()));
        guard.key = Some(binary.to_path_buf());
    }
}

/// Invalidate the cache. Useful if we ever wire profile create/delete
/// commands into the worker. Not currently called.
#[allow(dead_code)]
pub fn invalidate_cache() {
    if let Ok(mut guard) = cache().lock() {
        guard.entries = None;
        guard.key = None;
    }
}

/// List the user's Hermes profiles. Caches the result for 60 s.
pub async fn list_profiles(binary: &Path) -> anyhow::Result<Vec<HermesProfile>> {
    if let Some(hit) = cached_for(binary) {
        return Ok(hit);
    }

    let output = Command::new(binary)
        .args(["profile", "list"])
        .output()
        .await
        .with_context(|| format!("spawning {} profile list", binary.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "`{} profile list` exited with {}: {}",
            binary.display(),
            output.status,
            stderr.trim()
        );
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let profiles = parse_profile_list(&text);
    store(binary, &profiles);
    Ok(profiles)
}

/// Parse the table that `hermes profile list` prints. Public for unit tests.
///
/// Strategy: ignore the fancy box-drawing alignment and just split each
/// data row on runs of 2+ spaces. The columns are positional and we know
/// the upstream order (Profile, Model, Gateway, Alias, Distribution).
/// Robust to header/separator drift; only breaks if a column value ever
/// contains two consecutive spaces (none of these fields do today).
pub fn parse_profile_list(text: &str) -> Vec<HermesProfile> {
    let lines: Vec<&str> = text.lines().collect();

    // Find the separator row (the one containing the box-drawing dash).
    let sep_idx = match lines.iter().position(|l| l.contains('\u{2500}')) {
        Some(i) => i,
        None => return Vec::new(),
    };

    let mut profiles = Vec::new();
    for line in lines.iter().skip(sep_idx + 1) {
        if line.trim().is_empty() {
            continue;
        }
        // Defensive: stop at any further separator row.
        if line.contains('\u{2500}') {
            break;
        }

        // Split on runs of 2+ whitespace chars. Trim leading/trailing first
        // so the leading-space convention from hermes's table doesn't add
        // an empty leading cell.
        let cells: Vec<&str> = split_on_double_space(line.trim());
        if cells.is_empty() {
            continue;
        }

        let (is_default, name) = strip_default_marker(cells[0]);
        if name.is_empty() {
            continue;
        }

        profiles.push(HermesProfile {
            name: name.to_string(),
            is_default,
            model: cells.get(1).copied().and_then(emdash_to_none),
            gateway: cells.get(2).copied().and_then(emdash_to_none),
            alias: cells.get(3).copied().and_then(emdash_to_none),
            distribution: cells.get(4).copied().and_then(emdash_to_none),
        });
    }

    profiles
}

/// Split a string on runs of two or more whitespace characters. Returns
/// trimmed non-empty cells.
fn split_on_double_space(s: &str) -> Vec<&str> {
    let mut cells = Vec::new();
    let mut last_end = 0usize;
    let mut run_start: Option<usize> = None;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b' ' || b == b'\t' {
            if run_start.is_none() {
                run_start = Some(i);
            }
            i += 1;
        } else {
            if let Some(rs) = run_start.take() {
                if i - rs >= 2 {
                    let cell = s[last_end..rs].trim();
                    if !cell.is_empty() {
                        cells.push(cell);
                    }
                    last_end = i;
                }
                // Run of length 1 — treat as part of the current cell.
            }
            i += 1;
        }
    }
    let tail = s[last_end..].trim();
    if !tail.is_empty() {
        cells.push(tail);
    }
    cells
}

fn strip_default_marker(cell: &str) -> (bool, &str) {
    let trimmed = cell.trim();
    if let Some(rest) = trimmed.strip_prefix('\u{25C6}') {
        (true, rest.trim_start())
    } else {
        (false, trimmed)
    }
}

fn emdash_to_none(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t == "\u{2014}" || t == "-" {
        None
    } else {
        Some(t.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\n Profile          Model                        Gateway      Alias        Distribution\n \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}    \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}    \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}    \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}    \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n \u{25C6}default         gemini-flash-lite-latest     stopped      \u{2014}            \u{2014}\n";

    #[test]
    fn parses_default_profile() {
        let profiles = parse_profile_list(SAMPLE);
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert_eq!(p.name, "default");
        assert!(p.is_default);
        assert_eq!(p.model.as_deref(), Some("gemini-flash-lite-latest"));
        assert_eq!(p.gateway.as_deref(), Some("stopped"));
        assert_eq!(p.alias, None);
        assert_eq!(p.distribution, None);
    }

    #[test]
    fn empty_output_returns_empty() {
        assert!(parse_profile_list("").is_empty());
        assert!(parse_profile_list("no separator here\n").is_empty());
    }

    #[test]
    fn strip_default_marker_handles_both_cases() {
        assert_eq!(strip_default_marker(" \u{25C6}foo "), (true, "foo"));
        assert_eq!(strip_default_marker(" bar"), (false, "bar"));
    }

    #[test]
    fn parses_real_captured_output() {
        let text = match std::fs::read_to_string("/tmp/hermes-output.txt") {
            Ok(t) => t,
            Err(_) => return, // file isn't always present; skip silently
        };
        let profiles = parse_profile_list(&text);
        eprintln!("Got {} profiles from real output", profiles.len());
        for p in &profiles {
            eprintln!("  {:#?}", p);
        }
        assert!(!profiles.is_empty(), "expected at least one profile");
        let p = &profiles[0];
        assert_eq!(p.name, "default");
        assert_eq!(p.model.as_deref(), Some("gemini-flash-lite-latest"));
    }

    #[test]
    fn emdash_becomes_none() {
        assert_eq!(emdash_to_none("\u{2014}"), None);
        assert_eq!(emdash_to_none("-"), None);
        assert_eq!(emdash_to_none(""), None);
        assert_eq!(emdash_to_none(" foo "), Some("foo".to_string()));
    }
}

