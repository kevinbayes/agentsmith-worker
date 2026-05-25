pub mod claude;
pub mod claude_prompt;
pub mod output_buffer;
pub mod pty;
pub mod zeroclaw;
pub mod zeroclaw_prompt;

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use crate::agent::interaction::{FeedbackRequest, InteractionAgent};
use crate::config::Config;
use crate::messaging::ThreadId;
use crate::session::claude::ClaudeSession;
use crate::session::claude_prompt::ClaudePromptSession;
use crate::session::zeroclaw::ZeroclawSession;
use crate::session::zeroclaw_prompt::ZeroclawPromptSession;
use crate::session::output_buffer::run_output_buffer;

/// Unique ID for a session.
pub type SessionId = u64;

/// The type of AI tool backing a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTool {
    Claude,
    Zeroclaw,
}

impl std::fmt::Display for SessionTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionTool::Claude => write!(f, "Claude"),
            SessionTool::Zeroclaw => write!(f, "ZeroClaw"),
        }
    }
}

impl SessionTool {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(SessionTool::Claude),
            "zeroclaw" => Some(SessionTool::Zeroclaw),
            _ => None,
        }
    }

    fn tool_label(&self) -> &'static str {
        match self {
            SessionTool::Claude => "Claude Code",
            SessionTool::Zeroclaw => "ZeroClaw",
        }
    }
}

/// Status of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Running,
    Idle,
    AwaitingInput,
    Stopped,
}

impl std::fmt::Display for SessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionStatus::Running => write!(f, "running"),
            SessionStatus::Idle => write!(f, "idle"),
            SessionStatus::AwaitingInput => write!(f, "awaiting input"),
            SessionStatus::Stopped => write!(f, "stopped"),
        }
    }
}

/// Info about a session for listing.
pub struct SessionInfo {
    pub id: SessionId,
    pub tool: SessionTool,
    pub status: SessionStatus,
}

/// Internal representation of a running session.
enum SessionInner {
    Claude(ClaudeSession),
    ClaudePrompt(ClaudePromptSession),
    Zeroclaw(ZeroclawSession),
    ZeroclawPrompt(ZeroclawPromptSession),
}

struct SessionEntry {
    inner: SessionInner,
    tool: SessionTool,
    status: SessionStatus,
    /// Sends text chunks from the session output buffer to the router.
    chunk_rx: mpsc::Receiver<String>,
}

impl SessionEntry {
    /// Get a reference to the interaction agent from whichever session type.
    fn interaction_agent(&self) -> Option<&Arc<Mutex<InteractionAgent>>> {
        match &self.inner {
            SessionInner::Claude(s) => s.interaction_agent(),
            SessionInner::ClaudePrompt(s) => s.interaction_agent(),
            SessionInner::Zeroclaw(s) => s.interaction_agent(),
            SessionInner::ZeroclawPrompt(s) => s.interaction_agent(),
        }
    }
}

/// Manages all AI sessions. Owned exclusively by the Router task.
pub struct SessionManager {
    sessions: HashMap<SessionId, SessionEntry>,
    /// Maps each thread to its active session.
    active_sessions: HashMap<ThreadId, SessionId>,
    next_id: SessionId,
    config: Config,
    /// Sender for feedback requests from interaction agents to the router.
    feedback_tx: mpsc::Sender<FeedbackRequest>,
    /// Receiver for feedback requests (polled by the router).
    feedback_rx: mpsc::Receiver<FeedbackRequest>,
}

impl SessionManager {
    pub fn new(config: Config) -> Self {
        let (feedback_tx, feedback_rx) = mpsc::channel(64);
        Self {
            sessions: HashMap::new(),
            active_sessions: HashMap::new(),
            next_id: 1,
            config,
            feedback_tx,
            feedback_rx,
        }
    }

    /// Create a new session and make it active for the given thread.
    pub async fn create_session(
        &mut self,
        tool: SessionTool,
        thread: &ThreadId,
    ) -> Result<SessionId> {
        let max = self.config.session_defaults.max_sessions;
        if self.sessions.len() >= max {
            anyhow::bail!("Maximum sessions ({}) reached. Stop a session first.", max);
        }

        let id = self.next_id;
        self.next_id += 1;

        let working_dir = self.config.daemon.working_dir.clone();
        let max_chunk = self.config.session_defaults.max_chunk_size;
        let flush_ms = self.config.session_defaults.output_flush_interval_ms;

        // Create output pipeline: session -> raw_output_tx -> buffer -> chunk_tx/chunk_rx
        let (raw_output_tx, raw_output_rx) = mpsc::channel::<String>(256);
        let (chunk_tx, chunk_rx) = mpsc::channel::<String>(64);

        // Spawn the output buffer task
        tokio::spawn(run_output_buffer(raw_output_rx, chunk_tx, max_chunk, flush_ms));

        let inner = match tool {
            SessionTool::Claude if self.config.claude.prompt_mode => {
                // Prompt mode: no PTY, no interaction agent
                let mut session =
                    ClaudePromptSession::new(self.config.claude.clone(), working_dir, raw_output_tx);
                session.start().await?;
                SessionInner::ClaudePrompt(session)
            }
            SessionTool::Claude => {
                let interaction = InteractionAgent::try_new(
                    &self.config.interaction_agent,
                    id,
                    tool.tool_label(),
                    self.feedback_tx.clone(),
                );
                let mut session =
                    ClaudeSession::new(self.config.claude.clone(), working_dir, raw_output_tx, interaction);
                session.start().await?;
                SessionInner::Claude(session)
            }
            SessionTool::Zeroclaw if self.config.zeroclaw.prompt_mode => {
                // Prompt mode: no PTY, no interaction agent
                let mut session =
                    ZeroclawPromptSession::new(self.config.zeroclaw.clone(), working_dir, raw_output_tx);
                session.start().await?;
                SessionInner::ZeroclawPrompt(session)
            }
            SessionTool::Zeroclaw => {
                let interaction = InteractionAgent::try_new(
                    &self.config.interaction_agent,
                    id,
                    tool.tool_label(),
                    self.feedback_tx.clone(),
                );
                let mut session =
                    ZeroclawSession::new(self.config.zeroclaw.clone(), working_dir, raw_output_tx, interaction);
                session.start().await?;
                SessionInner::Zeroclaw(session)
            }
        };

        let entry = SessionEntry {
            inner,
            tool,
            status: SessionStatus::Idle,
            chunk_rx,
        };

        self.sessions.insert(id, entry);
        self.active_sessions.insert(thread.clone(), id);

        tracing::info!("Created {} session #{} for {}", tool, id, thread);
        Ok(id)
    }

    /// Send user input to a session.
    pub async fn send_input(&mut self, session_id: SessionId, text: &str) -> Result<()> {
        let entry = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| anyhow::anyhow!("Session #{} not found", session_id))?;

        entry.status = SessionStatus::Running;

        match &mut entry.inner {
            SessionInner::Claude(session) => {
                session.send_input(text)?;
            }
            SessionInner::ClaudePrompt(session) => {
                session.send_input(text).await?;
            }
            SessionInner::Zeroclaw(session) => {
                session.send_input(text)?;
            }
            SessionInner::ZeroclawPrompt(session) => {
                session.send_input(text).await?;
            }
        }

        Ok(())
    }

    /// Send a feedback response to a session's PTY stdin.
    pub fn send_feedback_response(
        &mut self,
        session_id: SessionId,
        formatted_input: &str,
    ) -> Result<()> {
        let entry = self
            .sessions
            .get_mut(&session_id)
            .ok_or_else(|| anyhow::anyhow!("Session #{} not found", session_id))?;

        match &mut entry.inner {
            SessionInner::Claude(session) => {
                session.send_input(formatted_input)?;
            }
            SessionInner::ClaudePrompt(_) => {
                anyhow::bail!("Feedback responses are not supported in prompt mode");
            }
            SessionInner::Zeroclaw(session) => {
                session.send_input(formatted_input)?;
            }
            SessionInner::ZeroclawPrompt(_) => {
                anyhow::bail!("Feedback responses are not supported in prompt mode");
            }
        }

        entry.status = SessionStatus::Running;
        Ok(())
    }

    /// Try to receive a feedback request from any interaction agent.
    pub fn try_recv_feedback(&mut self) -> Option<FeedbackRequest> {
        self.feedback_rx.try_recv().ok()
    }

    /// Get the interaction agent for a session (for format_response calls).
    pub fn interaction_agent_for(
        &self,
        session_id: SessionId,
    ) -> Option<Arc<Mutex<InteractionAgent>>> {
        self.sessions
            .get(&session_id)
            .and_then(|e| e.interaction_agent().cloned())
    }

    /// Mark a session as awaiting input.
    pub fn set_awaiting_input(&mut self, session_id: SessionId) {
        if let Some(entry) = self.sessions.get_mut(&session_id) {
            entry.status = SessionStatus::AwaitingInput;
        }
    }

    /// Get the active session ID for a thread.
    pub fn active_session(&self, thread: &ThreadId) -> Option<SessionId> {
        self.active_sessions.get(thread).copied()
    }

    /// Switch the active session for a thread.
    pub fn switch_active(&mut self, thread: &ThreadId, session_id: SessionId) -> Result<()> {
        if !self.sessions.contains_key(&session_id) {
            anyhow::bail!("Session #{} not found", session_id);
        }
        self.active_sessions.insert(thread.clone(), session_id);
        Ok(())
    }

    /// List all sessions.
    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions
            .iter()
            .map(|(&id, entry)| SessionInfo {
                id,
                tool: entry.tool,
                status: entry.status,
            })
            .collect()
    }

    /// Stop a specific session.
    pub fn stop_session(&mut self, session_id: SessionId) -> Result<()> {
        if let Some(entry) = self.sessions.remove(&session_id) {
            // Drop the entry, which drops senders and PTY handles
            drop(entry);
            // Remove from active sessions
            self.active_sessions.retain(|_, &mut sid| sid != session_id);
            tracing::info!("Stopped session #{}", session_id);
            Ok(())
        } else {
            anyhow::bail!("Session #{} not found", session_id);
        }
    }

    /// Stop all sessions.
    pub fn shutdown_all(&mut self) {
        let ids: Vec<SessionId> = self.sessions.keys().copied().collect();
        for id in ids {
            let _ = self.stop_session(id);
        }
        tracing::info!("All sessions stopped");
    }

    /// Try to receive a chunk of output from any session.
    /// Returns (session_id, chunk) if data is available.
    pub fn try_recv_output(&mut self) -> Option<(SessionId, String)> {
        for (&id, entry) in self.sessions.iter_mut() {
            // For prompt sessions, flip status back to Idle when the process finishes
            if entry.status == SessionStatus::Running {
                match &entry.inner {
                    SessionInner::ClaudePrompt(ref s) => {
                        if !s.is_running_sync() {
                            entry.status = SessionStatus::Idle;
                        }
                    }
                    SessionInner::ZeroclawPrompt(ref s) => {
                        if !s.is_running_sync() {
                            entry.status = SessionStatus::Idle;
                        }
                    }
                    _ => {}
                }
            }

            if let Ok(chunk) = entry.chunk_rx.try_recv() {
                return Some((id, chunk));
            }
        }
        None
    }

    /// Get the threads that have a particular session active.
    pub fn threads_for_session(&self, session_id: SessionId) -> Vec<ThreadId> {
        self.active_sessions
            .iter()
            .filter(|(_, &sid)| sid == session_id)
            .map(|(thread, _)| thread.clone())
            .collect()
    }
}
