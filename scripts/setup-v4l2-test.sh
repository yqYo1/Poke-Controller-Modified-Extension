#!/usr/bin/env bash
# ── setup-v4l2-test.sh ────────────────────────────────────────────────────
# Create a virtual V4L2 camera device for headless testing.
#
# Prerequisites:
#   - v4l2loopback kernel module (loaded by this script if possible)
#   - ffmpeg (for streaming test pattern)
#   - v4l-utils (for v4l2-ctl)
#
# Usage:
#   sudo ./scripts/setup-v4l2-test.sh           # normal mode
#   sudo ./scripts/setup-v4l2-test.sh cleanup   # tear down
#
# Environment variables:
#   V4L2_DEVICE: target video device (default: /dev/video0)
#   V4L2_WIDTH:  test frame width  (default: 640)
#   V4L2_HEIGHT: test frame height (default: 480)
#   V4L2_FPS:    test frame rate   (default: 15)
# ===========================================================================

set -euo pipefail

DEVICE="${V4L2_DEVICE:-/dev/video0}"
WIDTH="${V4L2_WIDTH:-640}"
HEIGHT="${V4L2_HEIGHT:-480}"
FPS="${V4L2_FPS:-15}"

cleanup() {
    echo "=== Cleaning up virtual V4L2 device ==="
    # Kill ffmpeg if running
    pkill -f "ffmpeg.*$DEVICE" 2>/dev/null || true
    # Unload v4l2loopback
    if lsmod | grep -q v4l2loopback; then
        # Remove all v4l2loopback devices first
        for dev in /dev/video*; do
            [ -e "$dev" ] || continue
            v4l2-ctl --device="$dev" --set-pwr=0 2>/dev/null || true
        done
        modprobe -r v4l2loopback 2>/dev/null || true
    fi
    echo "=== Cleanup done ==="
}

trap cleanup EXIT INT TERM

if [ "${1:-}" = "cleanup" ]; then
    cleanup
    exit 0
fi

echo "=== Setting up virtual V4L2 camera ==="
echo "  Device:  $DEVICE"
echo "  Size:    ${WIDTH}x${HEIGHT}"
echo "  FPS:     $FPS"

# Load v4l2loopback module
if ! lsmod | grep -q v4l2loopback; then
    echo "Loading v4l2loopback kernel module..."
    modprobe v4l2loopback devices=1 video_nr=0 card_label="Test Camera" exclusive_caps=1
    echo "  Module loaded."
else
    echo "  v4l2loopback already loaded."
fi

# Verify device exists
if [ ! -e "$DEVICE" ]; then
    echo "ERROR: $DEVICE not found after loading module." >&2
    exit 1
fi
echo "  Device $DEVICE is available."

# Get the video device index from the device path
DEV_INDEX="${DEVICE##/dev/video}"
echo "  Device index: $DEV_INDEX"

# Set format on the device using v4l2-ctl
echo "Setting format on $DEVICE..."
v4l2-ctl --device="$DEVICE" \
    --set-fmt-video="width=$WIDTH,height=$HEIGHT,pixelformat=YUYV" \
    --set-ctrl=quality=100 \
    --verbose 2>&1 | head -n5 || true

echo "Starting ffmpeg test pattern stream..."
ffmpeg -re -f lavfi -i "testsrc=size=${WIDTH}x${HEIGHT}:rate=${FPS}:duration=999999" \
    -pix_fmt yuyv422 \
    -f v4l2 "$DEVICE" \
    -loglevel warning &

FFPID=$!
echo "  ffmpeg PID: $FFPID"

# Give ffmpeg time to start
sleep 1

if kill -0 "$FFPID" 2>/dev/null; then
    echo "=== Virtual V4L2 camera ready on $DEVICE ==="
    echo "  Stream: test pattern (${WIDTH}x${HEIGHT} @ ${FPS}fps)"
    echo "  Run 'v4l2-ctl --device=$DEVICE --all' to inspect."
    echo "  To stop: $0 cleanup"
else
    echo "ERROR: ffmpeg failed to start." >&2
    exit 1
fi
