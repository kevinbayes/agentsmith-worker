use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

use std::sync::Arc;
use tokio::sync::RwLock;

use agentsmith_remote_worker::agent_mgmt::Manager as AgentMgmtManager;
use agentsmith_remote_worker::config::Config;
use agentsmith_remote_worker::messaging::signal;
use agentsmith_remote_worker::messaging::slack::SlackAdapter;
use agentsmith_remote_worker::messaging::telegram::TelegramAdapter;
use agentsmith_remote_worker::messaging::web::{StatusSnapshot, WebAdapter};
use agentsmith_remote_worker::messaging::{IncomingMessage, OutgoingMessage};
use agentsmith_remote_worker::router::Router;
use agentsmith_remote_worker::scheduler::Scheduler;
use agentsmith_remote_worker::scheduler::store::ScheduleStore;
use agentsmith_remote_worker::shutdown;

#[derive(Parser, Debug)]
#[command(name = "agentsmith-remote-worker")]
#[command(about = "AgentSmith Remote Worker - Bridge messaging platforms with AI CLI tools")]
#[command(version)]
struct Cli {
    /// Path to config file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Link Signal device (one-time setup)
    #[arg(long)]
    link_signal: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Install rustls crypto provider before any TLS connections are made
    // (required by slack-morphism's hyper-rustls)
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("failed to install rustls crypto provider");

    let cli = Cli::parse();

    // Load config
    let config = Config::load(&cli.config)?;

    // Init tracing
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.daemon.log_level));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    tracing::info!("AgentSmith Remote Worker starting...");

    // Ensure data directory exists
    std::fs::create_dir_all(&config.daemon.data_dir)?;

    // Handle --link-signal mode
    if cli.link_signal {
        tracing::info!("Running Signal device linking...");
        let db_path = config.signal_db_path();
        std::fs::create_dir_all(&db_path)?;
        signal::link_device(&db_path, &config.signal.device_name).await?;
        tracing::info!("Signal device linked successfully. You can now run the daemon.");
        return Ok(());
    }

    // Create cancellation token
    let cancel = CancellationToken::new();
    shutdown::install_signal_handlers(cancel.clone());

    // Create channels
    let (incoming_tx, incoming_rx) = mpsc::channel::<IncomingMessage>(256);
    let mut outgoing_txs: Vec<mpsc::Sender<OutgoingMessage>> = Vec::new();

    // Start enabled adapters
    if config.signal.enabled {
        tracing::info!("Starting Signal adapter...");
        let db_path = config.signal_db_path();
        let signal_config = config.signal.clone();
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<OutgoingMessage>(256);
        outgoing_txs.push(outgoing_tx);

        let incoming_tx = incoming_tx.clone();
        let cancel = cancel.clone();

        // Signal's presage Manager is !Send and its receive_messages() stream
        // is !Send, so everything runs on a dedicated OS thread with a
        // single-threaded runtime + LocalSet. Both receive and send use the
        // same Manager to share one WebSocket connection (Signal only allows
        // one per device).
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build signal runtime");

            let local = tokio::task::LocalSet::new();
            local.block_on(&rt, async move {
                match signal::load_registered(&db_path).await {
                    Ok(manager) => {
                        let adapter =
                            signal::SignalAdapter::new(signal_config, manager);
                        if let Err(e) =
                            adapter.run(incoming_tx, outgoing_rx, cancel).await
                        {
                            tracing::error!("Signal adapter error: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Failed to load Signal manager (run --link-signal first?): {}",
                            e
                        );
                    }
                }
            });
        });
    }

    if config.slack.enabled {
        tracing::info!("Starting Slack adapter...");
        let adapter = SlackAdapter::new(config.slack.clone());
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<OutgoingMessage>(256);
        outgoing_txs.push(outgoing_tx);

        let incoming_tx = incoming_tx.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.run(incoming_tx, outgoing_rx, cancel).await {
                tracing::error!("Slack adapter error: {}", e);
            }
        });
    }

    if config.telegram.enabled {
        tracing::info!("Starting Telegram adapter...");
        let adapter = TelegramAdapter::new(config.telegram.clone());
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<OutgoingMessage>(256);
        outgoing_txs.push(outgoing_tx);

        let incoming_tx = incoming_tx.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.run(incoming_tx, outgoing_rx, cancel).await {
                tracing::error!("Telegram adapter error: {}", e);
            }
        });
    }

    // Scheduler
    let scheduler = if config.scheduler.enabled {
        tracing::info!("Starting scheduler...");
        let store = ScheduleStore::new(&config.daemon.data_dir);
        let sched = Scheduler::new(config.scheduler.clone(), store);
        Some(Arc::new(RwLock::new(sched)))
    } else {
        None
    };

    // Agent management
    let agent_mgmt = if config.agent_management.enabled {
        tracing::info!("Starting agent management...");
        Some(AgentMgmtManager::new(
            &config.agent_management,
            &config.daemon.data_dir,
        ))
    } else {
        None
    };

    // Web adapter
    let status_snapshot = if config.web.enabled {
        Some(Arc::new(RwLock::new(StatusSnapshot::default())))
    } else {
        None
    };

    if config.web.enabled {
        tracing::info!("Starting Web adapter...");
        let adapter = WebAdapter::new(
            config.web.clone(),
            status_snapshot.clone().unwrap(),
            scheduler.clone(),
            agent_mgmt.clone(),
        );
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<OutgoingMessage>(256);
        outgoing_txs.push(outgoing_tx);

        let incoming_tx = incoming_tx.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            if let Err(e) = adapter.run(incoming_tx, outgoing_rx, cancel).await {
                tracing::error!("Web adapter error: {}", e);
            }
        });
    }

    if outgoing_txs.is_empty() {
        tracing::warn!("No messaging adapters enabled! Enable at least one in config.");
        tracing::warn!("Set signal.enabled = true, slack.enabled = true, telegram.enabled = true, or web.enabled = true");
    }

    // Start router
    let router = Router::new(
        config,
        incoming_rx,
        outgoing_txs,
        cancel.clone(),
        status_snapshot,
        scheduler,
        agent_mgmt,
    );

    tracing::info!("AgentSmith Remote Worker is running. Press Ctrl+C to stop.");
    router.run().await?;

    tracing::info!("AgentSmith Remote Worker stopped.");
    Ok(())
}
