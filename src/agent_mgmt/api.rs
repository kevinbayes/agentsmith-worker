use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::agent_mgmt::protocol::{AgentCommand, ResultStatus};
use crate::agent_mgmt::Manager;

#[derive(Clone)]
pub struct AgentApiState {
    pub manager: Arc<Manager>,
}

pub fn agent_routes(state: AgentApiState) -> Router {
    Router::new()
        .route("/api/agents", get(list_agents))
        .route("/api/agents/_command", post(command_handler))
        .route("/api/agents/{name}", get(get_agent))
        .with_state(state)
}

async fn list_agents(State(state): State<AgentApiState>) -> impl IntoResponse {
    let list = state.manager.list();
    Json(serde_json::to_value(&list).unwrap_or_default())
}

async fn get_agent(
    State(state): State<AgentApiState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.manager.status(&name) {
        Some(s) => (
            StatusCode::OK,
            Json(serde_json::to_value(&s).unwrap_or_default()),
        ),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("no recipe for '{}'", name)})),
        ),
    }
}

async fn command_handler(
    State(state): State<AgentApiState>,
    Json(cmd): Json<AgentCommand>,
) -> impl IntoResponse {
    let result = state.manager.execute(cmd).await;
    let status = match result.status {
        ResultStatus::Success => StatusCode::OK,
        ResultStatus::InProgress => StatusCode::ACCEPTED,
        ResultStatus::Failed => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(serde_json::to_value(&result).unwrap_or_default()))
}
