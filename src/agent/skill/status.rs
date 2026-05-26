use crate::session::SessionTool;

use super::{SkillContext, SkillOutput};

/// System status and session management skill.
pub struct StatusSkill;

impl StatusSkill {
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        let lower = input.to_lowercase();

        // Session management commands
        if lower.contains("create") || lower.contains("start") || lower.contains("new") {
            return self.handle_create(input, ctx).await;
        }
        if lower.contains("stop") || lower.contains("kill") {
            return self.handle_stop(input, ctx).await;
        }

        // Default: show status
        self.handle_status(ctx).await
    }

    async fn handle_status(&self, ctx: &mut SkillContext<'_>) -> anyhow::Result<SkillOutput> {
        let sessions = ctx.session_mgr.list_sessions();
        let active_id = ctx.session_mgr.active_session(ctx.thread);

        let mut lines = vec!["**AgentSmith Status**".to_string()];

        if sessions.is_empty() {
            lines.push("No active sessions.".to_string());
        } else {
            lines.push(format!("Active sessions: {}", sessions.len()));
            for s in &sessions {
                let marker = if active_id == Some(s.id) {
                    " (active)"
                } else {
                    ""
                };
                lines.push(format!("  #{} - {} [{}]{}", s.id, s.tool, s.status, marker));
            }
        }

        lines.push(format!(
            "Default tool: {}",
            ctx.config.session_defaults.default_tool
        ));
        lines.push(format!(
            "Max sessions: {}",
            ctx.config.session_defaults.max_sessions
        ));

        Ok(SkillOutput::Reply(lines.join("\n")))
    }

    async fn handle_create(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        // Try to extract tool name from input
        let lower = input.to_lowercase();
        let tool = if lower.contains("zeroclaw") {
            SessionTool::Zeroclaw
        } else if lower.contains("hermes") {
            SessionTool::Hermes
        } else {
            // Default to configured default or Claude
            SessionTool::from_str(&ctx.config.session_defaults.default_tool)
                .unwrap_or(SessionTool::Claude)
        };

        match ctx.session_mgr.create_session(tool, ctx.thread).await {
            Ok(id) => Ok(SkillOutput::Reply(format!(
                "Created {} session #{}. It is now your active session.",
                tool, id
            ))),
            Err(e) => Ok(SkillOutput::Reply(format!(
                "Failed to create session: {}",
                e
            ))),
        }
    }

    async fn handle_stop(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        // Try to extract session ID
        let lower = input.to_lowercase();

        if lower.contains("all") {
            ctx.session_mgr.shutdown_all();
            return Ok(SkillOutput::Reply("All sessions stopped.".to_string()));
        }

        // Try to find a session ID in the input
        for word in lower.split_whitespace() {
            let stripped = word.trim_start_matches('#');
            if let Ok(id) = stripped.parse::<u64>() {
                return match ctx.session_mgr.stop_session(id) {
                    Ok(()) => Ok(SkillOutput::Reply(format!("Session #{} stopped.", id))),
                    Err(e) => Ok(SkillOutput::Reply(format!("Failed to stop: {}", e))),
                };
            }
        }

        // No ID found, stop the active session
        if let Some(id) = ctx.session_mgr.active_session(ctx.thread) {
            match ctx.session_mgr.stop_session(id) {
                Ok(()) => Ok(SkillOutput::Reply(format!(
                    "Stopped active session #{}.",
                    id
                ))),
                Err(e) => Ok(SkillOutput::Reply(format!("Failed to stop: {}", e))),
            }
        } else {
            Ok(SkillOutput::Reply(
                "No active session to stop.".to_string(),
            ))
        }
    }
}
