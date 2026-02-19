use anyhow::Result;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use tokio::sync::mpsc;

/// Handle for a running PTY session.
pub struct PtyHandle {
    master_write: Box<dyn Write + Send>,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
}

/// Spawn a command in a PTY. Returns a PtyHandle and a receiver for stdout data.
pub fn spawn_pty(
    binary: &str,
    args: &[String],
    working_dir: &Path,
) -> Result<(PtyHandle, mpsc::Receiver<Vec<u8>>)> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| anyhow::anyhow!("Failed to open PTY: {}", e))?;

    let mut cmd = CommandBuilder::new(binary);
    cmd.args(args);
    cmd.cwd(working_dir);

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| anyhow::anyhow!("Failed to spawn command in PTY: {}", e))?;

    let master_write = pair
        .master
        .take_writer()
        .map_err(|e| anyhow::anyhow!("Failed to get PTY writer: {}", e))?;

    let master_read = pair
        .master
        .try_clone_reader()
        .map_err(|e| anyhow::anyhow!("Failed to get PTY reader: {}", e))?;

    let (data_tx, data_rx) = mpsc::channel::<Vec<u8>>(64);

    // Read PTY output in a blocking thread
    tokio::task::spawn_blocking(move || {
        let mut reader = master_read;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let chunk = buf[..n].to_vec();
                    if data_tx.blocking_send(chunk).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!("PTY read error (process may have exited): {}", e);
                    break;
                }
            }
        }
    });

    let handle = PtyHandle {
        master_write,
        _child: child,
    };

    Ok((handle, data_rx))
}

impl PtyHandle {
    /// Write data to the PTY's stdin.
    pub fn write_input(&mut self, data: &[u8]) -> Result<()> {
        self.master_write
            .write_all(data)
            .map_err(|e| anyhow::anyhow!("Failed to write to PTY: {}", e))?;
        self.master_write
            .flush()
            .map_err(|e| anyhow::anyhow!("Failed to flush PTY: {}", e))?;
        Ok(())
    }
}
