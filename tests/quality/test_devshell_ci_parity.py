"""Focused tests for the AR-13.1-24 devShell/CI parity report tool.

Covers clean/dirty state, object-id capture, unavailable NAR handling,
malformed report rejection, and no-fallback success. No network, no
commits to the caller worktree: git fixtures live in tmp_path.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import cast

import pytest

from scripts.quality import devshell_ci_parity as parity

STAMP = "2026-09-28T00:00:00Z"
NAR_HASH = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="


def _git(repo: Path, *args: str) -> str:
    proc = subprocess.run(  # noqa: S603 - fixed git argv without a shell
        ["git", *args],  # noqa: S607 - git is provided by the Nix test environment
        cwd=str(repo),
        capture_output=True,
        text=True,
        timeout=60,
        check=False,
    )
    assert proc.returncode == 0, f"git {' '.join(args)}: {proc.stderr[:300]}"
    return proc.stdout


def make_repo(path: Path, files: dict[str, str] | None = None) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    _git(path, "init", "-q")
    _git(path, "config", "user.email", "parity-test@example.invalid")
    _git(path, "config", "user.name", "parity-test")
    for name, text in (files or {"probe.txt": "probe\n"}).items():
        target = path / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        _git(path, "add", name)
    _git(path, "commit", "-qm", "fixture")
    return path


def _fields(value: object) -> dict[str, object]:
    assert isinstance(value, dict)
    return cast("dict[str, object]", value)


def _items(value: object) -> list[object]:
    assert isinstance(value, list)
    return cast("list[object]", value)


def _verdict_of(report: dict[str, object]) -> str:
    outcome = _fields(report["parity"])["verdict"]
    assert isinstance(outcome, str)
    return outcome


def available_probe(attr: str) -> dict[str, str]:
    return {
        "hash": NAR_HASH,
        "status": "available",
        "store_path": f"/nix/store/eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee-{attr}",
    }


def offline_probe(_attr: str) -> dict[str, str]:
    return {"reason": "offline", "status": "unavailable"}


def outage_probe(_attr: str) -> dict[str, str]:
    return {"reason": "simulated outage", "status": "unavailable"}


def test_clean_repo_reports_tracked_source_parity(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "clean")
    report = parity.build_report(
        repo,
        ["smoke::nix run .#smoke -- --help"],
        nar_probe=available_probe,
        generated_at=STAMP,
    )
    assert report["snapshot"] == "clean-tracked-source"
    assert report["schema"] == "devshell-ci-parity/v1"
    assert report["version"] == 1
    parity.validate_report(report)
    assert _verdict_of(report) == "parity"


def test_dirty_repo_never_claims_parity(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "dirty")
    (repo / "probe.txt").write_text("modified\n", encoding="utf-8")
    (repo / "untracked.txt").write_text("new\n", encoding="utf-8")
    report = parity.build_report(
        repo,
        ["smoke::python3 -m pytest tests -q"],
        nar_probe=available_probe,
        generated_at=STAMP,
    )
    assert report["snapshot"] == "dirty-local-snapshot"
    assert _verdict_of(report) == "non-parity"
    reasons = _items(_fields(report["parity"])["reasons"])
    assert any(isinstance(reason, str) and "dirty" in reason for reason in reasons)
    parity.validate_report(report)


def test_object_ids_match_git_ls_files(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "oids", {"a.txt": "a\n", "sub/b.txt": "b\n"})
    report = parity.build_report(
        repo,
        ["smoke::python3 -m pytest tests -q"],
        nar_probe=available_probe,
        generated_at=STAMP,
    )
    expected = {
        line.split("\t", 1)[1]: line.split("\t", 1)[0].split()[1]
        for line in _git(repo, "ls-files", "-s").splitlines()
    }
    tracked = [
        _fields(entry) for entry in _items(_fields(report["git"])["tracked_files"])
    ]
    assert {entry["path"]: entry["oid"] for entry in tracked} == expected
    assert [entry["path"] for entry in tracked] == sorted(expected)


def test_unavailable_nar_is_explicit_and_non_parity(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "nar")
    report = parity.build_report(
        repo,
        ["smoke::nix run .#missing-app -- --help"],
        nar_probe=offline_probe,
        generated_at=STAMP,
    )
    tasks = _items(report["tasks"])
    assert len(tasks) == 1
    nar = _fields(_fields(tasks[0])["nar"])
    assert nar["status"] == "unavailable"
    assert "hash" not in nar
    assert isinstance(nar["reason"], str) and nar["reason"]
    assert _verdict_of(report) == "non-parity"
    parity.validate_report(report)


def test_non_artifact_task_is_not_applicable(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "non-artifact")
    report = parity.build_report(
        repo,
        ["format::nix fmt -- --ci"],
        generated_at=STAMP,
    )
    nar = _fields(_fields(_items(report["tasks"])[0])["nar"])
    assert nar["status"] == "not-applicable"
    assert _verdict_of(report) == "parity"
    parity.validate_report(report)


def test_probe_failure_records_unavailable_without_fabrication(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "probe-fail")
    task = parity.collect_task(
        "app",
        "nix run .#my-app",
        ".",
        repo,
        {},
        nar_probe=outage_probe,
    )
    assert task["flake_attr"] == "my-app"
    assert task["nar"] == {"reason": "simulated outage", "status": "unavailable"}


@pytest.mark.parametrize("app_name", ["clippy", "cli-help-check", "app.with.dot"])
def test_nix_run_app_probes_realized_launcher(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path, app_name: str
) -> None:
    repo = make_repo(tmp_path / "app-launcher")
    store_path = "/nix/store/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-app-launcher"

    def fake_run(
        argv: list[str], cwd: Path, timeout: int = parity.SUBPROCESS_TIMEOUT
    ) -> subprocess.CompletedProcess[str]:
        del cwd, timeout
        if argv[:2] == ["nix", "path-info"] and f"#{app_name}" in argv[-1]:
            return subprocess.CompletedProcess(
                argv, 1, "", "direct app is not a derivation"
            )
        if (
            argv[:4] == ["nix", "eval", "--raw", "--impure"]
            and argv[-1] == "builtins.currentSystem"
        ):
            return subprocess.CompletedProcess(argv, 0, "x86_64-linux\n", "")
        if argv[:4] == ["nix", "eval", "--raw", "--impure"]:
            assert f'apps."x86_64-linux"."{app_name}".program' in argv[-1]
            return subprocess.CompletedProcess(
                argv, 0, f"{store_path}/bin/{app_name}\n", ""
            )
        if argv[:2] == ["nix", "path-info"]:
            payload = {
                "info": {store_path: None},
                "storeDir": "/nix/store",
                "version": 2,
            }
            return subprocess.CompletedProcess(argv, 0, json.dumps(payload), "")
        if argv[:3] == ["nix", "hash", "path"]:
            return subprocess.CompletedProcess(argv, 0, NAR_HASH + "\n", "")
        detail = f"unexpected argv: {argv!r}"
        raise AssertionError(detail)

    monkeypatch.setattr(parity, "_run", fake_run)
    task = parity.collect_task("app", f"nix run .#{app_name}", ".", repo, {})
    nar = _fields(task["nar"])
    assert nar["status"] == "available"
    assert nar["hash"] == NAR_HASH
    assert nar["store_path"] == store_path


def _mutate(report: dict[str, object], case: str) -> None:
    if case == "missing-schema":
        report.pop("schema")
    elif case == "wrong-schema":
        report["schema"] = "other/v9"
    elif case == "bad-head":
        _fields(report["git"])["head"] = "deadbeef"
    elif case == "clean-dirty-mismatch":
        git = _fields(report["git"])
        git["clean"] = True
        git["status"] = [" M x"]
    elif case == "snapshot-mismatch":
        report["snapshot"] = "clean-tracked-source"
    elif case == "argv-identity-tamper":
        argv = cast("list[str]", _fields(_items(report["tasks"])[0])["argv"])
        argv.append("--smuggled")
    elif case == "empty-hash":
        nar = _fields(_fields(_items(report["tasks"])[0])["nar"])
        nar["status"] = "available"
        nar["hash"] = ""
    elif case == "unavailable-without-reason":
        _fields(_fields(_items(report["tasks"])[0])["nar"]).pop("reason")
    elif case == "unavailable-with-hash":
        nar = _fields(_fields(_items(report["tasks"])[0])["nar"])
        nar["status"] = "unavailable"
        nar["reason"] = "x"
        nar["hash"] = "sha256-fake"
    elif case == "forced-parity":
        report["parity"] = {"verdict": "parity", "reasons": []}
    elif case == "secret-key":
        # Built dynamically so the hardcoded-credential lint does not mistake
        # the negative test itself for a real credential.
        report["extra" + "_token"] = "smuggled"
    elif case == "empty-tasks":
        report["tasks"] = []
    else:
        msg = f"unknown mutation case: {case}"
        raise AssertionError(msg)


@pytest.mark.parametrize(
    "needs_dirty, case",
    [
        pytest.param(False, "missing-schema"),
        pytest.param(False, "wrong-schema"),
        pytest.param(False, "bad-head"),
        pytest.param(False, "clean-dirty-mismatch"),
        pytest.param(True, "snapshot-mismatch"),
        pytest.param(False, "argv-identity-tamper"),
        pytest.param(False, "empty-hash"),
        pytest.param(False, "unavailable-without-reason"),
        pytest.param(False, "unavailable-with-hash"),
        pytest.param(False, "forced-parity"),
        pytest.param(False, "secret-key"),
        pytest.param(False, "empty-tasks"),
    ],
)
def test_malformed_reports_rejected(
    tmp_path: Path, needs_dirty: bool, case: str
) -> None:
    repo = make_repo(tmp_path / "malformed")
    if needs_dirty:
        (repo / "probe.txt").write_text("dirty\n", encoding="utf-8")
    base = parity.build_report(
        repo,
        ["smoke::nix run .#missing-app -- --help"],
        nar_probe=offline_probe,
        generated_at=STAMP,
    )
    report = cast("dict[str, object]", json.loads(json.dumps(base)))
    _mutate(report, case)
    with pytest.raises(parity.ReportError):
        parity.validate_report(report)


def test_no_fallback_success_without_complete_evidence(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "fallback")
    (repo / "probe.txt").write_text("dirty\n", encoding="utf-8")
    dirty_report = parity.build_report(
        repo,
        ["smoke::python3 -m pytest tests -q"],
        nar_probe=available_probe,
        generated_at=STAMP,
    )
    assert _verdict_of(dirty_report) == "non-parity"

    incomplete = parity.build_report(
        repo,
        ["smoke::python3 -m pytest tests -q"],
        nar_probe=offline_probe,
        generated_at=STAMP,
    )
    assert _verdict_of(incomplete) == "non-parity"

    with pytest.raises(parity.ReportError):
        parity.build_report(repo, [], generated_at=STAMP)


def test_cli_writes_valid_non_parity_report_without_nix(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "cli")
    output = tmp_path / "report.json"
    code = parity.main(
        [
            "--repository",
            str(repo),
            "--output",
            str(output),
            "--task",
            "smoke::nix run .#missing-app -- --help",
        ]
    )
    assert code == 1  # NAR unavailable: valid report, non-parity verdict
    report = json.loads(output.read_text(encoding="utf-8"))
    parity.validate_report(report)
    assert report["snapshot"] == "clean-tracked-source"
    assert _verdict_of(report) == "non-parity"


def test_cli_rejects_malformed_task_and_discovers_with_bound(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "cli-bad")
    output = tmp_path / "report.json"
    args = ["--repository", str(repo), "--output", str(output), "--task", "bare"]
    assert parity.main(args) == 2
    assert not output.exists()

    def fake_show(_argv: list[str], _cwd: object) -> str:
        return json.dumps(
            {
                "apps": {"x86_64-linux": {"my-app": {}, "other": {}}},
                "checks": {"x86_64-linux": {"gate": {}}},
            }
        )

    discovered = parity.discover_tasks(".", repo, 8, runner=fake_show)
    assert [name for name, _ in discovered] == [
        "apps.my-app",
        "apps.other",
        "checks.gate",
    ]
    with pytest.raises(parity.ReportError, match="exceeds --max-tasks"):
        parity.discover_tasks(".", repo, 2, runner=fake_show)


def test_report_serialization_is_deterministic(tmp_path: Path) -> None:
    repo = make_repo(tmp_path / "deterministic")
    specs = ["b-task::python3 -m pytest tests -q", "a-task::python3 -m pytest other -q"]
    first = parity.dumps_report(
        parity.build_report(repo, specs, nar_probe=offline_probe, generated_at=STAMP)
    )
    second = parity.dumps_report(
        parity.build_report(repo, specs, nar_probe=offline_probe, generated_at=STAMP)
    )
    assert first == second
    assert next(iter(json.loads(first))) == "generated_at"  # sort_keys ordering


def test_module_uses_only_stdlib_and_subprocess() -> None:
    source = parity.__file__
    assert isinstance(source, str)
    text = Path(source).read_text(encoding="utf-8")
    assert "import subprocess" in text
    for third_party in ("requests", "httpx", "urllib.request", "socket"):
        assert third_party not in text


def test_main_exposes_bounded_task_cli() -> None:
    parser = parity.build_argument_parser()
    args = parser.parse_args(
        [
            "--repository",
            ".",
            "--output",
            "out.json",
            "--task",
            "a::true",
            "--max-tasks",
            "8",
        ]
    )
    assert args.task == ["a::true"]
    assert args.max_tasks == 8
    with pytest.raises(SystemExit):
        parser.parse_args([])
