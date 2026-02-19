use anyhow::Result;
use futures::{channel::oneshot, future, pin_mut, StreamExt};
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{Content, ContentBody, DataMessage};
use presage::libsignal_service::prelude::Uuid;
use presage::libsignal_service::protocol::{DeviceId, ProtocolAddress, ServiceId, SessionStore};
use presage::model::identity::OnNewIdentity;
use presage::model::messages::Received;
use presage::store::{ContentsStore, StateStore, Store};
use presage::Manager;
use presage_store_sqlite::SqliteStore;
use std::path::Path;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::SignalConfig;
use crate::messaging::{IncomingMessage, OutgoingMessage, Platform, ThreadId};

/// Signal messaging adapter — handles both receiving and sending messages.
///
/// Runs on a dedicated thread with LocalSet + current_thread runtime because
/// presage's receive_messages() stream is !Send (Rc in cipher internals).
///
/// IMPORTANT: Signal only allows ONE WebSocket connection per device. Both
/// receiving and sending MUST use the same Manager instance to share the
/// cached WebSocket connection. Using separate Managers causes the server
/// to disconnect the first with "Connected elsewhere" (4409).
pub struct SignalAdapter {
    config: SignalConfig,
    manager: Manager<SqliteStore, presage::manager::Registered>,
}

/// Open or create the Signal SQLite store.
pub async fn open_store(db_path: &Path) -> Result<SqliteStore> {
    let url = format!("sqlite://{}/signal.db?mode=rwc", db_path.display());
    let store = SqliteStore::open(&url, OnNewIdentity::Trust).await?;
    Ok(store)
}

/// Load a registered manager from the store.
pub async fn load_registered(
    db_path: &Path,
) -> Result<Manager<SqliteStore, presage::manager::Registered>> {
    let store = open_store(db_path).await?;
    let manager = Manager::load_registered(store).await?;
    Ok(manager)
}

/// Link this device as a secondary device (one-time setup).
/// Displays QR code in terminal for user to scan with Signal.
pub async fn link_device(db_path: &Path, device_name: &str) -> Result<()> {
    let store = open_store(db_path).await?;

    let (provisioning_link_tx, provisioning_link_rx) = oneshot::channel();

    let device_name = device_name.to_string();
    let (manager_result, _) = future::join(
        Manager::link_secondary_device(
            store,
            SignalServers::Production,
            device_name,
            provisioning_link_tx,
        ),
        async move {
            match provisioning_link_rx.await {
                Ok(url) => {
                    let url_str = url.to_string();
                    tracing::info!("Scan this QR code with your Signal app:");
                    if let Err(e) = qr2term::print_qr(&url_str) {
                        tracing::error!("Failed to print QR code: {}", e);
                        println!("QR URL: {}", url_str);
                    }
                    println!("\nOr open this URL: {}", url_str);
                }
                Err(e) => {
                    tracing::error!("Linking was cancelled: {}", e);
                }
            }
        },
    )
    .await;

    let _manager = manager_result?;
    tracing::info!("Device linked successfully!");
    Ok(())
}

impl SignalAdapter {
    pub fn new(
        config: SignalConfig,
        manager: Manager<SqliteStore, presage::manager::Registered>,
    ) -> Self {
        Self { config, manager }
    }

    /// Run the adapter. Must be called on a LocalSet + current_thread runtime
    /// because presage's receive_messages() stream is !Send.
    ///
    /// Handles both incoming and outgoing messages using a single Manager
    /// and a single WebSocket connection (required by Signal's server).
    pub async fn run(
        mut self,
        incoming_tx: mpsc::Sender<IncomingMessage>,
        mut outgoing_rx: mpsc::Receiver<OutgoingMessage>,
        cancel: CancellationToken,
    ) -> Result<()> {
        tracing::info!("Signal adapter starting...");

        let messages = self.manager.receive_messages().await?;
        pin_mut!(messages);

        // Wait for initial queue drain before allowing sends.
        // presage recommends processing all queued messages first so
        // sessions and profile keys are up to date.
        let mut queue_synced = false;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    tracing::info!("Signal adapter shutting down");
                    break;
                }
                msg = messages.next() => {
                    match msg {
                        Some(Received::Content(content)) => {
                            if let Some(incoming) = self.process_incoming(&content) {
                                let _ = incoming_tx.send(incoming).await;
                            }
                        }
                        Some(Received::QueueEmpty) => {
                            tracing::info!("Signal message queue synced");
                            queue_synced = true;
                        }
                        Some(Received::Contacts) => {
                            tracing::debug!("Signal contacts synced");
                        }
                        None => {
                            tracing::warn!("Signal message stream ended");
                            break;
                        }
                    }
                }
                outgoing = outgoing_rx.recv() => {
                    match outgoing {
                        Some(msg) => {
                            if msg.thread.platform != Platform::Signal {
                                continue;
                            }
                            if !queue_synced {
                                tracing::debug!("Deferring send until queue is synced");
                                // Send it back to process after queue syncs
                                // For now just warn and try anyway
                                tracing::warn!(
                                    "Sending before queue sync — session state may not be current"
                                );
                            }
                            if let Err(e) = self.send_message(&msg).await {
                                tracing::error!("Failed to send Signal message: {:?}", e);
                            }
                        }
                        None => {
                            tracing::info!("Outgoing channel closed");
                            break;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn process_incoming(&self, content: &Content) -> Option<IncomingMessage> {
        let sender_uuid = content.metadata.sender.raw_uuid();

        // Authorization check
        if let Some(ref auth_user) = self.config.authorized_user {
            if !auth_user.is_empty() {
                if let Ok(authorized_uuid) = Uuid::parse_str(auth_user) {
                    if sender_uuid != authorized_uuid {
                        tracing::debug!(
                            "Ignoring message from unauthorized user: {}",
                            sender_uuid
                        );
                        return None;
                    }
                }
            }
        }

        // Extract text from DataMessage or SynchronizeMessage (Note to Self / linked device sync)
        let text = match &content.body {
            ContentBody::DataMessage(dm) => {
                tracing::debug!("Received DataMessage from {}", sender_uuid);
                dm.body.as_deref()?
            }
            ContentBody::SynchronizeMessage(sync) => {
                // SyncMessages contain messages sent from another device on this account.
                // Only process "Note to Self" messages (destination == sender) to avoid
                // intercepting messages the user sends to other people.
                let sent = sync.sent.as_ref()?;
                let dest = sent.destination_service_id.as_deref()?;
                let sender_str = sender_uuid.to_string();

                // destination_service_id may be prefixed (e.g. "ACI:<uuid>") or bare
                if !dest.contains(&sender_str) {
                    tracing::trace!(
                        "Ignoring SyncMessage to other recipient: {}",
                        dest
                    );
                    return None;
                }

                let dm = sent.message.as_ref()?;
                let body = dm.body.as_deref()?;
                tracing::debug!(
                    "Received SyncMessage (Note to Self) from {}",
                    sender_uuid
                );
                body
            }
            other => {
                tracing::trace!("Ignoring content type: {:?}", std::mem::discriminant(other));
                return None;
            }
        };

        let thread_id = ThreadId {
            platform: Platform::Signal,
            id: sender_uuid.to_string(),
        };

        Some(IncomingMessage {
            thread: thread_id,
            sender: sender_uuid.to_string(),
            text: text.to_string(),
            timestamp: chrono::Utc::now(),
        })
    }

    async fn send_message(&mut self, msg: &OutgoingMessage) -> Result<()> {
        let uuid = Uuid::parse_str(&msg.thread.id)?;
        let recipient = ServiceId::Aci(uuid.into());

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time went backwards")
            .as_millis() as u64;

        // --- Diagnostic logging: inspect state before send ---
        let reg_data = self.manager.registration_data();
        let our_aci = reg_data.service_ids.aci;
        let our_device_id = self.manager.device_id();
        let message_to_self = uuid == Uuid::from(our_aci);

        tracing::info!(
            %uuid,
            %timestamp,
            our_aci = %Uuid::from(our_aci),
            our_device_id,
            message_to_self,
            body_len = msg.text.len(),
            "SEND: preparing message"
        );

        // Check session state for recipient device 1 (primary)
        let protocol_store = self.manager.store().aci_protocol_store();
        let recipient_addr = ProtocolAddress::new(uuid.to_string(), DeviceId::from(1u32));
        match protocol_store.load_session(&recipient_addr).await {
            Ok(Some(session)) => {
                match session.session_version() {
                    Ok(ver) => tracing::info!(
                        address = %recipient_addr,
                        session_version = ver,
                        "SEND: session exists for recipient device 1"
                    ),
                    Err(e) => tracing::error!(
                        address = %recipient_addr,
                        error = %e,
                        "SEND: session exists but session_version() failed"
                    ),
                }
                match session.remote_registration_id() {
                    Ok(reg_id) => tracing::info!(
                        address = %recipient_addr,
                        remote_registration_id = reg_id,
                        "SEND: remote registration ID"
                    ),
                    Err(e) => tracing::error!(
                        address = %recipient_addr,
                        error = %e,
                        "SEND: remote_registration_id() failed"
                    ),
                }
            }
            Ok(None) => {
                tracing::warn!(
                    address = %recipient_addr,
                    "SEND: NO session for recipient device 1 — will need pre-key fetch"
                );
            }
            Err(e) => {
                tracing::error!(
                    address = %recipient_addr,
                    error = %e,
                    "SEND: load_session FAILED (this is likely the source of the error)"
                );
            }
        }

        // Also check session for our own device if NOT self (sync message path)
        if !message_to_self {
            let self_addr = ProtocolAddress::new(
                Uuid::from(our_aci).to_string(),
                DeviceId::from(our_device_id),
            );
            match protocol_store.load_session(&self_addr).await {
                Ok(Some(_)) => tracing::info!(
                    address = %self_addr,
                    "SEND: session exists for our own device (for sync)"
                ),
                Ok(None) => tracing::info!(
                    address = %self_addr,
                    "SEND: no session for our own device (normal for self)"
                ),
                Err(e) => tracing::error!(
                    address = %self_addr,
                    error = %e,
                    "SEND: load_session for self FAILED"
                ),
            }
        }

        // Check profile key for recipient (determines sealed sender usage)
        match self.manager.store().profile_key(&uuid).await {
            Ok(Some(_)) => tracing::info!(
                %uuid,
                "SEND: profile key EXISTS for recipient — sealed sender will be attempted"
            ),
            Ok(None) => tracing::info!(
                %uuid,
                "SEND: NO profile key for recipient — identified sender will be used"
            ),
            Err(e) => tracing::error!(
                %uuid,
                error = %e,
                "SEND: profile_key() lookup failed"
            ),
        }

        // Check sender certificate (needed for sealed sender)
        match self.manager.store().sender_certificate().await {
            Ok(Some(cert)) => {
                match cert.expiration() {
                    Ok(exp) => {
                        let now_ms = timestamp;
                        let exp_ms = exp.epoch_millis();
                        tracing::info!(
                            expires_ms = exp_ms,
                            now_ms,
                            expired = exp_ms <= now_ms,
                            "SEND: sender certificate present"
                        );
                    }
                    Err(e) => tracing::error!(
                        error = %e,
                        "SEND: sender certificate expiration() FAILED \
                         — InvalidProtobufEncoding likely here"
                    ),
                }
            }
            Ok(None) => tracing::info!(
                "SEND: NO sender certificate cached — will be fetched"
            ),
            Err(e) => tracing::error!(
                error = %e,
                "SEND: sender_certificate() lookup failed"
            ),
        }

        // --- Build and send the message ---
        let data_message = DataMessage {
            body: Some(msg.text.clone()),
            timestamp: Some(timestamp),
            ..Default::default()
        };

        tracing::info!("SEND: calling manager.send_message()...");

        match self
            .manager
            .send_message(recipient, data_message, timestamp)
            .await
        {
            Ok(()) => {
                tracing::info!(%uuid, "SEND: message sent successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    %uuid,
                    error = ?e,
                    "SEND: manager.send_message() FAILED"
                );
                Err(e.into())
            }
        }
    }
}
