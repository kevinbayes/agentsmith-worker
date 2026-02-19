use anyhow::Result;
use slack_morphism::prelude::*;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::SlackConfig;
use crate::messaging::{IncomingMessage, OutgoingMessage, Platform, ThreadId};

/// Slack messaging adapter using Socket Mode.
pub struct SlackAdapter {
    config: SlackConfig,
}

impl SlackAdapter {
    pub fn new(config: SlackConfig) -> Self {
        Self { config }
    }

    pub async fn run(
        self,
        incoming_tx: mpsc::Sender<IncomingMessage>,
        mut outgoing_rx: mpsc::Receiver<OutgoingMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        tracing::info!("Slack adapter starting...");

        let app_token = self
            .config
            .app_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Slack app_token not configured"))?
            .clone();
        let bot_token = self
            .config
            .bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Slack bot_token not configured"))?
            .clone();

        let client = SlackClient::new(SlackClientHyperConnector::new()?);
        let client = Arc::new(client);
        let socket_mode_token = SlackApiToken::new(app_token.into());
        let bot_api_token = SlackApiToken::new(bot_token.into());

        // Store the incoming_tx in user state so our callback can access it
        let listener_environment = Arc::new(
            SlackClientEventsListenerEnvironment::new(client.clone())
                .with_user_state(IncomingTxState(incoming_tx.clone())),
        );

        let socket_mode_callbacks = SlackSocketModeListenerCallbacks::new()
            .with_push_events(push_events_handler);

        let socket_mode_config = SlackClientSocketModeConfig::new();

        let socket_mode_listener = SlackClientSocketModeListener::new(
            &socket_mode_config,
            listener_environment.clone(),
            socket_mode_callbacks,
        );

        // Register the app token for socket mode
        socket_mode_listener
            .listen_for(&socket_mode_token)
            .await?;

        // Spawn the outgoing message handler
        let client_for_outgoing = client.clone();
        let bot_token_for_outgoing = bot_api_token.clone();
        let cancel_clone = cancel.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = cancel_clone.cancelled() => break,
                    msg = outgoing_rx.recv() => {
                        match msg {
                            Some(outgoing) => {
                                if outgoing.thread.platform == Platform::Slack {
                                    if let Err(e) = send_slack_message(
                                        &client_for_outgoing,
                                        &bot_token_for_outgoing,
                                        &outgoing,
                                    ).await {
                                        tracing::error!("Failed to send Slack message: {}", e);
                                    }
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });

        // Start socket mode and run until cancellation
        socket_mode_listener.start().await;

        // Wait for cancellation
        cancel.cancelled().await;
        tracing::info!("Slack adapter shutting down");
        socket_mode_listener.shutdown().await;

        Ok(())
    }
}

/// State wrapper so we can store the incoming_tx in Slack's user state system.
#[derive(Clone)]
struct IncomingTxState(mpsc::Sender<IncomingMessage>);

async fn push_events_handler(
    event: SlackPushEventCallback,
    _client: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> UserCallbackResult<()> {
    tracing::debug!("Received Slack push event");

    if let SlackEventCallbackBody::Message(msg_event) = &event.event {
        // Skip bot messages to avoid echo loops
        if msg_event.sender.bot_id.is_some() {
            return Ok(());
        }

        let channel = msg_event
            .origin
            .channel
            .as_ref()
            .map(|c| c.to_string())
            .unwrap_or_default();

        let sender = msg_event
            .sender
            .user
            .as_ref()
            .map(|u| u.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let text = msg_event
            .content
            .as_ref()
            .and_then(|c| c.text.as_deref())
            .unwrap_or_default();

        if text.is_empty() {
            return Ok(());
        }

        let thread_id = ThreadId {
            platform: Platform::Slack,
            id: channel,
        };

        let incoming = IncomingMessage {
            thread: thread_id,
            sender,
            text: text.to_string(),
            timestamp: chrono::Utc::now(),
        };

        // Read from the RwLock to access user state
        let guard = states.read().await;
        if let Some(tx_state) = guard.get_user_state::<IncomingTxState>() {
            let _ = tx_state.0.send(incoming).await;
        }
    }

    Ok(())
}

async fn send_slack_message(
    client: &SlackClient<SlackClientHyperHttpsConnector>,
    token: &SlackApiToken,
    msg: &OutgoingMessage,
) -> Result<()> {
    let session = client.open_session(token);
    let channel_id: SlackChannelId = msg.thread.id.clone().into();

    let request = SlackApiChatPostMessageRequest::new(
        channel_id,
        SlackMessageContent::new().with_text(msg.text.clone()),
    );

    session.chat_post_message(&request).await?;
    Ok(())
}
