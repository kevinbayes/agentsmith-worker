use axum::extract::{FromRef, Path, State, Extension, Multipart, DefaultBodyLimit};
use axum::extract::Query;
use axum::response::IntoResponse;
use axum::{Json, middleware, Router};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use serde::Deserialize;
use serde_json::Value;
use sqlx::{MySql, Pool};
use crate::error::{LocalResult, LocalError as CError };
use tower_http::{trace::TraceLayer};
use crate::config::config::Config;
use crate::service::job_service::JobService;

#[derive(Clone, FromRef)]
struct JobState {
    jobs_service: JobService,
}


pub fn job_routes (job_service: JobService, config: Config) -> Router {
    let job_state = JobState { jobs_service: job_service };

    Router::new()
        .route("/api/jobs", get(jobs_list_handler))
        .with_state(job_state)
}

pub async fn jobs_list_handler(
    headers: HeaderMap,
    State(state): State<JobState>,
) -> LocalResult<Json<Value>> {

    let list = state.jobs_service.list().await?;
    Ok(Json(list))
}