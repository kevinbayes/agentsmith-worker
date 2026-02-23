use super::{SkillContext, SkillOutput};

/// Direct LLM conversation skill. Handles general chat, brainstorming,
/// and any task that doesn't require a CLI tool session.
pub struct ChatSkill;

impl ChatSkill {
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        let system = "You are a helpful AI assistant integrated into AgentSmith, a remote worker \
            daemon that bridges messaging platforms (Signal, Slack, Telegram, Web) with AI CLI tools \
            (Claude Code, Gemini CLI, Goose, ZeroClaw). \
            You can help with general questions, brainstorming, planning, and conversation. \
            If the user needs a coding task done, suggest they delegate to a CLI tool session. \
            Keep responses concise and useful.";

        let response = ctx.llm.generate(system, input).await?;
        Ok(SkillOutput::Reply(response))
    }
}
