#!/usr/bin/env python3
# ruff: noqa: EM101, EM102
"""Run the CI-only browser loopback performance acceptance fixture.

The fixture deliberately uses only loopback resources.  It measures browser
and transport work with a local multipart-JPEG server, a Canvas WebRTC
loopback, a MessageChannel controller loopback, and browser animation/input
callbacks.  It does not claim to measure physical camera, serial, or console
latency.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import platform as platform_module
import re
import shutil
import subprocess
import sys
import tempfile
import threading
from datetime import UTC, datetime
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any, Final, cast
from urllib.parse import urlsplit

JsonObject = dict[str, object]

METRIC_ORDER: Final = (
    "webrtc_video_latency",
    "mjpeg_video_latency",
    "controller_input_latency",
    "ui_frame_rate",
    "ui_input_latency",
)
METRIC_UNITS: Final = {
    "webrtc_video_latency": "ms",
    "mjpeg_video_latency": "ms",
    "controller_input_latency": "ms",
    "ui_frame_rate": "fps",
    "ui_input_latency": "ms",
}
SAMPLE_COUNT: Final = 300
WARMUP_SECONDS: Final = 60
MIN_ACCEPTANCE_SAMPLE_COUNT: Final = SAMPLE_COUNT
MIN_ACCEPTANCE_WARMUP_SECONDS: Final = WARMUP_SECONDS
FRAME_WIDTH: Final = 1920
FRAME_HEIGHT: Final = 1080
PERFORMANCE_FIXTURE_ID: Final = "browser-loopback-v2"
REGRESSION_FACTOR_LATENCY: Final = 1.10
REGRESSION_FACTOR_FPS: Final = 0.95
SOURCE_COMMIT_PATTERN: Final = re.compile(r"^[0-9a-f]{40}$")

# These are the thresholds already specified in SPECIFICATION.md and the
# acceptance-record.schema.json.  The runner applies them independently of the
# JSON schema so a malformed or incomplete report cannot pass by serialization.
ABSOLUTE_THRESHOLDS: Final = {
    "webrtc_video_latency": ("p95", "<", 100.0),
    "mjpeg_video_latency": ("p95", "<=", 150.0),
    "controller_input_latency": ("p95", "<", 50.0),
    "ui_frame_rate": ("p50", ">=", 60.0),
    "ui_input_latency": ("p95", "<", 16.0),
}

PAGE_TEMPLATE: Final = r"""<!doctype html>
<meta charset="utf-8">
<title>PokeCon CI performance fixture</title>
<canvas id="source" width="1920" height="1080"></canvas>
<video id="remote" autoplay playsinline muted width="1920" height="1080"></video>
<button id="input" type="button">loopback input</button>
<script>
"use strict";
const SAMPLE_COUNT = __SAMPLE_COUNT__;
const FRAME_WIDTH = 1920;
const FRAME_HEIGHT = 1080;
const MARKER_WIDTH = 640;
const MARKER_HEIGHT = 240;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const now = () => performance.now();

function waitForIceComplete(peer) {
  if (peer.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("ICE gathering timeout")), 10000);
    peer.addEventListener("icegatheringstatechange", () => {
      if (peer.iceGatheringState === "complete") {
        clearTimeout(timer);
        resolve();
      }
    });
  });
}

async function makeWebRtcLoopback() {
  if (!window.RTCPeerConnection || !HTMLCanvasElement.prototype.captureStream) {
    throw new Error("Canvas WebRTC loopback is unavailable");
  }
  const source = document.getElementById("source");
  const remote = document.getElementById("remote");
  const sourceContext = source.getContext("2d", { alpha: false });
  sourceContext.fillStyle = "rgb(32,32,32)";
  sourceContext.fillRect(0, 0, FRAME_WIDTH, FRAME_HEIGHT);
  const stream = source.captureStream(60);
  const captureTrack = stream.getVideoTracks()[0];
  let frameColor = "rgb(32,32,200)";
  const captureTimer = setInterval(() => {
    sourceContext.fillStyle = frameColor;
    sourceContext.fillRect(0, 0, MARKER_WIDTH, MARKER_HEIGHT);
  }, 1000 / 60);
  const sender = new RTCPeerConnection({ iceServers: [] });
  const receiver = new RTCPeerConnection({ iceServers: [] });
  let receivedTrack;
  let resolveReceivedTrack;
  let resolveUnmute;
  const receivedTrackPromise = new Promise((resolve) => {
    resolveReceivedTrack = resolve;
  });
  const unmutePromise = new Promise((resolve) => {
    resolveUnmute = resolve;
  });
  sender.addTrack(captureTrack, stream);
  receiver.ontrack = (event) => {
    remote.srcObject = event.streams[0] || new MediaStream([event.track]);
    receivedTrack = event.track;
    event.track.addEventListener("unmute", () => resolveUnmute());
    if (!event.track.muted) resolveUnmute();
    resolveReceivedTrack(event.track);
  };
  const offer = await sender.createOffer();
  await sender.setLocalDescription(offer);
  await waitForIceComplete(sender);
  await receiver.setRemoteDescription(sender.localDescription);
  const answer = await receiver.createAnswer();
  await receiver.setLocalDescription(answer);
  await waitForIceComplete(receiver);
  await sender.setRemoteDescription(receiver.localDescription);
  const connectionDeadline = now() + 15000;
  while (sender.connectionState !== "connected" ||
         receiver.connectionState !== "connected") {
    if (sender.connectionState === "failed" ||
        receiver.connectionState === "failed" || now() >= connectionDeadline) {
      throw new Error(`WebRTC connection failed: ${sender.connectionState}/${receiver.connectionState}`);
    }
    await sleep(20);
  }
  const track = await Promise.race([
    receivedTrackPromise,
    sleep(10000).then(() => { throw new Error("remote video track timeout"); }),
  ]);
  await Promise.race([
    unmutePromise,
    sleep(10000).then(() => { throw new Error("remote video track stayed muted"); }),
  ]);

  remote.play().catch(() => {});
  return {
    source,
    sourceContext,
    remote,
    track,
    captureTrack,
    sender,
    receiver,
    setFrameColor: (color) => { frameColor = color; },
    stopCapture: () => clearInterval(captureTimer),
  };
}

function closeWebRtcLoopback(loopback) {
  loopback.stopCapture();
  loopback.captureTrack.stop();
  loopback.track.stop();
  loopback.sender.close();
  loopback.receiver.close();
  loopback.remote.srcObject = null;
}

async function collectWebRtc(loopback) {
  const { sourceContext, track, captureTrack, setFrameColor } = loopback;

  const decode = document.createElement("canvas");
  decode.width = 1;
  decode.height = 1;
  const decodeContext = decode.getContext("2d", { willReadFrequently: true });
  if (!window.MediaStreamTrackProcessor) {
    throw new Error("MediaStreamTrackProcessor is unavailable");
  }
  const processor = new MediaStreamTrackProcessor({ track });
  const reader = processor.readable.getReader();
  const samples = [];
  let readCount = 0;
  const started = now();
  const completion = (async () => {
    while (samples.length < SAMPLE_COUNT) {
      const redFrame = samples.length % 2 === 0;
      setFrameColor(redFrame ? "rgb(200,32,32)" : "rgb(32,32,200)");
      sourceContext.fillStyle = redFrame ? "rgb(200,32,32)" : "rgb(32,32,200)";
      sourceContext.fillRect(0, 0, MARKER_WIDTH, MARKER_HEIGHT);
      const sampleStarted = now();
      if (typeof captureTrack.requestFrame === "function") captureTrack.requestFrame();
      while (true) {
        const { value, done } = await reader.read();
        if (done) throw new Error("WebRTC video track ended during collection");
        readCount += 1;
        decodeContext.drawImage(value, 0, 0, 1, 1);
        const pixel = decodeContext.getImageData(0, 0, 1, 1).data;
        const receivedRed = pixel[0] > pixel[2];
        value.close();
        if (receivedRed === redFrame) {
          samples.push(now() - sampleStarted);
          break;
        }
        if (now() - sampleStarted > 1000) {
          throw new Error(`WebRTC colour marker timeout after ${readCount} frames`);
        }
      }

    }
  })();
  await Promise.race([
    completion,
    sleep(60000).then(() => {
      throw new Error(`WebRTC sample timeout: ${samples.length}/${SAMPLE_COUNT}`);
    }),
  ]);
  await reader.cancel();
  loopback.stopCapture();
  return { samples, duration_ms: now() - started };
}

async function makeJpegFixture() {
  const source = document.getElementById("source");
  const context = source.getContext("2d", { alpha: false });
  context.fillStyle = "rgb(48,96,160)";
  context.fillRect(0, 0, FRAME_WIDTH, FRAME_HEIGHT);
  context.fillStyle = "white";
  context.fillRect(0, 0, 256, 64);
  const blob = await new Promise((resolve, reject) => {
    source.toBlob((value) => value ? resolve(value) : reject(new Error("JPEG encode failed")), "image/jpeg", 0.85);
  });
  return blob;
}

function extractJpeg(bytes) {
  let start = -1;
  let end = -1;
  for (let index = 0; index + 1 < bytes.length; index += 1) {
    if (start < 0 && bytes[index] === 0xff && bytes[index + 1] === 0xd8) start = index;
    if (start >= 0 && bytes[index] === 0xff && bytes[index + 1] === 0xd9) {
      end = index + 2;
      break;
    }
  }
  if (start < 0 || end < 0) throw new Error("multipart response has no JPEG frame");
  return bytes.slice(start, end);
}

async function collectMjpeg() {
  const fixture = await makeJpegFixture();
  const decode = document.createElement("canvas");
  decode.width = 1;
  decode.height = 1;
  const context = decode.getContext("2d");
  const samples = [];
  const started = now();
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    const sampleStarted = now();
    const response = await fetch("/mjpeg", {
      method: "POST",
      body: fixture,
      cache: "no-store",
    });
    if (!response.ok) throw new Error(`MJPEG fixture returned ${response.status}`);
    const frame = extractJpeg(new Uint8Array(await response.arrayBuffer()));
    const bitmap = await createImageBitmap(new Blob([frame], { type: "image/jpeg" }));
    context.drawImage(bitmap, 0, 0, 1, 1);
    bitmap.close();
    samples.push(now() - sampleStarted);
  }
  return { samples, duration_ms: now() - started };
}

async function collectController() {
  const channel = new MessageChannel();
  const samples = [];
  channel.port1.start();
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    const sampleStarted = now();
    await new Promise((resolve) => {
      channel.port1.onmessage = () => resolve();
      channel.port2.postMessage(index);
    });
    samples.push(now() - sampleStarted);
  }
  channel.port1.close();
  channel.port2.close();
  return { samples, duration_ms: samples.reduce((a, b) => a + b, 0) };
}

async function collectFrameRate() {
  const samples = [];
  const started = now();
  await new Promise((resolve) => {
    let previous;
    const tick = (timestamp) => {
      if (previous !== undefined) samples.push(Math.round(1000 / (timestamp - previous)));
      previous = timestamp;
      if (samples.length >= SAMPLE_COUNT) resolve();
      else requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  return { samples, duration_ms: now() - started };
}

async function collectUiInput() {
  const button = document.getElementById("input");
  const samples = [];
  const started = now();
  for (let index = 0; index < SAMPLE_COUNT; index += 1) {
    const sampleStarted = now();
    await new Promise((resolve) => {
      const onClick = () => {
        button.removeEventListener("click", onClick);
        resolve();
      };
      button.addEventListener("click", onClick, { once: true });
      button.click();
    });
    samples.push(now() - sampleStarted);
  }
  return { samples, duration_ms: now() - started };
}

async function warmup(seconds) {
  const started = now();
  let frames = 0;
  while (now() - started < seconds * 1000) {
    await new Promise((resolve) => requestAnimationFrame(resolve));
    frames += 1;
  }
  return { duration_ms: now() - started, frames };
}

async function main() {
  const loopback = await makeWebRtcLoopback();
  const warmupResult = await warmup(__WARMUP_SECONDS__);
  const metrics = {};
  try {
    metrics.webrtc_video_latency = await collectWebRtc(loopback);
  } finally {
    closeWebRtcLoopback(loopback);
  }
  metrics.mjpeg_video_latency = await collectMjpeg();
  metrics.controller_input_latency = await collectController();
  metrics.ui_frame_rate = await collectFrameRate();
  metrics.ui_input_latency = await collectUiInput();
  await fetch("/result", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      fixture: { width: FRAME_WIDTH, height: FRAME_HEIGHT, stream_fps: 60 },
      warmup: warmupResult,
      metrics,
    }),
  });
}

window.addEventListener("error", (event) => {
  fetch("/error", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ error: String(event.error || event.message || "browser error") }),
  }).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  fetch("/error", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ error: String(event.reason || "unhandled rejection") }),
  }).catch(() => {});
});
main().catch((error) => {
  fetch("/error", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ error: String(error && error.stack || error) }),
  }).catch(() => {});
});
</script>
"""


class PerformanceError(RuntimeError):
    """Raised when the performance fixture cannot produce a valid report."""


class _State:
    page: bytes
    result: JsonObject | None
    error: str | None
    jpeg: bytes | None
    done: threading.Event
    lock: threading.Lock

    def __init__(self, page: str) -> None:
        self.page = page.encode("utf-8")
        self.result = None
        self.error = None
        self.jpeg = None
        self.done = threading.Event()
        self.lock = threading.Lock()


class _Handler(BaseHTTPRequestHandler):
    server: _Server

    def log_message(self, format: str, *_args: object) -> None:  # noqa: A002, ARG002
        return

    def _reply(self, status: HTTPStatus, content_type: str, body: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self) -> None:
        if urlsplit(self.path).path in {"/", "/benchmark"}:
            self._reply(
                HTTPStatus.OK, "text/html; charset=utf-8", self.server.state.page
            )
            return
        self._reply(HTTPStatus.NOT_FOUND, "text/plain; charset=utf-8", b"not found\n")

    def do_POST(self) -> None:
        length_header = self.headers.get("Content-Length")
        try:
            length = int(length_header or "-1")
        except ValueError:
            length = -1
        if length < 0 or length > 64 * 1024 * 1024:
            self._reply(
                HTTPStatus.BAD_REQUEST, "text/plain; charset=utf-8", b"invalid body\n"
            )
            return
        body = self.rfile.read(length)
        path = urlsplit(self.path).path
        if path == "/mjpeg":
            with self.server.state.lock:
                if self.server.state.jpeg is None:
                    self.server.state.jpeg = body
                jpeg = self.server.state.jpeg
            if not jpeg.startswith(b"\xff\xd8"):
                self._reply(
                    HTTPStatus.BAD_REQUEST,
                    "text/plain; charset=utf-8",
                    b"body is not JPEG\n",
                )
                return
            boundary = b"frame"
            response = (
                b"--" + boundary + b"\r\n"
                b"Content-Type: image/jpeg\r\n"
                b"Content-Length: "
                + str(len(jpeg)).encode("ascii")
                + b"\r\n\r\n"
                + jpeg
                + b"\r\n--"
                + boundary
                + b"--\r\n"
            )
            self._reply(
                HTTPStatus.OK, "multipart/x-mixed-replace; boundary=frame", response
            )
            return
        if path in {"/result", "/error"}:
            try:
                parsed_value: object = json.loads(body.decode("utf-8"))
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                self._reply(
                    HTTPStatus.BAD_REQUEST,
                    "text/plain; charset=utf-8",
                    str(error).encode(),
                )
                return
            if not isinstance(parsed_value, dict):
                self._reply(
                    HTTPStatus.BAD_REQUEST,
                    "text/plain; charset=utf-8",
                    b"JSON body must be an object\n",
                )
                return
            parsed = cast("JsonObject", parsed_value)
            with self.server.state.lock:
                if path == "/result":
                    self.server.state.result = parsed
                else:
                    self.server.state.error = str(parsed.get("error", "browser error"))
                self.server.state.done.set()
            self._reply(HTTPStatus.OK, "application/json", b'{"ok":true}\n')
            return
        self._reply(HTTPStatus.NOT_FOUND, "text/plain; charset=utf-8", b"not found\n")


class _Server(ThreadingHTTPServer):
    state: _State

    def __init__(self, state: _State) -> None:
        super().__init__(("127.0.0.1", 0), _Handler)
        self.state = state


def _utc_now() -> str:
    return datetime.now(UTC).isoformat().replace("+00:00", "Z")


def _finite_number(value: object, label: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise PerformanceError(f"{label} must be a number")
    number = float(value)
    if not math.isfinite(number) or number < 0:
        raise PerformanceError(f"{label} must be a finite non-negative number")
    return number


def nearest_rank(values: list[float], quantile: float) -> float:
    """Return the nearest-rank quantile used by the acceptance contract."""
    if not values:
        raise PerformanceError("cannot calculate statistics for an empty sample set")
    ordered = sorted(values)
    index = max(0, math.ceil(quantile * len(ordered)) - 1)
    return ordered[index]


def _metric_summary(
    metric: str, payload: object, expected_count: int
) -> tuple[dict[str, Any], dict[str, Any]]:
    if not isinstance(payload, dict):
        raise PerformanceError(f"metric {metric} payload must be an object")
    payload_object = cast("JsonObject", payload)
    raw_value = payload_object.get("samples")
    if not isinstance(raw_value, list):
        raise PerformanceError(f"metric {metric} samples must be a list")
    raw = cast("list[object]", raw_value)
    if len(raw) != expected_count:
        actual = len(raw)
        raise PerformanceError(
            f"metric {metric} sample count {actual} != {expected_count}"
        )
    samples = [
        _finite_number(value, f"{metric}.samples[{index}]")
        for index, value in enumerate(raw)
    ]
    duration_ms = _finite_number(
        payload_object.get("duration_ms"), f"{metric}.duration_ms"
    )
    if duration_ms <= 0:
        raise PerformanceError(f"metric {metric} duration must be positive")
    summary = {
        "metric": metric,
        "unit": METRIC_UNITS[metric],
        "sample_count": len(samples),
        "p50": nearest_rank(samples, 0.50),
        "p95": nearest_rank(samples, 0.95),
        "maximum": max(samples),
    }
    detail = {
        "p99": nearest_rank(samples, 0.99),
        "throughput_per_second": len(samples) / (duration_ms / 1000.0),
        "duration_ms": duration_ms,
        "samples": samples,
    }
    return summary, detail


def _absolute_passes(summary: dict[str, Any]) -> tuple[bool, dict[str, Any]]:
    metric = cast("str", summary["metric"])
    field, operator, threshold = ABSOLUTE_THRESHOLDS[metric]
    observed = float(summary[field])
    if operator == "<":
        passed = observed < threshold
    elif operator == "<=":
        passed = observed <= threshold
    else:
        passed = observed >= threshold
    return passed, {
        "kind": "absolute",
        "field": field,
        "operator": operator,
        "threshold": threshold,
        "observed": observed,
        "passed": passed,
    }


def _baseline_report_paths(path: Path) -> tuple[Path, ...]:
    if path.is_dir():
        paths = tuple(sorted(path.glob("*.json")))
        if not paths:
            raise PerformanceError("baseline directory contains no JSON reports")
        return paths
    return (path,)


def _load_baseline(
    path: Path | None,
    platform_name: str,
    *,
    fixture_id: str | None = None,
) -> dict[str, dict[str, Any]] | None:
    if path is None:
        return None
    reports: list[dict[str, dict[str, Any]]] = []
    sample_counts: list[int] = []
    for report_path in _baseline_report_paths(path):
        try:
            document_value: object = json.loads(report_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            raise PerformanceError(
                f"unable to read baseline {report_path}: {error}"
            ) from error
        if not isinstance(document_value, dict):
            raise PerformanceError("baseline must be an object")
        document = cast("JsonObject", document_value)
        if document.get("result") != "passed":
            raise PerformanceError("baseline result must be passed")
        if document.get("platform") != platform_name:
            raise PerformanceError("baseline platform does not match the current run")
        if fixture_id is not None and document.get("fixture_id") != fixture_id:
            raise PerformanceError("baseline fixture does not match the current run")
        sample_count = document.get("sample_count_required")
        if (
            not isinstance(sample_count, int)
            or sample_count < MIN_ACCEPTANCE_SAMPLE_COUNT
        ):
            raise PerformanceError("baseline does not contain 300 samples per metric")
        sample_counts.append(sample_count)
        warmup_seconds = document.get("warmup_seconds_required")
        if (
            not isinstance(warmup_seconds, int)
            or warmup_seconds < MIN_ACCEPTANCE_WARMUP_SECONDS
        ):
            raise PerformanceError("baseline does not contain a 60 second warm-up")
        measurements = document.get("measurements")
        if not isinstance(measurements, list):
            raise PerformanceError("baseline must contain a measurements array")
        measurements_list = cast("list[object]", measurements)
        report: dict[str, dict[str, Any]] = {}
        for measurement_value in measurements_list:
            if not isinstance(measurement_value, dict):
                raise PerformanceError("baseline contains an invalid measurement")
            measurement = cast("JsonObject", measurement_value)
            metric = measurement.get("metric")
            if not isinstance(metric, str):
                raise PerformanceError(
                    "baseline contains an invalid measurement metric"
                )
            if (
                metric not in METRIC_UNITS
                or measurement.get("unit") != METRIC_UNITS[metric]
            ):
                raise PerformanceError("baseline contains an invalid measurement unit")
            for field in ("p50", "p95", "maximum"):
                _finite_number(measurement.get(field), f"baseline.{metric}.{field}")
            report[metric] = measurement
        if len(measurements_list) != len(METRIC_ORDER) or set(report) != set(
            METRIC_ORDER
        ):
            raise PerformanceError(
                "baseline must contain exactly the five performance metrics"
            )
        reports.append(report)

    aggregated: dict[str, dict[str, Any]] = {}
    for metric in METRIC_ORDER:
        relative_field = "p50" if metric == "ui_frame_rate" else "p95"
        relative_values = [float(report[metric][relative_field]) for report in reports]
        aggregated[metric] = {
            "metric": metric,
            "unit": METRIC_UNITS[metric],
            "sample_count": min(sample_counts),
            "p50": nearest_rank(
                [float(report[metric]["p50"]) for report in reports], 0.5
            ),
            "p95": nearest_rank(
                [float(report[metric]["p95"]) for report in reports], 0.5
            ),
            "maximum": nearest_rank(
                [float(report[metric]["maximum"]) for report in reports], 0.5
            ),
            "relative_high": max(relative_values),
            "relative_low": min(relative_values),
        }
    return aggregated


def _regression_evaluation(
    summary: dict[str, Any], baseline: dict[str, dict[str, Any]] | None
) -> dict[str, Any]:
    metric = cast("str", summary["metric"])
    if baseline is None:
        return {"kind": "bootstrap", "passed": True, "baseline": None}
    baseline_measurement = baseline[metric]
    if metric == "ui_frame_rate":
        field = "p50"
        factor = REGRESSION_FACTOR_FPS
        operator = ">="
    else:
        field = "p95"
        factor = REGRESSION_FACTOR_LATENCY
        operator = "<="
    baseline_value = _finite_number(
        baseline_measurement.get(field), f"baseline.{metric}.{field}"
    )
    observed = float(summary[field])
    if baseline_value == 0.0:
        # A zero baseline cannot define a meaningful multiplicative threshold.
        # Keep the gate fail-closed by using the metric's absolute threshold.
        _, absolute = _absolute_passes(summary)
        return {
            "kind": "absolute_zero_baseline",
            "field": field,
            "operator": absolute["operator"],
            "factor": factor,
            "baseline": baseline_value,
            "threshold": absolute["threshold"],
            "observed": observed,
            "passed": absolute["passed"],
        }
    if operator == ">=":
        extreme_key = "relative_low"
        threshold_rule = "min(median*factor, historical_low)"
        baseline_extreme = _finite_number(
            baseline_measurement.get(extreme_key, baseline_value),
            f"baseline.{metric}.{extreme_key}",
        )
        threshold = min(baseline_value * factor, baseline_extreme)
    else:
        extreme_key = "relative_high"
        threshold_rule = "max(median*factor, historical_high)"
        baseline_extreme = _finite_number(
            baseline_measurement.get(extreme_key, baseline_value),
            f"baseline.{metric}.{extreme_key}",
        )
        threshold = max(baseline_value * factor, baseline_extreme)
    passed = observed >= threshold if operator == ">=" else observed <= threshold
    return {
        "kind": "relative",
        "field": field,
        "operator": operator,
        "factor": factor,
        "baseline": baseline_value,
        "baseline_extreme": baseline_extreme,
        "threshold_rule": threshold_rule,
        "threshold": threshold,
        "observed": observed,
        "passed": passed,
    }


def _find_browser(explicit: str | None) -> Path:
    if explicit:
        candidate = Path(explicit)
        if not candidate.is_file():
            raise PerformanceError(
                f"browser executable is not a regular file: {candidate}"
            )
        return candidate
    candidates = [
        os.environ.get("POKECON_PERFORMANCE_BROWSER", ""),
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "msedge",
        os.path.join(
            os.environ.get("PROGRAMFILES", ""),
            "Google",
            "Chrome",
            "Application",
            "chrome.exe",
        ),
        os.path.join(
            os.environ.get("PROGRAMFILES(X86)", ""),
            "Google",
            "Chrome",
            "Application",
            "chrome.exe",
        ),
        os.path.join(
            os.environ.get("PROGRAMFILES", ""),
            "Microsoft",
            "Edge",
            "Application",
            "msedge.exe",
        ),
        os.path.join(
            os.environ.get("PROGRAMFILES(X86)", ""),
            "Microsoft",
            "Edge",
            "Application",
            "msedge.exe",
        ),
    ]
    for value in candidates:
        if not value:
            continue
        resolved = shutil.which(value) or value
        candidate = Path(resolved)
        if candidate.is_file():
            return candidate
    raise PerformanceError("no Chromium/Chrome/Edge executable was found")


def _browser_name(path: Path) -> str:
    name = path.name.lower()
    return "edge" if "edge" in name or name == "msedge.exe" else "chrome"


def _sha256(path: Path | None) -> str | None:
    if path is None:
        return None
    if not path.is_file():
        raise PerformanceError(f"artifact is not a regular file: {path}")
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _run_browser(
    browser: Path, page: str, timeout_seconds: float
) -> tuple[dict[str, Any], str, str]:
    state = _State(page)
    server = _Server(state)
    thread = threading.Thread(
        target=server.serve_forever, name="performance-http", daemon=True
    )
    thread.start()
    with tempfile.TemporaryDirectory(prefix="pokecon-performance-browser-") as profile:
        command = [
            str(browser),
            "--headless=new",
            "--use-gl=swiftshader",
            "--disable-dev-shm-usage",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "--window-size=1920,1080",
            f"--user-data-dir={profile}",
            f"http://127.0.0.1:{server.server_port}/benchmark",
        ]
        if os.name != "nt":
            command.insert(2, "--no-sandbox")
        try:
            process = subprocess.Popen(  # noqa: S603 - executable is discovered/validated above.
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
        except OSError as error:
            server.shutdown()
            raise PerformanceError(f"unable to start browser: {error}") from error
        try:
            if not state.done.wait(timeout_seconds):
                raise PerformanceError("browser performance fixture timed out")
            with state.lock:
                error = state.error
                result = state.result
            if error:
                raise PerformanceError(error)
            if result is None:
                raise PerformanceError("browser finished without a result")
        finally:
            if process.poll() is None:
                process.terminate()
            try:
                stdout, stderr = process.communicate(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                stdout, stderr = process.communicate()
            server.shutdown()
            thread.join(timeout=5)
    return result, stdout[-10000:], stderr[-20000:]


def _record_for_failure(
    *,
    source_commit: str,
    platform_name: str,
    build_identity: str,
    started_at: str,
    error: str,
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "specification_version": "2.2.0",
        "source_commit": source_commit,
        "capability": "performance",
        "platform": platform_name,
        "started_at": started_at,
        "completed_at": _utc_now(),
        "operator": "GitHub Actions performance fixture",
        "environment": {
            "os_name": platform_module.system() or platform_name,
            "os_version": platform_module.platform() or "unknown",
            "app_mode": "headless",
            "build_identity": build_identity,
            "device_inventory": ["local browser loopback fixture"],
            "browser": {
                "name": "chrome",
                "version": "unknown",
                "client_os": platform_name,
            },
        },
        "requirement_ids": ["§7.2", "§7.3", "§7.9", "§11.1"],
        "steps": [
            {
                "id": "measurement_environment",
                "result": "failed",
                "evidence": ["performance-report.json"],
            },
            {
                "id": "warmup",
                "result": "failed",
                "evidence": ["performance-report.json"],
            },
            {
                "id": "sample_collection",
                "result": "failed",
                "evidence": ["performance-report.json"],
            },
            {
                "id": "threshold_evaluation",
                "result": "failed",
                "evidence": ["performance-report.json"],
            },
        ],
        "measurements": [],
        "result": "failed",
        "notes": f"CI loopback performance fixture failed closed: {error}",
    }


def run(args: argparse.Namespace) -> int:
    source_commit = args.source_commit
    if not SOURCE_COMMIT_PATTERN.fullmatch(source_commit):
        raise PerformanceError(
            "source commit must be 40 lowercase hexadecimal characters"
        )
    if args.samples < 1 or args.samples > 10000:
        raise PerformanceError("samples must be between 1 and 10000")
    if args.warmup_seconds < 0 or args.warmup_seconds > 3600:
        raise PerformanceError("warmup-seconds must be between 0 and 3600")
    short_run = (
        args.samples < MIN_ACCEPTANCE_SAMPLE_COUNT
        or args.warmup_seconds < MIN_ACCEPTANCE_WARMUP_SECONDS
    )
    if short_run and not args.allow_short_run:
        raise PerformanceError(
            "acceptance requires 60 seconds warm-up and 300 samples; "
            "use --allow-short-run only for debug runs"
        )
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    report_path = output.with_name("performance-report.json")
    samples_path = output.with_name("performance-samples.json")
    browser_log_path = output.with_name("performance-browser.log")
    started_at = _utc_now()
    build_identity = args.build_identity or source_commit
    baseline_path = Path(args.baseline) if args.baseline else None
    fixture_id = os.environ.get(
        "POKECON_PERFORMANCE_FIXTURE_ID", PERFORMANCE_FIXTURE_ID
    )
    baseline_paths = (
        _baseline_report_paths(baseline_path) if baseline_path is not None else ()
    )
    browser = _find_browser(args.browser)
    browser_name = _browser_name(browser)
    stdout = ""
    stderr = ""
    try:
        version_output = subprocess.run(  # noqa: S603 - validated executable and fixed argument.
            [str(browser), "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=20,
        )
        browser_version = (
            version_output.stdout or version_output.stderr
        ).strip() or "unknown"
    except (OSError, subprocess.SubprocessError) as error:
        raise PerformanceError(f"unable to query browser version: {error}") from error
    baseline = _load_baseline(
        baseline_path,
        args.platform,
        fixture_id=fixture_id,
    )
    if args.require_baseline and baseline is None:
        raise PerformanceError("--require-baseline needs --baseline")
    page = PAGE_TEMPLATE.replace("__SAMPLE_COUNT__", str(args.samples)).replace(
        "__WARMUP_SECONDS__", str(args.warmup_seconds)
    )
    try:
        browser_result, stdout, stderr = _run_browser(
            browser, page, args.timeout_seconds
        )
    except PerformanceError as error:
        failure = _record_for_failure(
            source_commit=source_commit,
            platform_name=args.platform,
            build_identity=build_identity,
            started_at=started_at,
            error=str(error),
        )
        output.write_text(
            json.dumps(failure, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        report_path.write_text(
            json.dumps({"error": str(error)}, indent=2) + "\n", encoding="utf-8"
        )
        browser_log_path.write_text(
            f"browser process output unavailable after failure:\n{error}\n",
            encoding="utf-8",
        )
        raise
    browser_log_path.write_text(
        f"stdout:\n{stdout}\nstderr:\n{stderr}\n", encoding="utf-8"
    )
    if not isinstance(browser_result, dict) or not isinstance(
        browser_result.get("metrics"), dict
    ):
        raise PerformanceError("browser result has no metrics object")
    raw_metrics = cast("dict[str, Any]", browser_result["metrics"])
    if set(raw_metrics) != set(METRIC_ORDER):
        raise PerformanceError(
            "browser result must contain exactly the five performance metrics"
        )
    measurements: list[dict[str, Any]] = []
    details: dict[str, Any] = {}
    all_passed = True
    evaluations: dict[str, Any] = {}
    for metric in METRIC_ORDER:
        summary, detail = _metric_summary(metric, raw_metrics[metric], args.samples)
        absolute_passed, absolute = _absolute_passes(summary)
        regression = _regression_evaluation(summary, baseline)
        all_passed = all_passed and absolute_passed and bool(regression["passed"])
        measurements.append(summary)
        details[metric] = detail
        evaluations[metric] = {"absolute": absolute, "regression": regression}
    completed_at = _utc_now()
    browser_result["metrics"] = {metric: details[metric] for metric in METRIC_ORDER}
    browser_result["source_commit"] = source_commit
    browser_result["platform"] = args.platform
    browser_result["browser"] = {"name": browser_name, "version": browser_version}
    browser_result["sample_count_required"] = args.samples
    browser_result["warmup_seconds_required"] = args.warmup_seconds
    samples_path.write_text(
        json.dumps(browser_result, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    report = {
        "schema_version": 1,
        "source_commit": source_commit,
        "platform": args.platform,
        "fixture_id": fixture_id,
        "browser": {"name": browser_name, "version": browser_version},
        "fixture": browser_result.get("fixture", {}),
        "warmup": browser_result.get("warmup", {}),
        "sample_count_required": args.samples,
        "warmup_seconds_required": args.warmup_seconds,
        "measurements": measurements,
        "statistics": details,
        "threshold_evaluation": evaluations,
        "baseline": {
            "status": "compared" if baseline is not None else "bootstrap",
            "path": args.baseline,
            "report_count": len(baseline_paths),
            "aggregation": (
                "nearest-rank median with historical relative extremes"
                if len(baseline_paths) > 1
                else ("single report" if baseline_paths else None)
            ),
        },
        "result": (
            "debug_only" if short_run else ("passed" if all_passed else "failed")
        ),
        "debug_only": short_run,
        "started_at": started_at,
        "completed_at": completed_at,
    }
    report_path.write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    if short_run:
        raise PerformanceError(
            "shortened run is debug-only; no acceptance record was written"
        )
    record = {
        "schema_version": 1,
        "specification_version": "2.2.0",
        "source_commit": source_commit,
        "capability": "performance",
        "platform": args.platform,
        "started_at": started_at,
        "completed_at": completed_at,
        "operator": "GitHub Actions performance fixture",
        "environment": {
            "os_name": platform_module.system() or args.platform,
            "os_version": platform_module.platform() or "unknown",
            "app_mode": "headless",
            "build_identity": build_identity,
            "fixture_id": fixture_id,
            "device_inventory": [
                "local multipart MJPEG server",
                "Canvas WebRTC loopback",
                "MessageChannel controller loopback",
            ],
            "browser": {
                "name": browser_name,
                "version": browser_version,
                "client_os": args.platform,
            },
        },
        "requirement_ids": ["§7.2", "§7.3", "§7.9", "§11.1"],
        "steps": [
            {
                "id": "measurement_environment",
                "result": "passed",
                "evidence": [report_path.name, browser_log_path.name],
            },
            {
                "id": "warmup",
                "result": "passed",
                "evidence": [report_path.name, samples_path.name],
            },
            {
                "id": "sample_collection",
                "result": "passed",
                "evidence": [samples_path.name, report_path.name],
            },
            {
                "id": "threshold_evaluation",
                "result": "passed" if all_passed else "failed",
                "evidence": [report_path.name],
            },
        ],
        "measurements": measurements,
        "result": "passed" if all_passed else "failed",
        "notes": (
            "CI-only loopback fixture; physical camera, serial, controller, and console latency are not measured. "
            + (
                "Baseline comparison was bootstrapped because no prior report was supplied."
                if baseline is None
                else (
                    "Baseline regression thresholds used the nearest-rank median "
                    f"and historical relative extremes of {len(baseline_paths)} "
                    "passing report(s)."
                )
            )
        ),
    }
    artifact_sha256 = _sha256(Path(args.artifact) if args.artifact else None)
    if artifact_sha256 is not None:
        environment = cast("dict[str, Any]", record["environment"])
        environment["artifact_sha256"] = artifact_sha256
    output.write_text(
        json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    if not all_passed:
        failed_metrics = {
            metric: evaluations[metric]
            for metric in METRIC_ORDER
            if not evaluations[metric]["absolute"]["passed"]
            or not evaluations[metric]["regression"]["passed"]
        }
        print(
            "performance gate evaluations: "
            + json.dumps(
                {"failed_metrics": failed_metrics, "report": str(report_path)},
                sort_keys=True,
            ),
            file=sys.stderr,
        )
        raise PerformanceError("performance threshold or regression gate failed")
    return 0


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--output", required=True, help="path for performance-record.json"
    )
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--platform", choices=("linux", "windows"), required=True)
    parser.add_argument("--build-identity")
    parser.add_argument("--artifact")
    parser.add_argument("--browser")
    parser.add_argument("--baseline")
    parser.add_argument("--require-baseline", action="store_true")
    parser.add_argument(
        "--allow-short-run",
        action="store_true",
        help="debug only; never emit an acceptance record",
    )
    parser.add_argument("--samples", type=int, default=SAMPLE_COUNT)
    parser.add_argument("--warmup-seconds", type=int, default=WARMUP_SECONDS)
    parser.add_argument("--timeout-seconds", type=float, default=240.0)
    return parser


def main() -> int:
    try:
        return run(_parser().parse_args())
    except PerformanceError as error:
        print(f"performance gate: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
