use tokio_util::sync::CancellationToken;

/// Create a cancellation token that triggers on SIGINT/SIGTERM.
pub fn install_signal_handlers(cancel: CancellationToken) {
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        let mut sigint =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                .expect("failed to install SIGINT handler");
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT, initiating shutdown...");
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM, initiating shutdown...");
            }
        }

        cancel_clone.cancel();
    });
}
