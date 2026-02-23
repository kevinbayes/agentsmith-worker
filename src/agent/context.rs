use std::collections::VecDeque;
use std::time::Instant;

/// Role of a conversation turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Agent,
    /// Skill results, delegation output, system observations.
    System,
}

/// A single turn in the conversation history.
#[derive(Debug, Clone)]
pub struct Turn {
    pub role: Role,
    pub content: String,
    pub timestamp: Instant,
}

/// Bounded conversation history for a per-thread agent instance.
pub struct ConversationContext {
    turns: VecDeque<Turn>,
    max_turns: usize,
}

impl ConversationContext {
    pub fn new(max_turns: usize) -> Self {
        Self {
            turns: VecDeque::with_capacity(max_turns),
            max_turns,
        }
    }

    /// Add a turn to the conversation history.
    pub fn push(&mut self, role: Role, content: String) {
        if self.turns.len() >= self.max_turns {
            self.turns.pop_front();
        }
        self.turns.push_back(Turn {
            role,
            content,
            timestamp: Instant::now(),
        });
    }

    /// Clear all conversation history.
    pub fn clear(&mut self) {
        self.turns.clear();
    }

    /// Serialize the conversation history to a format suitable for an LLM prompt.
    /// Returns a single string with role-prefixed turns.
    pub fn to_prompt(&self) -> String {
        let mut parts = Vec::with_capacity(self.turns.len());
        for turn in &self.turns {
            let prefix = match turn.role {
                Role::User => "User",
                Role::Agent => "Assistant",
                Role::System => "System",
            };
            parts.push(format!("{}: {}", prefix, turn.content));
        }
        parts.join("\n\n")
    }

    /// Get the number of turns.
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    /// Get the last user message, if any.
    pub fn last_user_message(&self) -> Option<&str> {
        self.turns
            .iter()
            .rev()
            .find(|t| t.role == Role::User)
            .map(|t| t.content.as_str())
    }
}
