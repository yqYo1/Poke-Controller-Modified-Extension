"""Documentation-drift regression test for PLAN AR-13.1-22.

The normative development-environment contract is
``docs/SPECIFICATION_BACKEND.md`` §1.2 (tool-only Nix devShell, task
operations via fixed flake apps, native Windows CI/package/release as
fixed-workflow exceptions). The six audited documents must stay
consistent with that contract. The tracked direnv entry point itself
(``.envrc``) is never read here; its tracked path is verified separately
with ``git ls-files`` outside this test.
"""

from __future__ import annotations

import re
from pathlib import Path

import pytest

REPOSITORY = Path(__file__).resolve().parents[2]

DOCUMENTS: tuple[str, ...] = (
    "AGENTS.md",
    "docs/SPECIFICATION_BACKEND.md",
    "PLAN.md",
    "README.md",
    "docs/DEVELOPMENT.md",
    "docs/TROUBLESHOOTING.md",
)

NORMATIVE_DOCUMENT = "docs/SPECIFICATION_BACKEND.md"

ENTRY_TOKENS: tuple[str, ...] = (
    "nix develop",
    "direnv",
    "devShell",
    ".envrc",
    "use flake",
)

HOST_PROHIBITION = re.compile(
    r"host.{0,80}(使用しない|使用しません|直接起動しない)"
    r"|Do NOT run host"
    r"|を直接使用しません"
    r"|を直接実行せず"
)

CONTRADICTIONS: tuple[tuple[str, re.Pattern[str]], ...] = (
    (
        "entry-side-effect-build",
        re.compile(
            r"(devShell|nix develop|direnv).{0,60}"
            r"(自動的|automatically).{0,40}(build|ビルド|テスト|依存更新)"
        ),
    ),
    (
        "host-toolchain-direct",
        re.compile(r"直接(実行|使用)し(ます|する|てください)"),
    ),
    (
        "envrc-tool-injection",
        re.compile(r"\.envrc.{0,30}(追記|追加).{0,20}(tool|ツール|導入)"),
    ),
    (
        "windows-nix-develop",
        re.compile(r"[Ww]indows.{0,40}nix develop"),
    ),
    (
        "devshell-builds-product",
        re.compile(r"devShell.{0,40}(build|ビルド).{0,20}(する|します|される|実行)"),
    ),
)

NORMATIVE_STATEMENTS: tuple[str, ...] = (
    "既定devShellはtoolと対話用環境だけを提供し",
    "入るだけでbuild、test、依存更新、generatorを実行しない",
    "完了gate、生成、package、用途別操作は`nix run .#<task>`",
    "native Windows CI、package、releaseはNixを利用できない明示的platform gate",
    "追跡対象の`.envrc`が読み込む既定devShell",
)

CONFORMING_FIXTURE = (
    "開発は `nix develop` または direnv が読み込む devShell へ入って行う。"
    "既定 devShell は tool-only であり、入るだけで build しない。"
    "完了 gate は `nix run .#check` で実行し、"
    "host の言語 runtime は使用しない。"
)


def audit_document(text: str) -> list[str]:
    """Return violation rule ids for a document describing the devShell contract."""
    violations: list[str] = []
    for rule_id, pattern in CONTRADICTIONS:
        if pattern.search(text) is not None:
            violations.append(rule_id)
    entry_hits = {token for token in ENTRY_TOKENS if token in text}
    if len(entry_hits) < 2:
        violations.append("missing-nix-entry")
    if "nix run" not in text:
        violations.append("missing-app-routing")
    if HOST_PROHIBITION.search(text) is None:
        violations.append("missing-host-prohibition")
    return violations


def audit_normative(text: str) -> list[str]:
    """Audit the normative contract source: shared rules plus exact statements."""
    violations = audit_document(text)
    for statement in NORMATIVE_STATEMENTS:
        if statement not in text:
            violations.append("missing-normative-statement")
            break
    return violations


def test_docs_agree_on_tool_only_devshell_contract() -> None:
    failures: dict[str, list[str]] = {}
    for relative in DOCUMENTS:
        text = (REPOSITORY / relative).read_text(encoding="utf-8")
        if relative == NORMATIVE_DOCUMENT:
            violations = audit_normative(text)
        else:
            violations = audit_document(text)
        if violations:
            failures[relative] = violations
    message = f"documentation drift from tool-only devShell contract: {failures}"
    assert not failures, message


CONTRADICTION_FIXTURES: tuple[tuple[str, str], ...] = (
    (
        "entry-side-effect-build",
        "開発者は nix develop に入ると自動的に build が実行され、製品が生成される。",
    ),
    (
        "host-toolchain-direct",
        "host の cargo を直接実行します。",
    ),
    (
        "envrc-tool-injection",
        "必要な tool は .envrc へ追記して導入します。",
    ),
    (
        "windows-nix-develop",
        "Windows 上でも nix develop で開発します。",
    ),
    (
        "devshell-builds-product",
        "既定の devShell は入室時に製品を build する。",
    ),
)


@pytest.mark.parametrize(
    ("rule_id", "contradiction"),
    CONTRADICTION_FIXTURES,
)
def test_detector_rejects_contradictory_fixture(
    rule_id: str, contradiction: str
) -> None:
    assert audit_document(CONFORMING_FIXTURE) == []
    violations = audit_document(f"{CONFORMING_FIXTURE}{contradiction}")
    assert violations == [rule_id]


def test_detector_rejects_fixture_missing_required_signals() -> None:
    violations = audit_document("この文書は開発環境に触れない。")
    assert "missing-nix-entry" in violations
    assert "missing-app-routing" in violations
    assert "missing-host-prohibition" in violations


def test_normative_detector_requires_exact_contract_statements() -> None:
    assert audit_normative(CONFORMING_FIXTURE) == ["missing-normative-statement"]
