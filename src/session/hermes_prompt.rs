use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::config::HermesConfig;

/// Hermes typically installs to `~/.hermes/hermes-agent/hermes` via its own
/// installer. Keep this list in sync with the discovery_paths declared on
/// the built-in agent_mgmt recipe in `agent_mgmt/builtin/hermes.rs`.
const HERMES_FALLBACK_PATHS: &[&str] =
    &["~/.hermes/hermes-agent/hermes", "~/.hermes/bin/hermes"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptState {
    Idle,
    Running,
}

/// A Hermes session that runs `hermes chat -q "<text>"` per message.
///
/// Hermes is currently invoked one-shot per query. Conversational continuity
/// (multi-turn context) isn't surfaced through the CLI yet; if/when Hermes
/// adds a `--continue` style flag we'll plug it in here the same way
/// ClaudePromptSession does.
///
/// No PTY, no ANSI stripping, no interaction agent — clean text in, clean
/// text out.
pub struct HermesPromptSession {
    config: HermesConfig,
    working_dir: PathBuf,
    output_tx: mpsc::Sender<String>,
    state: Arc<Mutex<PromptState>>,
    current_task: Option<JoinHandle<()>>,
    /// Absolute path to the hermes binary, resolved once at session creation.
    /// Populated by `start()`.
    resolved_binary: Option<PathBuf>,
}

impl HermesPromptSession {
    pub fn new(
        config: HermesConfig,
        working_dir: PathBuf,
        output_tx: mpsc::Sender<String>,
    ) -> Self {
        Self {
            config,
            working_dir,
            output_tx,
            state: Arc::new(Mutex::new(PromptState::Idle)),
            current_task: None,
            resolved_binary: None,
        }
    }

    /// Resolve the hermes binary path so we don't depend on the daemon's
    /// `$PATH` matching the user's interactive shell. Order:
    ///   1. `config.binary` as-is (if absolute and exists, or on $PATH).
    ///   2. `which::which(config.binary)`.
    ///   3. Known hermes install locations under the daemon user's home.
    pub async fn start(&mut self) -> Result<()> {
        let resolved = resolve_hermes_binary(&self.config.binary);
        match &resolved {
            Some(p) => tracing::info!("Hermes prompt session ready (binary: {})", p.display()),
            None => tracing::warn!(
                "Hermes binary '{}' not found at config path, on $PATH, or in known fallback locations. \
                 Set [hermes] binary = \"/absolute/path/to/hermes\" in config.toml.",
                self.config.binary
            ),
        }
        self.resolved_binary = resolved;
        Ok(())
    }

    /// Send user input by spawning `hermes chat -q "<text>"`.
    pub async fn send_input(&mut self, text: &str) -> Result<()> {
        {
            let st = self.state.lock().await;
            if *st == PromptState::Running {
                anyhow::bail!("Hermes is still processing the previous message. Please wait.");
            }
        }

        let mut args = vec!["chat".to_string(), "-q".to_string(), text.to_string()];
        args.extend(self.config.extra_args.clone());

        let binary: PathBuf = self
            .resolved_binary
            .clone()
            .unwrap_or_else(|| PathBuf::from(&self.config.binary));

        tracing::debug!(
            "Spawning hermes prompt: {} {}",
            binary.display(),
            args.join(" ")
        );

        let mut child = match Command::new(&binary)
            .args(&args)
            .current_dir(&self.working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                anyhow::bail!(
                    "Could not find the hermes binary at '{}'. Set [hermes] binary in config.toml to its absolute path, or symlink it onto the daemon's $PATH.",
                    binary.display()
                );
            }
            Err(e) => return Err(e.into()),
        };

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        *self.state.lock().await = PromptState::Running;

        let state = self.state.clone();
        let output_tx = self.output_tx.clone();

        let handle = tokio::spawn(async move {
            let stdout_tx = output_tx.clone();
            let stdout_task = tokio::spawn(async move {
                if let Some(stdout) = stdout {
                    read_stream(stdout, stdout_tx).await;
                }
            });

            let stderr_tx = output_tx.clone();
            let stderr_task = tokio::spawn(async move {
                if let Some(stderr) = stderr {
                    read_stream(stderr, stderr_tx).await;
                }
            });

            let _ = stdout_task.await;
            let _ = stderr_task.await;

            match child.wait().await {
                Ok(status) => {
                    if !status.success() {
                        let _ = output_tx
                            .send(format!("[Process exited with {}]\n", status))
                            .await;
                    }
                }
                Err(e) => {
                    let _ = output_tx.send(format!("[Process error: {}]\n", e)).await;
                }
            }

            *state.lock().await = PromptState::Idle;
        });

        self.current_task = Some(handle);
        Ok(())
    }

    /// Prompt mode has no interaction agent.
    pub fn interaction_agent(
        &self,
    ) -> Option<&Arc<Mutex<crate::agent::interaction::InteractionAgent>>> {
        None
    }

    pub fn is_running_sync(&self) -> bool {
        match self.state.try_lock() {
            Ok(st) => *st == PromptState::Running,
            Err(_) => true,
        }
    }
}

impl Drop for HermesPromptSession {
    fn drop(&mut self) {
        if let Some(handle) = self.current_task.take() {
            handle.abort();
        }
    }
}

/// Locate the hermes binary using the same fallback chain as agent_mgmt
/// detection. Returns `None` if none of the candidates exist on disk —
/// callers should treat that as "binary missing" and surface a clear error.
fn resolve_hermes_binary(configured: &str) -> Option<PathBuf> {
    // 1. If config gives a path that already exists, use it.
    let direct = Path::new(configured);
    if direct.is_absolute() && direct.exists() {
        return Some(direct.to_path_buf());
    }

    // 2. PATH lookup.
    if let Ok(p) = which::which(configured) {
        return Some(p);
    }

    // 3. Known hermes install locations under the daemon user's home.
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

async fn read_stream<R: tokio::io::AsyncRead + Unpin>(reader: R, tx: mpsc::Sender<String>) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(format!("{}\n", line)).await.is_err() {
            break;
        }
    }
}
