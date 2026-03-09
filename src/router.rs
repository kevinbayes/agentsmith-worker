use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agent::interaction::FeedbackRequest;
use crate::agent::orchestrator::AgentResponse;
use crate::agent::{AgentManager, ThreadMode};
use crate::command::{self, Command, MonitorAction, ScheduleAction, StopTarget};
use crate::config::Config;
use crate::messaging::web::{AgentSnapshotItem, SessionSnapshotItem, StatusSnapshot};
use crate::messaging::{IncomingMessage, OutgoingMessage, Platform, ThreadId};
use crate::monitor::Monitor;
use crate::reporter::Reporter;
use crate::scheduler::Scheduler;
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
    /// Shared status snapshot for the web UI.
    status_snapshot: Option<Arc<RwLock<StatusSnapshot>>>,
    /// Instant the router started, for uptime tracking.
    started_at: Instant,
    /// Per-thread agent manager (None if agent mode is disabled/unconfigured).
    agent_mgr: Option<AgentManager>,
    /// Per-thread mode: Session (default) or Agent.
    thread_modes: HashMap<ThreadId, ThreadMode>,
    /// Shared scheduler instance (None if scheduling is disabled).
    scheduler: Option<Arc<RwLock<Scheduler>>>,
}

impl Router {
    pub fn new(
        config: Config,
        incoming_rx: mpsc::Receiver<IncomingMessage>,
        outgoing_txs: Vec<mpsc::Sender<OutgoingMessage>>,
        cancel: CancellationToken,
        status_snapshot: Option<Arc<RwLock<StatusSnapshot>>>,
        scheduler: Option<Arc<RwLock<Scheduler>>>,
    ) -> Self {
        let session_mgr = SessionManager::new(config.clone());
        let monitor = Monitor::from_config(&config);
        let reporter = Reporter::new(&config.reporter);
        let agent_mgr = AgentManager::try_new(&config.agent);
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
            status_snapshot,
            started_at: Instant::now(),
            agent_mgr,
            thread_modes: HashMap::new(),
            scheduler,
        }
    }

    /// Get the current mode for a thread. Defaults to Session unless
    /// auto_agent_mode is enabled and the agent is available.
    fn thread_mode(&self, thread: &ThreadId) -> ThreadMode {
        if let Some(&mode) = self.thread_modes.get(thread) {
            return mode;
        }
        // Default: check if auto_agent_mode is configured
        if let Some(ref mgr) = self.agent_mgr {
            if mgr.auto_agent_mode() {
                return ThreadMode::Agent;
            }
        }
        ThreadMode::Session
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
                    // Poll scheduled tasks
                    self.poll_scheduled_tasks().await;
                    self.poll_schedule_results().await;
                    // Update web UI status snapshot
                    if let Some(ref snapshot) = self.status_snapshot {
                        let sessions = self.session_mgr.list_sessions();
                        let tools = self.monitor.status();
                        let mut snap = snapshot.write().await;
                        snap.sessions = sessions
                            .iter()
                            .map(|s| SessionSnapshotItem {
                                id: s.id,
                                tool: s.tool.to_string(),
                                status: s.status.to_string(),
                            })
                            .collect();
                        snap.agents = tools
                            .iter()
                            .map(|t| AgentSnapshotItem {
                                name: t.name.clone(),
                                binary: t.binary.clone(),
                                installed: t.installed,
                                version: t.version.clone(),
                                running_count: t.running_instances.len(),
                            })
                            .collect();
                        snap.version = env!("CARGO_PKG_VERSION").to_string();
                        snap.uptime_secs = self.started_at.elapsed().as_secs();
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
            // New agent mode commands
            Command::Agent => {
                self.enter_agent_mode(&thread).await;
            }
            Command::Session | Command::Back => {
                self.enter_session_mode(&thread).await;
            }
            Command::Clear => {
                self.handle_clear(&thread).await;
            }
            // Existing slash commands work in both modes
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
            Command::Schedule { action } => {
                self.handle_schedule(&thread, action).await;
            }
            // Text routing depends on mode
            Command::Text(text) => {
                match self.thread_mode(&thread) {
                    ThreadMode::Session => {
                        self.handle_text(&thread, &text).await;
                    }
                    ThreadMode::Agent => {
                        self.handle_agent_text(&thread, &text).await;
                    }
                }
            }
        }
    }

    /// Enter agent mode for a thread.
    async fn enter_agent_mode(&mut self, thread: &ThreadId) {
        if self.agent_mgr.is_none() {
            self.send_reply(
                thread,
                "Agent mode is not available. Check that `[agent]` is configured with a valid LLM provider and API key.",
            )
            .await;
            return;
        }
        self.thread_modes.insert(thread.clone(), ThreadMode::Agent);
        self.send_reply(
            thread,
            "Entered *agent mode*. I'm your AI assistant. Ask me anything, or say \"delegate to claude\" to start a coding task.\nUse `/session` or `/back` to return to session mode.",
        )
        .await;
    }

    /// Enter session mode for a thread.
    async fn enter_session_mode(&mut self, thread: &ThreadId) {
        self.thread_modes.insert(thread.clone(), ThreadMode::Session);
        self.send_reply(
            thread,
            "Returned to *session mode*. Text now goes directly to your active CLI session.\nUse `/agent` to re-enter agent mode.",
        )
        .await;
    }

    /// Clear agent conversation context for a thread.
    async fn handle_clear(&mut self, thread: &ThreadId) {
        if let Some(ref mut mgr) = self.agent_mgr {
            mgr.clear_context(thread);
        }
        self.send_reply(thread, "Agent conversation context cleared.").await;
    }

    /// Handle text routed to the agent.
    async fn handle_agent_text(&mut self, thread: &ThreadId, text: &str) {
        let agent_mgr = match self.agent_mgr.as_mut() {
            Some(mgr) => mgr,
            None => {
                // Shouldn't happen since we check in enter_agent_mode, but be safe
                self.send_reply(thread, "Agent mode is not available.").await;
                return;
            }
        };

        let scheduler_clone = self.scheduler.clone();
        let response = agent_mgr
            .handle_message(thread, text, &mut self.session_mgr, &self.config, scheduler_clone)
            .await;

        match response {
            AgentResponse::Reply(text) => {
                self.send_reply(thread, &text).await;
            }
            AgentResponse::SwitchToSession {
                session_id,
                message,
            } => {
                self.thread_modes.insert(thread.clone(), ThreadMode::Session);
                self.session_mgr.switch_active(thread, session_id).ok();
                self.send_reply(
                    thread,
                    &format!(
                        "[Switched to session #{}. Use `/agent` to return.]",
                        session_id
                    ),
                )
                .await;
                if let Some(msg) = message {
                    if let Err(e) = self.session_mgr.send_input(session_id, &msg).await {
                        self.send_reply(
                            thread,
                            &format!("Error sending to session #{}: {}", session_id, e),
                        )
                        .await;
                    }
                }
            }
            AgentResponse::SessionCreated { session_id: _, info } => {
                self.send_reply(thread, &info).await;
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
        let mode = self.thread_mode(thread);
        let mut reply = format!(
            "*AgentSmith Status:*\nActive sessions: {}\nDefault tool: {}\nMax sessions: {}\nThread mode: {}",
            active_count,
            self.config.session_defaults.default_tool,
            self.config.session_defaults.max_sessions,
            match mode {
                ThreadMode::Session => "session",
                ThreadMode::Agent => "agent",
            },
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
        if self.agent_mgr.is_some() {
            reply.push_str(&format!(
                "\nAgent mode: available (provider: {})",
                self.config.agent.llm.provider
            ));
        } else {
            reply.push_str("\nAgent mode: not available");
        }
        if let Some(ref scheduler) = self.scheduler {
            let sched = scheduler.read().await;
            let summary = sched.summary();
            reply.push_str(&format!(
                "\nScheduler: {} schedules ({} active, {} running, {} paused)",
                summary.total, summary.active, summary.running, summary.paused
            ));
        } else if self.config.scheduler.enabled {
            reply.push_str("\nScheduler: enabled (no schedules)");
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

    /// Handle schedule management commands.
    async fn handle_schedule(&mut self, thread: &ThreadId, action: ScheduleAction) {
        // InstallTool doesn't need the scheduler running
        if let ScheduleAction::InstallTool { ref agent } = action {
            self.handle_install_tool(thread, agent).await;
            return;
        }

        let scheduler = match &self.scheduler {
            Some(s) => s.clone(),
            None => {
                self.send_reply(
                    thread,
                    "Scheduler is not enabled. Set `scheduler.enabled = true` in config.",
                )
                .await;
                return;
            }
        };

        match action {
            ScheduleAction::Add {
                cron_expr,
                tool,
                prompt,
            } => {
                let mut sched = scheduler.write().await;
                match sched.add_job(
                    &cron_expr,
                    &tool,
                    &prompt,
                    None,
                    &thread.platform.to_string(),
                    &thread.id,
                ) {
                    Ok(job) => {
                        let next = job
                            .next_run
                            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "N/A".to_string());
                        self.send_reply(
                            thread,
                            &format!(
                                "Schedule #{} created: `{}` runs `{}` with `{}`\nCron: `{}`\nNext run: {}",
                                job.id, job.name, tool, prompt, job.cron_expr, next
                            ),
                        )
                        .await;
                    }
                    Err(e) => {
                        self.send_reply(thread, &format!("Failed to create schedule: {}", e))
                            .await;
                    }
                }
            }
            ScheduleAction::List => {
                let sched = scheduler.read().await;
                let jobs = sched.list_jobs();
                if jobs.is_empty() {
                    self.send_reply(thread, "No scheduled tasks. Use `/schedule add` to create one.")
                        .await;
                    return;
                }
                let mut lines = vec!["*Scheduled Tasks:*".to_string()];
                for job in jobs {
                    let next = job
                        .next_run
                        .map(|t| t.format("%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "-".to_string());
                    lines.push(format!(
                        "  #{} `{}` [{}] {} → {} (next: {})",
                        job.id, job.name, job.status, job.tool, short_prompt(&job.prompt), next
                    ));
                }
                self.send_reply(thread, &lines.join("\n")).await;
            }
            ScheduleAction::Delete { id } => {
                let mut sched = scheduler.write().await;
                match sched.delete_job(id) {
                    Ok(()) => {
                        self.send_reply(thread, &format!("Schedule #{} deleted.", id))
                            .await;
                    }
                    Err(e) => {
                        self.send_reply(thread, &format!("Failed: {}", e)).await;
                    }
                }
            }
            ScheduleAction::Pause { id } => {
                let mut sched = scheduler.write().await;
                match sched.pause_job(id) {
                    Ok(()) => {
                        self.send_reply(thread, &format!("Schedule #{} paused.", id))
                            .await;
                    }
                    Err(e) => {
                        self.send_reply(thread, &format!("Failed: {}", e)).await;
                    }
                }
            }
            ScheduleAction::Resume { id } => {
                let mut sched = scheduler.write().await;
                match sched.resume_job(id) {
                    Ok(()) => {
                        self.send_reply(thread, &format!("Schedule #{} resumed.", id))
                            .await;
                    }
                    Err(e) => {
                        self.send_reply(thread, &format!("Failed: {}", e)).await;
                    }
                }
            }
            ScheduleAction::Run { id } => {
                let mut sched = scheduler.write().await;
                match sched.trigger_now(id) {
                    Ok(()) => {
                        self.send_reply(
                            thread,
                            &format!("Schedule #{} triggered. It will run on the next poll cycle.", id),
                        )
                        .await;
                    }
                    Err(e) => {
                        self.send_reply(thread, &format!("Failed: {}", e)).await;
                    }
                }
            }
            ScheduleAction::Info { id } => {
                let sched = scheduler.read().await;
                match sched.get_job(id) {
                    Some(job) => {
                        let next = job
                            .next_run
                            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "N/A".to_string());
                        let last = job
                            .last_run
                            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "never".to_string());
                        let last_result = job
                            .last_result
                            .as_ref()
                            .map(|r| {
                                format!(
                                    "{} ({}s)",
                                    if r.success { "success" } else { "failed" },
                                    r.duration_secs
                                )
                            })
                            .unwrap_or_else(|| "N/A".to_string());
                        self.send_reply(
                            thread,
                            &format!(
                                "*Schedule #{}:*\nName: `{}`\nStatus: {}\nTool: {}\nPrompt: {}\nCron: `{}`\nNext run: {}\nLast run: {}\nLast result: {}\nRun count: {}\nConsecutive failures: {}",
                                job.id, job.name, job.status, job.tool, job.prompt,
                                job.cron_expr, next, last, last_result, job.run_count, job.consecutive_failures
                            ),
                        )
                        .await;
                    }
                    None => {
                        self.send_reply(thread, &format!("Schedule #{} not found.", id))
                            .await;
                    }
                }
            }
            ScheduleAction::InstallTool { .. } => {
                // Handled above before scheduler check — unreachable
            }
        }
    }

    /// Poll for scheduled tasks that are due and spawn executions.
    async fn poll_scheduled_tasks(&mut self) {
        let scheduler = match &self.scheduler {
            Some(s) => s.clone(),
            None => return,
        };

        let mut sched = scheduler.write().await;
        let due = sched.collect_due_jobs();

        for id in due {
            if !sched.can_execute() {
                tracing::debug!("Max concurrent executions reached, deferring schedule #{}", id);
                break;
            }

            let job = match sched.get_job(id) {
                Some(j) => j.clone(),
                None => continue,
            };

            let config = self.config.clone();
            let working_dir = self.config.daemon.working_dir.clone();
            let timeout = sched.execution_timeout_secs();

            let handle = tokio::spawn(async move {
                crate::scheduler::executor::execute_scheduled_job(
                    &job.tool,
                    &job.prompt,
                    &config,
                    &working_dir,
                    timeout,
                )
                .await
            });

            sched.register_execution(id, handle);
            tracing::info!("Scheduled job #{} '{}' started execution", id, job.name);
        }
    }

    /// Poll for completed schedule executions, deliver output, and archive.
    async fn poll_schedule_results(&mut self) {
        let scheduler = match &self.scheduler {
            Some(s) => s.clone(),
            None => return,
        };

        let finished = {
            let mut sched = scheduler.write().await;
            sched.collect_finished()
        };

        for (id, result) in finished {
            let job = {
                let sched = scheduler.read().await;
                sched.get_job(id).cloned()
            };

            if let Some(job) = &job {
                // Deliver output to origin thread
                let thread = ThreadId {
                    platform: match job.origin.platform.as_str() {
                        "Signal" => Platform::Signal,
                        "Slack" => Platform::Slack,
                        "Telegram" => Platform::Telegram,
                        "Web" => Platform::Web,
                        _ => Platform::Web,
                    },
                    id: job.origin.thread_id.clone(),
                };

                let status_str = if result.success { "completed" } else { "failed" };
                let output_preview = short_output(&result.output, 500);
                self.send_reply(
                    &thread,
                    &format!(
                        "[Schedule #{} '{}' {} ({}s)]\n{}",
                        job.id, job.name, status_str, result.duration_secs, output_preview
                    ),
                )
                .await;

                // Archive the result
                let data_dir = self.config.daemon.data_dir.clone();
                let gcs_config = self.config.scheduler.gcs.clone();
                let job_clone = job.clone();
                let result_clone = result.clone();
                tokio::spawn(async move {
                    crate::scheduler::output::archive_result(
                        &job_clone,
                        &result_clone,
                        &data_dir,
                        gcs_config.as_ref(),
                    )
                    .await;
                });
            }

            // Record completion and check for auto-pause
            let auto_paused_info = {
                let mut sched = scheduler.write().await;
                sched.record_completion(id, &result);

                // Check if auto-paused
                sched.get_job(id).and_then(|j| {
                    if j.status == crate::scheduler::ScheduleStatus::Paused
                        && j.consecutive_failures >= self.config.scheduler.max_consecutive_failures
                    {
                        Some((j.id, j.name.clone(), j.consecutive_failures, j.origin.clone()))
                    } else {
                        None
                    }
                })
            };

            // Send auto-pause notification outside the lock
            if let Some((job_id, name, failures, origin)) = auto_paused_info {
                let thread = thread_from_origin(&origin);
                self.send_reply(
                    &thread,
                    &format!(
                        "Schedule #{} '{}' has been auto-paused after {} consecutive failures. Use `/schedule resume {}` to re-enable.",
                        job_id, name, failures, job_id
                    ),
                )
                .await;
            }
        }
    }

    /// Handle `/schedule install-tool <agent>` — configure an AI CLI to use the MCP server.
    async fn handle_install_tool(&mut self, thread: &ThreadId, agent: &str) {
        let settings_path = match agent {
            "claude" => {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".claude.json")
            }
            "gemini" => {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".gemini").join("settings.json")
            }
            _ => {
                self.send_reply(
                    thread,
                    &format!(
                        "Unsupported agent '{}'. Supported: claude, gemini",
                        agent
                    ),
                )
                .await;
                return;
            }
        };

        let script_path = match find_mcp_script() {
            Some(p) => p,
            None => {
                self.send_reply(
                    thread,
                    "Could not locate `tools/agentsmith-scheduler-mcp.py`. \
                     Ensure it exists next to the AgentSmith binary or in the working directory.",
                )
                .await;
                return;
            }
        };

        let api_url = format!("http://{}:{}", self.config.web.host, self.config.web.port);

        // Build MCP server config entry
        let mcp_config = serde_json::json!({
            "type": "stdio",
            "command": "python3",
            "args": [script_path.to_string_lossy()],
            "env": {
                "AGENTSMITH_URL": api_url,
            }
        });

        // Read existing settings or start with empty object
        let mut settings: serde_json::Value = match std::fs::read_to_string(&settings_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        };

        // Ensure mcpServers key exists
        if settings.get("mcpServers").is_none() {
            settings["mcpServers"] = serde_json::json!({});
        }
        settings["mcpServers"]["agentsmith-scheduler"] = mcp_config;

        // Write back with pretty-print, creating parent dirs if needed
        if let Some(parent) = settings_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.send_reply(
                    thread,
                    &format!("Failed to create directory {}: {}", parent.display(), e),
                )
                .await;
                return;
            }
        }

        match serde_json::to_string_pretty(&settings) {
            Ok(content) => {
                if let Err(e) = std::fs::write(&settings_path, &content) {
                    self.send_reply(
                        thread,
                        &format!("Failed to write settings: {}", e),
                    )
                    .await;
                    return;
                }
            }
            Err(e) => {
                self.send_reply(
                    thread,
                    &format!("Failed to serialize settings: {}", e),
                )
                .await;
                return;
            }
        }

        self.send_reply(
            thread,
            &format!(
                "MCP server installed for *{}*.\n\
                 Settings: `{}`\n\
                 Script: `{}`\n\
                 API URL: `{}`\n\n\
                 Restart {} to pick up the new MCP server.",
                agent,
                settings_path.display(),
                script_path.display(),
                api_url,
                match agent {
                    "claude" => "Claude Code",
                    "gemini" => "Gemini CLI",
                    _ => agent,
                },
            ),
        )
        .await;
    }

    fn all_active_threads(&self) -> Vec<ThreadId> {
        self.session_mgr
            .list_sessions()
            .iter()
            .flat_map(|s| self.session_mgr.threads_for_session(s.id))
            .collect()
    }
}

/// Truncate a prompt string for display in listings.
fn short_prompt(s: &str) -> String {
    if s.len() <= 40 {
        s.to_string()
    } else {
        format!("{}...", &s[..37])
    }
}

/// Truncate output for inline display.
fn short_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

/// Convert a PersistedThreadId to a ThreadId.
fn thread_from_origin(origin: &crate::scheduler::PersistedThreadId) -> ThreadId {
    ThreadId {
        platform: match origin.platform.as_str() {
            "Signal" => Platform::Signal,
            "Slack" => Platform::Slack,
            "Telegram" => Platform::Telegram,
            "Web" => Platform::Web,
            _ => Platform::Web,
        },
        id: origin.thread_id.clone(),
    }
}

/// Locate the MCP server script, checking several candidate paths.
fn find_mcp_script() -> Option<PathBuf> {
    let script_name = "tools/agentsmith-scheduler-mcp.py";

    // Next to the current executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(script_name);
            if candidate.is_file() {
                return Some(candidate);
            }
            // One level up (handles target/debug layout)
            let candidate = exe_dir.join("..").join(script_name);
            if candidate.is_file() {
                return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
            }
        }
    }

    // Current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join(script_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}
