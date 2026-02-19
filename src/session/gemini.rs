use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};

use crate::agent::interaction::InteractionAgent;
use crate::config::GeminiConfig;
use crate::session::pty::{spawn_pty, PtyHandle};

/// A Gemini CLI session running in a PTY.
pub struct GeminiSession {
    config: GeminiConfig,
    working_dir: PathBuf,
    pty: Option<PtyHandle>,
    output_tx: mpsc::Sender<String>,
    interaction: Option<Arc<Mutex<InteractionAgent>>>,
}

impl GeminiSession {
    pub fn new(
        config: GeminiConfig,
        working_dir: PathBuf,
        output_tx: mpsc::Sender<String>,
        interaction: Option<InteractionAgent>,
    ) -> Self {
        Self {
            config,
            working_dir,
            pty: None,
            output_tx,
            interaction: interaction.map(|ia| Arc::new(Mutex::new(ia))),
        }
    }

    /// Start the Gemini CLI in a PTY.
    pub async fn start(&mut self) -> Result<()> {
        let args: Vec<String> = self.config.extra_args.clone();

        let (handle, mut data_rx) = spawn_pty(&self.config.binary, &args, &self.working_dir)?;
        self.pty = Some(handle);

        let output_tx = self.output_tx.clone();
        let interaction = self.interaction.clone();

        tokio::spawn(async move {
            let tick_interval = Duration::from_millis(500);
            let mut tick_timer = tokio::time::interval(tick_interval);
            tick_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    raw = data_rx.recv() => {
                        match raw {
                            Some(raw_bytes) => {
                                let cleaned = strip_ansi_escapes::strip(&raw_bytes);
                                let text = match String::from_utf8(cleaned) {
                                    Ok(t) => t,
                                    Err(_) => String::from_utf8_lossy(&raw_bytes).to_string(),
                                };
                                if text.is_empty() {
                                    continue;
                                }

                                if let Some(ref ia) = interaction {
                                    let forward = ia.lock().await.process_output(&text);
                                    if let Some(fwd) = forward {
                                        let _ = output_tx.send(fwd).await;
                                    }
                                } else {
                                    let _ = output_tx.send(text).await;
                                }
                            }
                            None => {
                                tracing::debug!("Gemini PTY output reader finished");
                                break;
                            }
                        }
                    }
                    _ = tick_timer.tick() => {
                        if let Some(ref ia) = interaction {
                            // Quick lock: check if classification is needed
                            let pending = ia.lock().await.prepare_tick();
                            if let Some(pending) = pending {
                                // LLM call runs WITHOUT holding the lock
                                let result = pending.agent.generate(&pending.system, &pending.prompt).await;
                                // Quick lock: apply the result
                                ia.lock().await.complete_classify(&pending, result);
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Send user input to the Gemini CLI via PTY stdin.
    /// Uses \r (carriage return) as the line ending since TUI apps in raw mode
    /// expect \r for Enter, not \n.
    pub fn send_input(&mut self, text: &str) -> Result<()> {
        if let Some(ref mut pty) = self.pty {
            let input = format!("{}\r", text);
            pty.write_input(input.as_bytes())?;
        } else {
            anyhow::bail!("Gemini session not started");
        }
        Ok(())
    }

    /// Get a reference to the interaction agent (for format_response calls).
    pub fn interaction_agent(&self) -> Option<&Arc<Mutex<InteractionAgent>>> {
        self.interaction.as_ref()
    }

    /// Check if the PTY is still alive.
    pub fn is_running(&self) -> bool {
        self.pty.is_some()
    }
}
