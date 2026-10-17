from __future__ import annotations

import json
import os
import stat
import subprocess
import tempfile
from pathlib import Path
from typing import Any

REPOSITORY = Path(__file__).resolve().parents[2]
SCRIPT = REPOSITORY / "scripts/ci-watch.sh"

JsonObject = dict[str, Any]

SHA = "a" * 40
BRANCH = "test-branch"


def _write_executable(path: Path, content: str) -> None:
    path.write_text(content, encoding="utf-8")
    path.chmod(path.stat().st_mode | stat.S_IEXEC)


def _fake_bin(
    tmp_path: Path,
    *,
    gh_response: str | None,
    gh_exit: int = 0,
    gh_stderr: str = "",
) -> tuple[Path, Path]:
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    epoch_file = tmp_path / "fake_epoch"
    epoch_file.write_text("1000", encoding="utf-8")

    _write_executable(
        bin_dir / "date",
        f"""#!/usr/bin/env bash
cat "{epoch_file}"
""",
    )
    _write_executable(
        bin_dir / "sleep",
        f"""#!/usr/bin/env bash
current=$(cat "{epoch_file}")
add=${{1:-0}}
echo $((current + add)) > "{epoch_file}"
""",
    )
    _write_executable(
        bin_dir / "git",
        f"""#!/usr/bin/env bash
if [[ "$1" == "branch" && "$2" == "--show-current" ]]; then
  echo "{BRANCH}"
  exit 0
fi
if [[ "$1" == "rev-parse" ]]; then
  echo "{SHA}"
  exit 0
fi
echo "unexpected git call: $@" >&2
exit 2
""",
    )
    if gh_response is None:
        gh_body = f"""#!/usr/bin/env bash
echo "{gh_stderr}" >&2
exit {gh_exit}
"""
    else:
        response_file = tmp_path / "gh_response.json"
        response_file.write_text(gh_response, encoding="utf-8")
        gh_body = f"""#!/usr/bin/env bash
if [[ "$1" == "api" ]]; then
  cat "{response_file}"
  exit {gh_exit}
fi
echo "unexpected gh call: $@" >&2
exit 2
"""
    _write_executable(bin_dir / "gh", gh_body)

    return bin_dir, epoch_file


def _slurp(pages: list[JsonObject]) -> str:
    return json.dumps(pages)


def _page(check_runs: list[JsonObject]) -> JsonObject:
    return {"total_count": len(check_runs), "check_runs": check_runs}


def _run(
    check_runs: list[JsonObject],
    *,
    pages: list[list[JsonObject]] | None = None,
    timeout: str = "130",
    gh_exit: int = 0,
    extra_env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess[str]:
    if pages is not None:
        slurp = _slurp([_page(p) for p in pages])
    else:
        slurp = _slurp([_page(check_runs)])

    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        bin_dir, _ = _fake_bin(tmp_path, gh_response=slurp, gh_exit=gh_exit)
        env = os.environ.copy()
        env["PATH"] = f"{bin_dir}:{env.get('PATH', '')}"
        if extra_env:
            env.update(extra_env)
        return subprocess.run(  # noqa: S603 - fixed repository script and fake helpers
            [str(SCRIPT), BRANCH, timeout],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
            env=env,
            cwd=tmp_path,
        )


def _check_run(
    name: str,
    *,
    status: str = "completed",
    conclusion: str | None = "success",
    head_sha: str = SHA,
    check_id: int,
    html_url: str = "https://example.com/run",
    completed_at: str | None = "2026-09-07T01:00:00Z",
    started_at: str | None = "2026-09-07T00:59:00Z",
) -> JsonObject:
    return {
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "head_sha": head_sha,
        "id": check_id,
        "html_url": html_url,
        "completed_at": completed_at,
        "started_at": started_at,
    }


def test_help_documents_required_contexts_and_exit_statuses() -> None:
    completed = subprocess.run(  # noqa: S603 - fixed repository script
        [str(SCRIPT), "--help"],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    assert completed.returncode == 0
    assert "Normal CI Required" in completed.stdout
    assert "Package CI Required" in completed.stdout
    assert "exit statuses" in completed.stdout
    assert "124" in completed.stdout
    assert "Until CI has one aggregate workflow" not in completed.stdout
    assert (
        "workflow run will not appear after the settlement window"
        not in completed.stdout
    )


def test_success_both_required_success_settles() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="success"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 0
    assert "all required checks completed successfully" in result.stdout
    assert "settlement window" in result.stdout


def test_failure_required_failed_is_distinct_from_timeout() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="failure"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 1
    assert "required check failed" in result.stderr
    assert "Normal CI Required" in result.stderr
    timeout_checks: list[JsonObject] = []
    timeout_result = _run(timeout_checks, timeout="130")
    assert timeout_result.returncode == 124
    assert timeout_result.returncode != result.returncode


def test_timeout_when_missing_required_context() -> None:
    result = _run([], timeout="130")
    assert result.returncode == 124
    assert "timed out" in result.stderr


def test_in_progress_is_pending_not_failure() -> None:
    checks = [
        _check_run(
            "Normal CI Required",
            status="in_progress",
            conclusion=None,
            check_id=1,
            completed_at=None,
        ),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks, timeout="130")
    assert result.returncode == 124
    assert "required check failed" not in result.stderr


def test_neutral_is_failure() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="neutral"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 1
    assert "neutral" in result.stderr


def test_skipped_is_failure() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="skipped"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 1


def test_cancelled_is_failure() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="cancelled"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 1


def test_timed_out_conclusion_is_failure() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="timed_out"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 1


def test_stale_sha_is_treated_as_missing() -> None:
    checks = [
        _check_run(
            "Normal CI Required", check_id=1, head_sha="b" * 40, conclusion="success"
        ),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
    ]
    result = _run(checks, timeout="130")
    assert result.returncode == 124


def test_duplicate_latest_success_wins() -> None:
    checks = [
        _check_run(
            "Normal CI Required",
            check_id=1,
            conclusion="failure",
            completed_at="2026-09-07T01:00:00Z",
            started_at="2026-09-07T00:59:00Z",
        ),
        _check_run(
            "Normal CI Required",
            check_id=2,
            conclusion="success",
            completed_at="2026-09-07T01:01:00Z",
            started_at="2026-09-07T01:00:30Z",
        ),
        _check_run("Package CI Required", check_id=3, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 0


def test_duplicate_latest_failure_fails() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="success"),
        _check_run("Normal CI Required", check_id=2, conclusion="failure"),
        _check_run("Package CI Required", check_id=3, conclusion="success"),
    ]
    result = _run(checks)
    assert result.returncode == 1


def test_optional_checks_ignored() -> None:
    checks = [
        _check_run("Normal CI Required", check_id=1, conclusion="success"),
        _check_run("Package CI Required", check_id=2, conclusion="success"),
        _check_run("Some Optional Check", check_id=3, conclusion="failure"),
        _check_run(
            "Another Optional",
            status="in_progress",
            conclusion=None,
            check_id=4,
            completed_at=None,
        ),
    ]
    result = _run(checks)
    assert result.returncode == 0


def test_pagination_aggregate_across_pages() -> None:
    page1 = [_check_run("Normal CI Required", check_id=1, conclusion="success")]
    page2 = [_check_run("Package CI Required", check_id=2, conclusion="success")]
    result = _run([], pages=[page1, page2])
    assert result.returncode == 0


def test_pagination_duplicate_across_pages_latest_wins() -> None:
    page1 = [_check_run("Normal CI Required", check_id=1, conclusion="failure")]
    page2 = [
        _check_run("Normal CI Required", check_id=5, conclusion="success"),
        _check_run("Package CI Required", check_id=6, conclusion="success"),
    ]
    result = _run([], pages=[page1, page2])
    assert result.returncode == 0

    page1_success = [_check_run("Normal CI Required", check_id=1, conclusion="success")]
    page2_failure = [
        _check_run("Normal CI Required", check_id=10, conclusion="failure"),
        _check_run("Package CI Required", check_id=11, conclusion="success"),
    ]
    result2 = _run([], pages=[page1_success, page2_failure])
    assert result2.returncode == 1


def test_branch_and_timeout_cli_preserved() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)
        bin_dir, _ = _fake_bin(tmp_path, gh_response=_slurp([_page([])]))
        env = os.environ.copy()
        env["PATH"] = f"{bin_dir}:{env.get('PATH', '')}"
        completed = subprocess.run(  # noqa: S603 - fixed repository script
            [str(SCRIPT), "a", "b", "c"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
            env=env,
            cwd=tmp_path,
        )
        assert completed.returncode == 2
        completed2 = subprocess.run(  # noqa: S603 - fixed repository script
            [str(SCRIPT), BRANCH, "not-a-number"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
            env=env,
            cwd=tmp_path,
        )
        assert completed2.returncode == 2
        completed3 = subprocess.run(  # noqa: S603 - fixed repository script
            [str(SCRIPT), BRANCH, "10"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
            env=env,
            cwd=tmp_path,
        )
        assert completed3.returncode == 2
