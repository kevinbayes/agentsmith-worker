use anyhow::Result;
use teloxide::prelude::*;
use teloxide::types::{ChatKind, UpdateKind};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::TelegramConfig;
use crate::messaging::{IncomingMessage, OutgoingMessage, Platform, ThreadId};

/// Telegram messaging adapter using long-polling via teloxide.
pub struct TelegramAdapter {
    config: TelegramConfig,
}

impl TelegramAdapter {
    pub fn new(config: TelegramConfig) -> Self {
        Self { config }
    }

    pub async fn run(
        self,
        incoming_tx: mpsc::Sender<IncomingMessage>,
        mut outgoing_rx: mpsc::Receiver<OutgoingMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        tracing::info!("Telegram adapter starting...");

        let bot_token = self
            .config
            .bot_token
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Telegram bot_token not configured"))?
            .clone();

        let bot = Bot::new(&bot_token);
        let bot_for_outgoing = bot.clone();

        let config = self.config.clone();
        let cancel_incoming = cancel.clone();

        // Spawn the incoming message handler (long-polling)
        tokio::spawn(async move {
            let mut offset: i32 = 0;

            loop {
                if cancel_incoming.is_cancelled() {
                    break;
                }

                let updates = bot
                    .get_updates()
                    .offset(offset)
                    .timeout(30)
                    .await;

                match updates {
                    Ok(updates) => {
                        for update in &updates {
                            offset = update.id.as_offset();

                            if let Some(msg) = process_update(update, &config) {
                                if let Err(e) = incoming_tx.send(msg).await {
                                    tracing::error!(
                                        "Failed to forward Telegram message: {}",
                                        e
                                    );
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if cancel_incoming.is_cancelled() {
                            break;
                        }
                        tracing::error!("Telegram get_updates error: {}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }

            tracing::info!("Telegram incoming handler stopped");
        });

        // Outgoing message handler
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                msg = outgoing_rx.recv() => {
                    match msg {
                        Some(outgoing) => {
                            if outgoing.thread.platform == Platform::Telegram {
                                if let Err(e) = send_telegram_message(
                                    &bot_for_outgoing,
                                    &outgoing,
                                ).await {
                                    tracing::error!(
                                        "Failed to send Telegram message: {}",
                                        e
                                    );
                                }
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        tracing::info!("Telegram adapter shutting down");
        Ok(())
    }
}

/// Process a Telegram update into an IncomingMessage, if applicable.
fn process_update(update: &Update, config: &TelegramConfig) -> Option<IncomingMessage> {
    let message = match &update.kind {
        UpdateKind::Message(msg) => msg,
        _ => return None,
    };

    // Only handle text messages
    let text = message.text()?;
    if text.is_empty() {
        return None;
    }

    let chat = &message.chat;

    // dm_only check: ignore non-private chats
    if config.dm_only {
        if !matches!(chat.kind, ChatKind::Private(_)) {
            tracing::trace!("Ignoring non-private Telegram chat: {}", chat.id);
            return None;
        }
    }

    let sender = message.from.as_ref()?;
    let sender_id = sender.id.0 as i64;

    // Authorization check
    if let Some(authorized) = config.authorized_user {
        if sender_id != authorized {
            tracing::debug!(
                "Ignoring message from unauthorized Telegram user: {}",
                sender_id
            );
            return None;
        }
    }

    let thread_id = ThreadId {
        platform: Platform::Telegram,
        id: chat.id.0.to_string(),
    };

    Some(IncomingMessage {
        thread: thread_id,
        sender: sender_id.to_string(),
        text: text.to_string(),
        timestamp: chrono::Utc::now(),
    })
}

/// Telegram has a 4096-character message limit. Split long messages.
const TELEGRAM_MAX_MSG_LEN: usize = 4096;

async fn send_telegram_message(bot: &Bot, msg: &OutgoingMessage) -> Result<()> {
    let chat_id: i64 = msg.thread.id.parse()?;

    if msg.text.len() <= TELEGRAM_MAX_MSG_LEN {
        bot.send_message(ChatId(chat_id), &msg.text).await?;
    } else {
        // Split into chunks at char boundaries
        for chunk in split_message(&msg.text, TELEGRAM_MAX_MSG_LEN) {
            bot.send_message(ChatId(chat_id), chunk).await?;
        }
    }

    Ok(())
}

fn split_message(text: &str, max_len: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = if start + max_len >= text.len() {
            text.len()
        } else {
            // Find a char boundary at or before start + max_len
            let mut end = start + max_len;
            while end > start && !text.is_char_boundary(end) {
                end -= 1;
            }
            if end == start {
                // Advance past the current multi-byte char
                end = start + 1;
                while end < text.len() && !text.is_char_boundary(end) {
                    end += 1;
                }
                end
            } else if let Some(nl) = text[start..end].rfind('\n') {
                // Try to split at a newline for cleaner output
                start + nl + 1
            } else {
                end
            }
        };

        chunks.push(&text[start..end]);
        start = end;
    }

    chunks
}
