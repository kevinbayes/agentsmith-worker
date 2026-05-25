use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Json};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use crate::agent_mgmt::api::{agent_routes, AgentApiState};
use crate::agent_mgmt::Manager as AgentMgmtManager;
use crate::config::WebConfig;
use crate::messaging::{IncomingMessage, OutgoingMessage, Platform, ThreadId};
use crate::scheduler::Scheduler;
use crate::scheduler::api::{ScheduleApiState, schedule_routes};

/// Snapshot of system status shared from Router to the web adapter.
#[derive(Debug, Clone, Serialize, Default)]
pub struct StatusSnapshot {
    pub sessions: Vec<SessionSnapshotItem>,
    pub agents: Vec<AgentSnapshotItem>,
    pub version: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSnapshotItem {
    pub id: u64,
    pub tool: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSnapshotItem {
    pub name: String,
    pub binary: String,
    pub installed: bool,
    pub version: Option<String>,
    pub running_count: usize,
    /// True when the row maps to an agent_mgmt recipe (i.e. the worker can
    /// install / update / remove the binary). False for monitor-only entries
    /// that we merely observe on the host.
    #[serde(default)]
    pub managed: bool,
    /// Recipe name to use as `AgentCommand.name` for lifecycle calls. Present
    /// only when `managed` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_name: Option<String>,
}

/// Shared state for axum handlers.
#[derive(Clone)]
struct AppState {
    incoming_tx: mpsc::Sender<IncomingMessage>,
    connections: Arc<RwLock<HashMap<String, mpsc::Sender<String>>>>,
    status_snapshot: Arc<RwLock<StatusSnapshot>>,
}

/// Web messaging adapter providing a browser-based chat UI and status dashboard.
pub struct WebAdapter {
    config: WebConfig,
    status_snapshot: Arc<RwLock<StatusSnapshot>>,
    scheduler: Option<Arc<RwLock<Scheduler>>>,
    agent_mgmt: Option<Arc<AgentMgmtManager>>,
}

impl WebAdapter {
    pub fn new(
        config: WebConfig,
        status_snapshot: Arc<RwLock<StatusSnapshot>>,
        scheduler: Option<Arc<RwLock<Scheduler>>>,
        agent_mgmt: Option<Arc<AgentMgmtManager>>,
    ) -> Self {
        Self {
            config,
            status_snapshot,
            scheduler,
            agent_mgmt,
        }
    }

    pub async fn run(
        self,
        incoming_tx: mpsc::Sender<IncomingMessage>,
        mut outgoing_rx: mpsc::Receiver<OutgoingMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        tracing::info!(
            "Web adapter starting on {}:{}...",
            self.config.host,
            self.config.port
        );

        let connections: Arc<RwLock<HashMap<String, mpsc::Sender<String>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let state = AppState {
            incoming_tx,
            connections: connections.clone(),
            status_snapshot: self.status_snapshot,
        };

        let mut app = Router::new()
            .route("/", get(index_handler))
            .route("/ws", get(ws_handler))
            .route("/api/status", get(status_handler))
            .with_state(state);

        // Add schedule API routes if scheduler is available
        if let Some(scheduler) = self.scheduler {
            let sched_state = ScheduleApiState { scheduler };
            app = app.merge(schedule_routes(sched_state));
        }

        // Add agent management API routes when enabled
        if let Some(manager) = self.agent_mgmt {
            let state = AgentApiState { manager };
            app = app.merge(agent_routes(state));
        }

        let addr = format!("{}:{}", self.config.host, self.config.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("Web UI available at http://{}", addr);

        // Spawn outgoing message dispatcher
        let cancel_outgoing = cancel.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_outgoing.cancelled() => break,
                    msg = outgoing_rx.recv() => {
                        match msg {
                            Some(outgoing) => {
                                if outgoing.thread.platform == Platform::Web {
                                    let conn_id = &outgoing.thread.id;
                                    let conns = connections.read().await;
                                    if let Some(tx) = conns.get(conn_id) {
                                        let payload = serde_json::json!({
                                            "type": "message",
                                            "text": outgoing.text,
                                        })
                                        .to_string();
                                        let _ = tx.send(payload).await;
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
            tracing::info!("Web outgoing dispatcher stopped");
        });

        // Run axum server with graceful shutdown
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                cancel.cancelled().await;
            })
            .await?;

        tracing::info!("Web adapter shutting down");
        Ok(())
    }
}

async fn index_handler() -> Html<&'static str> {
    Html(include_str!("../../static/index.html"))
}

async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let snap = state.status_snapshot.read().await;
    Json(snap.clone())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AppState) {
    use futures::{SinkExt, StreamExt};

    let conn_id = uuid::Uuid::new_v4().to_string();

    tracing::info!("Web client connected: {}", conn_id);

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Per-connection outgoing channel
    let (out_tx, mut out_rx) = mpsc::channel::<String>(64);

    // Register connection
    {
        let mut conns = state.connections.write().await;
        conns.insert(conn_id.clone(), out_tx);
    }

    // Send task: forward outgoing channel messages to WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Receive task: read WebSocket messages and forward as IncomingMessage
    let incoming_tx = state.incoming_tx.clone();
    let conn_id_recv = conn_id.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // Parse JSON: {"text": "..."}
                    #[derive(Deserialize)]
                    struct WsInput {
                        text: String,
                    }
                    if let Ok(input) = serde_json::from_str::<WsInput>(&text) {
                        let thread = ThreadId {
                            platform: Platform::Web,
                            id: conn_id_recv.clone(),
                        };
                        let incoming = IncomingMessage {
                            thread,
                            sender: "web".to_string(),
                            text: input.text,
                            timestamp: chrono::Utc::now(),
                        };
                        if incoming_tx.send(incoming).await.is_err() {
                            break;
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish (client disconnect)
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Cleanup
    {
        let mut conns = state.connections.write().await;
        conns.remove(&conn_id);
    }
    tracing::info!("Web client disconnected: {}", conn_id);
}
