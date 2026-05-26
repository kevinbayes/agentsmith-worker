use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sysinfo::{ProcessRefreshKind, RefreshKind, System};
use tokio::sync::Mutex;

use crate::config::AgentManagementConfig;

pub mod api;
pub mod builtin;
pub mod installer;
pub mod killer;
pub mod protocol;
pub mod recipe;
pub mod registry;

pub use protocol::{
    AgentCommand, AgentCommandKind, AgentCommandResult, ResultStatus,
};

/// Public summary of one agent, returned by `Manager::list` and `Status`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentSummary {
    pub name: String,
    pub display_name: String,
    pub installed: bool,
    pub version: Option<String>,
    pub payload_path: Option<PathBuf>,
    pub symlink_path: Option<PathBuf>,
    pub running_pids: Vec<u32>,
    pub recipe_source: Option<registry::RecipeSource>,
}

/// Lifecycle manager for installable agents (install / update / remove / kill).
///
/// Constructed once at startup and shared as `Arc<Manager>` between the
/// chat router, the REST API, and the reporter pull-back path.
pub struct Manager {
    bin_dir: PathBuf,
    data_dir: PathBuf,
    kill_grace: Duration,
    registry: registry::RecipeRegistry,
    /// Per-agent locks so two install/update/remove calls on the same agent
    /// don't race. Commands for *different* agents run in parallel.
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Manager {
    pub fn new(config: &AgentManagementConfig, data_dir: &Path) -> Arc<Self> {
        let bin_dir = expand_bin_dir(&config.bin_dir);
        installer::check_bin_dir_on_path(&bin_dir);

        let registry = registry::RecipeRegistry::new(config.recipes.clone());

        Arc::new(Self {
            bin_dir,
            data_dir: data_dir.to_path_buf(),
            kill_grace: Duration::from_millis(config.kill_grace_ms),
            registry,
            locks: Mutex::new(HashMap::new()),
        })
    }

    pub fn bin_dir(&self) -> &Path {
        &self.bin_dir
    }

    /// Where this agent's payload lives.
    fn payload_dir(&self, name: &str) -> PathBuf {
        self.data_dir.join("agents").join(name)
    }

    async fn lock_for(&self, name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Execute one command. Returns a structured result suitable for the
    /// REST API, chat output, and the control-plane pull-back queue.
    pub async fn execute(&self, cmd: AgentCommand) -> AgentCommandResult {
        let id = cmd.id.clone();
        let name = cmd.name.clone();

        // `List` ignores the name field.
        if matches!(cmd.kind, AgentCommandKind::List) {
            return self.handle_list().with_id(id);
        }

        if name.trim().is_empty() {
            return AgentCommandResult::failed("", "agent name is required").with_id(id);
        }

        let lock = self.lock_for(&name).await;
        let _guard = lock.lock().await;

        let result = match cmd.kind {
            AgentCommandKind::Install {
                version,
                recipe_override,
            } => self.handle_install(&name, version, recipe_override).await,
            AgentCommandKind::Update { version } => self.handle_update(&name, version).await,
            AgentCommandKind::Remove => self.handle_remove(&name).await,
            AgentCommandKind::Kill { pid } => self.handle_kill(&name, pid).await,
            AgentCommandKind::Status => self.handle_status(&name).await,
            AgentCommandKind::List => unreachable!("handled above"),
        };

        result.with_id(id)
    }

    /// Snapshot of all known agents — built-in recipes ∪ config recipes,
    /// each enriched with disk and process state.
    pub fn list(&self) -> Vec<AgentSummary> {
        self.registry
            .known_names()
            .into_iter()
            .map(|n| self.summarize(&n))
            .collect()
    }

    /// Full status of one agent (or None if no recipe exists for the name).
    pub fn status(&self, name: &str) -> Option<AgentSummary> {
        self.registry.resolve(name, None)?;
        Some(self.summarize(name))
    }

    fn summarize(&self, name: &str) -> AgentSummary {
        let resolved = self.registry.resolve(name, None);
        let payload = self.payload_dir(name);
        let payload_present = payload.exists();
        let recipe = resolved.as_ref().map(|(r, _)| r);
        let display_name = recipe
            .map(|r| r.display_name.clone())
            .unwrap_or_else(|| name.to_string());

        // Locate the binary. Three checks, in priority order:
        //   1. Worker-managed symlink at <bin_dir>/<symlink_name> — present
        //      only when this worker installed the agent.
        //   2. Recipe entry-point name on the operator's $PATH — catches
        //      external installs (cargo install, system package, manual).
        //   3. None — agent is not installed.
        let (symlink_path, version) = if let Some(r) = recipe {
            let mut found: Option<PathBuf> = None;
            'eps: for ep in &r.entry_points {
                let managed = self.bin_dir.join(&ep.symlink_name);
                if managed.exists() {
                    found = Some(managed);
                    break;
                }
                if let Ok(on_path) = which::which(&ep.symlink_name) {
                    found = Some(on_path);
                    break;
                }
                for hint in &ep.discovery_paths {
                    let expanded = expand_home(hint);
                    if expanded.exists() {
                        found = Some(expanded);
                        break 'eps;
                    }
                }
            }
            let version = found.as_ref().and_then(|p| detect_version(p));
            (found, version)
        } else {
            (None, None)
        };

        // "Installed" if we have either a worker-managed payload or a
        // discoverable binary anywhere on PATH.
        let installed = payload_present || symlink_path.is_some();

        let running_pids = recipe
            .map(|r| collect_running_pids(r))
            .unwrap_or_default();

        AgentSummary {
            name: name.to_string(),
            display_name,
            installed,
            version,
            payload_path: if payload_present { Some(payload) } else { None },
            symlink_path,
            running_pids,
            recipe_source: resolved.map(|(_, src)| src),
        }
    }

    // ── Command handlers ────────────────────────────────────────────────

    async fn handle_install(
        &self,
        name: &str,
        version: Option<String>,
        recipe_override: Option<recipe::Recipe>,
    ) -> AgentCommandResult {
        let (recipe, _src) = match self.registry.resolve(name, recipe_override.as_ref()) {
            Some(r) => r,
            None => {
                return AgentCommandResult::failed(
                    name,
                    format!("no recipe found for agent '{}'", name),
                );
            }
        };

        let version = match version {
            Some(v) if !v.trim().is_empty() => v,
            _ => {
                return AgentCommandResult::failed(
                    name,
                    "version is required for install in v1 (auto-latest is deferred to v2)",
                );
            }
        };

        let payload = self.payload_dir(name);
        match installer::install(&recipe, &version, &payload, &self.bin_dir).await {
            Ok(outcome) => AgentCommandResult::success(
                name,
                format!(
                    "installed {} {} → {}",
                    name,
                    version,
                    outcome.payload_path.display()
                ),
            )
            .with_details(serde_json::to_value(&outcome).unwrap_or_default()),
            Err(e) => AgentCommandResult::failed(name, format!("install failed: {:#}", e)),
        }
    }

    async fn handle_update(
        &self,
        name: &str,
        version: Option<String>,
    ) -> AgentCommandResult {
        let (recipe, _src) = match self.registry.resolve(name, None) {
            Some(r) => r,
            None => {
                return AgentCommandResult::failed(
                    name,
                    format!("no recipe found for agent '{}'", name),
                );
            }
        };

        // Strict: kill running processes before swapping the payload.
        let kill_result = self.kill_recipe_processes(&recipe).await;

        let result = self
            .handle_install(name, version, None)
            .await;

        // Augment install details with the kill summary so the operator can
        // see what went down.
        merge_kill_into_details(result, kill_result)
    }

    async fn handle_remove(&self, name: &str) -> AgentCommandResult {
        let (recipe, _src) = match self.registry.resolve(name, None) {
            Some(r) => r,
            None => {
                return AgentCommandResult::failed(
                    name,
                    format!("no recipe found for agent '{}'", name),
                );
            }
        };

        let kill_result = self.kill_recipe_processes(&recipe).await;

        let payload = self.payload_dir(name);
        match installer::remove(&recipe, &payload, &self.bin_dir) {
            Ok(outcome) => {
                let mut details =
                    serde_json::to_value(&outcome).unwrap_or(serde_json::Value::Null);
                if let serde_json::Value::Object(ref mut map) = details {
                    map.insert("kill".to_string(), serde_json::to_value(&kill_result).unwrap_or_default());
                }
                AgentCommandResult::success(name, format!("removed {}", name)).with_details(details)
            }
            Err(e) => AgentCommandResult::failed(name, format!("remove failed: {:#}", e)),
        }
    }

    async fn handle_kill(&self, name: &str, pid: Option<u32>) -> AgentCommandResult {
        let result = match pid {
            Some(p) => killer::kill_by_pid(p, self.kill_grace).await,
            None => match self.registry.resolve(name, None) {
                Some((recipe, _)) => self.kill_recipe_processes(&recipe).await,
                None => {
                    return AgentCommandResult::failed(
                        name,
                        format!("no recipe found for agent '{}'", name),
                    );
                }
            },
        };

        let msg = if result.none_found {
            format!("no running processes for {}", name)
        } else if result.failed.is_empty() {
            format!("killed {} processes", result.killed.len())
        } else {
            format!(
                "killed {}, failed {}",
                result.killed.len(),
                result.failed.len()
            )
        };

        AgentCommandResult::success(name, msg)
            .with_details(serde_json::to_value(&result).unwrap_or_default())
    }

    async fn handle_status(&self, name: &str) -> AgentCommandResult {
        match self.status(name) {
            Some(s) => AgentCommandResult::success(
                name,
                if s.installed {
                    "installed".to_string()
                } else {
                    "not installed".to_string()
                },
            )
            .with_details(serde_json::to_value(&s).unwrap_or_default()),
            None => AgentCommandResult::failed(
                name,
                format!("no recipe found for agent '{}'", name),
            ),
        }
    }

    fn handle_list(&self) -> AgentCommandResult {
        let list = self.list();
        AgentCommandResult::success("", format!("{} agents known", list.len()))
            .with_details(serde_json::to_value(&list).unwrap_or_default())
    }

    /// Kill every process matching any entry point of the recipe. If the
    /// recipe has no entry points (defensive), returns an empty result.
    async fn kill_recipe_processes(&self, recipe: &recipe::Recipe) -> killer::KillResult {
        let mut combined = killer::KillResult {
            none_found: true,
            ..Default::default()
        };
        for ep in &recipe.entry_points {
            let one = killer::kill_by_binary(&ep.symlink_name, self.kill_grace).await;
            combined.merge(one);
        }
        combined
    }
}

fn merge_kill_into_details(
    mut result: AgentCommandResult,
    kill: killer::KillResult,
) -> AgentCommandResult {
    let mut details = std::mem::take(&mut result.details);
    if !matches!(details, serde_json::Value::Object(_)) {
        details = serde_json::json!({});
    }
    if let serde_json::Value::Object(ref mut map) = details {
        map.insert("kill".to_string(), serde_json::to_value(&kill).unwrap_or_default());
    }
    result.details = details;
    result
}

fn collect_running_pids(r: &recipe::Recipe) -> Vec<u32> {
    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    let mut pids = Vec::new();
    for ep in &r.entry_points {
        let basename = &ep.symlink_name;
        for (pid, process) in sys.processes() {
            let n = process.name().to_string_lossy().to_string();
            if &n == basename
                || (n == "node"
                    && process
                        .cmd()
                        .iter()
                        .any(|a| a.to_string_lossy().contains(basename)))
            {
                pids.push(pid.as_u32());
            }
        }
    }
    pids.sort();
    pids.dedup();
    pids
}

fn detect_version(binary: &Path) -> Option<String> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
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

fn expand_bin_dir(p: &Path) -> PathBuf {
    expand_home(&p.to_string_lossy())
}

/// Expand a leading `~/` in a string to the daemon user's home directory.
/// Returns the path unchanged if no expansion applies.
fn expand_home(s: &str) -> PathBuf {
    if let Some(stripped) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(s)
}
