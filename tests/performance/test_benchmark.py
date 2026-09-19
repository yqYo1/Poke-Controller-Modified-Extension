from __future__ import annotations

import json
from typing import TYPE_CHECKING

import pytest

from scripts.performance.benchmark import (
    MIN_ACCEPTANCE_SAMPLE_COUNT,
    MIN_ACCEPTANCE_WARMUP_SECONDS,
    PERFORMANCE_FIXTURE_ID,
    PerformanceError,
    _load_baseline,
    _metric_summary,
    _regression_evaluation,
    nearest_rank,
)

if TYPE_CHECKING:
    from pathlib import Path


def test_acceptance_minimums_match_the_ci_contract() -> None:
    assert MIN_ACCEPTANCE_SAMPLE_COUNT == 300
    assert MIN_ACCEPTANCE_WARMUP_SECONDS == 60


def _write_baseline(path: Path, *, mjpeg_p95: float, webrtc_p95: float) -> None:
    measurements = [
        {
            "metric": "webrtc_video_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": webrtc_p95 - 5.0,
            "p95": webrtc_p95,
            "maximum": webrtc_p95 + 5.0,
        },
        {
            "metric": "mjpeg_video_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": mjpeg_p95 - 1.0,
            "p95": mjpeg_p95,
            "maximum": mjpeg_p95 + 1.0,
        },
        {
            "metric": "controller_input_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": 0.0,
            "p95": 0.1,
            "maximum": 0.2,
        },
        {
            "metric": "ui_frame_rate",
            "unit": "fps",
            "sample_count": 300,
            "p50": 60.0,
            "p95": 60.0,
            "maximum": 60.0,
        },
        {
            "metric": "ui_input_latency",
            "unit": "ms",
            "sample_count": 300,
            "p50": 0.0,
            "p95": 0.1,
            "maximum": 0.2,
        },
    ]
    path.write_text(
        json.dumps(
            {
                "result": "passed",
                "platform": "linux",
                "fixture_id": PERFORMANCE_FIXTURE_ID,
                "sample_count_required": 300,
                "warmup_seconds_required": 60,
                "measurements": measurements,
            }
        ),
        encoding="utf-8",
    )


def test_nearest_rank_uses_contract_nearest_rank() -> None:
    assert nearest_rank([4.0, 1.0, 3.0, 2.0], 0.5) == 2.0
    assert nearest_rank([4.0, 1.0, 3.0, 2.0], 0.95) == 4.0


def test_baseline_directory_uses_nearest_rank_median(tmp_path: Path) -> None:
    baseline_dir = tmp_path / "baselines"
    baseline_dir.mkdir()
    for index, mjpeg_p95 in enumerate((6.2, 8.0, 8.1, 8.5, 8.9)):
        _write_baseline(
            baseline_dir / f"{index:02d}.json",
            mjpeg_p95=mjpeg_p95,
            webrtc_p95=50.0 + index,
        )

    baseline = _load_baseline(baseline_dir, "linux")

    assert baseline is not None
    assert baseline["mjpeg_video_latency"]["p95"] == 8.1
    assert _regression_evaluation(
        {"metric": "mjpeg_video_latency", "p95": 8.9}, baseline
    )["passed"]
    assert not _regression_evaluation(
        {"metric": "mjpeg_video_latency", "p95": 9.0}, baseline
    )["passed"]


def test_baseline_fixture_identity_is_validated(tmp_path: Path) -> None:
    baseline_path = tmp_path / "performance-report.json"
    _write_baseline(baseline_path, mjpeg_p95=8.0, webrtc_p95=50.0)

    assert (
        _load_baseline(baseline_path, "linux", fixture_id=PERFORMANCE_FIXTURE_ID)
        is not None
    )
    with pytest.raises(PerformanceError, match="fixture"):
        _load_baseline(baseline_path, "linux", fixture_id="different-fixture")


def test_metric_summary_requires_exact_sample_count() -> None:
    with pytest.raises(PerformanceError, match="sample count"):
        _metric_summary(
            "controller_input_latency", {"samples": [1.0], "duration_ms": 1.0}, 2
        )


def test_metric_summary_contains_p50_p95_maximum_and_p99() -> None:
    summary, detail = _metric_summary(
        "controller_input_latency",
        {"samples": [1.0, 2.0, 3.0, 4.0], "duration_ms": 100.0},
        4,
    )
    assert summary == {
        "metric": "controller_input_latency",
        "unit": "ms",
        "sample_count": 4,
        "p50": 2.0,
        "p95": 4.0,
        "maximum": 4.0,
    }
    assert detail["p99"] == 4.0
    assert detail["throughput_per_second"] == 40.0


def test_latency_regression_is_limited_to_ten_percent() -> None:
    baseline = {"controller_input_latency": {"p95": 10.0}}
    assert _regression_evaluation(
        {"metric": "controller_input_latency", "p95": 10.9}, baseline
    )["passed"]
    assert not _regression_evaluation(
        {"metric": "controller_input_latency", "p95": 11.1}, baseline
    )["passed"]


def test_zero_latency_baseline_uses_absolute_threshold() -> None:
    baseline = {"ui_input_latency": {"p95": 0.0}}
    accepted = _regression_evaluation(
        {"metric": "ui_input_latency", "p95": 0.1}, baseline
    )
    assert accepted["kind"] == "absolute_zero_baseline"
    assert accepted["threshold"] == 16.0
    assert accepted["passed"]
    assert not _regression_evaluation(
        {"metric": "ui_input_latency", "p95": 16.0}, baseline
    )["passed"]


def test_frame_rate_regression_is_limited_to_five_percent() -> None:
    baseline = {"ui_frame_rate": {"p50": 60.0}}
    assert _regression_evaluation({"metric": "ui_frame_rate", "p50": 57.1}, baseline)[
        "passed"
    ]
    assert not _regression_evaluation(
        {"metric": "ui_frame_rate", "p50": 56.9}, baseline
    )["passed"]


def test_missing_baseline_is_explicit_bootstrap() -> None:
    evaluation = _regression_evaluation({"metric": "ui_frame_rate", "p50": 60.0}, None)
    assert evaluation == {"kind": "bootstrap", "passed": True, "baseline": None}


def test_short_baseline_cannot_disable_regression_gate(tmp_path: Path) -> None:
    baseline_path = tmp_path / "performance-report.json"
    baseline_path.write_text(
        '{"result":"passed","platform":"linux",'
        '"sample_count_required":20,"warmup_seconds_required":60,'
        '"measurements":[]}',
        encoding="utf-8",
    )
    with pytest.raises(PerformanceError, match="300 samples"):
        _load_baseline(baseline_path, "linux")
