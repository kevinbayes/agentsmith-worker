use super::{SkillContext, SkillOutput};

/// Summarize the recent output from an active CLI session.
pub struct SummarizeSkill;

impl SummarizeSkill {
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        // Determine which session to summarize
        let session_id = if let Some(id) = parse_session_id(input) {
            id
        } else if let Some(id) = ctx.session_mgr.active_session(ctx.thread) {
            id
        } else {
            return Ok(SkillOutput::Reply(
                "No active session to summarize. Use `/new` to create one.".to_string(),
            ));
        };

        // Get session info
        let sessions = ctx.session_mgr.list_sessions();
        let session_info = sessions.iter().find(|s| s.id == session_id);

        let info_str = match session_info {
            Some(info) => format!(
                "Session #{} ({}, status: {})",
                info.id, info.tool, info.status
            ),
            None => {
                return Ok(SkillOutput::Reply(format!(
                    "Session #{} not found.",
                    session_id
                )));
            }
        };

        // Since we don't have direct access to the output buffer history from here,
        // we report what we know about the session state.
        let reply = format!(
            "**{}**\n\nThe session is currently {}. \
             To see live output, switch to session mode with `/session` or `/back`.",
            info_str,
            session_info.map(|s| s.status.to_string()).unwrap_or_default()
        );

        Ok(SkillOutput::Reply(reply))
    }
}

/// Try to extract a session ID from user input (e.g. "summarize session 3" or "summarize #3").
fn parse_session_id(input: &str) -> Option<u64> {
    let lower = input.to_lowercase();
    // Try patterns like "#3", "session 3", "3"
    for word in lower.split_whitespace() {
        let stripped = word.trim_start_matches('#');
        if let Ok(id) = stripped.parse::<u64>() {
            return Some(id);
        }
    }
    None
}
