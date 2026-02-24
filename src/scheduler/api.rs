use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::RwLock;

use crate::scheduler::Scheduler;

/// Shared state for schedule API handlers.
#[derive(Clone)]
pub struct ScheduleApiState {
    pub scheduler: Arc<RwLock<Scheduler>>,
}

/// Request body for creating a schedule.
#[derive(Deserialize)]
pub struct CreateScheduleRequest {
    pub cron: String,
    pub tool: String,
    pub prompt: String,
    pub name: Option<String>,
}

/// Build the schedule API routes.
pub fn schedule_routes(state: ScheduleApiState) -> Router {
    Router::new()
        .route("/api/schedules", get(list_schedules).post(create_schedule))
        .route("/api/schedules/{id}", get(get_schedule).delete(delete_schedule))
        .route("/api/schedules/{id}/pause", post(pause_schedule))
        .route("/api/schedules/{id}/resume", post(resume_schedule))
        .route("/api/schedules/{id}/trigger", post(trigger_schedule))
        .with_state(state)
}

async fn list_schedules(State(state): State<ScheduleApiState>) -> impl IntoResponse {
    let sched = state.scheduler.read().await;
    let jobs = sched.list_jobs();
    Json(serde_json::to_value(&jobs).unwrap_or_default())
}

async fn get_schedule(
    State(state): State<ScheduleApiState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let sched = state.scheduler.read().await;
    match sched.get_job(id) {
        Some(job) => (
            axum::http::StatusCode::OK,
            Json(serde_json::to_value(job).unwrap_or_default()),
        ),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "Schedule not found"})),
        ),
    }
}

async fn create_schedule(
    State(state): State<ScheduleApiState>,
    Json(req): Json<CreateScheduleRequest>,
) -> impl IntoResponse {
    let origin_platform = "API".to_string();
    let origin_thread_id = "api".to_string();

    let mut sched = state.scheduler.write().await;
    match sched.add_job(
        &req.cron,
        &req.tool,
        &req.prompt,
        req.name.as_deref(),
        &origin_platform,
        &origin_thread_id,
    ) {
        Ok(job) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::to_value(job).unwrap_or_default()),
        ),
        Err(e) => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

async fn delete_schedule(
    State(state): State<ScheduleApiState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let mut sched = state.scheduler.write().await;
    match sched.delete_job(id) {
        Ok(()) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"deleted": id})),
        ),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

async fn pause_schedule(
    State(state): State<ScheduleApiState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let mut sched = state.scheduler.write().await;
    match sched.pause_job(id) {
        Ok(()) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"paused": id})),
        ),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

async fn resume_schedule(
    State(state): State<ScheduleApiState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let mut sched = state.scheduler.write().await;
    match sched.resume_job(id) {
        Ok(()) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"resumed": id})),
        ),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}

async fn trigger_schedule(
    State(state): State<ScheduleApiState>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let mut sched = state.scheduler.write().await;
    match sched.trigger_now(id) {
        Ok(()) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({"triggered": id})),
        ),
        Err(e) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        ),
    }
}
