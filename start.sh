#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LOG_FILE="$SCRIPT_DIR/worker.log"
PID_FILE="$SCRIPT_DIR/worker.pid"

if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Daemon already running (PID: $(cat "$PID_FILE"))"
    exit 1
fi

echo "Building release binary..."
cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"

nohup "$SCRIPT_DIR/target/release/agentsmith-remote-worker" --config "$SCRIPT_DIR/config.toml" >> "$LOG_FILE" 2>&1 &
PID=$!
echo "$PID" > "$PID_FILE"

echo "Daemon started (PID: $PID), logging to $LOG_FILE"
echo "Stop with: kill \$(cat $PID_FILE)"
