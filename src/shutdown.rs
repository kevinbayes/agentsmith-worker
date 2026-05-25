use tokio_util::sync::CancellationToken;

/// Spawn a task that cancels `cancel` when the OS requests shutdown.
///
/// - On Unix: listens for SIGINT and SIGTERM.
/// - On Windows: listens for Ctrl-C (which is what `tokio::signal::ctrl_c`
///   wraps — the closest cross-runtime equivalent to SIGINT). Windows has no
///   direct SIGTERM analogue; service stop is handled by the host (e.g.
///   NSSM, Windows Service Control Manager) which delivers a console-close
///   event that ctrl_c also catches.
pub fn install_signal_handlers(cancel: CancellationToken) {
    #[cfg(unix)]
    install_unix(cancel);
    #[cfg(windows)]
    install_windows(cancel);
}

#[cfg(unix)]
fn install_unix(cancel: CancellationToken) {
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

        cancel.cancel();
    });
}

#[cfg(windows)]
fn install_windows(cancel: CancellationToken) {
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::info!("Received Ctrl-C, initiating shutdown...");
        } else {
            tracing::warn!("Failed to install Ctrl-C handler; shutting down on cancel only");
        }
        cancel.cancel();
    });
}
