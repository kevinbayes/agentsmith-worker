use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::config::GeminiConfig;

/// State of the prompt session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptState {
    Idle,
    Running,
}

/// A Gemini CLI session that runs `gemini "text"` per message.
///
/// The first message creates a new session. Subsequent messages use
/// `-r "latest"` to resume the most recent conversation.
///
/// No PTY, no ANSI stripping, no interaction agent — just clean text output.
pub struct GeminiPromptSession {
    config: GeminiConfig,
    working_dir: PathBuf,
    output_tx: mpsc::Sender<String>,
    message_count: u64,
    state: Arc<Mutex<PromptState>>,
    current_task: Option<JoinHandle<()>>,
}

impl GeminiPromptSession {
    pub fn new(
        config: GeminiConfig,
        working_dir: PathBuf,
        output_tx: mpsc::Sender<String>,
    ) -> Self {
        Self {
            config,
            working_dir,
            output_tx,
            message_count: 0,
            state: Arc::new(Mutex::new(PromptState::Idle)),
            current_task: None,
        }
    }

    /// Start is a no-op for prompt mode — the process is spawned per message.
    pub async fn start(&mut self) -> Result<()> {
        tracing::info!("Gemini prompt session ready");
        Ok(())
    }

    /// Send user input by spawning a `gemini` process.
    pub async fn send_input(&mut self, text: &str) -> Result<()> {
        {
            let st = self.state.lock().await;
            if *st == PromptState::Running {
                anyhow::bail!("Gemini is still processing the previous message. Please wait.");
            }
        }

        // Build command arguments
        // First message: `gemini "text"` (creates a new session).
        // Subsequent messages: `gemini -r "latest" "text"` (resumes the most recent session).
        let mut args = Vec::new();

        if self.message_count > 0 {
            args.extend(["-r".to_string(), "latest".to_string()]);
        }

        args.push(text.to_string());

        if self.config.skip_permissions {
            args.extend(["--approval-mode".to_string(), "yolo".to_string()]);
        }

        args.extend(self.config.extra_args.clone());

        tracing::debug!(
            "Spawning gemini prompt: {} {}",
            self.config.binary,
            args.join(" ")
        );

        let mut child = Command::new(&self.config.binary)
            .args(&args)
            .current_dir(&self.working_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        self.message_count += 1;
        *self.state.lock().await = PromptState::Running;

        let state = self.state.clone();
        let output_tx = self.output_tx.clone();

        let handle = tokio::spawn(async move {
            // Spawn separate tasks for stdout and stderr
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

            // Wait for both readers to finish
            let _ = stdout_task.await;
            let _ = stderr_task.await;

            // Wait for the child process
            match child.wait().await {
                Ok(status) => {
                    if !status.success() {
                        let msg = format!("[Process exited with {}]\n", status);
                        let _ = output_tx.send(msg).await;
                    }
                }
                Err(e) => {
                    let msg = format!("[Process error: {}]\n", e);
                    let _ = output_tx.send(msg).await;
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

    /// Non-blocking check if the session is currently processing a message.
    pub fn is_running_sync(&self) -> bool {
        match self.state.try_lock() {
            Ok(st) => *st == PromptState::Running,
            Err(_) => true, // lock contention means the task is active
        }
    }
}

impl Drop for GeminiPromptSession {
    fn drop(&mut self) {
        if let Some(handle) = self.current_task.take() {
            handle.abort();
        }
    }
}

/// Read lines from an async reader and send them to the output channel.
async fn read_stream<R: tokio::io::AsyncRead + Unpin>(reader: R, tx: mpsc::Sender<String>) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if tx.send(format!("{}\n", line)).await.is_err() {
            break;
        }
    }
}
