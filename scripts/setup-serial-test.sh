#!/usr/bin/env bash
# ── setup-serial-test.sh ──────────────────────────────────────────────────
# Create a virtual serial port pair for headless testing using socat.
#
# Prerequisites:
#   - socat (available via nixpkgs)
#
# Usage:
#   ./scripts/setup-serial-test.sh             # create PTY pair
#   ./scripts/setup-serial-test.sh cleanup     # tear down
#
# Environment variables:
#   SERIAL_A: first PTY path  (default: auto-assigned by socat -> /tmp/serial-test-a)
#   SERIAL_B: second PTY path (default: auto-assigned by socat -> /tmp/serial-test-b)
#   BAUDRATE: baud rate       (default: 115200)
# ===========================================================================

set -euo pipefail

BAUDRATE="${BAUDRATE:-115200}"
PID_FILE="/tmp/serial-test-socat.pid"
SERIAL_A_LINK="/tmp/serial-test-a"
SERIAL_B_LINK="/tmp/serial-test-b"

cleanup() {
    echo "=== Cleaning up virtual serial ports ==="
    if [ -f "$PID_FILE" ]; then
        PID=$(cat "$PID_FILE")
        if kill -0 "$PID" 2>/dev/null; then
            echo "  Killing socat (PID: $PID)..."
            kill "$PID" 2>/dev/null || true
        fi
        rm -f "$PID_FILE"
    fi
    pkill -f "socat.*pty.*serial-test" 2>/dev/null || true
    rm -f "$SERIAL_A_LINK" "$SERIAL_B_LINK"
    echo "=== Cleanup done ==="
}

trap cleanup EXIT INT TERM

if [ "${1:-}" = "cleanup" ]; then
    cleanup
    exit 0
fi

echo "=== Setting up virtual serial port pair ==="
echo "  Baudrate: $BAUDRATE"

# Start socat creating a PTY pair
# socat creates two linked pseudo-terminals
# We symlink to predictable paths so Rust tests can find them
socat -d -d \
    pty,raw,echo=0,link="$SERIAL_A_LINK",mode=666,waitslave \
    pty,raw,echo=0,link="$SERIAL_B_LINK",mode=666,waitslave \
    &

SOCAT_PID=$!
echo "$SOCAT_PID" > "$PID_FILE"
echo "  socat PID: $SOCAT_PID"

# Wait for symlinks to appear
echo "  Waiting for PTY symlinks..."
for i in $(seq 1 10); do
    if [ -L "$SERIAL_A_LINK" ] && [ -L "$SERIAL_B_LINK" ]; then
        break
    fi
    sleep 0.5
done

if [ -L "$SERIAL_A_LINK" ] && [ -L "$SERIAL_B_LINK" ]; then
    REAL_A=$(readlink -f "$SERIAL_A_LINK")
    REAL_B=$(readlink -f "$SERIAL_B_LINK")
    echo "=== Virtual serial port pair ready ==="
    echo "  Port A: $SERIAL_A_LINK -> $REAL_A"
    echo "  Port B: $SERIAL_B_LINK -> $REAL_B"
    echo ""
    echo "  Set SERIAL_TEST_PORT=$SERIAL_A_LINK in test env."
    echo "  To stop: $0 cleanup"
else
    echo "ERROR: PTY symlinks not created." >&2
    cat "$SERIAL_A_LINK" 2>/dev/null || echo "  $SERIAL_A_LINK not found"
    cat "$SERIAL_B_LINK" 2>/dev/null || echo "  $SERIAL_B_LINK not found"
    exit 1
fi
