use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::agent::interaction::FeedbackRequest;
use crate::command::{self, Command, MonitorAction, StopTarget};
use crate::config::Config;
use crate::messaging::{IncomingMessage, OutgoingMessage, ThreadId};
use crate::monitor::Monitor;
use crate::reporter::Reporter;
use crate::session::{SessionId, SessionManager, SessionTool};

/// Tracks a pending feedback request awaiting user response.
struct PendingFeedback {
    request: FeedbackRequest,
    threads: Vec<ThreadId>,
    created_at: Instant,
}

/// The central message router. Owns the SessionManager and coordinates
/// messages between adapters and AI sessions.
pub struct Router {
    config: Config,
    session_mgr: SessionManager,
    incoming_rx: mpsc::Receiver<IncomingMessage>,
    outgoing_txs: Vec<mpsc::Sender<OutgoingMessage>>,
    cancel: CancellationToken,
    /// Pending feedback requests keyed by session ID.
    pending_feedback: HashMap<SessionId, PendingFeedback>,
    /// Timeout for feedback responses (5 minutes).
    feedback_timeout: Duration,
    /// Tool installation and process monitor.
    monitor: Monitor,
    /// Periodic state reporter to control center.
    reporter: Option<Reporter>,
}

impl Router {
    pub fn new(
        config: Config,
        incoming_rx: mpsc::Receiver<IncomingMessage>,
        outgoing_txs: Vec<mpsc::Sender<OutgoingMessage>>,
        cancel: CancellationToken,
    ) -> Self {
        let session_mgr = SessionManager::new(config.clone());
        let monitor = Monitor::from_config(&config);
        let reporter = Reporter::new(&config.reporter);
        Self {
            config,
            session_mgr,
            incoming_rx,
            outgoing_txs,
            cancel,
            pending_feedback: HashMap::new(),
            feedback_timeout: Duration::from_secs(300),
            monitor,
            reporter,
        }
    }

    /// Run the router event loop.
    pub async fn run(mut self) -> anyhow::Result<()> {
        tracing::info!("Router started");

        let poll_interval = Duration::from_millis(50);

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    tracing::info!("Router shutting down...");
                    self.session_mgr.shutdown_all();
                    // Notify active threads
                    for thread in self.all_active_threads() {
                        self.send_reply(&thread, "AgentSmith is shutting down. All sessions stopped.").await;
                    }
                    break;
                }
                msg = self.incoming_rx.recv() => {
                    match msg {
                        Some(incoming) => {
                            self.handle_message(incoming).await;
                        }
                        None => {
                            tracing::info!("All incoming channels closed, shutting down router");
                            break;
                        }
                    }
                }
                _ = tokio::time::sleep(poll_interval) => {
                    // Poll for session output
                    self.poll_session_output().await;
                    // Poll for feedback requests from interaction agents
                    self.poll_feedback_requests().await;
                    // Check for feedback timeouts
                    self.check_feedback_timeouts().await;
                    // Report state to control center
                    if let Some(ref mut reporter) = self.reporter {
                        reporter.maybe_report(&self.monitor, &self.session_mgr).await;
                    }
                }
            }
        }

        tracing::info!("Router stopped");
        Ok(())
    }

    async fn handle_message(&mut self, msg: IncomingMessage) {
        let thread = msg.thread.clone();
        let cmd = command::parse_command(&msg.text);

        tracing::debug!("Received from {}: {:?}", thread, cmd);

        match cmd {
            Command::New { tool } => {
                self.handle_new_session(&thread, &tool).await;
            }
            Command::List => {
                self.handle_list(&thread).await;
            }
            Command::Switch { session_id } => {
                self.handle_switch(&thread, session_id).await;
            }
            Command::Stop { target } => {
                self.handle_stop(&thread, target).await;
            }
            Command::Help => {
                self.send_reply(&thread, command::help_text()).await;
            }
            Command::Status => {
                self.handle_status(&thread).await;
            }
            Command::Monitor { action } => {
                self.handle_monitor(&thread, action).await;
            }
            Command::Text(text) => {
                self.handle_text(&thread, &text).await;
            }
        }
    }

    async fn handle_new_session(&mut self, thread: &ThreadId, tool_name: &str) {
        let tool = match SessionTool::from_str(tool_name) {
            Some(t) => t,
            None => {
                self.send_reply(
                    thread,
                    &format!(
                        "Unknown tool '{}'. Use `/new claude`, `/new gemini`, `/new goose`, or `/new zeroclaw`.",
                        tool_name
                    ),
                )
                .await;
                return;
            }
        };

        match self.session_mgr.create_session(tool, thread).await {
            Ok(id) => {
                self.send_reply(
                    thread,
                    &format!("Created {} session #{}. It is now your active session.", tool, id),
                )
                .await;
            }
            Err(e) => {
                self.send_reply(thread, &format!("Failed to create session: {}", e))
                    .await;
            }
        }
    }

    async fn handle_list(&mut self, thread: &ThreadId) {
        let sessions = self.session_mgr.list_sessions();
        if sessions.is_empty() {
            self.send_reply(thread, "No active sessions. Use `/new claude`, `/new gemini`, `/new goose`, or `/new zeroclaw` to start one.")
                .await;
            return;
        }

        let active_id = self.session_mgr.active_session(thread);
        let mut lines = vec!["*Sessions:*".to_string()];
        for s in &sessions {
            let marker = if active_id == Some(s.id) {
                " (active)"
            } else {
                ""
            };
            lines.push(format!("  #{} - {} [{}]{}", s.id, s.tool, s.status, marker));
        }
        self.send_reply(thread, &lines.join("\n")).await;
    }

    async fn handle_switch(&mut self, thread: &ThreadId, session_id: u64) {
        match self.session_mgr.switch_active(thread, session_id) {
            Ok(()) => {
                self.send_reply(thread, &format!("Switched to session #{}.", session_id))
                    .await;
            }
            Err(e) => {
                self.send_reply(thread, &format!("Failed to switch: {}", e))
                    .await;
            }
        }
    }

    async fn handle_stop(&mut self, thread: &ThreadId, target: StopTarget) {
        match target {
            StopTarget::Session(id) => {
                // Clean up any pending feedback for this session
                self.pending_feedback.remove(&id);
                match self.session_mgr.stop_session(id) {
                    Ok(()) => {
                        self.send_reply(thread, &format!("Session #{} stopped.", id))
                            .await;
                    }
                    Err(e) => {
                        self.send_reply(thread, &format!("Failed to stop: {}", e))
                            .await;
                    }
                }
            }
            StopTarget::All => {
                self.pending_feedback.clear();
                self.session_mgr.shutdown_all();
                self.send_reply(thread, "All sessions stopped.").await;
            }
        }
    }

    async fn handle_status(&mut self, thread: &ThreadId) {
        let sessions = self.session_mgr.list_sessions();
        let active_count = sessions.len();
        let pending_count = self.pending_feedback.len();
        let mut reply = format!(
            "*AgentSmith Status:*\nActive sessions: {}\nDefault tool: {}\nMax sessions: {}",
            active_count,
            self.config.session_defaults.default_tool,
            self.config.session_defaults.max_sessions,
        );
        if pending_count > 0 {
            reply.push_str(&format!("\nPending feedback requests: {}", pending_count));
        }
        if self.config.interaction_agent.enabled && self.config.interaction_agent.llm.resolved_api_key().is_some() {
            reply.push_str(&format!(
                "\nInteraction agent: enabled (provider: {})",
                self.config.interaction_agent.llm.provider
            ));
        } else {
            reply.push_str("\nInteraction agent: disabled");
        }
        self.send_reply(thread, &reply).await;
    }

    async fn handle_monitor(&mut self, thread: &ThreadId, action: MonitorAction) {
        match action {
            MonitorAction::Status => {
                let report = self.monitor.format_status();
                self.send_reply(thread, &report).await;
            }
            MonitorAction::Kill { tool } => {
                if tool == "openclaw" {
                    let result = self.monitor.kill_openclaw();
                    let mut reply = "*Kill OpenClaw:*".to_string();
                    if result.killed.is_empty() && result.failed.is_empty() {
                        reply.push_str("\nNo OpenClaw processes found.");
                    } else {
                        if !result.killed.is_empty() {
                            reply.push_str(&format!(
                                "\nKilled PIDs: {}",
                                result
                                    .killed
                                    .iter()
                                    .map(|p| p.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                        if !result.failed.is_empty() {
                            reply.push_str(&format!(
                                "\nFailed to kill PIDs: {}",
                                result
                                    .failed
                                    .iter()
                                    .map(|p| p.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                    }
                    self.send_reply(thread, &reply).await;
                } else {
                    self.send_reply(
                        thread,
                        &format!(
                            "Kill is only supported for `openclaw`. Got: '{}'",
                            tool
                        ),
                    )
                    .await;
                }
            }
        }
    }

    async fn handle_text(&mut self, thread: &ThreadId, text: &str) {
        let session_id = match self.session_mgr.active_session(thread) {
            Some(id) => id,
            None => {
                // Auto-create a session with the default tool
                let tool_name = self.config.session_defaults.default_tool.clone();
                let tool = SessionTool::from_str(&tool_name).unwrap_or(SessionTool::Claude);
                match self.session_mgr.create_session(tool, thread).await {
                    Ok(id) => {
                        self.send_reply(
                            thread,
                            &format!("Auto-created {} session #{}.", tool, id),
                        )
                        .await;
                        id
                    }
                    Err(e) => {
                        self.send_reply(
                            thread,
                            &format!("No active session and failed to auto-create: {}", e),
                        )
                        .await;
                        return;
                    }
                }
            }
        };

        // Check if this session has pending feedback — if so, treat the message
        // as a feedback response instead of normal session input.
        if let Some(pending) = self.pending_feedback.remove(&session_id) {
            self.handle_feedback_response(thread, session_id, &pending.request, text)
                .await;
            return;
        }

        if let Err(e) = self.session_mgr.send_input(session_id, text).await {
            self.send_reply(
                thread,
                &format!("Error sending to session #{}: {}", session_id, e),
            )
            .await;
        }
    }

    /// Handle a user's response to a pending feedback request.
    async fn handle_feedback_response(
        &mut self,
        thread: &ThreadId,
        session_id: SessionId,
        request: &FeedbackRequest,
        user_message: &str,
    ) {
        // Try to use the interaction agent to format the response
        let formatted = if let Some(ia) = self.session_mgr.interaction_agent_for(session_id) {
            let mut agent = ia.lock().await;
            let result = agent.format_response(&request.raw_context, user_message).await;
            agent.clear_awaiting_feedback();
            match result {
                Ok(formatted) => formatted,
                Err(e) => {
                    tracing::warn!("Failed to format response via LLM: {}, using raw input", e);
                    user_message.to_string()
                }
            }
        } else {
            user_message.to_string()
        };

        // Send the formatted response to the session's PTY
        match self.session_mgr.send_feedback_response(session_id, &formatted) {
            Ok(()) => {
                self.send_reply(
                    thread,
                    &format!("[Response sent to session #{}]", session_id),
                )
                .await;
            }
            Err(e) => {
                self.send_reply(
                    thread,
                    &format!("Error sending response to session #{}: {}", session_id, e),
                )
                .await;
            }
        }
    }

    /// Poll for feedback requests from interaction agents.
    async fn poll_feedback_requests(&mut self) {
        while let Some(request) = self.session_mgr.try_recv_feedback() {
            let session_id = request.session_id;
            let threads = self.session_mgr.threads_for_session(session_id);

            if threads.is_empty() {
                tracing::warn!(
                    "Feedback request for session #{} but no threads found",
                    session_id
                );
                continue;
            }

            // Mark session as awaiting input
            self.session_mgr.set_awaiting_input(session_id);

            // Format the feedback message for the user
            let message = format!(
                "[Session #{} needs your input ({})]\n{}\n\nReply with your answer.",
                session_id, request.input_type, request.question
            );

            // Send to all threads associated with this session
            for thread in &threads {
                self.send_reply(thread, &message).await;
            }

            // Store the pending feedback
            self.pending_feedback.insert(
                session_id,
                PendingFeedback {
                    request,
                    threads,
                    created_at: Instant::now(),
                },
            );
        }
    }

    /// Check for feedback requests that have timed out.
    async fn check_feedback_timeouts(&mut self) {
        let timed_out: Vec<SessionId> = self
            .pending_feedback
            .iter()
            .filter(|(_, pf)| pf.created_at.elapsed() >= self.feedback_timeout)
            .map(|(&id, _)| id)
            .collect();

        for session_id in timed_out {
            if let Some(pending) = self.pending_feedback.remove(&session_id) {
                tracing::info!(
                    "Feedback request for session #{} timed out after {:?}",
                    session_id,
                    self.feedback_timeout
                );

                // Notify the user
                for thread in &pending.threads {
                    self.send_reply(
                        thread,
                        &format!(
                            "[Session #{} feedback request timed out. Sending default denial.]",
                            session_id
                        ),
                    )
                    .await;
                }

                // Send a default denial/cancel to the PTY
                let _ = self.session_mgr.send_feedback_response(session_id, "n");

                // Clear interaction agent state
                if let Some(ia) = self.session_mgr.interaction_agent_for(session_id) {
                    ia.lock().await.clear_awaiting_feedback();
                }
            }
        }
    }

    /// Poll all sessions for output and route it back to the appropriate threads.
    async fn poll_session_output(&mut self) {
        while let Some((session_id, chunk)) = self.session_mgr.try_recv_output() {
            let threads = self.session_mgr.threads_for_session(session_id);
            for thread in threads {
                self.send_reply(&thread, &chunk).await;
            }
        }
    }

    /// Send a reply to a specific thread via the appropriate adapter.
    async fn send_reply(&self, thread: &ThreadId, text: &str) {
        let msg = OutgoingMessage {
            thread: thread.clone(),
            text: text.to_string(),
        };

        // Send to all outgoing channels (adapters will filter by platform)
        for tx in &self.outgoing_txs {
            let _ = tx.send(msg.clone()).await;
        }
    }

    fn all_active_threads(&self) -> Vec<ThreadId> {
        self.session_mgr
            .list_sessions()
            .iter()
            .flat_map(|s| self.session_mgr.threads_for_session(s.id))
            .collect()
    }
}
