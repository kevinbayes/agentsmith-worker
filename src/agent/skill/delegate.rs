use crate::session::SessionTool;

use super::{SkillContext, SkillOutput};

/// Delegate a task to a CLI tool session. Creates or reuses a session
/// and either hands off control (user interacts directly) or runs autonomously.
pub struct DelegateSkill;

impl DelegateSkill {
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        // Use LLM to extract the tool and task from natural language input
        let system = r#"You are a task parser. Given the user's delegation request, extract:
1. The tool to use: "claude" or "zeroclaw". Default to "claude" if not specified.
2. The task to send to the tool.

Respond with ONLY a JSON object (no markdown fencing):
{"tool": "<tool_name>", "task": "<task_description>"}"#;

        let parsed = ctx.llm.generate(system, input).await?;

        // Parse the JSON response
        let (tool_name, task) = parse_delegation(&parsed).unwrap_or_else(|| {
            // Fallback: default to claude with the original input as the task
            ("claude".to_string(), input.to_string())
        });

        let tool = SessionTool::from_str(&tool_name).unwrap_or(SessionTool::Claude);

        // Check for an existing session or create a new one
        let session_id = if let Some(id) = ctx.session_mgr.active_session(ctx.thread) {
            // Check if the active session matches the requested tool
            let sessions = ctx.session_mgr.list_sessions();
            let matches = sessions.iter().any(|s| s.id == id && s.tool == tool);
            if matches {
                id
            } else {
                // Create a new session with the requested tool
                ctx.session_mgr.create_session(tool, ctx.thread).await?
            }
        } else {
            ctx.session_mgr.create_session(tool, ctx.thread).await?
        };

        // Hand off: switch thread to session mode with the initial message
        Ok(SkillOutput::HandOff {
            session_id,
            initial_message: task,
        })
    }
}

/// Parse the LLM's JSON response for tool and task.
fn parse_delegation(response: &str) -> Option<(String, String)> {
    // Try to find JSON in the response (handle potential markdown wrapping)
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            return None;
        }
    } else {
        return None;
    };

    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let tool = value.get("tool")?.as_str()?.to_string();
    let task = value.get("task")?.as_str()?.to_string();
    Some((tool, task))
}
