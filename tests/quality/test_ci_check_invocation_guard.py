"""Check-invocation consolidation guard (AR-10.10-16).

Commit ``dad5f45`` ("ci: eliminate duplicate verification builds") removed the
duplicate verification workflows (``basedpyright.yml``, ``ruff.yml``,
``nix-source-filter-check.yml``, ``spa-404-check.yml``) and deleted the two
non-checking help probes from ``remote-flake.yml``:

* ``Run check app help from remote`` ran
  ``nix run --refresh github:...#check -- --help``;
* ``Run check app help locally`` ran ``nix run . -- --help``.

Both probes exercised only the ``--help`` stub of the ``check`` app, which
prints ``Run aggregate source verification gates; packaged CLI and UI use
dedicated apps`` and exits ``0`` without running any gate. The only real
``nix run .#check`` invocation left is the ``Run complete release gate`` step
in ``release.yml``. This test pins that state with the standard library only
(no PyYAML):

* no workflow file contains ``--help`` (the help probes stay removed);
* exactly ``{"release.yml"}`` invokes ``nix run .#check`` (local form or the
  ``--refresh github:...#check`` remote form);
* the ``release.yml`` invocation is the release gate step, not a help probe;
* ``flake.nix`` keeps the ``--help`` stub so ``--help`` cannot silently start
  running gates;
* the workflow file set stays exactly the four consolidated files.
"""

from __future__ import annotations

import re
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
WORKFLOWS_DIR = REPOSITORY / ".github/workflows"
FLAKE = REPOSITORY / "flake.nix"

EXPECTED_WORKFLOWS = (
    "compatibility-roll.yml",
    "normal-ci.yml",
    "package.yml",
    "release.yml",
)

EXPECTED_CHECK_INVOKERS = {"release.yml"}
EXPECTED_RELEASE_STEP_NAME = "Run complete release gate"
EXPECTED_HELP_DESCRIPTION = (
    "Run aggregate source verification gates; packaged CLI and UI use dedicated apps"
)

_CHECK_INVOCATION_RE = re.compile(
    r"nix run\s+(?:--refresh\s+)?github:[^\s'\"]*#check(?![\w-])"
    r"|nix run\s+\.#check(?![\w-])"
)
_STEP_NAME_RE = re.compile(r"(?m)^\s*-\s*name:\s*(.+?)\s*$")
_FLAKE_CHECK_MARKER = 'name = "check";'


def _workflow_files(workflows_dir: Path = WORKFLOWS_DIR) -> list[Path]:
    return sorted(workflows_dir.glob("*.yml"))


def _help_hits(text: str) -> list[int]:
    """Return 1-based line numbers containing ``--help``."""
    return [
        lineno
        for lineno, line in enumerate(text.splitlines(), start=1)
        if "--help" in line
    ]


def _check_invocation_lines(text: str) -> list[int]:
    """Return 1-based line numbers invoking ``nix run .#check`` exactly.

    Covers the local form (``nix run .#check``) and the remote form
    (``nix run [--refresh] github:...#check``). The negative lookahead
    excludes neighbouring apps such as ``.#checks.x86_64-linux...`` or a
    hypothetical ``.#check-foo``.
    """
    return [
        lineno
        for lineno, line in enumerate(text.splitlines(), start=1)
        if _CHECK_INVOCATION_RE.search(line) is not None
    ]


def _step_name_for_line(text: str, lineno: int) -> str | None:
    """Return the nearest preceding ``- name:`` step name for a line."""
    lines = text.splitlines()
    for line in reversed(lines[: lineno - 1]):
        match = _STEP_NAME_RE.match(line)
        if match is not None:
            return match.group(1)
    return None


def _flake_check_block(text: str, window: int = 8000) -> str:
    assert _FLAKE_CHECK_MARKER in text, 'flake.nix has no `name = "check";` marker'
    start = text.index(_FLAKE_CHECK_MARKER)
    return text[start : start + window]


def test_workflow_file_set_is_exact() -> None:
    files = _workflow_files()
    assert files, "no workflow files found"
    assert [path.name for path in files] == list(EXPECTED_WORKFLOWS)


def test_no_workflow_contains_help_probe() -> None:
    offenders: dict[str, list[int]] = {}
    for path in _workflow_files():
        hits = _help_hits(path.read_text(encoding="utf-8"))
        if hits:
            offenders[path.name] = hits
    assert not offenders, f"help probe reintroduced in workflows: {offenders}"


def test_only_release_invokes_check_app() -> None:
    invokers: dict[str, list[int]] = {}
    for path in _workflow_files():
        lines = _check_invocation_lines(path.read_text(encoding="utf-8"))
        if lines:
            invokers[path.name] = lines
    assert set(invokers) == EXPECTED_CHECK_INVOKERS, (
        f"nix run .#check invokers changed: {invokers}"
    )


def test_remote_form_check_invocation_is_detected() -> None:
    remote = (
        "        run: nix run --refresh github:yqYo1/Poke-Controller-Modified-"
        "Extension/abc123#check\n"
    )
    local = "        run: nix run .#check\n"
    neighbours = (
        "        run: nix run --refresh github:yqYo1/Poke-Controller-Modified-"
        "Extension/abc123#default\n"
        '          target_drv="$(nix path-info --derivation '
        '.#checks.x86_64-linux.rust-core-artifacts)"\n'
    )
    assert _check_invocation_lines(remote) == [1]
    assert _check_invocation_lines(local) == [1]
    assert _check_invocation_lines(neighbours) == []


def test_release_check_invocation_is_release_gate() -> None:
    text = (WORKFLOWS_DIR / "release.yml").read_text(encoding="utf-8")
    lines = _check_invocation_lines(text)
    assert len(lines) == 1, f"release.yml check invocations changed: {lines}"
    lineno = lines[0]
    assert "--help" not in text.splitlines()[lineno - 1], (
        "release.yml check invocation became a help probe"
    )
    assert _step_name_for_line(text, lineno) == EXPECTED_RELEASE_STEP_NAME, (
        "release.yml check invocation is not the release gate step"
    )


def test_flake_check_app_keeps_help_stub() -> None:
    block = _flake_check_block(FLAKE.read_text(encoding="utf-8"))
    assert "${1:-}" in block, "check app lost its first-argument test"
    assert '"--help"' in block, "check app lost its --help branch"
    assert EXPECTED_HELP_DESCRIPTION in block, "check app lost its help text"
    assert "exit 0" in block, "check app help branch no longer exits early"
    # The stub must precede the gates so --help never runs them.
    help_pos = block.index('"--help"')
    for gate_marker in ("source_filter", "release.gate"):
        assert gate_marker in block, (
            f"check app block no longer contains gate {gate_marker!r}"
        )
        assert help_pos < block.index(gate_marker), (
            f"check app --help branch no longer precedes {gate_marker!r}"
        )
