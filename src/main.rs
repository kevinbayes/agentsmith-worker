mod config;
mod handler;
mod error;
mod context;
mod cli;
mod common;
mod service;

use std::collections::HashSet;
use std::{env, thread};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use axum::{Json, middleware, Router};
use axum::http::{Method, Uri};
use axum::routing::get;
use axum::response::Response;
use axum::response::IntoResponse;
use clap::{Arg, ArgMatches, Command};
use serde_json::json;
use tower_http::services::ServeDir;
use tracing::info;
use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use sqlx::{MySql, Pool};
use uuid::Uuid;
use crate::config::config::{read_json_config,};
use crate::context::Context;
use crate::error::{LocalError, LocalResult};
use crate::handler::actuator::actuator_routes;
use env_logger::{Logger, Env};
use log::debug;
use crate::cli::worker_command::create_worker_command;
use crate::handler::job::job_routes;
use crate::service::job_service::JobService;

#[tokio::main]
async fn main() {

    env_logger::init();
    cli().await;
}

async fn cli() {

    let matches = Command::new("Agent Smith Worker")
        .version("1.0")
        .about("Agent smith worker service")
        .next_line_help(true)
        .subcommand_required(false)
        .subcommand(create_worker_command())
        .get_matches();

    match matches.clone().subcommand() {
        Some(("worker", cli_command)) => {
            debug!("enter: worker");
            handle_worker_command(matches, Some(cli_command)).await;
        },
        _ => {
            println!("fallback: unknown command");
        }
    }
}

async fn handle_worker_command(p0: ArgMatches, p1: Option<&ArgMatches>) {

    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let value = env::var("CONFIG_LOCATION").unwrap_or_else(|_| "./config/default.json".to_string());
    let all_configuration = read_json_config(&value).unwrap();
    let host_config = all_configuration.clone().host.unwrap();
    let security_config = all_configuration.clone().security.unwrap();

    let public_app = Router::new()
        .merge(job_routes(JobService::new(), all_configuration.clone()));

    let app = Router::new()
        .merge(actuator_routes())
        .merge(public_app)
        .layer(middleware::map_response(main_response_mapper))
        .fallback_service(static_routes())
        ;

    let addr = format!("{}:{}", host_config.host, host_config.port);
    println!("Starting service on {}.", addr);
    let listener = tokio::net::TcpListener::bind(addr.as_str())
        .await
        .unwrap();
    println!("listening on {}", addr.as_str());
    tracing::warn!("listening on {}", addr.as_str());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
        .await
        .unwrap();
}

async fn main_response_mapper(
    res: Response,
) -> Response {
    println!("->> {:<12} - main_response_mapper", "RES_MAPPER");

    let uuid = Uuid::new_v4();

    // -- Get the eventual response error.
    let service_error = res.extensions().get::<LocalError>();
    let client_status_error = service_error.map(|se| se.client_status_and_error());

    let error_response =
        client_status_error
            .as_ref()
            .map(|(status_code, client_error)| {
                let client_error_body = json!({
    				"error": {
    					"type": client_error.as_ref(),
    					"req_uuid": uuid.to_string(),
    				}
    			});

                println!("    ->> client_error_body: {client_error_body}");

                // Build the new response from the client_error_body
                (*status_code, Json(client_error_body)).into_response()
            });

    error_response.unwrap_or(res)
}

fn static_routes ()-> Router {
    let web_location = env::var("WEB_LOCATION").unwrap_or_else(|_| "./web/dist".to_string());
    Router::new().fallback_service(ServeDir::new(web_location))
}

