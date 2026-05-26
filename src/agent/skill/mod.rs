pub mod chat;
pub mod delegate;
pub mod schedule;
pub mod status;
pub mod summarize;
pub mod toolconfig;

use std::sync::Arc;

use crate::agent::SimpleAgent;
use crate::config::Config;
use crate::messaging::ThreadId;
use crate::scheduler::Scheduler;
use crate::session::{SessionId, SessionManager};

use self::chat::ChatSkill;
use self::delegate::DelegateSkill;
use self::schedule::ScheduleSkill;
use self::status::StatusSkill;
use self::summarize::SummarizeSkill;
use self::toolconfig::ToolConfigSkill;

/// All available skills the agent can invoke.
pub enum Skill {
    Chat(ChatSkill),
    Delegate(DelegateSkill),
    Summarize(SummarizeSkill),
    Status(StatusSkill),
    ToolConfig(ToolConfigSkill),
    Schedule(ScheduleSkill),
}

/// Result of a skill execution.
pub enum SkillOutput {
    /// Direct text reply to user.
    Reply(String),
    /// Delegate to session and switch thread to session mode.
    HandOff {
        session_id: SessionId,
        initial_message: String,
    },
    /// Session created for autonomous delegation; output will stream.
    Delegated {
        session_id: SessionId,
        task_summary: String,
    },
    /// No final result yet; agent should continue reasoning with this info.
    Continue(String),
}

/// Shared context passed to skills during execution.
pub struct SkillContext<'a> {
    pub session_mgr: &'a mut SessionManager,
    pub config: &'a Config,
    pub thread: &'a ThreadId,
    pub llm: &'a SimpleAgent,
    pub scheduler: Option<Arc<tokio::sync::RwLock<Scheduler>>>,
}

impl Skill {
    /// Human-readable name of this skill.
    pub fn name(&self) -> &str {
        match self {
            Skill::Chat(_) => "chat",
            Skill::Delegate(_) => "delegate",
            Skill::Summarize(_) => "summarize",
            Skill::Status(_) => "status",
            Skill::ToolConfig(_) => "toolconfig",
            Skill::Schedule(_) => "schedule",
        }
    }

    /// Description for inclusion in LLM prompts.
    pub fn description(&self) -> &str {
        match self {
            Skill::Chat(_) => {
                "Direct conversation with the user. Use for general questions, brainstorming, \
                 planning, or any task that doesn't require a CLI tool."
            }
            Skill::Delegate(_) => {
                "Delegate a task to a CLI tool session (Claude Code, Hermes, ZeroClaw). \
                 Creates or reuses a session and either hands off control or runs autonomously."
            }
            Skill::Summarize(_) => {
                "Summarize the recent output from an active CLI session. Useful when the user \
                 wants to know what a session has been doing."
            }
            Skill::Status(_) => {
                "Show system status including active sessions, their tools and statuses, \
                 pending feedback, and system info. Also handles session management \
                 (create, stop, list, switch)."
            }
            Skill::ToolConfig(_) => {
                "Manage MCP servers, custom commands, and permissions for CLI tools \
                 (Claude Code). Install, remove, list, enable/disable configurations."
            }
            Skill::Schedule(_) => {
                "Manage scheduled/recurring tasks (cron jobs). Schedule prompts to run on a cron \
                 against a specific AI tool (Claude, Hermes, ZeroClaw). Supports add, \
                 list, delete, pause, resume, and run operations."
            }
        }
    }

    /// Execute the skill with the given input and context.
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        match self {
            Skill::Chat(s) => s.execute(input, ctx).await,
            Skill::Delegate(s) => s.execute(input, ctx).await,
            Skill::Summarize(s) => s.execute(input, ctx).await,
            Skill::Status(s) => s.execute(input, ctx).await,
            Skill::ToolConfig(s) => s.execute(input, ctx).await,
            Skill::Schedule(s) => s.execute(input, ctx).await,
        }
    }
}

/// Build the skills description block for inclusion in LLM system prompts.
pub fn skills_description() -> String {
    let skills: Vec<(&str, &str)> = vec![
        ("chat", "Direct conversation with the user. Use for general questions, brainstorming, planning, or any task that doesn't require a CLI tool."),
        ("delegate", "Delegate a task to a CLI tool session (Claude Code, Hermes, ZeroClaw). Input should describe the task to delegate and optionally the tool to use (e.g. 'use claude to refactor auth module')."),
        ("summarize", "Summarize the recent output from an active CLI session. Input can optionally specify a session ID."),
        ("status", "Show system status: active sessions, tools, pending feedback. Also handles session management commands like create, stop, list."),
        ("toolconfig", "Manage MCP servers, custom commands, and permissions for CLI tools. Input describes what to install, remove, list, or configure."),
        ("schedule", "Manage scheduled/recurring tasks (cron jobs). Schedule prompts to run on a cron against an AI tool. Supports: add a schedule, list schedules, delete/pause/resume/run a schedule. Examples: 'schedule claude to check the weather every morning', 'list my schedules', 'delete schedule 3'."),
    ];

    let mut out = String::from("Available skills:\n");
    for (name, desc) in skills {
        out.push_str(&format!("- {}: {}\n", name, desc));
    }
    out
}
