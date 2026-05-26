use crate::session::SessionId;

use super::context::Role;
use super::skill::chat::ChatSkill;
use super::skill::delegate::DelegateSkill;
use super::skill::schedule::ScheduleSkill;
use super::skill::status::StatusSkill;
use super::skill::summarize::SummarizeSkill;
use super::skill::toolconfig::ToolConfigSkill;
use super::skill::{skills_description, Skill, SkillContext, SkillOutput};
use super::AgentInstance;

/// Result of agent message processing, consumed by the Router.
pub enum AgentResponse {
    /// Send text reply to user.
    Reply(String),
    /// Switch thread to session mode and forward message to the session.
    SwitchToSession {
        session_id: SessionId,
        message: Option<String>,
    },
    /// Agent created a session for delegation; output will stream.
    SessionCreated {
        session_id: SessionId,
        info: String,
    },
}

/// Process a user message through the hybrid orchestrator.
///
/// Flow:
/// 1. Add user message to context
/// 2. Rule-based check (fast path)
/// 3. LLM reasoning (fallback)
/// 4. Execute selected skill
/// 5. Add result to context
/// 6. Return response
pub async fn process_message(
    agent: &mut AgentInstance,
    text: &str,
    ctx: &mut SkillContext<'_>,
) -> anyhow::Result<AgentResponse> {
    // 1. Add user message to context
    agent.context.push(Role::User, text.to_string());

    // 2. Rule-based fast path
    let skill = match rule_match(text) {
        Some(s) => s,
        None => {
            // 3. LLM reasoning fallback
            llm_select_skill(text, &agent.context, ctx).await?
        }
    };

    tracing::debug!("Agent selected skill: {}", skill.name());

    // 4. Execute skill
    let output = skill.execute(text, ctx).await?;

    // 5. Convert skill output to agent response and record in context
    match output {
        SkillOutput::Reply(reply) => {
            agent.context.push(Role::Agent, reply.clone());
            Ok(AgentResponse::Reply(reply))
        }
        SkillOutput::HandOff {
            session_id,
            initial_message,
        } => {
            let msg = format!(
                "Delegating to session #{}. Switching to session mode.",
                session_id
            );
            agent.context.push(Role::System, msg.clone());
            Ok(AgentResponse::SwitchToSession {
                session_id,
                message: Some(initial_message),
            })
        }
        SkillOutput::Delegated {
            session_id,
            task_summary,
        } => {
            let info = format!(
                "Task delegated to session #{}: {}",
                session_id, task_summary
            );
            agent.context.push(Role::System, info.clone());
            Ok(AgentResponse::SessionCreated { session_id, info })
        }
        SkillOutput::Continue(info) => {
            agent.context.push(Role::System, info.clone());
            // For now, just reply with the continuation info
            Ok(AgentResponse::Reply(info))
        }
    }
}

/// Rule-based fast path for common intents.
fn rule_match(text: &str) -> Option<Skill> {
    let lower = text.to_lowercase().trim().to_string();

    // Status patterns
    if lower == "status"
        || lower == "what's running"
        || lower == "what is running"
        || lower == "sessions"
        || lower == "list sessions"
        || lower.starts_with("show sessions")
        || lower.starts_with("show status")
    {
        return Some(Skill::Status(StatusSkill));
    }

    // Session management
    if lower.starts_with("create a session")
        || lower.starts_with("start a session")
        || lower.starts_with("new session")
        || lower.starts_with("create session")
        || lower.starts_with("start session")
    {
        return Some(Skill::Status(StatusSkill));
    }

    if lower.starts_with("stop session")
        || lower.starts_with("kill session")
        || lower == "stop all"
        || lower == "stop all sessions"
    {
        return Some(Skill::Status(StatusSkill));
    }

    // Summarize patterns
    if lower == "summarize"
        || lower == "summary"
        || lower == "what happened"
        || lower == "what's happened"
        || lower.starts_with("summarize session")
        || lower.starts_with("summarize #")
    {
        return Some(Skill::Summarize(SummarizeSkill));
    }

    // Explicit delegation patterns
    if lower.starts_with("ask claude")
        || lower.starts_with("ask hermes")
        || lower.starts_with("ask zeroclaw")
        || lower.starts_with("use claude")
        || lower.starts_with("use hermes")
        || lower.starts_with("use zeroclaw")
        || lower.starts_with("delegate to")
        || lower.starts_with("delegate ")
        || lower.starts_with("send to claude")
        || lower.starts_with("send to hermes")
        || lower.starts_with("send to zeroclaw")
    {
        return Some(Skill::Delegate(DelegateSkill));
    }

    // Schedule patterns
    if lower.starts_with("schedule ")
        || lower.starts_with("every morning")
        || lower.starts_with("every evening")
        || lower.starts_with("every day")
        || lower.starts_with("every hour")
        || lower.starts_with("every week")
        || lower.starts_with("twice a day")
        || lower.starts_with("daily ")
        || lower.starts_with("list schedule")
        || lower.starts_with("list my schedule")
        || lower.starts_with("show schedule")
        || lower.starts_with("delete schedule")
        || lower.starts_with("remove schedule")
        || lower.starts_with("pause schedule")
        || lower.starts_with("resume schedule")
        || lower.starts_with("run schedule")
        || lower == "schedules"
        || lower == "my schedules"
        || lower == "list schedules"
    {
        return Some(Skill::Schedule(ScheduleSkill));
    }

    // Tool config patterns
    if lower.contains("mcp server")
        || lower.contains("mcp servers")
        || lower.starts_with("install mcp")
        || lower.starts_with("list mcp")
        || lower.starts_with("remove mcp")
        || lower.contains("claude settings")
        || lower.contains("claude permissions")
        || lower.starts_with("show settings")
        || lower.starts_with("show permissions")
    {
        return Some(Skill::ToolConfig(ToolConfigSkill));
    }

    None
}

/// Use LLM to determine which skill to invoke.
async fn llm_select_skill(
    text: &str,
    context: &super::context::ConversationContext,
    ctx: &mut SkillContext<'_>,
) -> anyhow::Result<Skill> {
    let skills_desc = skills_description();
    let history = context.to_prompt();

    let system = format!(
        "You are the AgentSmith orchestrator. Your job is to select the most appropriate skill \
         to handle the user's message.\n\n\
         {}\n\
         Conversation history:\n{}\n\n\
         Respond with ONLY a JSON object (no markdown fencing):\n\
         {{\"skill\": \"chat|delegate|summarize|status|toolconfig|schedule\", \"input\": \"<refined input for the skill>\", \"reasoning\": \"<brief explanation>\"}}",
        skills_desc, history
    );

    let response = ctx.llm.generate(&system, text).await?;

    // Parse LLM response
    if let Some(skill) = parse_skill_selection(&response, text) {
        Ok(skill)
    } else {
        // Default to chat if we can't parse the response
        tracing::warn!("Could not parse skill selection from LLM, defaulting to chat");
        Ok(Skill::Chat(ChatSkill))
    }
}

fn parse_skill_selection(response: &str, original_input: &str) -> Option<Skill> {
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
    let skill_name = value.get("skill")?.as_str()?;
    let _input = value
        .get("input")
        .and_then(|v| v.as_str())
        .unwrap_or(original_input);

    match skill_name {
        "chat" => Some(Skill::Chat(ChatSkill)),
        "delegate" => Some(Skill::Delegate(DelegateSkill)),
        "summarize" => Some(Skill::Summarize(SummarizeSkill)),
        "status" => Some(Skill::Status(StatusSkill)),
        "toolconfig" => Some(Skill::ToolConfig(ToolConfigSkill)),
        "schedule" => Some(Skill::Schedule(ScheduleSkill)),
        _ => None,
    }
}
