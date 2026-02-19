use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// Buffers output from an AI session and flushes it in chunks,
/// respecting max chunk size and a debounce interval.
pub struct OutputBuffer {
    buffer: String,
    max_chunk_size: usize,
    flush_interval: Duration,
    last_data: Instant,
}

impl OutputBuffer {
    pub fn new(max_chunk_size: usize, flush_interval_ms: u64) -> Self {
        Self {
            buffer: String::new(),
            max_chunk_size,
            flush_interval: Duration::from_millis(flush_interval_ms),
            last_data: Instant::now(),
        }
    }

    /// Append new data to the buffer.
    pub fn push(&mut self, data: &str) {
        self.buffer.push_str(data);
        self.last_data = Instant::now();
    }

    /// Check if the buffer should be flushed (either full or debounce expired).
    pub fn should_flush(&self) -> bool {
        if self.buffer.is_empty() {
            return false;
        }
        self.buffer.len() >= self.max_chunk_size
            || self.last_data.elapsed() >= self.flush_interval
    }

    /// Drain up to `max_chunk_size` from the buffer and return it.
    /// Returns `None` if the buffer is empty.
    pub fn flush(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }

        if self.buffer.len() <= self.max_chunk_size {
            let out = std::mem::take(&mut self.buffer);
            Some(out)
        } else {
            // Find a good split point (prefer newline boundaries)
            let split_at = find_split_point(&self.buffer, self.max_chunk_size);
            let remainder = self.buffer.split_off(split_at);
            let chunk = std::mem::replace(&mut self.buffer, remainder);
            Some(chunk)
        }
    }

    /// Flush all remaining data, possibly in multiple chunks.
    pub fn flush_all(&mut self) -> Vec<String> {
        let mut chunks = Vec::new();
        while !self.buffer.is_empty() {
            if let Some(chunk) = self.flush() {
                chunks.push(chunk);
            }
        }
        chunks
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Find a split point at or before `max` that doesn't break mid-line.
fn find_split_point(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }

    // Look for last newline before max
    if let Some(pos) = s[..max].rfind('\n') {
        return pos + 1; // include the newline
    }

    // Look for last space before max
    if let Some(pos) = s[..max].rfind(' ') {
        return pos + 1;
    }

    // No good split point, just split at max (ensure valid UTF-8 boundary)
    let mut split = max;
    while split > 0 && !s.is_char_boundary(split) {
        split -= 1;
    }
    split
}

/// Run an output buffer loop that collects data from a receiver,
/// buffers it, and sends flushed chunks to a sender.
pub async fn run_output_buffer(
    mut data_rx: mpsc::Receiver<String>,
    chunk_tx: mpsc::Sender<String>,
    max_chunk_size: usize,
    flush_interval_ms: u64,
) {
    let mut buffer = OutputBuffer::new(max_chunk_size, flush_interval_ms);
    let flush_dur = Duration::from_millis(flush_interval_ms);

    loop {
        tokio::select! {
            data = data_rx.recv() => {
                match data {
                    Some(text) => {
                        buffer.push(&text);
                        // If buffer is full, flush immediately
                        while buffer.buffer.len() >= max_chunk_size {
                            if let Some(chunk) = buffer.flush() {
                                if chunk_tx.send(chunk).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    None => {
                        // Input closed, flush remaining
                        for chunk in buffer.flush_all() {
                            let _ = chunk_tx.send(chunk).await;
                        }
                        return;
                    }
                }
            }
            _ = tokio::time::sleep(flush_dur) => {
                if buffer.should_flush() {
                    if let Some(chunk) = buffer.flush() {
                        if chunk_tx.send(chunk).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }
}
