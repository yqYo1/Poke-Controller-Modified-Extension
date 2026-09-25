"""Architecture handoff consistency tests.

These tests validate that the tracked handoff names the current package and
private module boundary without treating a documentation shape check as proof
of runtime ownership or lifecycle behavior. Runtime and fault acceptance stay
in the Rust integration tests and external gates named by the handoff.
"""

from __future__ import annotations

import re
import tomllib
from pathlib import Path

REPOSITORY = Path(__file__).resolve().parents[2]
HANDOFF = REPOSITORY / "docs/ARCHITECTURE_HANDOFF.md"
LIBRARY = REPOSITORY / "rust/pokecon/src/lib.rs"
WORKSPACE_MANIFEST = REPOSITORY / "Cargo.toml"
PACKAGE_MANIFEST = REPOSITORY / "rust/pokecon/Cargo.toml"


def _section(text: str, heading: str, next_heading: str) -> str:
    start = text.index(heading)
    end = text.index(next_heading, start + len(heading))
    return text[start:end]


def _table_rows(section: str) -> list[str]:
    return [
        line
        for line in section.splitlines()
        if line.startswith("|") and not line.startswith("|---")
    ]


def test_handoff_has_unique_ownership_and_boundary_rows() -> None:
    text = HANDOFF.read_text(encoding="utf-8")
    ownership = _section(text, "## 3. Ownership table", "### 3.1")
    boundaries = _section(text, "## 5. Public boundary table", "### 5.1")
    forbidden = _section(text, "### 9.2", "## 10.")

    resource_prefixes = (
        "controllerの正準入力とrelease",
        "camera native handle",
        "serial port",
        "visible settings",
        "HTTP socket",
        "user-script worker process",
        "dynamic Python",
        "process-wide shutdown",
        "正準設定registry",
        "compatibility baseline",
        "build\uff0fpackage\uff0frelease artifact",
    )
    ownership_rows = _table_rows(ownership)
    for prefix in resource_prefixes:
        matching = [row for row in ownership_rows if row.startswith(f"| {prefix}")]
        assert len(matching) == 1, (prefix, matching)
        assert "| source-verified" in matching[0] or "| documented" in matching[0]

    modules = _section(text, "### 3.1 Module ownership manifest", "### 3.2")
    module_prefixes = (
        "`entrypoint`\uff0f`production`",
        "`application_backend`",
        "`server`",
        "`device::input`",
        "`device::serial`",
        "`camera`",
        "`worker::supervisor`\uff0f`worker::generation`",
        "`worker_binary`",
        "`dynamic::transaction`\uff0fdynamic host",
        "`settings`\uff0f`contracts`\uff0f`registry`",
        "`runtime::shutdown`",
        "frontend\uff0fgenerated client",
    )
    module_rows = _table_rows(modules)
    for prefix in module_prefixes:
        assert sum(row.startswith(f"| {prefix}") for row in module_rows) == 1

    public_inventory = _section(text, "### 5.1 公開面inventory", "### 5.2")
    for marker in (
        "server/rest/mod.rs:35-48,197",
        "`/ws` upgrade",
        "POKECON-CONTROL",
        "api/openapi.json",
        "registry/protocol.json",
        "typed MessagePack `IpcValue`",
    ):
        assert marker in public_inventory, marker
    for source_path in (
        "rust/pokecon/src/server/rest/mod.rs",
        "rust/pokecon/src/server/websocket.rs",
        "rust/pokecon/src/server/webrtc.rs",
        "rust/pokecon/src/bin/generate_openapi.rs",
        "rust/pokecon/registry/settings.json",
        "rust/pokecon/src/worker/ipc/mod.rs",
    ):
        assert source_path in public_inventory, source_path

    callers = (
        "Web\uff0fTauri frontend",
        "user script",
        "dynamic Python\uff0fLua",
        "worker supervisor",
        "compatibility tool",
    )
    boundary_rows = _table_rows(boundaries)
    for caller in callers:
        assert sum(row.startswith(f"| {caller}") for row in boundary_rows) == 1
    assert any("native handle" in row for row in boundary_rows[1:])
    assert any("Rust module" in row for row in boundary_rows[1:])

    forbidden_edges = (
        "frontend →",
        "worker →",
        "lower module →",
        "generated artifact →",
        "candidate promotion →",
        "UI\uff0ftransport →",
    )
    forbidden_rows = _table_rows(forbidden)
    for edge in forbidden_edges:
        matching = [row for row in forbidden_rows if row.startswith(f"| {edge}")]
        assert len(matching) == 1, (edge, matching)
        assert any(
            status in matching[0]
            for status in ("pending-test", "documented", "source-verified")
        )


def test_handoff_source_boundary_matches_current_workspace() -> None:
    handoff = HANDOFF.read_text(encoding="utf-8")
    library = LIBRARY.read_text(encoding="utf-8")
    workspace = tomllib.loads(WORKSPACE_MANIFEST.read_text(encoding="utf-8"))
    package = tomllib.loads(PACKAGE_MANIFEST.read_text(encoding="utf-8"))

    assert workspace["workspace"]["members"] == ["rust/pokecon"]
    assert package["package"]["name"] == "pokecon"
    assert 'name = "pokecon-worker"' in PACKAGE_MANIFEST.read_text(encoding="utf-8")

    internal_modules = (
        "application_backend",
        "camera",
        "command_service",
        "contracts",
        "desktop",
        "device",
        "dynamic_host",
        "entrypoint",
        "platform",
        "production",
        "profile_service",
        "runtime",
        "script_host",
        "script_runtime",
        "server",
        "settings",
        "settings_runtime",
        "worker",
    )
    for module in internal_modules:
        assert re.search(rf"(?m)^mod {re.escape(module)};$", library), module
        assert not re.search(rf"(?m)^pub mod {re.escape(module)};$", library), module

    assert "## 9. Dependency rule manifest" in handoff
    assert "## 10. Migration inventoryと検証command" in handoff
    assert "source-verified、fault test pending" in handoff
    assert "Release tag pending" in handoff


def test_handoff_navigation_is_reachable_from_document_entries() -> None:
    readme = (REPOSITORY / "docs/README.md").read_text(encoding="utf-8")
    index = (REPOSITORY / "docs/TRACEABILITY_INDEX.md").read_text(encoding="utf-8")

    assert "(ARCHITECTURE_HANDOFF.md)" in readme
    assert "(ARCHITECTURE_HANDOFF.md)" in index
    assert "architecture説明だけでruntime受入、実機、browser受入を主張しない" in index
