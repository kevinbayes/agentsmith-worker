#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="$SCRIPT_DIR/worker.pid"

if [ ! -f "$PID_FILE" ]; then
    echo "No PID file found. Daemon not running?"
    exit 1
fi

PID=$(cat "$PID_FILE")

if kill -0 "$PID" 2>/dev/null; then
    kill "$PID"
    echo "Daemon stopped (PID: $PID)"
else
    echo "Process $PID not running (stale PID file)"
fi

rm -f "$PID_FILE"
