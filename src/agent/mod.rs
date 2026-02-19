pub mod interaction;

use crate::llm::LlmProvider;

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
