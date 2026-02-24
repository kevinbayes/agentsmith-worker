pub mod context;
pub mod interaction;
pub mod orchestrator;
pub mod skill;

use std::collections::HashMap;
use std::sync::Arc;

use crate::config::{AgentConfig, Config};
use crate::llm::LlmProvider;
use crate::messaging::ThreadId;
use crate::scheduler::Scheduler;
use crate::session::SessionManager;

use self::context::ConversationContext;
use self::orchestrator::{process_message, AgentResponse};
use self::skill::SkillContext;

/// A lightweight agent wrapping an LLM provider with a name.
pub struct SimpleAgent {
    name: String,
    llm: LlmProvider,
}

impl SimpleAgent {
    pub fn new(name: String, llm: LlmProvider) -> Self {
        Self { name, llm }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn generate(&self, system: &str, user: &str) -> anyhow::Result<String> {
        self.llm.generate(system, user).await
    }
}

/// Thread routing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadMode {
    /// Direct passthrough to CLI session (default).
    Session,
    /// Routed to per-thread agent instance.
    Agent,
}

/// Per-thread agent instance with its own conversation context.
pub struct AgentInstance {
    pub thread: ThreadId,
    pub context: ConversationContext,
}

/// Manages per-thread agent instances.
pub struct AgentManager {
    agents: HashMap<ThreadId, AgentInstance>,
    config: AgentConfig,
    llm: Arc<SimpleAgent>,
}

impl AgentManager {
    /// Create a new AgentManager. Returns None if the agent LLM cannot be initialized.
    pub fn try_new(config: &AgentConfig) -> Option<Self> {
        if !config.enabled {
            tracing::info!("Agent mode disabled in config");
            return None;
        }

        // Use a higher max_tokens default for the agent LLM than the interaction agent
        let mut llm_config = config.llm.clone();
        if llm_config.max_tokens == 256 {
            llm_config.max_tokens = 1024;
        }
        // Default to a more capable model for the agent
        if llm_config.model.is_none() {
            llm_config.model = Some("claude-sonnet-4-5-20250929".to_string());
        }

        let provider = match LlmProvider::from_config(&llm_config) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Agent mode disabled: failed to create LLM provider: {}", e);
                return None;
            }
        };

        let llm = Arc::new(SimpleAgent::new("agent".to_string(), provider));

        Some(Self {
            agents: HashMap::new(),
            config: config.clone(),
            llm,
        })
    }

    /// Get or create an agent instance for a thread.
    pub fn get_or_create(&mut self, thread: &ThreadId) -> &mut AgentInstance {
        if !self.agents.contains_key(thread) {
            let instance = AgentInstance {
                thread: thread.clone(),
                context: ConversationContext::new(self.config.max_context_turns),
            };
            self.agents.insert(thread.clone(), instance);
        }
        self.agents.get_mut(thread).unwrap()
    }

    /// Handle a user message in agent mode.
    pub async fn handle_message(
        &mut self,
        thread: &ThreadId,
        text: &str,
        session_mgr: &mut SessionManager,
        config: &Config,
        scheduler: Option<Arc<tokio::sync::RwLock<Scheduler>>>,
    ) -> AgentResponse {
        let llm = Arc::clone(&self.llm);
        let agent = self.get_or_create(thread);

        let mut skill_ctx = SkillContext {
            session_mgr,
            config,
            thread,
            llm: &llm,
            scheduler,
        };

        match process_message(agent, text, &mut skill_ctx).await {
            Ok(response) => response,
            Err(e) => {
                tracing::error!("Agent error: {}", e);
                AgentResponse::Reply(format!("Agent error: {}", e))
            }
        }
    }

    /// Clear conversation context for a thread.
    pub fn clear_context(&mut self, thread: &ThreadId) {
        if let Some(agent) = self.agents.get_mut(thread) {
            agent.context.clear();
        }
    }

    /// Remove an agent instance for a thread.
    pub fn remove(&mut self, thread: &ThreadId) {
        self.agents.remove(thread);
    }

    /// Whether the agent is configured for auto-agent mode on new threads.
    pub fn auto_agent_mode(&self) -> bool {
        self.config.auto_agent_mode
    }
}
