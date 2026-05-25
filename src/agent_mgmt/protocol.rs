use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::agent_mgmt::recipe::Recipe;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "PascalCase")]
pub enum AgentCommandKind {
    Install {
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        recipe_override: Option<Recipe>,
    },
    Update {
        #[serde(default)]
        version: Option<String>,
    },
    Remove,
    Kill {
        #[serde(default)]
        pid: Option<u32>,
    },
    Status,
    List,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommand {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
    pub kind: AgentCommandKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ResultStatus {
    Success,
    Failed,
    InProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCommandResult {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub status: ResultStatus,
    pub message: String,
    #[serde(default)]
    pub details: serde_json::Value,
    pub completed_at: DateTime<Utc>,
}

impl AgentCommandResult {
    pub fn success(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            status: ResultStatus::Success,
            message: message.into(),
            details: serde_json::Value::Null,
            completed_at: Utc::now(),
        }
    }

    pub fn failed(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            id: None,
            name: name.into(),
            status: ResultStatus::Failed,
            message: message.into(),
            details: serde_json::Value::Null,
            completed_at: Utc::now(),
        }
    }

    pub fn with_id(mut self, id: Option<String>) -> Self {
        self.id = id;
        self
    }

    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = details;
        self
    }
}
