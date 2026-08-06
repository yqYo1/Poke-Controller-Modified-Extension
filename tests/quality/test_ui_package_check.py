from __future__ import annotations

import hashlib
import json
import os
import posixpath
import re
import shutil
import stat
import subprocess
import sys
import textwrap
import tomllib
from functools import lru_cache
from pathlib import Path
from typing import TypeIs

import pytest

type CargoDependencyTable = dict[str, object]
type JsonValue = (
    None | bool | int | float | str | list[JsonValue] | dict[str, JsonValue]
)
type ProductionRoutingMutation = tuple[str, dict[str, str]]
type CorsSuccessRequiredHeaderFault = tuple[str, str, str, str]

REPOSITORY = Path(__file__).resolve().parents[2]
PRODUCTION_ROUTING_MUTATION_SHARD_INDEX_ENV = (
    "POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX"
)
PRODUCTION_ROUTING_MUTATION_SHARD_COUNT_ENV = (
    "POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_COUNT"
)
PRODUCTION_ROUTING_MUTATION_MAX_SHARDS = 8
CARGO_CACHE_DIRECTORY_TAG = (
    "Signature: 8a477f597d28d172789f06886806bc55\n"
    "# This file is a cache directory tag created by cargo.\n"
    "# For information about cache directory tags see https://bford.info/cachedir/\n"
)
RETIRED_DESKTOP_PRODUCT_FEATURE = "tauri" + "-shell"

EXPECTED_OPENAPI_OPERATIONS: tuple[tuple[str, str, str], ...] = (
    ("/api/camera/retry", "post", "retry_camera"),
    ("/api/camera/screenshot", "post", "screenshot"),
    ("/api/commands/control", "post", "control_command"),
    ("/api/commands/reload", "post", "reload_commands"),
    ("/api/devices/cameras", "get", "get_cameras"),
    ("/api/devices/serial-ports", "get", "get_serial_ports"),
    ("/api/dynamic-config/control", "post", "control_dynamic_config"),
    ("/api/notifications/test", "post", "test_notification"),
    ("/api/profiles/generate-launcher", "post", "generate_launcher"),
    ("/api/script-ui/action", "post", "script_ui_action"),
    ("/api/serial/control", "post", "control_serial"),
    ("/api/settings", "get", "get_settings"),
    ("/api/settings", "patch", "patch_settings"),
    ("/api/state", "get", "get_state"),
    ("/api/update/check", "post", "check_update"),
    ("/ws", "get", "websocket"),
)

REST_ROUTE_PATHS_BY_SOURCE: dict[str, tuple[str, ...]] = {
    "server/rest/commands.rs": (
        "/api/commands/control",
        "/api/commands/reload",
    ),
    "server/rest/devices.rs": (
        "/api/devices/cameras",
        "/api/devices/serial-ports",
        "/api/serial/control",
        "/api/camera/retry",
        "/api/camera/screenshot",
    ),
    "server/rest/dynamic_config.rs": ("/api/dynamic-config/control",),
    "server/rest/mod.rs": ("/api", "/api/{*path}"),
    "server/rest/notifications.rs": ("/api/notifications/test",),
    "server/rest/profiles.rs": ("/api/profiles/generate-launcher",),
    "server/rest/script_ui.rs": ("/api/script-ui/action",),
    "server/rest/settings.rs": ("/api/settings",),
    "server/rest/state.rs": ("/api/state",),
    "server/rest/update.rs": ("/api/update/check",),
}
REST_ROUTER_MODULES: tuple[str, ...] = (
    "commands",
    "devices",
    "dynamic_config",
    "notifications",
    "profiles",
    "script_ui",
    "settings",
    "state",
    "update",
)
REST_ROUTER_MERGES: tuple[str, ...] = (
    "settings",
    "commands",
    "devices",
    "notifications",
    "dynamic_config",
    "profiles",
    "script_ui",
    "state",
    "update",
)

POKECON_MANIFEST_SOURCE = "@pokecon/Cargo.toml"
WORKSPACE_MANIFEST_SOURCE = "@workspace/Cargo.toml"
WORKSPACE_LOCK_SOURCE = "@workspace/Cargo.lock"
FLAKE_SOURCE = "@flake.nix"
FLAKE_LOCK_SOURCE = "@flake.lock"
FLAKE_LOCK_INVENTORY_SOURCE = "@flake-lock-inventory"
RUST_TOOLCHAIN_SOURCE = "@rust-toolchain.toml"
RUST_TOOLCHAIN_INVENTORY_SOURCE = "@rust-toolchain-inventory"
CARGO_CONFIG_INVENTORY_SOURCE = "@repository/.cargo-config-inventory"
BUILD_SCRIPT_INVENTORY_SOURCE = "@workspace/build-script-inventory"
PRODUCTION_BUILD_INPUT_INVENTORY_SOURCE = "@production-build-input-inventory"
TAURI_CONFIG_INVENTORY_SOURCE = "@tauri-config-inventory"
PRODUCTION_CARGO_TARGET_ROOT_INVENTORY_SOURCE = (
    "@production-cargo-target-root-inventory"
)
TAURI_ACL_INPUT_INVENTORY_SOURCE = "@tauri-acl-input-inventory"
PRODUCTION_BUILD_INPUT_SOURCES: dict[str, str] = {
    "@.gitignore": ".gitignore",
    "@LICENSE": "LICENSE",
    "@pyproject.toml": "pyproject.toml",
    "@release/build_runtime.py": "scripts/release/build_runtime.py",
    "@release/normalize_debian_package.py": (
        "scripts/release/normalize_debian_package.py"
    ),
    "@release/normalize_linux_elf.py": "scripts/release/normalize_linux_elf.py",
    "@release/stage.py": "scripts/release/stage.py",
    "@tauri/linux/70-pokecon-controller.rules": (
        "rust/pokecon/linux/70-pokecon-controller.rules"
    ),
    "@tauri/linux/reload-udev.sh": "rust/pokecon/linux/reload-udev.sh",
    "@uv.lock": "uv.lock",
}
PRODUCTION_BINARY_BUILD_INPUT_SOURCES: dict[str, str] = {
    "@tauri/icons/32x32.png": "rust/pokecon/icons/32x32.png",
    "@tauri/icons/128x128.png": "rust/pokecon/icons/128x128.png",
    "@tauri/icons/128x128@2x.png": "rust/pokecon/icons/128x128@2x.png",
    "@tauri/icons/icon.icns": "rust/pokecon/icons/icon.icns",
    "@tauri/icons/icon.ico": "rust/pokecon/icons/icon.ico",
}
PRODUCTION_CARGO_TARGET_ROOT_SOURCES: dict[str, str] = {
    "@rust/pokecon/src/bin/worker.rs": "rust/pokecon/src/bin/worker.rs",
    "@rust/pokecon/src/lib.rs": "rust/pokecon/src/lib.rs",
    "@rust/pokecon/src/main.rs": "rust/pokecon/src/main.rs",
}
TAURI_ACL_INPUT_ROOTS: tuple[str, ...] = (
    "rust/pokecon/capabilities",
    "rust/pokecon/permissions",
)
TAURI_AUTO_CONFIG_PATTERN = re.compile(
    r"(?:tauri(?:\.[^.]+)?\.conf\.(?:json|json5)|Tauri(?:\.[^.]+)?\.toml)"
)
WORKSPACE_MEMBERS: tuple[str, ...] = ("rust/pokecon",)
WORKSPACE_DEFAULT_MEMBERS: tuple[str, ...] = ("rust/pokecon",)
WORKSPACE_MANIFEST_SOURCES: dict[str, str] = {
    member: f"@{member.removeprefix('rust/')}/Cargo.toml"
    for member in WORKSPACE_MEMBERS
}
WORKSPACE_BUILD_SCRIPT_SOURCES: dict[str, str] = {
    "rust/pokecon/build.rs": "@pokecon/build.rs",
}
EXPECTED_WORKSPACE_MANIFEST_HASHES: dict[str, str] = {
    "rust/pokecon": "cb2f55bcc3f59c62a980cee0453be238fb5bf64ccb082ff412aeca5ca84b6791",
}

EXPECTED_WORKSPACE_PROVENANCE = tomllib.loads(
    r"""
[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
license = "MIT"
authors = ["yqYo1"]
repository = "https://github.com/yqYo1/Poke-Controller-Modified-Extension"
homepage = "https://github.com/yqYo1/Poke-Controller-Modified-Extension"
keywords = ["pokemon", "automation", "switch", "3ds", "controller", "serial"]
categories = ["games", "hardware-support", "embedded"]

[workspace.dependencies]
async-trait = "0.1"
axum = "0.8.9"
clap = { version = "4.5", features = ["derive"] }
futures-util = "0.3.33"
tokio = { version = "1", features = ["full"] }
tokio-tungstenite = "0.29.0"
tokio-util = "0.7"
tower = { version = "0.5.3", features = ["util"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
thiserror = "2"
anyhow = "1"
base64 = "0.22"
atomic-write-file = "0.3"
chrono = { version = "0.4", features = ["serde"] }
fs4 = "1"
getrandom = "0.4"
hex = "0.4"
hmac = "0.12"
image = { version = "0.25.10", default-features = false, features = ["jpeg", "png"] }
mime_guess = "2.0.5"
openh264 = "0.9.7"
parking_lot = "0.12"
pep440_rs = { version = "0.7", features = ["version-ranges"] }
pyo3 = { version = "0.29", features = ["abi3-py314"] }
tauri = { version = "2.11.5", default-features = false, features = ["dynamic-acl", "image-png", "tray-icon", "wry"] }
tauri-plugin-single-instance = "2.3.6"
rfd = { version = "0.15.4", default-features = false, features = ["gtk3"] }
opener = "0.8.3"
mlua = { version = "0.10", features = ["luajit", "vendored", "async", "send", "serialize"] }
nix = { version = "0.30", features = ["process", "signal", "term"] }
nokhwa = { version = "0.10.11", default-features = false }
toml = "0.8"
toml_edit = { version = "0.22", features = ["serde"] }
regex = "1"
semver = "1"
rmp-serde = "1"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "multipart", "rustls-no-provider"] }
rumqttc = { version = "0.25.1", default-features = false }
rustls = { version = "0.23.42", default-features = false, features = ["ring", "std"] }
tokio-serial = { version = "5.5.0", features = ["libudev"] }
gilrs = { version = "0.11.2", default-features = false, features = ["xinput"] }
sha2 = "0.10"
shared_memory = "0.12.4"
tempfile = "3"
url = "2"
utoipa = { version = "5.5.0", features = ["axum_extras", "preserve_order"] }
v4l = "0.14"
version-ranges = "0.1"
webrtc = "0.14.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
"""
)["workspace"]

EXPECTED_POKECON_MANIFEST = tomllib.loads(
    r"""
[package]
name = "pokecon"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
build = "build.rs"

[[bin]]
name = "pokecon"
path = "src/main.rs"

[[bin]]
name = "pokecon-worker"
path = "src/bin/worker.rs"

[[bin]]
name = "pokecon-compatibility"
path = "src/bin/compatibility.rs"

[[bin]]
name = "pokecon-worker-fault-fixture"
path = "tests/fixtures/fault_worker.rs"
test = false
bench = false

[[bin]]
name = "generate_contracts"
path = "src/bin/generate_contracts.rs"
required-features = ["contract-generator"]

[[bin]]
name = "generate_openapi"
path = "src/bin/generate_openapi.rs"
required-features = ["contract-generator"]

[features]
default = []
contract-generator = []

[dependencies]
async-trait.workspace = true
atomic-write-file.workspace = true
axum = { workspace = true, features = ["ws"] }
base64.workspace = true
chrono.workspace = true
clap.workspace = true
fs4.workspace = true
futures-util.workspace = true
getrandom.workspace = true
gilrs.workspace = true
hex.workspace = true
hmac.workspace = true
image.workspace = true
mime_guess.workspace = true
mlua.workspace = true
opener.workspace = true
openh264.workspace = true
parking_lot.workspace = true
pep440_rs.workspace = true
pyo3.workspace = true
reqwest.workspace = true
rfd.workspace = true
rmp-serde.workspace = true
rumqttc.workspace = true
rustls.workspace = true
semver.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
shared_memory.workspace = true
tempfile.workspace = true
thiserror.workspace = true
tauri.workspace = true
tauri-plugin-single-instance.workspace = true
tokio.workspace = true
tokio-serial.workspace = true
tokio-util.workspace = true
toml.workspace = true
toml_edit.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
url.workspace = true
utoipa.workspace = true
version-ranges.workspace = true
webrtc.workspace = true

[build-dependencies]
dunce = "1.0.5"
hex.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
tauri-build = { version = "2.5.4", features = [] }
toml.workspace = true

[dev-dependencies]
proc-macro2 = "1"
regex.workspace = true
syn = { version = "2", features = ["full", "visit"] }
tokio-tungstenite.workspace = true
tower.workspace = true

[target.'cfg(unix)'.dependencies]
nix = { workspace = true, features = ["fs"] }

[target.'cfg(unix)'.dev-dependencies]
nix.workspace = true

[target.'cfg(target_os = "linux")'.dependencies]
nokhwa = { workspace = true, features = ["input-v4l"] }
v4l.workspace = true

[target.'cfg(windows)'.dependencies]
nokhwa = { workspace = true, features = ["input-msmf"] }
winapi-util = "0.1.11"
win32_notif = { version = "0.15.3", default-features = false }

[lints.rust]
unsafe_code = "deny"
unsafe_op_in_unsafe_fn = "deny"

[lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "deny", priority = -1 }
module_name_repetitions = "allow"
must_use_candidate = "allow"
"""
)

REST_ROUTER_SIGNATURE = (
    r"\bpub\s*\(\s*super\s*\)\s+fn\s+router\s*\(\s*\)\s*"
    r"->\s*Router\s*<\s*RestState\s*>"
)
ROUTER_FACTORY_SIGNATURES: dict[str, str] = {
    source_name: REST_ROUTER_SIGNATURE
    for source_name in REST_ROUTE_PATHS_BY_SOURCE
    if source_name != "server/rest/mod.rs"
}
ROUTER_FACTORY_SIGNATURES |= {
    "server/rest/mod.rs": (
        r"\bpub\s+fn\s+router\s*\(\s*backend\s*:\s*Arc\s*<\s*dyn\s+RestBackend\s*>"
        r"\s*\)\s*->\s*Router"
    ),
    "server/websocket.rs": (r"\bpub\s+fn\s+router\s*\(\s*&\s*self\s*\)\s*->\s*Router"),
    "server/static_files.rs": (r"\bpub\s+fn\s+router\s*\(\s*self\s*\)\s*->\s*Router"),
    "server/router.rs": (
        r"\bpub\s+fn\s+public_router\s*\(\s*api\s*:\s*Router\s*,"
        r"\s*static_files\s*:\s*StaticFiles\s*,\s*security\s*:\s*RequestSecurity"
        r"\s*\)\s*->\s*Router"
    ),
}
ROUTER_FACTORY_BODIES: dict[str, str] = {
    "server/rest/commands.rs": """
        Router::new()
            .route("/api/commands/control", post(control))
            .route("/api/commands/reload", post(reload))
    """,
    "server/rest/devices.rs": """
        Router::new()
            .route(
                "/api/devices/cameras",
                on(MethodFilter::GET, cameras).on(MethodFilter::HEAD, method_not_allowed),
            )
            .route(
                "/api/devices/serial-ports",
                on(MethodFilter::GET, serial_ports).on(MethodFilter::HEAD, method_not_allowed),
            )
            .route("/api/serial/control", post(control_serial))
            .route("/api/camera/retry", post(retry_camera))
            .route("/api/camera/screenshot", post(screenshot))
    """,
    "server/rest/dynamic_config.rs": """
        Router::new().route("/api/dynamic-config/control", post(control))
    """,
    "server/rest/mod.rs": """
        let state = RestState { backend };
        Router::<RestState>::new()
            .merge(settings::router())
            .merge(commands::router())
            .merge(devices::router())
            .merge(notifications::router())
            .merge(dynamic_config::router())
            .merge(profiles::router())
            .merge(script_ui::router())
            .merge(state::router())
            .merge(update::router())
            .route("/api", any(not_found))
            .route("/api/{*path}", any(not_found))
            .method_not_allowed_fallback(method_not_allowed)
            .with_state(state)
    """,
    "server/rest/notifications.rs": """
        Router::new().route("/api/notifications/test", post(test_notification))
    """,
    "server/rest/profiles.rs": """
        Router::new().route("/api/profiles/generate-launcher", post(generate_launcher))
    """,
    "server/rest/script_ui.rs": """
        Router::new().route("/api/script-ui/action", post(script_ui_action))
    """,
    "server/rest/settings.rs": """
        Router::new().route(
            "/api/settings",
            on(MethodFilter::GET, get_settings)
                .on(MethodFilter::HEAD, method_not_allowed)
                .patch(patch_settings),
        )
    """,
    "server/rest/state.rs": """
        Router::new().route(
            "/api/state",
            on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
        )
    """,
    "server/rest/update.rs": """
        Router::new().route("/api/update/check", post(check))
    """,
    "server/websocket.rs": """
        Router::new()
            .route(
                "/ws",
                on(MethodFilter::GET, websocket_upgrade)
                    .on(MethodFilter::HEAD, websocket_method_not_allowed)
                    .fallback(websocket_method_not_allowed),
            )
            .with_state(self.state.clone())
    """,
    "server/static_files.rs": """
        Router::new().fallback(serve_static).with_state(self)
    """,
    "server/router.rs": """
        secure_router(api.fallback_service(static_files.router()), security)
    """,
}


def rust_lexical_view(source: str, *, mask_literals: bool) -> str:
    """Blank comments and, optionally, literals without moving Rust delimiters."""
    view = list(source)
    source_length = len(source)

    def blank(start: int, end: int) -> None:
        for index in range(start, end):
            if source[index] not in "\r\n":
                view[index] = " "

    def quoted_literal_end(start: int, quote: int) -> int:
        index = quote + 1
        while index < source_length:
            if source[index] == "\\":
                index += 2
                continue
            if source[index] == '"':
                return index + 1
            index += 1
        diagnostic = f"unterminated Rust string literal at byte {start}"
        raise AssertionError(diagnostic)

    def character_literal_end(quote: int) -> int | None:
        index = quote + 1
        if index >= source_length or source[index] in "\r\n'":
            return None
        if source[index] == "\\":
            index += 1
            if index >= source_length:
                return None
            if source[index] == "u" and index + 1 < source_length:
                if source[index + 1] != "{":
                    return None
                close = source.find("}", index + 2)
                if close == -1:
                    return None
                index = close + 1
            elif source[index] == "x":
                index += 3
            else:
                index += 1
        else:
            index += 1
        if index < source_length and source[index] == "'":
            return index + 1
        return None

    index = 0
    while index < source_length:
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            end = source_length if end == -1 else end
            blank(index, end)
            index = end
            continue
        if source.startswith("/*", index):
            depth = 1
            end = index + 2
            while end < source_length and depth != 0:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            assert depth == 0, f"unterminated Rust block comment at byte {index}"
            blank(index, end)
            index = end
            continue

        raw_literal = re.match(r'(?:br|cr|r)(?P<hashes>#{0,255})"', source[index:])
        if raw_literal is not None:
            hashes = raw_literal.group("hashes")
            delimiter = f'"{hashes}'
            content_start = index + raw_literal.end()
            close = source.find(delimiter, content_start)
            assert close != -1, f"unterminated Rust raw literal at byte {index}"
            end = close + len(delimiter)
            if mask_literals:
                blank(index, end)
            index = end
            continue

        quote = index
        if source.startswith(('b"', 'c"'), index):
            quote += 1
        if source[quote : quote + 1] == '"':
            end = quoted_literal_end(index, quote)
            if mask_literals:
                blank(index, end)
            index = end
            continue

        character_quote = index + 1 if source.startswith("b'", index) else index
        if source[character_quote : character_quote + 1] == "'":
            end = character_literal_end(character_quote)
            if end is not None:
                if mask_literals:
                    blank(index, end)
                index = end
                continue
        index += 1

    return "".join(view)


@lru_cache(maxsize=256)
def rust_lexical_mask(source: str) -> str:
    return rust_lexical_view(source, mask_literals=True)


def rust_without_comments(source: str) -> str:
    return rust_lexical_view(source, mask_literals=False)


def matching_rust_brace(mask: str, open_brace: int) -> int:
    assert mask[open_brace] == "{"
    depth = 0
    for index in range(open_brace, len(mask)):
        if mask[index] == "{":
            depth += 1
        elif mask[index] == "}":
            depth -= 1
            if depth == 0:
                return index
    diagnostic = f"unclosed Rust block at byte {open_brace}"
    raise AssertionError(diagnostic)


def terminal_rust_semicolon(mask: str, start: int) -> int:
    delimiters = {"(": ")", "[": "]", "{": "}"}
    stack: list[str] = []
    for index in range(start, len(mask)):
        character = mask[index]
        if character in delimiters:
            stack.append(delimiters[character])
        elif stack and character == stack[-1]:
            stack.pop()
        elif character == ";" and not stack:
            return index
    diagnostic = f"unterminated Rust item at byte {start}"
    raise AssertionError(diagnostic)


def production_rust_source(source: str) -> str:
    mask = rust_lexical_mask(source)
    test_boundaries = tuple(
        re.finditer(
            r"(?m)^#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\][ \t]*(?:\r?\n)?",
            mask,
        )
    )
    if not test_boundaries:
        return source
    production = list(source)
    brace_depths, parenthesis_depths, bracket_depths = rust_delimiter_depths(mask)
    for boundary_index, test_boundary in enumerate(test_boundaries):
        item_start = test_boundary.end()
        while item_start < len(mask) and mask[item_start].isspace():
            item_start += 1

        braced_item = re.match(
            r"(?:mod\s+[A-Za-z_][A-Za-z0-9_]*|"
            r"(?:pub(?:\s*\([^)]*\))?\s+)?(?:async\s+|const\s+)?fn\b)",
            mask[item_start:],
        )
        if braced_item is not None:
            open_brace = next(
                (
                    index
                    for index in range(item_start + braced_item.end(), len(mask))
                    if mask[index] == "{"
                    and brace_depths[index] == 0
                    and parenthesis_depths[index] == 0
                    and bracket_depths[index] == 0
                ),
                -1,
            )
            assert open_brace != -1, "cfg(test) item has no top-level body"
            item_end = matching_rust_brace(mask, open_brace) + 1
        else:
            test_reexport = re.match(
                r"pub\s*\(\s*crate\s*\)\s+use\b",
                mask[item_start:],
            )
            assert test_reexport is not None, "unsupported top-level cfg(test) item"
            item_end = terminal_rust_semicolon(mask, item_start) + 1

        if boundary_index == len(test_boundaries) - 1:
            assert not mask[item_end:].strip(), (
                "production tokens follow the final cfg(test) item"
            )
        for index in range(test_boundary.start(), item_end):
            if source[index] not in "\r\n":
                production[index] = " "
    return "".join(production)


def load_production_routing_sources() -> dict[str, str]:
    rust_root = REPOSITORY / "rust/pokecon/src"
    sources = {
        source_path.relative_to(rust_root).as_posix(): production_rust_source(
            source_path.read_text()
        )
        for source_path in rust_root.rglob("*.rs")
    }
    sources[WORKSPACE_MANIFEST_SOURCE] = (REPOSITORY / "Cargo.toml").read_text()
    sources[WORKSPACE_LOCK_SOURCE] = (REPOSITORY / "Cargo.lock").read_text()
    for member, source_name in WORKSPACE_MANIFEST_SOURCES.items():
        sources[source_name] = (REPOSITORY / member / "Cargo.toml").read_text()
    build_script_inventory: list[dict[str, str]] = []
    for member in WORKSPACE_MEMBERS:
        relative_build_script = f"{member}/build.rs"
        build_script = REPOSITORY / relative_build_script
        try:
            build_script_status = build_script.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISREG(build_script_status.st_mode):
            build_script_kind = "regular"
        elif stat.S_ISLNK(build_script_status.st_mode):
            build_script_kind = "symlink"
        else:
            build_script_kind = "other"
        build_script_inventory.append(
            {"path": relative_build_script, "kind": build_script_kind}
        )
        source_name = f"@{member.removeprefix('rust/')}/build.rs"
        sources[source_name] = (
            build_script.read_text() if build_script_kind == "regular" else ""
        )
    sources[BUILD_SCRIPT_INVENTORY_SOURCE] = json.dumps(build_script_inventory)
    sources[FLAKE_SOURCE] = (REPOSITORY / "flake.nix").read_text()
    flake_lock_path = REPOSITORY / "flake.lock"
    flake_lock_status = flake_lock_path.lstat()
    if stat.S_ISREG(flake_lock_status.st_mode):
        flake_lock_kind = "regular"
    elif stat.S_ISLNK(flake_lock_status.st_mode):
        flake_lock_kind = "symlink"
    else:
        flake_lock_kind = "other"
    sources[FLAKE_LOCK_SOURCE] = (
        flake_lock_path.read_text() if flake_lock_kind == "regular" else ""
    )
    sources[FLAKE_LOCK_INVENTORY_SOURCE] = json.dumps(
        {"path": "flake.lock", "kind": flake_lock_kind}
    )
    rust_toolchain_path = REPOSITORY / "rust-toolchain.toml"
    rust_toolchain_status = rust_toolchain_path.lstat()
    if stat.S_ISREG(rust_toolchain_status.st_mode):
        rust_toolchain_kind = "regular"
    elif stat.S_ISLNK(rust_toolchain_status.st_mode):
        rust_toolchain_kind = "symlink"
    else:
        rust_toolchain_kind = "other"
    sources[RUST_TOOLCHAIN_SOURCE] = (
        rust_toolchain_path.read_text() if rust_toolchain_kind == "regular" else ""
    )
    sources[RUST_TOOLCHAIN_INVENTORY_SOURCE] = json.dumps(
        {"path": "rust-toolchain.toml", "kind": rust_toolchain_kind}
    )
    production_build_input_inventory: list[dict[str, str]] = []
    for source_name, relative_path in PRODUCTION_BUILD_INPUT_SOURCES.items():
        build_input = REPOSITORY / relative_path
        build_input_status = build_input.lstat()
        if stat.S_ISREG(build_input_status.st_mode):
            build_input_kind = "regular"
        elif stat.S_ISLNK(build_input_status.st_mode):
            build_input_kind = "symlink"
        else:
            build_input_kind = "other"
        production_build_input_inventory.append(
            {"path": relative_path, "kind": build_input_kind}
        )
        sources[source_name] = (
            build_input.read_text() if build_input_kind == "regular" else ""
        )
    for source_name, relative_path in PRODUCTION_BINARY_BUILD_INPUT_SOURCES.items():
        build_input = REPOSITORY / relative_path
        build_input_status = build_input.lstat()
        if stat.S_ISREG(build_input_status.st_mode):
            build_input_kind = "regular"
        elif stat.S_ISLNK(build_input_status.st_mode):
            build_input_kind = "symlink"
        else:
            build_input_kind = "other"
        production_build_input_inventory.append(
            {"path": relative_path, "kind": build_input_kind}
        )
        sources[source_name] = (
            build_input.read_bytes().hex() if build_input_kind == "regular" else ""
        )
    sources[PRODUCTION_BUILD_INPUT_INVENTORY_SOURCE] = json.dumps(
        production_build_input_inventory
    )
    production_cargo_target_root_inventory: list[dict[str, str]] = []
    for source_name, relative_path in PRODUCTION_CARGO_TARGET_ROOT_SOURCES.items():
        target_root = REPOSITORY / relative_path
        target_root_status = target_root.lstat()
        if stat.S_ISREG(target_root_status.st_mode):
            target_root_kind = "regular"
        elif stat.S_ISLNK(target_root_status.st_mode):
            target_root_kind = "symlink"
        else:
            target_root_kind = "other"
        production_cargo_target_root_inventory.append(
            {"path": relative_path, "kind": target_root_kind}
        )
        sources[source_name] = (
            target_root.read_text() if target_root_kind == "regular" else ""
        )
    sources[PRODUCTION_CARGO_TARGET_ROOT_INVENTORY_SOURCE] = json.dumps(
        production_cargo_target_root_inventory
    )
    tauri_config_inventory: list[dict[str, str]] = []
    for tauri_config in sorted((REPOSITORY / "rust/pokecon").iterdir()):
        if TAURI_AUTO_CONFIG_PATTERN.fullmatch(tauri_config.name) is None:
            continue
        relative_tauri_config = tauri_config.relative_to(REPOSITORY).as_posix()
        tauri_config_status = tauri_config.lstat()
        if stat.S_ISREG(tauri_config_status.st_mode):
            tauri_config_kind = "regular"
        elif stat.S_ISLNK(tauri_config_status.st_mode):
            tauri_config_kind = "symlink"
        else:
            tauri_config_kind = "other"
        tauri_config_inventory.append(
            {"path": relative_tauri_config, "kind": tauri_config_kind}
        )
        sources[f"@{relative_tauri_config}"] = (
            tauri_config.read_text() if tauri_config_kind == "regular" else ""
        )
    sources[TAURI_CONFIG_INVENTORY_SOURCE] = json.dumps(tauri_config_inventory)
    tauri_acl_input_inventory: list[dict[str, str]] = []
    for relative_root in TAURI_ACL_INPUT_ROOTS:
        acl_root = REPOSITORY / relative_root
        try:
            acl_root_status = acl_root.lstat()
        except FileNotFoundError:
            continue
        if stat.S_ISLNK(acl_root_status.st_mode):
            tauri_acl_input_inventory.append({"path": relative_root, "kind": "symlink"})
            continue
        if not stat.S_ISDIR(acl_root_status.st_mode):
            tauri_acl_input_inventory.append({"path": relative_root, "kind": "other"})
            continue
        for directory, directory_names, file_names in os.walk(
            acl_root, topdown=True, followlinks=False
        ):
            current_directory = Path(directory)
            relative_directory = current_directory.relative_to(acl_root)
            if relative_root.endswith("/permissions") and relative_directory == Path(
                "."
            ):
                directory_names[:] = [
                    name for name in directory_names if name != "autogenerated"
                ]
            directory_names.sort()
            file_names.sort()
            for directory_name in tuple(directory_names):
                acl_directory = current_directory / directory_name
                acl_directory_status = acl_directory.lstat()
                if stat.S_ISDIR(acl_directory_status.st_mode):
                    continue
                directory_names.remove(directory_name)
                relative_acl_directory = acl_directory.relative_to(
                    REPOSITORY
                ).as_posix()
                tauri_acl_input_inventory.append(
                    {
                        "path": relative_acl_directory,
                        "kind": (
                            "symlink"
                            if stat.S_ISLNK(acl_directory_status.st_mode)
                            else "other"
                        ),
                    }
                )
            for file_name in file_names:
                acl_input = current_directory / file_name
                acl_input_status = acl_input.lstat()
                if stat.S_ISREG(acl_input_status.st_mode):
                    acl_input_kind = "regular"
                elif stat.S_ISLNK(acl_input_status.st_mode):
                    acl_input_kind = "symlink"
                else:
                    acl_input_kind = "other"
                relative_acl_input = acl_input.relative_to(REPOSITORY).as_posix()
                tauri_acl_input_inventory.append(
                    {"path": relative_acl_input, "kind": acl_input_kind}
                )
                sources[f"@{relative_acl_input}"] = (
                    acl_input.read_text() if acl_input_kind == "regular" else ""
                )
    tauri_acl_input_inventory.sort(key=lambda item: item["path"])
    sources[TAURI_ACL_INPUT_INVENTORY_SOURCE] = json.dumps(tauri_acl_input_inventory)
    cargo_config_inventory: list[dict[str, str]] = []
    generated_directory_names = {
        ".git",
        ".mypy_cache",
        ".pytest_cache",
        ".ruff_cache",
        ".venv",
        ".worktree",
        "node_modules",
        "target",
    }
    for cargo_directory in sorted(REPOSITORY.rglob(".cargo")):
        relative_cargo_directory = cargo_directory.relative_to(REPOSITORY)
        if generated_directory_names.intersection(relative_cargo_directory.parts):
            continue
        cargo_directory_status = cargo_directory.lstat()
        if not stat.S_ISDIR(cargo_directory_status.st_mode):
            cargo_config_inventory.append(
                {
                    "path": f"{relative_cargo_directory.as_posix()}/<redirected>",
                    "kind": (
                        "symlink"
                        if stat.S_ISLNK(cargo_directory_status.st_mode)
                        else "other"
                    ),
                }
            )
            continue
        for config_name in ("config", "config.toml"):
            cargo_config = cargo_directory / config_name
            try:
                cargo_config_status = cargo_config.lstat()
            except FileNotFoundError:
                continue
            if stat.S_ISREG(cargo_config_status.st_mode):
                cargo_config_kind = "regular"
            elif stat.S_ISLNK(cargo_config_status.st_mode):
                cargo_config_kind = "symlink"
            else:
                cargo_config_kind = "other"
            cargo_config_inventory.append(
                {
                    "path": cargo_config.relative_to(REPOSITORY).as_posix(),
                    "kind": cargo_config_kind,
                }
            )
    sources[CARGO_CONFIG_INVENTORY_SOURCE] = json.dumps(cargo_config_inventory)
    return sources


def production_routing_mutation_shard() -> tuple[int, int]:
    shard_index_text = os.environ.get(PRODUCTION_ROUTING_MUTATION_SHARD_INDEX_ENV)
    shard_count_text = os.environ.get(PRODUCTION_ROUTING_MUTATION_SHARD_COUNT_ENV)
    assert (shard_index_text is None) == (shard_count_text is None), (
        "production routing mutation shard index and count must be set together"
    )
    if shard_index_text is None or shard_count_text is None:
        return 0, 1

    canonical_decimal = re.compile(r"(?:0|[1-9][0-9]*)")
    assert canonical_decimal.fullmatch(shard_index_text) is not None
    assert canonical_decimal.fullmatch(shard_count_text) is not None
    shard_index = int(shard_index_text)
    shard_count = int(shard_count_text)
    assert 1 <= shard_count <= PRODUCTION_ROUTING_MUTATION_MAX_SHARDS
    assert 0 <= shard_index < shard_count
    return shard_index, shard_count


def compact_rust(source: str) -> str:
    return " ".join(source.split())


def exact_rust(source: str) -> str:
    return textwrap.dedent(rust_without_comments(source)).strip()


@lru_cache(maxsize=256)
def rust_delimiter_depths(
    mask: str,
) -> tuple[tuple[int, ...], tuple[int, ...], tuple[int, ...]]:
    brace_depths: list[int] = []
    parenthesis_depths: list[int] = []
    bracket_depths: list[int] = []
    brace_depth = 0
    parenthesis_depth = 0
    bracket_depth = 0
    for character in mask:
        brace_depths.append(brace_depth)
        parenthesis_depths.append(parenthesis_depth)
        bracket_depths.append(bracket_depth)
        if character == "{":
            brace_depth += 1
        elif character == "}":
            brace_depth -= 1
            assert brace_depth >= 0, "unmatched Rust closing brace"
        elif character == "(":
            parenthesis_depth += 1
        elif character == ")":
            parenthesis_depth -= 1
            assert parenthesis_depth >= 0, "unmatched Rust closing parenthesis"
        elif character == "[":
            bracket_depth += 1
        elif character == "]":
            bracket_depth -= 1
            assert bracket_depth >= 0, "unmatched Rust closing bracket"
    assert brace_depth == parenthesis_depth == bracket_depth == 0
    return tuple(brace_depths), tuple(parenthesis_depths), tuple(bracket_depths)


def rust_top_level_matches(
    source: str,
    pattern: str,
    *,
    search_view: str | None = None,
) -> tuple[re.Match[str], ...]:
    mask = rust_lexical_mask(source)
    view = mask if search_view is None else search_view
    assert len(view) == len(mask)
    brace_depths, parenthesis_depths, bracket_depths = rust_delimiter_depths(mask)
    matches = tuple(re.finditer(pattern, view))
    top_level = tuple(
        match
        for match in matches
        if brace_depths[match.start()] == 0
        and parenthesis_depths[match.start()] == 0
        and bracket_depths[match.start()] == 0
    )
    assert len(top_level) == len(matches), pattern
    return top_level


def rust_item_attributes(
    source: str,
    mask: str,
    item_start: int,
) -> tuple[str, ...]:
    attributes: list[str] = []
    cursor = item_start
    while True:
        while cursor > 0 and mask[cursor - 1].isspace():
            cursor -= 1
        if cursor == 0 or mask[cursor - 1] != "]":
            break
        close_bracket = cursor - 1
        bracket_depth = 0
        open_bracket = -1
        for index in range(close_bracket, -1, -1):
            if mask[index] == "]":
                bracket_depth += 1
            elif mask[index] == "[":
                bracket_depth -= 1
                if bracket_depth == 0:
                    open_bracket = index
                    break
        assert open_bracket != -1, "unclosed Rust item attribute"
        hash_cursor = open_bracket
        while hash_cursor > 0 and mask[hash_cursor - 1] in " \t":
            hash_cursor -= 1
        if hash_cursor == 0 or mask[hash_cursor - 1] != "#":
            break
        attribute_start = hash_cursor - 1
        attributes.append(compact_rust(source[attribute_start : close_bracket + 1]))
        cursor = attribute_start
    attributes.reverse()
    return tuple(attributes)


def rust_function_body_at_depth(
    source: str,
    mask: str,
    signature_match: re.Match[str],
    *,
    brace_depth: int,
    scope_end: int,
) -> str:
    brace_depths, parenthesis_depths, bracket_depths = rust_delimiter_depths(mask)
    open_brace = next(
        (
            index
            for index in range(signature_match.end(), scope_end)
            if mask[index] == "{"
            and brace_depths[index] == brace_depth
            and parenthesis_depths[index] == 0
            and bracket_depths[index] == 0
        ),
        -1,
    )
    assert open_brace != -1, signature_match.re.pattern
    close_brace = matching_rust_brace(mask, open_brace)
    assert close_brace < scope_end, signature_match.re.pattern
    return source[open_brace + 1 : close_brace]


def rust_top_level_function_body(
    source: str,
    signature: str,
    *,
    attributes: tuple[str, ...] = (),
) -> str:
    mask = rust_lexical_mask(source)
    matches = rust_top_level_matches(source, signature)
    assert len(matches) == 1, signature
    assert rust_item_attributes(source, mask, matches[0].start()) == attributes
    return rust_function_body_at_depth(
        source,
        mask,
        matches[0],
        brace_depth=0,
        scope_end=len(mask),
    )


def rust_impl_method_body(
    source: str,
    owner: str,
    signature: str,
    *,
    attributes: tuple[str, ...] = (),
    trait_name: str | None = None,
) -> str:
    mask = rust_lexical_mask(source)
    brace_depths, parenthesis_depths, bracket_depths = rust_delimiter_depths(mask)
    impl_owner = re.escape(owner)
    if trait_name is not None:
        impl_owner = rf"{re.escape(trait_name)}\s+for\s+{impl_owner}"
    impl_pattern = rf"(?m)^[ \t]*impl\s+{impl_owner}\s*\{{"
    impl_matches = tuple(re.finditer(impl_pattern, mask))
    assert len(impl_matches) == 1, impl_pattern
    impl_match = impl_matches[0]
    assert brace_depths[impl_match.start()] == 0, impl_pattern
    assert parenthesis_depths[impl_match.start()] == 0, impl_pattern
    assert bracket_depths[impl_match.start()] == 0, impl_pattern
    assert not rust_item_attributes(source, mask, impl_match.start()), impl_pattern
    impl_open = impl_match.end() - 1
    assert mask[impl_open] == "{", impl_pattern
    impl_close = matching_rust_brace(mask, impl_open)

    method_matches = tuple(re.finditer(signature, mask))
    assert len(method_matches) == 1, signature
    method_match = method_matches[0]
    assert impl_open < method_match.start() < impl_close, signature
    assert brace_depths[method_match.start()] == 1, signature
    assert parenthesis_depths[method_match.start()] == 0, signature
    assert bracket_depths[method_match.start()] == 0, signature
    assert rust_item_attributes(source, mask, method_match.start()) == attributes
    return rust_function_body_at_depth(
        source,
        mask,
        method_match,
        brace_depth=1,
        scope_end=impl_close,
    )


def is_object_dict(value: object) -> TypeIs[dict[object, object]]:
    return isinstance(value, dict)


def is_object_list(value: object) -> TypeIs[list[object]]:
    return isinstance(value, list)


def validated_string_object_dict(value: object, context: object) -> dict[str, object]:
    assert is_object_dict(value), context
    raw_mapping = value
    validated: dict[str, object] = {}
    for key, nested in raw_mapping.items():
        assert isinstance(key, str), (context, key)
        validated[key] = nested
    return validated


def validated_json_value(value: object, context: object) -> JsonValue:
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    if is_object_list(value):
        raw_items = value
        return [validated_json_value(item, context) for item in raw_items]
    if is_object_dict(value):
        raw_mapping = value
        validated: dict[str, JsonValue] = {}
        for key, nested in raw_mapping.items():
            assert isinstance(key, str), (context, key)
            validated[key] = validated_json_value(nested, context)
        return validated
    raise AssertionError((context, value))


def validated_json_object(value: object, context: object) -> dict[str, JsonValue]:
    validated = validated_json_value(value, context)
    assert isinstance(validated, dict), context
    return validated


def cargo_dependency_tables(
    manifest: dict[str, object],
) -> tuple[CargoDependencyTable, ...]:
    tables: list[CargoDependencyTable] = []
    for table_name in ("dependencies", "dev-dependencies", "build-dependencies"):
        table = validated_string_object_dict(manifest.get(table_name, {}), table_name)
        tables.append(table)

    targets = validated_string_object_dict(manifest.get("target", {}), "target")
    for target_name, target_manifest_value in targets.items():
        target_manifest = validated_string_object_dict(
            target_manifest_value, target_name
        )
        for table_name in ("dependencies", "dev-dependencies", "build-dependencies"):
            table = validated_string_object_dict(
                target_manifest.get(table_name, {}), (target_name, table_name)
            )
            tables.append(table)
    return tuple(tables)


def assert_canonical_cargo_provenance(sources: dict[str, str]) -> None:
    assert (
        sorted(
            source_name
            for source_name, source in sources.items()
            if RETIRED_DESKTOP_PRODUCT_FEATURE in source
        )
        == []
    )
    expected_package_names = {
        "rust/pokecon": "pokecon",
    }
    expected_package_build: dict[str, str | bool] = {
        member: False for member in WORKSPACE_MEMBERS
    }
    expected_package_build["rust/pokecon"] = "build.rs"
    expected_build_dependencies: dict[str, dict[str, object]] = {
        member: {} for member in WORKSPACE_MEMBERS
    }
    expected_build_dependencies["rust/pokecon"] = {
        "dunce": "1.0.5",
        "hex": {"workspace": True},
        "serde": {"workspace": True},
        "serde_json": {"workspace": True},
        "sha2": {"workspace": True},
        "tauri-build": {"version": "2.5.4", "features": []},
        "toml": {"workspace": True},
    }

    workspace_manifest = validated_string_object_dict(
        tomllib.loads(sources[WORKSPACE_MANIFEST_SOURCE]), WORKSPACE_MANIFEST_SOURCE
    )
    workspace = validated_string_object_dict(
        workspace_manifest["workspace"], "workspace"
    )
    assert workspace["resolver"] == "2"
    assert workspace["members"] == list(WORKSPACE_MEMBERS)
    assert workspace["default-members"] == list(WORKSPACE_DEFAULT_MEMBERS)
    for workspace_section in ("package", "dependencies", "lints"):
        assert (
            workspace[workspace_section]
            == EXPECTED_WORKSPACE_PROVENANCE[workspace_section]
        )
    assert workspace_manifest["profile"] == {
        "dev": {"debug": 0},
        "test": {"debug": 0},
    }
    assert not ({"patch", "replace"} & workspace_manifest.keys())

    manifests: dict[str, dict[str, object]] = {
        member: validated_string_object_dict(
            tomllib.loads(sources[source_name]), source_name
        )
        for member, source_name in WORKSPACE_MANIFEST_SOURCES.items()
    }
    assert {
        member: hashlib.sha256(sources[source_name].encode()).hexdigest()
        for member, source_name in WORKSPACE_MANIFEST_SOURCES.items()
    } == EXPECTED_WORKSPACE_MANIFEST_HASHES
    assert (
        hashlib.sha256(sources[WORKSPACE_LOCK_SOURCE].encode()).hexdigest()
        == "e2ee3588b851a88de300712ca0b00d7508f5f0ba1cba8acf92be26122531480b"
    )
    assert manifests["rust/pokecon"] == EXPECTED_POKECON_MANIFEST
    assert all(
        RETIRED_DESKTOP_PRODUCT_FEATURE not in sources[source_name]
        for source_name in WORKSPACE_MANIFEST_SOURCES.values()
    )
    for member, dependency_names in (
        (
            "rust/pokecon",
            ("opener", "rfd", "tauri", "tauri-plugin-single-instance"),
        ),
    ):
        dependencies = validated_string_object_dict(
            manifests[member]["dependencies"], (member, "dependencies")
        )
        for dependency_name in dependency_names:
            dependency = validated_string_object_dict(
                dependencies[dependency_name], (member, dependency_name)
            )
            assert dependency == {"workspace": True}, (member, dependency_name)
    assert {
        member: validated_string_object_dict(manifest["package"], (member, "package"))[
            "name"
        ]
        for member, manifest in manifests.items()
    } == expected_package_names
    assert {
        member: validated_string_object_dict(
            manifest["package"], (member, "package")
        ).get("build")
        for member, manifest in manifests.items()
    } == expected_package_build
    assert {
        member: manifest.get("build-dependencies", {})
        for member, manifest in manifests.items()
    } == expected_build_dependencies
    for member, manifest in manifests.items():
        targets = validated_string_object_dict(manifest.get("target", {}), member)
        for target_name, target_manifest_value in targets.items():
            target_manifest = validated_string_object_dict(
                target_manifest_value, (member, target_name)
            )
            assert target_manifest.get("build-dependencies", {}) == {}, (
                member,
                target_name,
            )

    path_dependencies: list[tuple[str, str, str, str]] = []
    workspace_dependencies = validated_string_object_dict(
        workspace.get("dependencies", {}), "workspace dependencies"
    )
    dependency_owners: list[tuple[str, tuple[CargoDependencyTable, ...]]] = [
        ("", (workspace_dependencies,))
    ]
    dependency_owners.extend(
        (member, cargo_dependency_tables(manifest))
        for member, manifest in manifests.items()
    )
    for owner, dependency_tables in dependency_owners:
        for dependency_table in dependency_tables:
            for dependency_name, dependency_spec in dependency_table.items():
                if not is_object_dict(dependency_spec) or "path" not in dependency_spec:
                    continue
                dependency_path = dependency_spec["path"]
                assert isinstance(dependency_path, str), (owner, dependency_name)
                dependency_package = dependency_spec.get("package", dependency_name)
                assert isinstance(dependency_package, str), (owner, dependency_name)
                path_dependencies.append(
                    (owner, dependency_name, dependency_package, dependency_path)
                )
    for (
        owner,
        dependency_name,
        dependency_package,
        dependency_path,
    ) in path_dependencies:
        assert not dependency_path.startswith(("/", "\\")), (
            owner,
            dependency_name,
            dependency_path,
        )
        assert "\\" not in dependency_path, (owner, dependency_name, dependency_path)
        resolved = posixpath.normpath(posixpath.join(owner, dependency_path))
        assert resolved in WORKSPACE_MEMBERS, (owner, dependency_name, dependency_path)
        expected_package = expected_package_names[resolved]
        assert dependency_name == expected_package, (
            owner,
            dependency_name,
            dependency_path,
        )
        assert dependency_package == expected_package, (
            owner,
            dependency_name,
            dependency_package,
            dependency_path,
        )

    expected_build_script_inventory = [
        {"path": build_script, "kind": "regular"}
        for build_script in WORKSPACE_BUILD_SCRIPT_SOURCES
    ]
    assert (
        json.loads(sources[BUILD_SCRIPT_INVENTORY_SOURCE])
        == expected_build_script_inventory
    )
    actual_build_script_sources = {
        source_name
        for source_name in sources
        if source_name.startswith("@") and source_name.endswith("/build.rs")
    }
    assert actual_build_script_sources == set(WORKSPACE_BUILD_SCRIPT_SOURCES.values())
    expected_build_script_hashes = {
        "@pokecon/build.rs": (
            "3a7feb6b0da1d702fed78e2e3a7d0c9362a3d6f4b3364089c2a40612ee594858"
        ),
    }
    assert {
        source_name: hashlib.sha256(sources[source_name].encode()).hexdigest()
        for source_name in actual_build_script_sources
    } == expected_build_script_hashes
    pokecon_build_script = sources["@pokecon/build.rs"]
    for staging_proof, expected_count in (
        ("prepare_tauri_build_context", 2),
        ('out_directory.join("tauri-build-context")', 1),
        ("let mut pending = vec![source_root.to_path_buf()]", 1),
        ("let destination = destination_root.join(relative);", 1),
        ("pending.push(source);", 1),
        (
            ".set_times(fs::FileTimes::new().set_modified(metadata.modified()?))",
            1,
        ),
        ('Some(Path::new("autogenerated"))', 1),
        ("let mut missing_optional_directory = false;", 1),
        (
            "if copy_optional_directory(&source, &destination, excluded_relative_root)?",
            1,
        ),
        ("missing_optional_directory = true;", 1),
        ("if missing_optional_directory {", 1),
        (
            'println!("cargo:rerun-if-changed={}", manifest_directory.display());',
            1,
        ),
        ('.permissions_path_pattern("./permissions/**/*")', 1),
        ('.capabilities_path_pattern("./capabilities/**/*")', 1),
        ("env::set_current_dir(&context_manifest_directory)?;", 1),
        ("let restore_result = env::set_current_dir(&manifest_directory);", 1),
        ('"icons/32x32.png"', 1),
        ('"icons/128x128.png"', 1),
        ('"icons/128x128@2x.png"', 1),
        ('"icons/icon.icns"', 1),
        ('"icons/icon.ico"', 1),
        ('"linux/70-pokecon-controller.rules"', 1),
        ('"linux/reload-udev.sh"', 1),
        ("dunce::canonicalize(", 2),
        ("tauri_current_directory_is_compatible", 2),
        ("Prefix::Disk(_)", 1),
        (
            "Tauri build context cannot be represented as a conventional Windows disk path",
            1,
        ),
    ):
        assert pokecon_build_script.count(staging_proof) == expected_count, (
            staging_proof
        )
    run_tauri_build_body = rust_top_level_function_body(
        pokecon_build_script,
        r"\bfn\s+run_tauri_build\s*\(",
    )
    assert "fs::canonicalize(" not in run_tauri_build_body
    assert (
        run_tauri_build_body.index('env::var_os("CARGO_MANIFEST_DIR")')
        < run_tauri_build_body.index('env::var_os("OUT_DIR")')
        < run_tauri_build_body.index("prepare_tauri_build_context(")
        < run_tauri_build_body.index("tauri_current_directory_is_compatible(")
        < run_tauri_build_body.index(
            "env::set_current_dir(&context_manifest_directory)?;"
        )
    )
    settings_resources_body = rust_top_level_function_body(
        pokecon_build_script,
        r"\bfn\s+generate_settings_resources\s*\(\s*\)",
    )
    for settings_resource_proof, expected_count in (
        ('manifest_directory.join("../../pyproject.toml")', 1),
        ('env::var_os("POKECON_BUILD_UV_PATH")', 1),
        ('env::var("POKECON_BUILD_UV_VERSION")', 1),
        ('"uv.exe"', 1),
        ('"uv"', 1),
        ("sha256: digest_build_input(&path)", 1),
        ('.join("application-requirements.json")', 1),
        ('.join("managed-uv-source.json")', 1),
    ):
        assert (
            settings_resources_body.count(settings_resource_proof) == expected_count
        ), settings_resource_proof
    assert (
        settings_resources_body.index("match (")
        < settings_resources_body.index('env::var_os("POKECON_BUILD_UV_PATH")')
        < settings_resources_body.index('env::var("POKECON_BUILD_UV_VERSION")')
        < settings_resources_body.index("sha256: digest_build_input(&path)")
        < settings_resources_body.index('.join("managed-uv-source.json")')
    )
    copied_file_permissions_body = rust_top_level_function_body(
        pokecon_build_script,
        r"\bfn\s+make_copied_file_owner_writable\s*\(",
    )
    assert compact_rust(copied_file_permissions_body) == compact_rust(
        """
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(permissions.mode() | 0o200);
        }
        #[cfg(not(unix))]
        {
            permissions.set_readonly(false);
        }
        fs::set_permissions(destination, permissions)
        """
    )
    copy_regular_file_body = rust_top_level_function_body(
        pokecon_build_script,
        r"\bfn\s+copy_regular_file\s*\(",
    )
    assert copy_regular_file_body.count("fs::symlink_metadata(source)?") == 1
    assert copy_regular_file_body.count("fs::copy(source, destination)?") == 1
    assert (
        copy_regular_file_body.count(
            "make_copied_file_owner_writable(destination, metadata.permissions())"
        )
        == 1
    )
    assert (
        copy_regular_file_body.index("fs::symlink_metadata(source)?")
        < copy_regular_file_body.index("metadata.file_type().is_symlink()")
        < copy_regular_file_body.index("fs::copy(source, destination)?")
        < copy_regular_file_body.index("make_copied_file_owner_writable(")
    )
    provenance_build_main = rust_top_level_function_body(
        pokecon_build_script,
        r"\bfn\s+main\s*\(\s*\)",
    )
    assert compact_rust(provenance_build_main) == compact_rust(
        """
        println!("cargo:rerun-if-env-changed=POKECON_BUILD_PYTHON");
        println!("cargo:rerun-if-env-changed={RESOURCE_PROVENANCE_ENVIRONMENT}");
        let provenance = validated_resource_provenance().unwrap_or_else(|error| panic!("{error}"));
        println!("cargo:rustc-env={RESOURCE_PROVENANCE_ENVIRONMENT}={provenance}");
        generate_settings_resources();
        run_tauri_build().expect("Tauri application metadata must be valid");
        """
    )
    assert (
        provenance_build_main.index(
            "cargo:rerun-if-env-changed={RESOURCE_PROVENANCE_ENVIRONMENT}"
        )
        < provenance_build_main.index("validated_resource_provenance()")
        < provenance_build_main.index(
            "cargo:rustc-env={RESOURCE_PROVENANCE_ENVIRONMENT}={provenance}"
        )
        < provenance_build_main.index("generate_settings_resources()")
        < provenance_build_main.index("run_tauri_build()")
    )
    for provenance_build_proof, expected_count in (
        ('"POKECON_RESOURCE_PROVENANCE"', 1),
        ("ResourceProvenanceEnvironmentError::Missing", 1),
        ("ResourceProvenanceEnvironmentError::NonUnicode", 1),
        ("ResourceProvenanceEnvironmentError::Malformed", 1),
        ('matches!(value.as_str(), "development" | "nix-exact")', 1),
        ('.strip_prefix("packaged:")', 1),
        ("value.len() == 64", 1),
        ("byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)", 1),
        ("cargo:rerun-if-env-changed={RESOURCE_PROVENANCE_ENVIRONMENT}", 1),
        (
            "cargo:rustc-env={RESOURCE_PROVENANCE_ENVIRONMENT}={provenance}",
            1,
        ),
    ):
        assert pokecon_build_script.count(provenance_build_proof) == expected_count, (
            provenance_build_proof
        )
    assert "{value}" not in pokecon_build_script
    assert "{_value}" not in pokecon_build_script
    assert json.loads(sources[CARGO_CONFIG_INVENTORY_SOURCE]) == []
    assert json.loads(sources[PRODUCTION_BUILD_INPUT_INVENTORY_SOURCE]) == [
        {"path": relative_path, "kind": "regular"}
        for relative_path in (
            *PRODUCTION_BUILD_INPUT_SOURCES.values(),
            *PRODUCTION_BINARY_BUILD_INPUT_SOURCES.values(),
        )
    ]
    expected_production_build_input_hashes = {
        "@.gitignore": (
            "2d4389264bc9e1f99657e1a6b41f8a58dca00c27b98d1dd05d8dc556633573b3"
        ),
        "@LICENSE": "263a077fd442c4196f1f54ef8840025030b6016d39192840651d3c7eb9330e4c",
        "@pyproject.toml": (
            "a0409c3e6fc3816c17c7ae09025d008c311cc0e0b6329863404eb097c653d6ac"
        ),
        "@release/build_runtime.py": (
            "545de05ab40d040fea2b8c40d0f1c6586f1c475e08e55cab1a3026ad2d7cf333"
        ),
        "@release/normalize_debian_package.py": (
            "eb52cfd61f91706afad0290964182bbacbd74b469e48d04f088c5ab8082cd576"
        ),
        "@release/normalize_linux_elf.py": (
            "04274ee342642e554d13d08b80977725ba5c33345e86a5a1f6c533a7329a2aa0"
        ),
        "@release/stage.py": (
            "7f490a625e156fbc2bf38f1d31532234268f89a84084875ad9173644054f8fff"
        ),
        "@tauri/linux/70-pokecon-controller.rules": (
            "d51f5162c4670cfa6ab5aafe02000a8b4e37973d9998a09e80ae532ed649fbe2"
        ),
        "@tauri/linux/reload-udev.sh": (
            "b176e4dbc7d8e71958822574838c0d572479a9300ced82e695ff8d381847d371"
        ),
        "@uv.lock": "096ff36b7db11dc61f761aa498f9118e6c6834a757a66cdd44de18c00b7c3ea7",
    }
    assert {
        source_name: hashlib.sha256(sources[source_name].encode()).hexdigest()
        for source_name in PRODUCTION_BUILD_INPUT_SOURCES
    } == expected_production_build_input_hashes
    build_runtime_source = sources["@release/build_runtime.py"]
    stage_source = sources["@release/stage.py"]
    run_start = build_runtime_source.index("def run(")
    sha256_file_start = build_runtime_source.index("def sha256_file(", run_start)
    python_install_request_start = build_runtime_source.index(
        "def python_install_request("
    )
    python_minor_redirect_name_start = build_runtime_source.index(
        "def python_minor_redirect_name(", python_install_request_start
    )
    python_executable_start = build_runtime_source.index(
        "def python_executable(", python_minor_redirect_name_start
    )
    normalize_python_bytecode_start = build_runtime_source.index(
        "def normalize_python_bytecode(", python_executable_start
    )
    audit_python_bytecode_start = build_runtime_source.index(
        "def audit_python_bytecode(", normalize_python_bytecode_start
    )
    normalize_python_sysconfig_start = build_runtime_source.index(
        "def normalize_python_sysconfig(", audit_python_bytecode_start
    )
    verify_python_start = build_runtime_source.index(
        "def verify_python(", normalize_python_sysconfig_start
    )
    managed_python_install_prefix_start = build_runtime_source.index(
        "def _is_real_regular_file(", verify_python_start
    )
    install_python_start = build_runtime_source.index(
        "def install_python(", managed_python_install_prefix_start
    )
    portable_python_elf_metadata_start = build_runtime_source.index(
        "def portable_python_elf_metadata(", install_python_start
    )
    build_release_runtime_start = build_runtime_source.index(
        "def build_release_runtime(", portable_python_elf_metadata_start
    )
    main_start = build_runtime_source.index("def main(", build_release_runtime_start)
    run_source = build_runtime_source[run_start:sha256_file_start]
    verify_python_source = build_runtime_source[
        verify_python_start:managed_python_install_prefix_start
    ]
    python_install_request_source = build_runtime_source[
        python_install_request_start:python_minor_redirect_name_start
    ]
    python_minor_redirect_name_source = build_runtime_source[
        python_minor_redirect_name_start:python_executable_start
    ]
    python_bytecode_normalization_source = build_runtime_source[
        normalize_python_bytecode_start:audit_python_bytecode_start
    ]
    python_bytecode_audit_source = build_runtime_source[
        audit_python_bytecode_start:normalize_python_sysconfig_start
    ]
    managed_python_install_prefix_source = build_runtime_source[
        managed_python_install_prefix_start:install_python_start
    ]
    install_python_source = build_runtime_source[
        install_python_start:portable_python_elf_metadata_start
    ]
    build_release_runtime_source = build_runtime_source[
        build_release_runtime_start:main_start
    ]
    reject_staged_bytecode_start = stage_source.index("def reject_python_bytecode(")
    normalized_copy_start = stage_source.index(
        "def normalized_copy(", reject_staged_bytecode_start
    )
    ignore_entries_start = stage_source.index(
        "def ignore_entries(", normalized_copy_start
    )
    copy_tree_start = stage_source.index("def copy_tree(", ignore_entries_start)
    stage_resources_start = stage_source.index("def stage_resources(", copy_tree_start)
    stage_main_start = stage_source.index("def main(", stage_resources_start)
    reject_staged_bytecode_source = stage_source[
        reject_staged_bytecode_start:normalized_copy_start
    ]
    ignore_entries_source = stage_source[ignore_entries_start:copy_tree_start]
    stage_resources_source = stage_source[stage_resources_start:stage_main_start]
    child_environment_start = run_source.index(
        "child_environment = dict(os.environ if environment is None else environment)"
    )
    no_bytecode_environment_start = run_source.index(
        'child_environment["PYTHONDONTWRITEBYTECODE"] = "1"'
    )
    subprocess_start = run_source.index("completed = subprocess.run(")
    subprocess_environment_start = run_source.index("env=child_environment")
    assert (
        child_environment_start
        < no_bytecode_environment_start
        < subprocess_start
        < subprocess_environment_start
    )
    assert run_source.count("PYTHONDONTWRITEBYTECODE") == 1
    assert run_source.count("env=child_environment") == 1
    assert "env=None if environment is None" not in run_source
    assert build_runtime_source.count('PYTHON_VERSION = "3.14.3"') == 1
    assert (
        build_runtime_source.count(
            'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"'
        )
        == 1
    )
    linux_python_install_request_match = re.search(
        r'(?m)^LINUX_PYTHON_INSTALL_REQUEST = "cpython-3\.14\.3-linux-x86_64-(?P<libc>[a-z0-9_]+)"$',
        build_runtime_source,
    )
    assert linux_python_install_request_match is not None
    assert linux_python_install_request_match.group("libc") == "gnu"
    assert (
        build_runtime_source.count(
            'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"'
        )
        == 1
    )
    assert (
        build_runtime_source.count(
            'LINUX_PYTHON_MINOR_REDIRECT = "cpython-3.14-linux-x86_64-gnu"'
        )
        == 1
    )
    assert (
        build_runtime_source.count(
            'WINDOWS_PYTHON_MINOR_REDIRECT = "cpython-3.14-windows-x86_64-none"'
        )
        == 1
    )
    assert build_runtime_source.count("LINUX_PYTHON_INSTALL_REQUEST") == 2
    assert build_runtime_source.count("WINDOWS_PYTHON_INSTALL_REQUEST") == 2
    assert build_runtime_source.count("LINUX_PYTHON_MINOR_REDIRECT") == 2
    assert build_runtime_source.count("WINDOWS_PYTHON_MINOR_REDIRECT") == 2
    assert build_runtime_source.count("PYTHON_VERSION") == 5
    for bytecode_normalization_proof in (
        "if not _is_real_directory(root):",
        "pending_directories: list[Path] = [root]",
        "directory = pending_directories.pop()",
        "entries = tuple(directory.iterdir())",
        "entry.is_symlink() or entry.is_junction()",
        "entry.stat(follow_symlinks=False).st_mode",
        'if entry.suffix == ".pyc":',
        "redirected or not stat.S_ISREG(entry_mode)",
        'if directory.name != "__pycache__":',
        "bytecode artifact is outside a real ",
        'if entry.name == "__pycache__":',
        "redirected or not stat.S_ISDIR(entry_mode)",
        'if directory.name == "__pycache__":',
        "bytecode cache contains non-bytecode residue",
        "if not redirected and stat.S_ISDIR(entry_mode):",
        "cache_directory = bytecode_file.parent",
        "if not _is_real_directory(cache_directory):",
        "if not _is_real_regular_file(bytecode_file):",
        "bytecode_file.unlink()",
        "key=lambda path: len(path.parts)",
        "if not _is_real_directory(cache_directory):",
        "residue = tuple(cache_directory.iterdir())",
        "if residue:",
        "cache_directory.rmdir()",
    ):
        assert bytecode_normalization_proof in python_bytecode_normalization_source
    assert python_bytecode_normalization_source.count('entry.suffix == ".pyc"') == 1
    assert (
        python_bytecode_normalization_source.count('entry.name == "__pycache__"') == 1
    )
    assert python_bytecode_normalization_source.count(".unlink()") == 1
    assert python_bytecode_normalization_source.count(".rmdir()") == 1
    bytecode_preflight_end = python_bytecode_normalization_source.index(
        "    for bytecode_file in bytecode_files:"
    )
    bytecode_preflight_source = python_bytecode_normalization_source[
        :bytecode_preflight_end
    ]
    assert ".unlink()" not in bytecode_preflight_source
    assert ".rmdir()" not in bytecode_preflight_source
    assert python_bytecode_normalization_source.index(
        'if directory.name != "__pycache__":'
    ) < python_bytecode_normalization_source.index("bytecode_files.append(entry)")
    for unsafe_bytecode_normalization in (
        ".rglob(",
        ".glob(",
        "os.walk(",
        "shutil.rmtree(",
        "follow_symlinks=True",
    ):
        assert unsafe_bytecode_normalization not in python_bytecode_normalization_source
    for bytecode_audit_proof in (
        "if not _is_real_directory(root):",
        "pending_directories: list[Path] = [root]",
        "directory = pending_directories.pop()",
        "if not _is_real_directory(directory):",
        "entries = tuple(directory.iterdir())",
        "entry.is_symlink() or entry.is_junction()",
        "entry.stat(follow_symlinks=False).st_mode",
        'if entry.suffix in {".pyc", ".pyo"} or entry.name == "__pycache__":',
        "contains a bytecode artifact after ",
        "if not redirected and stat.S_ISDIR(entry_mode):",
        "pending_directories.append(entry)",
    ):
        assert bytecode_audit_proof in python_bytecode_audit_source
    assert python_bytecode_audit_source.count('entry.suffix in {".pyc", ".pyo"}') == 1
    assert python_bytecode_audit_source.count('entry.name == "__pycache__"') == 1
    for unsafe_bytecode_audit in (
        ".unlink(",
        ".rmdir(",
        "shutil.rmtree(",
        ".rglob(",
        ".glob(",
        "os.walk(",
        "follow_symlinks=True",
    ):
        assert unsafe_bytecode_audit not in python_bytecode_audit_source
    for staged_bytecode_audit_proof in (
        "if not _is_real_directory(root):",
        "pending_directories: list[Path] = [root]",
        "directory = pending_directories.pop()",
        "if not _is_real_directory(directory):",
        "entries = tuple(directory.iterdir())",
        "entry.is_symlink() or entry.is_junction()",
        "entry.stat(follow_symlinks=False).st_mode",
        'entry.name == "__pycache__"',
        "entry.suffix.casefold() in BYTECODE_SUFFIXES",
        "Python distribution contains bytecode artifact",
        "if not redirected and stat.S_ISDIR(entry_mode):",
        "pending_directories.append(entry)",
    ):
        assert staged_bytecode_audit_proof in reject_staged_bytecode_source
    for unsafe_staged_bytecode_audit in (
        ".unlink(",
        ".rmdir(",
        "shutil.rmtree(",
        ".rglob(",
        ".glob(",
        "os.walk(",
        "follow_symlinks=True",
    ):
        assert unsafe_staged_bytecode_audit not in reject_staged_bytecode_source
    assert 'BYTECODE_SUFFIXES = {".pyc", ".pyo"}' in stage_source
    assert 'IGNORED_NAMES = {".pytest_cache", ".ruff_cache"}' in stage_source
    assert "IGNORED_SUFFIXES" not in stage_source
    assert '"__pycache__"' not in ignore_entries_source
    assert "BYTECODE_SUFFIXES" not in ignore_entries_source
    assert ignore_entries_source.count(".is_symlink()") == 1
    assert ignore_entries_source.count(".is_junction()") == 1
    staged_input_audit_start = stage_resources_source.index(
        "    reject_python_bytecode(python)"
    )
    staged_output_guard_start = stage_resources_source.index("    if output.exists():")
    staged_python_copy_start = stage_resources_source.index(
        '    copy_tree(python, output / "python")'
    )
    staged_output_audit_start = stage_resources_source.index(
        '    reject_python_bytecode(output / "python")'
    )
    staged_inventory_start = stage_resources_source.index(
        "    files = resource_inventory(output)"
    )
    assert (
        staged_input_audit_start
        < staged_output_guard_start
        < staged_python_copy_start
        < staged_output_audit_start
        < staged_inventory_start
    )
    assert stage_resources_source.count("reject_python_bytecode(") == 2
    for install_request_selector_proof in (
        'if platform_name == "linux":',
        "return LINUX_PYTHON_INSTALL_REQUEST",
        'if platform_name == "win32":',
        "return WINDOWS_PYTHON_INSTALL_REQUEST",
        "portable Python install request does not support {platform_name!r}",
        "raise ValueError(message)",
    ):
        assert python_install_request_source.count(install_request_selector_proof) == 1
    assert "startswith" not in python_install_request_source
    assert "PYTHON_VERSION" not in python_install_request_source
    for minor_redirect_selector_proof in (
        'if platform_name == "linux":',
        "return LINUX_PYTHON_MINOR_REDIRECT",
        'if platform_name == "win32":',
        "return WINDOWS_PYTHON_MINOR_REDIRECT",
        "portable Python minor redirect does not support {platform_name!r}",
        "raise ValueError(message)",
    ):
        assert (
            python_minor_redirect_name_source.count(minor_redirect_selector_proof) == 1
        )
    assert "startswith" not in python_minor_redirect_name_source
    assert "PYTHON_VERSION" not in python_minor_redirect_name_source
    assert "if version != PYTHON_VERSION:" in verify_python_source
    assert "differs from {PYTHON_VERSION}" in verify_python_source
    assert "python_install_request" not in verify_python_source
    assert "PYTHON_INSTALL_REQUEST" not in verify_python_source
    for managed_install_inventory_proof in (
        "expected_install_request = python_install_request(platform_name)",
        "minor_redirect_name = python_minor_redirect_name(platform_name)",
        "if install_request != expected_install_request:",
        "not path.is_symlink()",
        "not path.is_junction()",
        "path.stat(follow_symlinks=False).st_mode",
        "except OSError:",
        "if not _is_real_directory(install_root):",
        "resolved_install_root = install_root.resolve(strict=True)",
        "entries = {entry.name: entry for entry in install_root.iterdir()}",
        '".gitignore",',
        '".lock",',
        '".temp",',
        "install_request,",
        "minor_redirect_name,",
        "if actual_names != expected_names:",
        'gitignore = entries[".gitignore"]',
        'gitignore.read_bytes() != b"*"',
        'lock = entries[".lock"]',
        'lock.read_bytes() != b""',
        'temporary = entries[".temp"]',
        "tuple(temporary.iterdir())",
        "full_install = entries[install_request]",
        "installed_prefix = full_install.resolve(strict=True)",
        "if installed_prefix.parent != resolved_install_root:",
        "minor_redirect = entries[minor_redirect_name]",
        'if platform_name == "linux":',
        "minor_redirect.is_symlink() and not minor_redirect.is_junction()",
        'elif platform_name == "win32":',
        "minor_redirect.is_junction() and not minor_redirect.is_symlink()",
        "minor_redirect.resolve(strict=True) != installed_prefix",
        "return installed_prefix",
    ):
        assert managed_install_inventory_proof in managed_python_install_prefix_source
    assert managed_python_install_prefix_source.count("except OSError:") == 2
    assert managed_python_install_prefix_source.count("not path.is_symlink()") == 2
    assert managed_python_install_prefix_source.count("not path.is_junction()") == 2
    assert managed_python_install_prefix_source.count("stat.S_ISREG(") == 1
    assert managed_python_install_prefix_source.count("stat.S_ISDIR(") == 1
    assert managed_python_install_prefix_source.count("resolve(strict=True)") == 3
    assert "installed_entries" not in managed_python_install_prefix_source
    assert "[0]" not in managed_python_install_prefix_source
    assert "next(" not in managed_python_install_prefix_source
    assert "any(" not in managed_python_install_prefix_source
    assert (
        "    install_request = python_install_request(sys.platform)\n"
        in install_python_source
    )
    assert (
        '            "install",\n            install_request,\n'
        in install_python_source
    )
    assert (
        "    installed_prefix = managed_python_install_prefix(\n"
        "        install_root,\n"
        "        install_request,\n"
        "        platform_name=sys.platform,\n"
        "    )\n" in install_python_source
    )
    assert install_python_source.count("python_install_request") == 1
    assert install_python_source.count("managed_python_install_prefix") == 1
    assert install_python_source.count("install_request") == 4
    assert (
        "    shutil.copytree(installed_prefix, output, symlinks=True)\n"
        "    normalize_python_bytecode(output)\n"
        "    normalize_python_sysconfig(output, installed_prefix)\n"
        in install_python_source
    )
    assert install_python_source.count("normalize_python_bytecode") == 1
    assert install_python_source.count("normalize_python_sysconfig") == 1
    assert ".iterdir()" not in install_python_source
    assert "PYTHON_INSTALL_REQUEST" not in install_python_source
    assert "PYTHON_VERSION" not in install_python_source
    build_wheels_start = build_release_runtime_source.index("        build_wheels(")
    verify_wheelhouse_start = build_release_runtime_source.index(
        "        inventory = verify_wheelhouse("
    )
    final_raw_digest_start = build_release_runtime_source.index(
        "        if sha256_file(raw_python) != raw_python_digest:"
    )
    final_raw_elf_start = build_release_runtime_source.index(
        "            if portable_python_elf_metadata(raw_python, patchelf) != ("
    )
    final_bytecode_audit_start = build_release_runtime_source.index(
        "        audit_python_bytecode(runtime_output)"
    )
    temporary_context_exit = build_release_runtime_source.index("    wheels = [")
    assert (
        build_wheels_start
        < verify_wheelhouse_start
        < final_raw_digest_start
        < final_raw_elf_start
        < final_bytecode_audit_start
        < temporary_context_exit
    )
    assert (
        build_release_runtime_source.count("audit_python_bytecode(runtime_output)") == 1
    )
    after_final_bytecode_audit = build_release_runtime_source[
        final_bytecode_audit_start:temporary_context_exit
    ]
    assert "run(" not in after_final_bytecode_audit
    assert "verify_python(" not in after_final_bytecode_audit
    assert "build_wheels(" not in after_final_bytecode_audit
    assert "verify_wheelhouse(" not in after_final_bytecode_audit
    expected_production_binary_build_input_hashes = {
        "@tauri/icons/32x32.png": (
            "275d022d06769e90fe434a0e1e78a1c9abcce875b6154566046a258c94444503"
        ),
        "@tauri/icons/128x128.png": (
            "fd753daf7b8a9929f4b69cebb7dab1564725f91e7a98f251f2925701b2725b60"
        ),
        "@tauri/icons/128x128@2x.png": (
            "d7af5d232f00d4fed039fe3cfa06af21c3bbcc0651cf4bbaa18cda57816b7308"
        ),
        "@tauri/icons/icon.icns": (
            "238b45d54769e870d139e9724009f2ffd219846061dd022b2837497df1a8799b"
        ),
        "@tauri/icons/icon.ico": (
            "52d7fc9b699fc30c3687682c2f68d14aef3a2600c5ff6ab25b1f9bb88eff1704"
        ),
    }
    assert {
        source_name: hashlib.sha256(bytes.fromhex(sources[source_name])).hexdigest()
        for source_name in PRODUCTION_BINARY_BUILD_INPUT_SOURCES
    } == expected_production_binary_build_input_hashes
    assert json.loads(sources[PRODUCTION_CARGO_TARGET_ROOT_INVENTORY_SOURCE]) == [
        {"path": relative_path, "kind": "regular"}
        for relative_path in PRODUCTION_CARGO_TARGET_ROOT_SOURCES.values()
    ]
    expected_production_cargo_target_root_hashes = {
        "@rust/pokecon/src/bin/worker.rs": (
            "8d0cdb537d0c2ee0226c38706d1c3939791b8fd5bb01014b059535ba4af15d21"
        ),
        "@rust/pokecon/src/lib.rs": (
            "420ce5e2a0b0c5ea7b67dcef273b7d7873ad3f83863b578e09338c85302709f0"
        ),
        "@rust/pokecon/src/main.rs": (
            "3d6086ac1a4eb099da412154307d639e18d0c51a39396cf0efe0d5a937a3c137"
        ),
    }
    assert {
        source_name: hashlib.sha256(sources[source_name].encode()).hexdigest()
        for source_name in PRODUCTION_CARGO_TARGET_ROOT_SOURCES
    } == expected_production_cargo_target_root_hashes
    assert (
        set(PRODUCTION_CARGO_TARGET_ROOT_SOURCES.values())
        | set(WORKSPACE_BUILD_SCRIPT_SOURCES)
    ) == {
        "rust/pokecon/src/bin/worker.rs",
        "rust/pokecon/src/lib.rs",
        "rust/pokecon/src/main.rs",
        "rust/pokecon/build.rs",
    }
    assert json.loads(sources[TAURI_ACL_INPUT_INVENTORY_SOURCE]) == []
    assert json.loads(sources[TAURI_CONFIG_INVENTORY_SOURCE]) == [
        {"path": "rust/pokecon/tauri.conf.json", "kind": "regular"}
    ]
    tauri_config_source_name = "@rust/pokecon/tauri.conf.json"
    assert (
        hashlib.sha256(sources[tauri_config_source_name].encode()).hexdigest()
        == "da9e84c38519abdfbf9b2f64f4710108bd57ca571266b3152b2b412252f94dae"
    )
    tauri_config = validated_json_object(
        json.loads(sources[tauri_config_source_name]), tauri_config_source_name
    )
    assert "build" not in tauri_config
    expected_debian_dependencies = [
        "libgl1",
        "libglib2.0-0",
        "libgtk-3-0",
        "libayatana-appindicator3-1",
        "libportaudio2",
        "libsm6",
        "libudev1",
        "libwebkit2gtk-4.1-0",
        "libxcb1",
        "libxext6",
        "libxrender1",
    ]
    tauri_bundle = validated_json_object(tauri_config["bundle"], "bundle")
    assert tauri_bundle["icon"] == [
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/icon.icns",
        "icons/icon.ico",
    ]
    tauri_linux_bundle = validated_json_object(tauri_bundle["linux"], "linux")
    tauri_debian_bundle = validated_json_object(tauri_linux_bundle["deb"], "deb")
    configured_debian_dependencies = tauri_debian_bundle["depends"]
    assert configured_debian_dependencies == expected_debian_dependencies
    forbidden_tauri_command_keys = {
        "beforebuildcommand",
        "beforebundlecommand",
        "beforedevcommand",
    }

    def assert_no_tauri_commands(value: JsonValue) -> None:
        if isinstance(value, dict):
            for key, nested in value.items():
                assert key.casefold() not in forbidden_tauri_command_keys
                assert_no_tauri_commands(nested)
        elif isinstance(value, list):
            for nested in value:
                assert_no_tauri_commands(nested)

    assert_no_tauri_commands(tauri_config)
    assert json.loads(sources[FLAKE_LOCK_INVENTORY_SOURCE]) == {
        "path": "flake.lock",
        "kind": "regular",
    }
    assert (
        hashlib.sha256(sources[FLAKE_LOCK_SOURCE].encode()).hexdigest()
        == "2d04f1fe263a629759c18a3f37ac016b488477404b4197f37bb6f5fb633ba2ee"
    )
    flake_lock = json.loads(sources[FLAKE_LOCK_SOURCE])
    assert flake_lock["version"] == 7
    assert flake_lock["root"] == "root"
    assert flake_lock["nodes"]["root"]["inputs"] == {
        "flake-parts": "flake-parts",
        "git-hooks": "git-hooks",
        "linux-release-nixpkgs": "linux-release-nixpkgs",
        "nixpkgs": "nixpkgs",
        "rust-overlay": "rust-overlay",
        "systems": "systems",
        "treefmt-nix": "treefmt-nix",
    }
    assert flake_lock["nodes"]["flake-parts"]["locked"] == {
        "lastModified": 1782949081,
        "narHash": "sha256-vp6Y/Grm98ESt6ceOkWiHWyZRDV3J1RID4w+6NWK9yA=",
        "owner": "hercules-ci",
        "repo": "flake-parts",
        "rev": "17c9d6cdfc60c64f4ee8d306f9bc0b4ccb51481e",
        "type": "github",
    }
    assert flake_lock["nodes"]["flake-parts"]["inputs"] == {"nixpkgs-lib": ["nixpkgs"]}
    assert flake_lock["nodes"]["git-hooks"]["locked"] == {
        "lastModified": 1784288435,
        "narHash": "sha256-ReRHaLgr/uVqdD8afFSn+myXIfpHeOhP0yYe0TJqAA8=",
        "owner": "cachix",
        "repo": "git-hooks.nix",
        "rev": "43b3c1ab9d40fb1dbb008f451988a91e375825e9",
        "type": "github",
    }
    assert flake_lock["nodes"]["git-hooks"]["inputs"] == {
        "flake-compat": "flake-compat",
        "nixpkgs": ["nixpkgs"],
    }
    assert flake_lock["nodes"]["linux-release-nixpkgs"]["locked"] == {
        "lastModified": 1735563628,
        "narHash": "sha256-OnSAY7XDSx7CtDoqNh8jwVwh4xNL/2HaJxGjryLWzX8=",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "b134951a4c9f3c995fd7be05f3243f8ecd65d798",
        "type": "github",
    }
    assert flake_lock["nodes"]["nixpkgs"]["locked"] == {
        "lastModified": 1784796856,
        "narHash": "sha256-wWFrV5/Qbm+lyt5x20E/bSbfJiGKMo4RCxZV8cl/WZI=",
        "owner": "NixOS",
        "repo": "nixpkgs",
        "rev": "e2587caef70cea85dd97d7daab492899902dbf5d",
        "type": "github",
    }
    assert flake_lock["nodes"]["rust-overlay"]["locked"] == {
        "lastModified": 1784870759,
        "narHash": "sha256-ZaS89rj5u6pygXKDRfCzPMGm7isJxLsSeIAR5EaSyu8=",
        "owner": "oxalica",
        "repo": "rust-overlay",
        "rev": "471286a5fadc690e2408ad854eb32325f5e74da7",
        "type": "github",
    }
    assert flake_lock["nodes"]["rust-overlay"]["inputs"] == {"nixpkgs": ["nixpkgs"]}
    assert flake_lock["nodes"]["systems"]["locked"] == {
        "lastModified": 1681028828,
        "narHash": "sha256-Vy1rq5AaRuLzOxct8nz4T6wlgyUR7zLU309k9mBC768=",
        "owner": "nix-systems",
        "repo": "default",
        "rev": "da67096a3b9bf56a91d16901293e51ba5b49a27e",
        "type": "github",
    }
    assert flake_lock["nodes"]["treefmt-nix"]["locked"] == {
        "lastModified": 1784369104,
        "narHash": "sha256-47cxbcZODibHv3rELFQ9vZly0vUNkND/atn/U7HLeb0=",
        "owner": "numtide",
        "repo": "treefmt-nix",
        "rev": "df3c0640565d04a0261253cdd89fce78ec50168a",
        "type": "github",
    }
    assert flake_lock["nodes"]["treefmt-nix"]["inputs"] == {"nixpkgs": ["nixpkgs"]}
    assert json.loads(sources[RUST_TOOLCHAIN_INVENTORY_SOURCE]) == {
        "path": "rust-toolchain.toml",
        "kind": "regular",
    }
    assert tomllib.loads(sources[RUST_TOOLCHAIN_SOURCE]) == {
        "toolchain": {
            "channel": "stable",
            "components": ["rustfmt", "clippy", "rust-src"],
            "targets": [],
        }
    }
    assert (
        hashlib.sha256(sources[RUST_TOOLCHAIN_SOURCE].encode()).hexdigest()
        == "d3ceb1cb2217972a209e49ca1ac585998f21a2dabf96e6ceda03e9442e34ee29"
    )

    flake = sources[FLAKE_SOURCE]
    canonical_flake_hash_matches = tuple(
        re.finditer(
            r'canonicalFlakeHash = "(?P<hash>[0-9a-f]{64})";',
            flake,
        )
    )
    assert len(canonical_flake_hash_matches) == 1
    canonical_flake_hash_match = canonical_flake_hash_matches[0]
    canonical_flake_hash = canonical_flake_hash_match.group("hash")
    normalized_flake = (
        flake[: canonical_flake_hash_match.start("hash")]
        + "<canonical-flake-sha256>"
        + flake[canonical_flake_hash_match.end("hash") :]
    )
    assert hashlib.sha256(normalized_flake.encode()).hexdigest() == canonical_flake_hash
    audit_test_hash_matches = tuple(
        re.finditer(
            r'expectedAuditTestHash = "(?P<hash>[0-9a-f]{64})";',
            normalized_flake,
        )
    )
    assert len(audit_test_hash_matches) == 1
    audit_test_hash_match = audit_test_hash_matches[0]
    fully_normalized_flake = (
        normalized_flake[: audit_test_hash_match.start("hash")]
        + "<production-routing-audit-sha256>"
        + normalized_flake[audit_test_hash_match.end("hash") :]
    )
    assert (
        hashlib.sha256(fully_normalized_flake.encode()).hexdigest()
        == "3f5c2426aaab9ad88cc43edc2f3e1bf1caf83d66aa88c6a7194194e28c36977d"
    )
    resolved_input_boundary = flake[: flake.index("flake-parts.lib.mkFlake")]
    assert (
        len(
            re.findall(
                r"(?m)^[ \t]*normalizedCanonicalFlakeText[ \t]*=(?!=)",
                resolved_input_boundary,
            )
        )
        == 1
    )
    assert (
        len(
            re.findall(
                r"(?m)^[ \t]*assert[ \t\r\n]+resolvedInputsAreCanonical\b",
                resolved_input_boundary,
            )
        )
        == 1
    )
    for resolved_input_proof in (
        "builtins.replaceStrings [ canonicalFlakeHash ]",
        'builtins.hashString "sha256" normalizedCanonicalFlakeText',
        "expectedResolvedInputs = [",
        'name = "flake-parts";',
        'name = "git-hooks";',
        'name = "linux-release-nixpkgs";',
        'name = "nixpkgs";',
        'name = "rust-overlay";',
        'name = "systems";',
        'name = "treefmt-nix";',
        "resolvedInputsAreCanonical = builtins.all",
    ):
        assert resolved_input_boundary.count(resolved_input_proof) == 1, (
            resolved_input_proof
        )
    runtime_libraries_start = flake.index("linuxReleaseRuntimeLibraries =")
    runtime_libraries_end = flake.index(
        "linuxReleaseBuildPath =", runtime_libraries_start
    )
    runtime_libraries = flake[runtime_libraries_start:runtime_libraries_end]
    assert tuple(
        line.strip() for line in runtime_libraries.splitlines() if line.strip()
    ) == (
        "linuxReleaseRuntimeLibraries = linuxReleasePkgs.symlinkJoin {",
        'name = "pokecon-linux-release-runtime-libraries";',
        "paths = [",
        "linuxReleasePortaudio",
        "linuxReleaseCc.cc.lib",
        "linuxReleasePkgs.zlib",
        "linuxReleasePkgs.xorg.libxcb",
        "linuxReleasePkgs.libglvnd",
        "linuxReleasePkgs.glib.out",
        "linuxReleasePkgs.xorg.libSM",
        "linuxReleasePkgs.xorg.libXext",
        "linuxReleasePkgs.xorg.libXrender",
        "];",
        "};",
    )
    assert flake.count("linuxReleaseRuntimeLibraries") == 4
    toolchain_construction_start = flake.index("pkgsWithOverlays =")
    toolchain_construction_end = flake.index(
        "pythonEnv =", toolchain_construction_start
    )
    toolchain_construction = flake[
        toolchain_construction_start:toolchain_construction_end
    ]
    assert (
        hashlib.sha256(toolchain_construction.strip().encode()).hexdigest()
        == "2b63f19035ba06a6aa5459a68f0eda5722f3d89fdd60747dce8c089eafc95e27"
    )
    for toolchain_proof in (
        "import inputs.nixpkgs",
        "overlays = [ (import rust-overlay) ];",
        '(builtins.readDir inputs.self.outPath)."rust-toolchain.toml" == "regular"',
        '== "d3ceb1cb2217972a209e49ca1ac585998f21a2dabf96e6ceda03e9442e34ee29"',
        "pkgsWithOverlays.rust-bin.fromRustupToolchainFile",
        'inputs.self.outPath + "/rust-toolchain.toml"',
        "rustPlatform = pkgs.makeRustPlatform",
        "cargo = rustToolchain;",
        "rustc = rustToolchain;",
    ):
        assert toolchain_proof in toolchain_construction, toolchain_proof
    python_build_inputs_start = toolchain_construction_end
    python_build_inputs_end = flake.index(
        "reproducibleRustcWrapper =", python_build_inputs_start
    )
    python_build_inputs = flake[python_build_inputs_start:python_build_inputs_end]
    assert (
        hashlib.sha256(python_build_inputs.strip().encode()).hexdigest()
        == "b1e14a0907dcef949c36a46821b1b852b2cfde6c04b43f0093d8c96bb65c8af0"
    )
    for python_build_input_proof in (
        "pythonEnv = pkgs.python314.withPackages",
        "pytest",
        'pythonPackageBuildUvVersion = "0.11.28"',
        "pkgs.uv.version == pythonPackageBuildUvVersion",
        'portableUvVersion = "0.11.8"',
        'portableUvVersionOutput = "uv 0.11.8 (x86_64-unknown-linux-gnu)"',
        'hash = "sha256-LWnCnwmLdeJIV4ytqFqWwwFPlTs+FlODuPJSTgTFngY="',
        'portableUvFileSha256 = "646adf5cf12ba17d1a41fa77c8dd6496f73651dcfeeed6b5f4ec019b36bc7153"',
        'portableUvSystemInterpreter = "/lib64/ld-linux-x86-64.so.2"',
        "portableUvExecutionLoader = linuxReleasePkgs.stdenv.cc.bintools.dynamicLinker",
        "portableUvExecutionLibraryPath = linuxReleasePkgs.lib.makeLibraryPath",
        "linuxReleasePkgs.glibc",
        "linuxReleaseCc.cc.lib",
        'linuxReleasePkgs.runCommand "pokecon-portable-uv-execution-${portableUvVersion}"',
        'if [ -L "$raw_uv" ] || [ ! -f "$raw_uv" ] || [ ! -x "$raw_uv" ]',
        'raw_uv_interpreter="$("${linuxReleasePkgs.patchelf}/bin/patchelf" --print-interpreter "$raw_uv")"',
        'if [ -n "$raw_uv_rpath" ]; then',
        '"${linuxReleasePkgs.patchelf}/bin/patchelf" --print-needed "$raw_uv"',
        '--set-interpreter "${portableUvExecutionLoader}"',
        '--set-rpath "${portableUvExecutionLibraryPath}"',
        "--inhibit-cache",
        '--library-path "${portableUvExecutionLibraryPath}"',
        'if ! actual_portable_uv_version_output="$("$out/bin/uv" --version)"; then',
        "portable uv execution copy version probe failed; actual output: $actual_portable_uv_version_output",
        'if [ "$actual_portable_uv_version_output" != "${portableUvVersionOutput}" ]; then',
        "portable uv execution copy reports an unexpected version; expected: ${portableUvVersionOutput}; actual: $actual_portable_uv_version_output",
        "portableUvExecutionBinary =",
    ):
        assert python_build_input_proof in python_build_inputs, python_build_input_proof
    assert python_build_inputs.count("portableUvVersionOutput") == 3
    assert python_build_inputs.count("actual_portable_uv_version_output") == 5
    portable_uv_needed_start = python_build_inputs.index(
        "portableUvNeededLibraries = ["
    )
    portable_uv_needed_end = python_build_inputs.index("];", portable_uv_needed_start)
    assert tuple(
        re.findall(
            r'^\s*"([^"]+)"\s*$',
            python_build_inputs[portable_uv_needed_start:portable_uv_needed_end],
            flags=re.MULTILINE,
        )
    ) == (
        "libc.so.6",
        "libdl.so.2",
        "libgcc_s.so.1",
        "libm.so.6",
        "libpthread.so.0",
        "librt.so.1",
    )
    reproducible_wrapper_start = flake.index(
        "reproducibleRustcWrapper = pkgs.writeShellScript"
    )
    pinned_wrapper_start = flake.index(
        "pinnedRustcWrapper = pkgs.writeShellScript", reproducible_wrapper_start
    )
    reproducible_wrapper = flake[reproducible_wrapper_start:pinned_wrapper_start]
    assert (
        hashlib.sha256(reproducible_wrapper.strip().encode()).hexdigest()
        == "ba283b0613e626069d43dfa85da25b4e96083947ad0fc4c291ffdbe95fdda8b7"
    )
    for wrapper_proof in (
        'if [ "$#" -lt 1 ]; then',
        'rustc="$1"',
        'if [ "$rustc" != "${rustToolchain}/bin/rustc" ]; then',
        'exec "$rustc"',
        '"--remap-path-prefix=${source}=/build/pokecon"',
        '"--remap-path-prefix=${controlledCargoSource}=/build/pokecon"',
        '"--remap-path-prefix=$POKECON_RUST_REMAP_SOURCE=/build/pokecon"',
    ):
        assert reproducible_wrapper.count(wrapper_proof) == 1, wrapper_proof
    assert reproducible_wrapper.index('rustc="$1"') < reproducible_wrapper.index(
        'exec "$rustc"'
    )

    cargo_invocation_root_start = flake.index(
        "cargoInvocationRoot = pkgs.runCommand", pinned_wrapper_start
    )
    pinned_wrapper = flake[pinned_wrapper_start:cargo_invocation_root_start]
    assert (
        hashlib.sha256(pinned_wrapper.strip().encode()).hexdigest()
        == "c6e95d6dbd9c83220009e76dbea55b367833cc0a0f0dfeab8b0180efb82139d8"
    )
    for wrapper_proof in (
        'if [ "$#" -lt 1 ]; then',
        'compiler="$1"',
        '"${rustToolchain}/bin/rustc")',
        '"${pkgs.cargo-auditable}/bin/cargo-auditable")',
        'if [ "$#" -lt 1 ] || [ "$1" != "${rustToolchain}/bin/rustc" ]; then',
    ):
        assert pinned_wrapper.count(wrapper_proof) == 1, wrapper_proof
    assert pinned_wrapper.count('exec "$compiler" "$@"') == 2
    assert pinned_wrapper.index('compiler="$1"') < pinned_wrapper.index(
        'exec "$compiler"'
    )

    cargo_invocation_root_end = flake.index("source =", cargo_invocation_root_start)
    cargo_invocation_root = flake[cargo_invocation_root_start:cargo_invocation_root_end]
    assert (
        hashlib.sha256(cargo_invocation_root.strip().encode()).hexdigest()
        == "f4b82b2c47f85c3180f28cb7824053b269d2c8cc9628eafba3e488c0f3726fbe"
    )
    assert (
        cargo_invocation_root.count(
            'cargoInvocationRoot = pkgs.runCommand "pokecon-cargo-invocation-root"'
        )
        == 1
    )
    assert cargo_invocation_root.count('mkdir -p "$out"') == 1
    assert "!(workspaceManifest ? patch) && !(workspaceManifest ? replace)" in flake

    source_filter_start = flake.index("source =", cargo_invocation_root_end)
    source_filter_end = flake.index("productionRoutingAuditTest =", source_filter_start)
    source_filter_section = flake[source_filter_start:source_filter_end]
    assert (
        hashlib.sha256(source_filter_section.strip().encode()).hexdigest()
        == "84a195661ddb9c3f5284bc0a30ac1d207d89078c506b906ebcfb79f01e5c5596"
    )
    assert source_filter_section.count('type == "directory"') == 1
    assert source_filter_section.count('type == "regular"') == 1
    assert source_filter_section.count('lib.hasSuffix ".json5" sourcePath') == 1
    assert (
        source_filter_section.count('sourcePath == "${inputs.self.outPath}/LICENSE"')
        == 1
    )

    workspace_provenance_start = flake.index(
        "workspaceMemberPaths =", source_filter_end
    )
    workspace_provenance_end = flake.index(
        "countStringOccurrences =", workspace_provenance_start
    )
    workspace_provenance_section = flake[
        workspace_provenance_start:workspace_provenance_end
    ]
    assert (
        hashlib.sha256(workspace_provenance_section.strip().encode()).hexdigest()
        == "21e506524e5e6b0dc73faba9e2c07eeaad7ed9ce9c1692f996c8a20e49a91354"
    )
    for cargo_graph_proof in (
        "expectedWorkspaceManifestHashes =",
        "workspaceTargetBuildDependenciesAreEmpty =",
        "workspaceMemberManifestsAreCanonical =",
        "repositoryCargoConfigInventory =",
        '== "e2ee3588b851a88de300712ca0b00d7508f5f0ba1cba8acf92be26122531480b"',
        'memberEntries."Cargo.toml" == "regular"',
        'memberEntries."build.rs" == "regular"',
        "dependency.dependencyName == expectedWorkspacePackageNames.${resolvedPath}",
        "dependency.packageName == expectedWorkspacePackageNames.${resolvedPath}",
    ):
        assert cargo_graph_proof in workspace_provenance_section, cargo_graph_proof

    controlled_manifest_start = workspace_provenance_end
    controlled_manifest_end = flake.index(
        "productionRoutingAudit =", controlled_manifest_start
    )
    controlled_manifest_section = flake[
        controlled_manifest_start:controlled_manifest_end
    ]
    assert (
        hashlib.sha256(controlled_manifest_section.strip().encode()).hexdigest()
        == "4749e115e0cf519da6370308219a8f7f9c346aece31b8e24a675a7d2d053a62c"
    )
    for exact_overlay_path, expected_count in (
        ('path = "${source}/rust/pokecon/src/lib.rs"', 2),
        ('path = \\"${source}/rust/pokecon/src/main.rs\\"', 1),
        ('build = "${source}/rust/pokecon/build.rs"', 2),
    ):
        assert (
            controlled_manifest_section.count(exact_overlay_path) == expected_count
        ), exact_overlay_path
    for controlled_manifest_proof in (
        "expectedControlledPokeconManifest =",
        "controlledWorkspaceManifest =",
        "controlledCargoLock =",
        "controlledWorkspaceMemberManifests = lib.mapAttrs",
        'install_controlled_cargo_manifest "${controlledWorkspaceManifest}" Cargo.toml',
        'install_controlled_cargo_manifest "${controlledCargoLock}" Cargo.lock',
        "controlledWorkspaceMemberManifests.${memberPath}",
        ") workspaceMemberPaths}",
        'controlled_workspace_root="$(pwd -P)"',
        'if [ -L "$controlled_workspace_directory" ]',
        '"${pkgs.diffutils}/bin/cmp" -s -- "$controlled_manifest_source" "$controlled_manifest_destination"',
        'controlledCargoSource = pkgs.runCommand "pokecon-controlled-cargo-source"',
        '"${pkgs.diffutils}/bin/diff"',
        '"${pkgs.coreutils}/bin/chmod" -R a-w -- "$out"',
        "\\( -type l -o -perm /0222 \\) -print -quit",
        "${verifyControlledCargoManifests}",
        'auditPytestConfig = pkgs.writeText "pokecon-audit-pytest.ini"',
        "production_routing_mutation: exhaustive fail-closed mutation audit",
    ):
        assert controlled_manifest_proof in controlled_manifest_section, (
            controlled_manifest_proof
        )
    assert (
        controlled_manifest_section.count(
            "builtins.fromTOML (builtins.unsafeDiscardStringContext controlled"
        )
        == 1
    )

    audit_start = flake.index("productionRoutingAudit =")
    mutation_runner_start = flake.index(
        "productionRoutingMutationAuditRunner =", audit_start
    )
    audit_end = mutation_runner_start
    audit_section = flake[audit_start:audit_end]
    assert (
        hashlib.sha256(audit_section.strip().encode()).hexdigest()
        == "623da76a199fe96a17bc299aacdbeea482580d766b31d16ddba45c06a69e5214"
    )
    assert (
        audit_section.count('pkgs.runCommand "pokecon-production-routing-audit"') == 1
    )
    assert (
        audit_section.count(
            '"${productionRoutingAuditTest}::'
            'test_rust_routes_override_implicit_head_and_websocket_any"'
        )
        == 1
    )
    assert audit_section.count('touch "$out/passed"') == 1
    for audit_harness_proof in (
        '"${pythonEnv}/bin/python" -I -m pytest',
        '-c "${auditPytestConfig}"',
        "--noconftest",
        "--import-mode=importlib",
        "-p no:cacheprovider",
        "unset PYTEST_ADDOPTS PYTEST_PLUGINS PYTHONPATH",
    ):
        assert audit_section.count(audit_harness_proof) == 1
    assert "export PYTHONPATH=" not in audit_section
    for directory_lock_probe_proof in (
        "pkgs.util-linux",
        'exec {artifact_directory_lock_probe_fd}< "$artifact_directory_lock_probe"',
        '"/proc/self/fd/$artifact_directory_lock_probe_fd"',
        'flock -x "$artifact_directory_lock_probe_fd"',
        'exec {artifact_directory_lock_contender_fd}< "$artifact_directory_lock_probe"',
        'if flock -n -x "$artifact_directory_lock_contender_fd"; then',
        "directory FD lock probe admitted a concurrent contender",
        "exec {artifact_directory_lock_contender_fd}<&-",
        "exec {artifact_directory_lock_probe_fd}<&-",
    ):
        assert audit_section.count(directory_lock_probe_proof) == 1
    assert audit_section.index(
        'exec {artifact_directory_lock_probe_fd}< "$artifact_directory_lock_probe"'
    ) < audit_section.index('flock -x "$artifact_directory_lock_probe_fd"')
    assert audit_section.index(
        'flock -x "$artifact_directory_lock_probe_fd"'
    ) < audit_section.index(
        'if flock -n -x "$artifact_directory_lock_contender_fd"; then'
    )

    mutation_runner_end = flake.index(
        "productionRoutingMutationAudit =", mutation_runner_start
    )
    mutation_runner_section = flake[mutation_runner_start:mutation_runner_end]
    assert (
        hashlib.sha256(mutation_runner_section.strip().encode()).hexdigest()
        == "622d395643a2cc269bffcd6fc3dae15172b7ea561b31c45da17ff364243265b6"
    )
    for mutation_runner_proof in (
        'name = "pokecon-production-routing-mutation-audit";',
        'excludeShellChecks = [ "SC2329" ];',
        "mutation_worker_limit=8",
        "mutation_worker_default_limit=4",
        'mutation_worker_count="$(nproc)"',
        'mutation_worker_count="$mutation_worker_default_limit"',
        'mutation_worker_count="$2"',
        "mutation worker count must be an integer from 1 through",
        "POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_COUNT",
        "POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX",
        'mutation_log_directory="$(mktemp -d -t pokecon-routing-mutations.XXXXXXXX)"',
        'mutation_worker_pids+=("$!")',
        "trap terminate_mutation_workers INT TERM",
        "mapfile -t mutation_active_worker_pids < <(jobs -p)",
        "for mutation_worker_pid in \"''${mutation_worker_pids[@]}\"; do",
        'if wait "$mutation_worker_pid"; then',
        'mutation_worker_statuses+=("$mutation_worker_status")',
        'cat -- "$mutation_log_directory/$mutation_shard_index.log"',
        'exit "$mutation_audit_status"',
        '"${pythonEnv}/bin/python" -I -m pytest',
        '-c "${auditPytestConfig}"',
        "--noconftest",
        "--import-mode=importlib",
        "-p no:cacheprovider",
        '"$mutation_test"',
        "Running 384 production-routing mutations across $mutation_worker_count process shards",
    ):
        assert mutation_runner_proof in mutation_runner_section, mutation_runner_proof
    assert (
        mutation_runner_section.count(
            'POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX="$mutation_shard_index"'
        )
        == 1
    )
    assert (
        mutation_runner_section.count(
            'POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_COUNT="$mutation_worker_count"'
        )
        == 1
    )
    assert (
        mutation_runner_section.count(
            "test_production_routing_audit_fails_closed_under_registration_mutations"
        )
        == 1
    )
    assert mutation_runner_section.count("unset \\") == 1
    assert mutation_runner_section.count("2>&1 &") == 1
    assert "export PYTHONPATH=" not in mutation_runner_section
    mutation_launch = mutation_runner_section.index('mutation_worker_pids+=("$!")')
    mutation_wait = mutation_runner_section.index(
        'if wait "$mutation_worker_pid"; then'
    )
    mutation_replay = mutation_runner_section.index(
        'cat -- "$mutation_log_directory/$mutation_shard_index.log"'
    )
    assert mutation_launch < mutation_wait < mutation_replay

    mutation_audit_start = mutation_runner_end
    mutation_audit_end = flake.index("mkApp =", mutation_audit_start)
    mutation_audit_section = flake[mutation_audit_start:mutation_audit_end]
    assert (
        hashlib.sha256(mutation_audit_section.strip().encode()).hexdigest()
        == "1c68ae8ae0e4ea1f2951a7aad951edba2a090045fe8ff4595a96af17377ea335"
    )
    assert (
        mutation_audit_section.count(
            '"${productionRoutingMutationAuditRunner}/bin/'
            'pokecon-production-routing-mutation-audit"'
        )
        == 1
    )
    assert mutation_audit_section.count('touch "$out/passed"') == 1
    assert "nativeBuildInputs = [ productionRoutingMutationAuditRunner ];" in (
        mutation_audit_section
    )
    assert '"${pythonEnv}/bin/python"' not in mutation_audit_section
    assert flake.count("checks.production-routing-mutation-audit =") == 1
    assert flake.count("test-production-routing-mutations =") == 1
    assert controlled_manifest_section.count("replaceManifestString") >= 4

    gate_environment_sanitizer_start = flake.index("sanitizeGateEnvironment =")
    sanitizer_start = flake.index(
        "sanitizeCargoCompilerEnvironment =", gate_environment_sanitizer_start
    )
    gate_environment_sanitizer = flake[gate_environment_sanitizer_start:sanitizer_start]
    assert (
        hashlib.sha256(gate_environment_sanitizer.strip().encode()).hexdigest()
        == "c3275e40e89e8e13c0f05c98cbd040c0979cfa6ea611230889c573ad06ab9ea6"
    )
    for gate_environment_proof in (
        'done < <("${pkgs.coreutils}/bin/env" -0)',
        "PATH | PWD)",
        'unset "$ambient_name"',
        "unset ambient_entry ambient_name",
        "umask 022",
    ):
        assert gate_environment_sanitizer.count(gate_environment_proof) == 1

    sanitizer_end = flake.index("assertNoRepositoryCargoConfigs =", sanitizer_start)
    sanitizer_section = flake[sanitizer_start:sanitizer_end]
    assert (
        hashlib.sha256(sanitizer_section.strip().encode()).hexdigest()
        == "e96861ad7cb5235da22353e3ab1a100f1f60f37a0cd6e9aac861ba2120656dc8"
    )
    for cargo_environment_alias in (
        "CARGO_BUILD_RUSTC",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_RUSTDOC",
        "CARGO_BUILD_RUSTDOCFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
        "CARGO_TARGET_*_RUSTC",
        "CARGO_TARGET_*_RUSTFLAGS",
        "CARGO_TARGET_*_RUSTDOC",
        "CARGO_TARGET_*_RUSTDOCFLAGS",
        "CARGO_TARGET_*_RUNNER",
        "CARGO_TARGET_*_LINKER",
    ):
        assert cargo_environment_alias in sanitizer_section, cargo_environment_alias

    repository_config_guard_end = flake.index(
        "assertNoCargoConfigAncestors =", sanitizer_end
    )
    repository_config_guard = flake[sanitizer_end:repository_config_guard_end]
    assert (
        hashlib.sha256(repository_config_guard.strip().encode()).hexdigest()
        == "d44b6a0629a3cf65c9d64237ae94048c9a2bfd351051f01e3ead1c4c9840f8e1"
    )
    for repository_config_proof in (
        '"${pkgs.coreutils}/bin/mktemp"',
        '--tmpdir="$repository_cargo_config_tmpdir"',
        '"${pkgs.coreutils}/bin/stat" -c %a',
        'if ! "${pkgs.findutils}/bin/find" -P "$PWD"',
        "-type l -name .cargo",
        "-path '*/.cargo/config'",
        "-path '*/.cargo/config.toml'",
        'done < "$repository_cargo_config_inventory"',
        'echo "repository Cargo configuration scan failed" >&2',
        'echo "failed to remove repository Cargo configuration inventory" >&2',
    ):
        assert repository_config_proof in repository_config_guard
    assert "done < <(" not in repository_config_guard

    ancestor_config_guard_end = flake.index(
        "resetTauriCargoTarget =", repository_config_guard_end
    )
    ancestor_config_guard = flake[repository_config_guard_end:ancestor_config_guard_end]
    assert (
        hashlib.sha256(ancestor_config_guard.strip().encode()).hexdigest()
        == "26fce5efa9fc439c50da352596e06d95760496f34453ac39d0fae6aa0e34a7fb"
    )
    for ancestor_config_proof in (
        'cargo_config_ancestor="$(pwd -P)"',
        "cargo_config_directory=\"''${cargo_config_ancestor%/}/.cargo\"",
        "for cargo_config_name in config config.toml; do",
        'if [ -e "$cargo_config_candidate" ] || [ -L "$cargo_config_candidate" ]',
        'if [ "$cargo_config_ancestor" = / ]; then',
    ):
        assert ancestor_config_proof in ancestor_config_guard

    tauri_boundary_end = flake.index("gateHomeExports =", ancestor_config_guard_end)
    tauri_boundary = flake[ancestor_config_guard_end:tauri_boundary_end]
    assert (
        hashlib.sha256(tauri_boundary.strip().encode()).hexdigest()
        == "90bdf3e7ae3cb836bc89c3f6ba72e12b7d1d26fc868973989e5795d8737b1da3"
    )
    tauri_invocation_boundary_start = tauri_boundary.index(
        "prepareTauriCargoInvocation ="
    )
    for cargo_target_boundary in (
        tauri_boundary[:tauri_invocation_boundary_start],
        tauri_boundary[tauri_invocation_boundary_start:],
    ):
        assert (
            cargo_target_boundary.count(
                'expected_cargo_target_dir="$gate_home/cargo-target"'
            )
            == 1
        )
    for tauri_boundary_proof in (
        "resetTauriCargoTarget =",
        '"${pkgs.coreutils}/bin/rm" -rf -- "$expected_cargo_target_dir"',
        "-mindepth 1 -print -quit",
        'export CARGO_TARGET_DIR="$expected_cargo_target_dir"',
        "prepareTauriCargoInvocation =",
        'cargo_source_root="${controlledCargoSource}"',
        "${verifyControlledCargoManifests}",
        "${assertNoRepositoryCargoConfigs}",
        "${assertNoCargoConfigAncestors}",
        '"${pkgs.diffutils}/bin/cmp" -s --',
        'export CARGO_HOME="${gateCargoHome}"',
        "${sanitizeCargoCompilerEnvironment}",
        'if [[ -v CARGO ]] && [ "$CARGO" != "${rustToolchain}/bin/cargo" ]',
        'if [[ -v RUSTC ]] && [ "$RUSTC" != "${rustToolchain}/bin/rustc" ]',
        "unset CARGO RUSTC RUSTC_WRAPPER",
        'export CARGO="${rustToolchain}/bin/cargo"',
        'export RUSTC="${rustToolchain}/bin/rustc"',
        'export RUSTC_WRAPPER="${reproducibleRustcWrapper}"',
        '"$CARGO_HOME" != "${gateCargoHome}"',
    ):
        assert tauri_boundary.count(tauri_boundary_proof) == 1, tauri_boundary_proof

    gate_home_exports_end = flake.index("canonicalizeGateHome =", tauri_boundary_end)
    gate_home_exports = flake[tauri_boundary_end:gate_home_exports_end]
    assert (
        hashlib.sha256(gate_home_exports.strip().encode()).hexdigest()
        == "036e6d75763d4bff33ffb17ef63097a0e93012ea61825093595c1f9f9369cad9"
    )
    for gate_home_export in (
        'export HOME="$gate_home"',
        'export XDG_CACHE_HOME="$gate_home/.cache"',
        'export XDG_CONFIG_HOME="$gate_home/.config"',
        'export XDG_RUNTIME_DIR="$gate_home/runtime"',
        'export XDG_STATE_HOME="$gate_home/.local/state"',
        'export TMPDIR="$gate_home/tmp"',
    ):
        assert gate_home_exports.count(gate_home_export) == 1

    cargo_config_restore_start = flake.index("restoreGateCargoConfig =")
    cargo_config_restore_end = flake.index(
        "setupIsolatedCargoHome =", cargo_config_restore_start
    )
    cargo_config_restore = flake[cargo_config_restore_start:cargo_config_restore_end]
    assert (
        hashlib.sha256(cargo_config_restore.strip().encode()).hexdigest()
        == "2b11f9f58b568b19a6aba47fc4f9e981cdc242bcd01faac2ae5e0cb23590680a"
    )
    assert (
        cargo_config_restore.count(
            'rm -rf -- "$CARGO_HOME/config" "$CARGO_HOME/config.toml"'
        )
        == 1
    )
    assert (
        cargo_config_restore.count(
            'ln -s -- "${gateCargoConfig}" "$CARGO_HOME/config.toml"'
        )
        == 1
    )

    isolated_cargo_home_start = cargo_config_restore_end
    isolated_cargo_home_end = flake.index(
        "setupPerRunCargoTarget =", isolated_cargo_home_start
    )
    isolated_cargo_home = flake[isolated_cargo_home_start:isolated_cargo_home_end]
    assert (
        hashlib.sha256(isolated_cargo_home.strip().encode()).hexdigest()
        == "77428525eca70ee1d84fa7bf25c43024c0e08cfd25dd4776bc7a55c4de0461ce"
    )
    assert isolated_cargo_home.count('export CARGO_HOME="$gate_home/cargo-home"') == 1
    assert isolated_cargo_home.count("${restoreGateCargoConfig}") == 1

    per_run_cargo_target_start = isolated_cargo_home_end
    per_run_cargo_target_end = flake.index(
        "setupCallerRustTaskEnvironment =", per_run_cargo_target_start
    )
    per_run_cargo_target = flake[per_run_cargo_target_start:per_run_cargo_target_end]
    assert (
        hashlib.sha256(per_run_cargo_target.strip().encode()).hexdigest()
        == "417d5ec33d4e8609f8863de1baa66f49b151e9297f065c29b0ff289639c20af7"
    )
    assert len(CARGO_CACHE_DIRECTORY_TAG.encode()) == 177
    indented_cargo_cache_tag = "".join(
        f"            {line}"
        for line in CARGO_CACHE_DIRECTORY_TAG.splitlines(keepends=True)
    )
    cargo_cache_tag_definition = (
        "          cargoCacheDirectoryTag = pkgs.writeText "
        "\"pokecon-cargo-cache-directory-tag\" ''\n"
        f"{indented_cargo_cache_tag}"
        "          '';\n"
    )
    assert flake.count(cargo_cache_tag_definition) == 1
    assert flake.count("cargoCacheDirectoryTag =") == 1
    assert flake.count("${cargoCacheDirectoryTag}") == 2
    assert (
        hashlib.sha256(CARGO_CACHE_DIRECTORY_TAG.encode()).hexdigest()
        == "6d9d1d216e0f83abc5e5662ca62c92b4f23009466b54fa27321a69acdb778bb2"
    )
    cargo_cache_tag_placement = (
        "            ${setupIsolatedCargoHome}\n"
        "            ${setupPerRunCargoTarget}\n"
        "            ${setupUvLinks}\n"
        "          '';\n\n"
        f"{cargo_cache_tag_definition}"
        "\n"
        "          setupWorkdir = ''"
    )
    assert flake.count(cargo_cache_tag_placement) == 1
    cargo_cache_tag_definition_start = flake.index(cargo_cache_tag_definition)
    setup_workdir_start = flake.index(
        "setupWorkdir =", cargo_cache_tag_definition_start
    )
    assert (
        per_run_cargo_target_end
        < cargo_cache_tag_definition_start
        < setup_workdir_start
    )

    target_mkdir = 'mkdir -- "$cargo_target_dir"'
    target_containment = '"$canonical_gate_home"/*)'
    cargo_cache_tag_path = 'cargo_cache_tag="$cargo_target_dir/CACHEDIR.TAG"'
    cargo_cache_tag_creation = (
        'if ! (set -o noclobber; umask 022; "${pkgs.coreutils}/bin/cat" '
        '"${cargoCacheDirectoryTag}" >"$cargo_cache_tag"); then'
    )
    cargo_cache_tag_identity_guard = (
        'if [ -L "$cargo_cache_tag" ] || [ ! -f "$cargo_cache_tag" ] \\'
    )
    cargo_cache_tag_canonical_guard = (
        '|| [ "$(readlink -f "$cargo_cache_tag")" != "$cargo_cache_tag" ]; then'
    )
    cargo_cache_tag_comparison = (
        'if ! "${pkgs.diffutils}/bin/cmp" -s -- "${cargoCacheDirectoryTag}" '
        '"$cargo_cache_tag"; then'
    )
    incremental_export = "export CARGO_INCREMENTAL=0"
    target_export = 'export CARGO_TARGET_DIR="$cargo_target_dir"'
    cargo_cache_tag_cleanup = (
        "unset canonical_gate_home cargo_cache_tag cargo_target_dir"
    )
    for cargo_target_proof in (
        'cargo_target_dir="$gate_home/target"',
        'if [ -e "$cargo_target_dir" ] || [ -L "$cargo_target_dir" ]',
        'case "$(readlink -f "$cargo_target_dir")" in',
        '"$canonical_gate_home"/*)',
        cargo_cache_tag_path,
        cargo_cache_tag_creation,
        'echo "failed to create the canonical Cargo cache directory tag: $cargo_cache_tag" >&2',
        cargo_cache_tag_identity_guard,
        cargo_cache_tag_canonical_guard,
        'echo "per-run Cargo cache directory tag is redirected or not a regular file: $cargo_cache_tag" >&2',
        cargo_cache_tag_comparison,
        'echo "per-run Cargo cache directory tag differs from Cargo\'s canonical tag: $cargo_cache_tag" >&2',
        incremental_export,
        target_export,
        cargo_cache_tag_cleanup,
    ):
        assert per_run_cargo_target.count(cargo_target_proof) == 1
    cargo_cache_tag_section = per_run_cargo_target[
        per_run_cargo_target.index(cargo_cache_tag_path) : per_run_cargo_target.index(
            incremental_export
        )
    ]
    assert tuple(
        line.strip() for line in cargo_cache_tag_section.splitlines() if line.strip()
    ) == (
        cargo_cache_tag_path,
        cargo_cache_tag_creation,
        'echo "failed to create the canonical Cargo cache directory tag: $cargo_cache_tag" >&2',
        "exit 2",
        "fi",
        cargo_cache_tag_identity_guard,
        cargo_cache_tag_canonical_guard,
        'echo "per-run Cargo cache directory tag is redirected or not a regular file: $cargo_cache_tag" >&2',
        "exit 2",
        "fi",
        cargo_cache_tag_comparison,
        'echo "per-run Cargo cache directory tag differs from Cargo\'s canonical tag: $cargo_cache_tag" >&2',
        "exit 2",
        "fi",
    )
    assert (
        per_run_cargo_target.index(target_mkdir)
        < per_run_cargo_target.index(target_containment)
        < per_run_cargo_target.index(cargo_cache_tag_path)
        < per_run_cargo_target.index(cargo_cache_tag_creation)
        < per_run_cargo_target.index(cargo_cache_tag_identity_guard)
        < per_run_cargo_target.index(cargo_cache_tag_canonical_guard)
        < per_run_cargo_target.index(cargo_cache_tag_comparison)
        < per_run_cargo_target.index(incremental_export)
        < per_run_cargo_target.index(target_export)
        < per_run_cargo_target.index(cargo_cache_tag_cleanup)
    )
    assert (
        flake.count(
            "            ${setupPerRunCargoTarget}\n            ${setupUvLinks}"
        )
        == 2
    )

    setup_workdir_end = flake.index("linuxDesktopPackages =", setup_workdir_start)
    setup_workdir = flake[setup_workdir_start:setup_workdir_end]
    assert (
        hashlib.sha256(setup_workdir.strip().encode()).hexdigest()
        == "24eda15cba1fe91db390a1b52049e19b73ae1e11d5fb7cd222669f2eb13efd76"
    )
    assert setup_workdir.count("${assertNoCargoConfigAncestors}") == 1
    for composed_cleanup_proof in (
        "gate_cleanup_status=0",
        "if declare -F pokecon_cleanup_task_artifacts >/dev/null; then",
        "pokecon_cleanup_task_artifacts || gate_cleanup_status=$?",
        'if [ "$gate_status" -eq 0 ] && [ "$gate_cleanup_status" -ne 0 ]; then',
        "gate_status=$gate_cleanup_status",
    ):
        assert setup_workdir.count(composed_cleanup_proof) == 1
    assert "pokecon_cleanup_task_artifacts || true" not in setup_workdir
    assert (
        '            cd "$workdir"\n'
        "            ${assertNoCargoConfigAncestors}\n"
        "          '';"
    ) in setup_workdir

    web_package_start = flake.index("webPackage = pkgs.stdenvNoCC.mkDerivation")
    runtime_package_start = flake.index("linuxReleaseRuntime =", web_package_start)
    web_package_section = flake[web_package_start:runtime_package_start]
    assert (
        hashlib.sha256(web_package_section.strip().encode()).hexdigest()
        == "a7339b708c39a8fe546cdb917916d6cbd03085026e63908cda1755651c1505a8"
    )
    assert web_package_section.count("POKECON_WEB_VERSION = workspaceVersion;") == 1
    assert web_package_section.count('SOURCE_DATE_EPOCH = "0";') == 1

    runtime_package_end = flake.index(
        "pokeconPackage = rustPlatform.buildRustPackage", runtime_package_start
    )
    runtime_package_section = flake[runtime_package_start:runtime_package_end]
    assert (
        hashlib.sha256(runtime_package_section.strip().encode()).hexdigest()
        == "5bbbb6dfb4067d4c605cc1869f71d16ffb890d67e66459f993cdbb13f9e70d20"
    )
    runtime_output_hash = (
        'outputHash = "sha256-/oX5m7mZkIwXl383NXN4qc2qGnfsJSlPMgVKMrurSmY=";'
    )
    assert runtime_package_section.count(runtime_output_hash) == 1
    assert flake.count(runtime_output_hash) == 1
    assert "lib.fakeHash" not in flake
    assert (
        len(
            re.findall(
                r"(?m)^[ \t]*else[ \t\r\n]+null;",
                runtime_package_section,
            )
        )
        == 1
    )
    for runtime_package_proof in (
        'if system == "x86_64-linux" then',
        '"${pythonEnv}/bin/python" -I "${source}/scripts/release/build_runtime.py"',
        '--project "${controlledCargoSource}"',
        '--uv "${portableUvExecutionBinary}"',
        '--execution-loader "${portableUvExecutionLoader}"',
        '--execution-library-path "${portableUvExecutionLibraryPath}"',
        "PYTHONDONTWRITEBYTECODE=1",
        "PYTHONNOUSERSITE=1",
        "PYTHONSAFEPATH=1",
        "PIP_CONFIG_FILE=/dev/null",
        'PKG_CONFIG_LIBDIR="${linuxReleasePortaudio}/lib/pkgconfig"',
        '--runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"',
        "UV_LIBC=gnu",
        "UV_NO_CONFIG=1",
        "fixed release runtime contains Python bytecode cache artifacts",
        "dontFixup = true;",
        'outputHashMode = "recursive";',
    ):
        assert runtime_package_section.count(runtime_package_proof) == 1, (
            runtime_package_proof
        )
    assert "PYTHONPATH=" not in runtime_package_section
    assert '--uv "${portableUvBinary}"' not in runtime_package_section
    assert runtime_package_section.count("linuxReleaseRuntimeLibraries") == 1
    assert "LD_LIBRARY_PATH" not in runtime_package_section
    for baked_runtime_library_path in (
        'CFLAGS="-I${linuxReleaseRuntimeLibraries}',
        'LDFLAGS="-L${linuxReleaseRuntimeLibraries}',
        'NIX_LDFLAGS="${linuxReleaseRuntimeLibraries}',
        '--set-rpath "${linuxReleaseRuntimeLibraries}',
    ):
        assert baked_runtime_library_path not in runtime_package_section
    runtime_environment_start = runtime_package_section.index(
        '"${pkgs.coreutils}/bin/env" -i \\\n'
    )
    runtime_builder_start = runtime_package_section.index(
        '"${pythonEnv}/bin/python" -I "${source}/scripts/release/build_runtime.py"',
        runtime_environment_start,
    )
    runtime_environment_section = runtime_package_section[
        runtime_environment_start:runtime_builder_start
    ]
    linux_python_libc = linux_python_install_request_match.group("libc")
    assert (
        runtime_environment_section.count(
            f"                    UV_LIBC={linux_python_libc} \\\n"
        )
        == 1
    )
    assert runtime_environment_section.count("UV_LIBC") == 1

    package_start = flake.index("pokeconPackage = rustPlatform.buildRustPackage")
    package_end = flake.index("gateCargoLock =", package_start)
    package_section = flake[package_start:package_end]
    assert (
        hashlib.sha256(package_section.strip().encode()).hexdigest()
        == "a7707ec55b72e712dfaf7639311cedb49d171a11f567857e986e7c0d9112379a"
    )
    assert package_section.count("${installControlledCargoManifests}") == 2
    assert package_section.count('"--locked"') == 1
    assert '"--features"' not in package_section
    assert 'test -f "${productionRoutingAudit}/passed"' in package_section
    assert 'RUSTC = "${rustToolchain}/bin/rustc";' in package_section
    assert 'RUSTC_WRAPPER = "${pinnedRustcWrapper}";' in package_section
    assert "RUSTC_WORKSPACE_WRAPPER" in package_section
    assert "RUSTFLAGS" in package_section
    for retired_native_artifact in (
        "*/pokecon/_native*.so",
        "*/pokecon/_native*.pyd",
        "*/pokecon/_native*.dylib",
        "*/pokecon/_native*.dll",
        "poke_controller_modified_extension-*.whl",
    ):
        assert package_section.count(retired_native_artifact) == 1
    assert package_section.count(r"\( -type f -o -type l \)") == 2
    assert "RUSTFLAGS =" not in package_section
    assert "CARGO_ENCODED_RUSTFLAGS =" not in package_section
    package_provenance_inventory = tuple(
        line.strip()
        for line in package_section.splitlines()
        if "POKECON_RESOURCE_PROVENANCE" in line
    )
    assert package_provenance_inventory == (
        'POKECON_RESOURCE_PROVENANCE = "nix-exact";',
    )
    assert package_section.count("preInstall =") == 0
    assert package_section.count("cargoBuildHook") == 0
    assert package_section.count("cargoInstallHook") == 0
    package_install = (
        "            installPhase = ''\n"
        "              runHook preInstall\n"
        "              : \"''${cargoBuildType:?cargoBuildType is required}\"\n"
        '              package_target="target/${pkgs.stdenv.targetPlatform.rust.cargoShortTarget}/$cargoBuildType"\n'
        "              for packaged_binary in pokecon pokecon-worker; do\n"
        '                packaged_source="$package_target/$packaged_binary"\n'
        '                if [ -L "$packaged_source" ] || [ ! -f "$packaged_source" ]'
        ' || [ ! -x "$packaged_source" ]; then\n'
        '                  echo "packaged executable is missing, redirected, or not executable:'
        ' $packaged_source" >&2\n'
        "                  exit 2\n"
        "                fi\n"
        '                "${pkgs.coreutils}/bin/install" -Dm755 -- \\\n'
        '                  "$packaged_source" "$out/bin/$packaged_binary"\n'
        "              done\n"
        "              unset packaged_binary packaged_source package_target\n"
        "              runHook postInstall\n"
        "            '';\n"
    )
    assert package_section.count(package_install) == 1
    assert package_section.count("installPhase =") == 1
    assert package_section.count("runHook preInstall") == 1
    assert package_section.count("runHook postInstall") == 1
    assert (
        package_section.count("for packaged_binary in pokecon pokecon-worker; do") == 1
    )
    assert package_section.count('"${pkgs.coreutils}/bin/install" -Dm755 --') == 1
    for forbidden_release_tree_install in (
        "cargoInstallPostBuildHook",
        "release-tmp",
        "${releaseDir}",
        "target/@targetSubdirectory@",
    ):
        assert forbidden_release_tree_install not in package_section
    assert package_section.count("doCheck = false;") == 1
    assert package_section.count("cargoBuildFlags = [") == 1
    assert package_section.count("cargoTestFlags = [") == 0
    for forbidden_package_phase in (
        "buildPhase =",
        "checkPhase =",
        "dontBuild =",
        "dontCargoBuild =",
        "dontCargoCheck =",
        "dontCargoInstall =",
        "dontCheck =",
        "dontInstall =",
        "phases =",
    ):
        assert forbidden_package_phase not in package_section
    package_prebuild_start = package_section.index("preBuild =")
    package_prebuild_end = package_section.index(
        "installPhase =", package_prebuild_start
    )
    package_prebuild = package_section[package_prebuild_start:package_prebuild_end]
    assert (
        hashlib.sha256(package_prebuild.strip().encode()).hexdigest()
        == "711d19dff45a4ff4ac5ef8edd99b263a0eb7dc22704de1719d135d132ea07ade"
    )
    assert package_prebuild.count("${installControlledCargoManifests}") == 1
    assert package_prebuild.count("${sanitizeCargoCompilerEnvironment}") == 1

    gate_cargo_vendor_start = package_end
    gate_cargo_vendor_end = flake.index("gateCargoConfig =", gate_cargo_vendor_start)
    gate_cargo_vendor_section = flake[gate_cargo_vendor_start:gate_cargo_vendor_end]
    assert (
        hashlib.sha256(gate_cargo_vendor_section.strip().encode()).hexdigest()
        == "f6c72fc870b648a1603cd8e7898fb4bfd212175628adbe3e1496f5e3a6dab141"
    )
    canonical_cargo_lock = tomllib.loads(sources[WORKSPACE_LOCK_SOURCE])
    external_cargo_packages = [
        package for package in canonical_cargo_lock["package"] if "source" in package
    ]
    gate_cargo_vendor_lock = {
        "version": canonical_cargo_lock["version"],
        "package": external_cargo_packages,
    }
    if "metadata" in canonical_cargo_lock:
        gate_cargo_vendor_lock["metadata"] = canonical_cargo_lock["metadata"]
    expected_gate_cargo_vendor_identity = (
        "206ade87ceac36a1c8a0b29358ad3320eced78535da9aa96d3a97c18b22b7202"
    )
    assert (
        hashlib.sha256(
            json.dumps(
                gate_cargo_vendor_lock,
                sort_keys=True,
                separators=(",", ":"),
            ).encode()
        ).hexdigest()
        == expected_gate_cargo_vendor_identity
    )
    for gate_cargo_vendor_proof in (
        "assert workspaceCargoInputsAreCanonical;",
        "builtins.fromTOML canonicalCargoLockText;",
        "inherit (gateCargoLock) version;",
        "builtins.filter (package: package ? source)",
        "gateCargoLock.package",
        "lib.optionalAttrs (gateCargoLock ? metadata)",
        "inherit (gateCargoLock) metadata;",
        'builtins.hashString "sha256"',
        "builtins.toJSON gateCargoVendorLock",
        f'"{expected_gate_cargo_vendor_identity}";',
        'lib.splitString "\\n[[package]]\\n" canonicalCargoLockText',
        "builtins.head gateCargoLockSections",
        "builtins.tail gateCargoLockSections",
        'lib.hasInfix "\\nsource = " packageText',
        'lib.concatStringsSep "\\n[[package]]\\n"',
        "builtins.attrNames gateCargoLock",
        "builtins.length gateCargoVendorPackageTexts",
        "builtins.length gateCargoVendorLock.package",
        "builtins.fromTOML gateCargoVendorLockText == gateCargoVendorLock",
        "gateCargoVendorIdentity == expectedGateCargoVendorIdentity",
        "rustPlatform.importCargoLock {",
        "lockFileContents = gateCargoVendorLockText;",
        "allowBuiltinFetchGit = true;",
    ):
        assert gate_cargo_vendor_section.count(gate_cargo_vendor_proof) == 1

    gate_cargo_config_start = gate_cargo_vendor_end
    gate_cargo_config_end = flake.index("gateCargoHome =", gate_cargo_config_start)
    gate_cargo_config_section = flake[gate_cargo_config_start:gate_cargo_config_end]
    assert (
        hashlib.sha256(gate_cargo_config_section.strip().encode()).hexdigest()
        == "31906d147ca5c3de90d8c72090c3d6f2c377e1a9194da6deecb8d2240a823a84"
    )
    gate_cargo_config_opening = (
        "gateCargoConfig = pkgs.writeText \"pokecon-gate-cargo-config.toml\" ''\n"
    )
    gate_cargo_config_closing = "          '';"
    gate_cargo_config_trimmed = gate_cargo_config_section.rstrip()
    assert gate_cargo_config_trimmed.startswith(gate_cargo_config_opening)
    assert gate_cargo_config_trimmed.endswith(gate_cargo_config_closing)
    gate_cargo_config_body = textwrap.dedent(
        gate_cargo_config_trimmed[
            len(gate_cargo_config_opening) : -len(gate_cargo_config_closing)
        ]
    )
    expected_gate_cargo_config_body = """\
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "${gateCargoVendorDir}"

[net]
offline = true
"""
    assert gate_cargo_config_body == expected_gate_cargo_config_body
    assert tomllib.loads(gate_cargo_config_body) == {
        "source": {
            "crates-io": {"replace-with": "vendored-sources"},
            "vendored-sources": {"directory": "${gateCargoVendorDir}"},
        },
        "net": {"offline": True},
    }
    for forbidden_gate_cargo_config in (
        "[alias]",
        "[build]",
        "[target.",
        "linker",
        "runner",
        "rustc",
        "rustflags",
    ):
        assert forbidden_gate_cargo_config not in gate_cargo_config_body

    gate_cargo_home_end = flake.index("cliHelpCheck =", gate_cargo_config_end)
    gate_cargo_home_section = flake[gate_cargo_config_end:gate_cargo_home_end]
    assert (
        hashlib.sha256(gate_cargo_home_section.strip().encode()).hexdigest()
        == "41f29db56f614a425361a13daed2c93da612963dc2e8bca5b16b83785d2c0cb9"
    )
    assert (
        gate_cargo_home_section.count(
            'gateCargoHome = pkgs.runCommand "pokecon-gate-cargo-home"'
        )
        == 1
    )
    assert (
        gate_cargo_home_section.count(
            '"${pkgs.coreutils}/bin/ln" -s -- "${gateCargoConfig}" "$out/config.toml"'
        )
        == 1
    )

    test_task_start = flake.index("            test = mkTask {\n")
    mutation_test_app_start = flake.index(
        "            test-production-routing-mutations = mkTask {\n", test_task_start
    )
    test_task_section = flake[test_task_start:mutation_test_app_start]
    for test_task_proof in (
        'pytest_arguments=("$@")',
        "if [ \"''${#pytest_arguments[@]}\" -eq 0 ]; then",
        "pytest_arguments=(tests)",
        '-m "not production_routing_mutation"',
        "\"''${pytest_arguments[@]}\"",
    ):
        assert test_task_section.count(test_task_proof) == 1, test_task_proof
    assert test_task_section.index("\"''${pytest_arguments[@]}\"") < (
        test_task_section.index('-m "not production_routing_mutation"')
    )
    assert "python -m pytest -p no:cacheprovider tests" not in test_task_section
    mutation_test_app_end = flake.index(
        "            tauri-build =\n", mutation_test_app_start
    )
    mutation_test_app_section = flake[mutation_test_app_start:mutation_test_app_end]
    assert mutation_test_app_section.count("mkTask {") == 1
    assert (
        mutation_test_app_section.count('name = "test-production-routing-mutations";')
        == 1
    )
    assert (
        mutation_test_app_section.count(
            'exec "${productionRoutingMutationAuditRunner}/bin/'
            'pokecon-production-routing-mutation-audit" "$@"'
        )
        == 1
    )

    development_command_section_boundaries = (
        (
            "cargo",
            "            cargo = mkTask {\n",
            "            web-dev = mkTask {\n",
        ),
        (
            "rust-ci-core",
            "            rust-ci-core = mkTask {\n",
            "            clippy = mkTask {\n",
        ),
        (
            "clippy",
            "            clippy = mkTask {\n",
            "            build-rust = mkTask {\n",
        ),
        (
            "build-rust",
            "            build-rust = mkTask {\n",
            "            cargo-test = mkTask {\n",
        ),
        (
            "cargo-test",
            "            cargo-test = mkTask {\n",
            "            virtual-io-check = mkTask {\n",
        ),
        (
            "check",
            "            check = mkTask {\n",
            "          pre-commit = {\n",
        ),
        (
            "tauri-check",
            "            tauri-check = mkTask {\n",
            "            tauri = mkTask {\n",
        ),
    )
    development_command_sections: dict[str, str] = {}
    for (
        section_name,
        start_marker,
        end_marker,
    ) in development_command_section_boundaries:
        assert flake.count(start_marker) == 1, section_name
        assert flake.count(end_marker) == 1, section_name
        section_start = flake.index(start_marker)
        section_end = flake.index(end_marker)
        assert section_start < section_end, section_name
        development_command_sections[section_name] = flake[section_start:section_end]
    development_command_sections_text = "".join(
        f"[{section_name}]\n{development_command_sections[section_name]}"
        for section_name, _, _ in development_command_section_boundaries
    )
    pre_commit_clippy_entry = (
        'entry = "nix run .#cargo -- clippy --locked --workspace '
        '--all-targets --all-features -- -D warnings";'
    )
    pre_commit_clippy_files = (
        r'files = "(^|/)(Cargo\\.toml|Cargo\\.lock|flake\\.nix|flake\\.lock|'
        r'rust-toolchain\\.toml)$|\\.rs$";'
    )
    assert flake.count(pre_commit_clippy_entry) == 1
    assert flake.count(pre_commit_clippy_files) == 1
    assert 'entry = "nix run .#clippy";' not in flake
    cargo_command_section = development_command_sections["cargo"]
    for cached_cargo_proof in (
        "${setupInteractiveCargoEnvironment}",
        "${acquireCargoTaskLock}",
        "if [ \"''${1:-}\" = update ]; then",
        "export CARGO_NET_OFFLINE=false",
        'export CARGO_HOME="${gateCargoHome}"',
        "export CARGO_NET_OFFLINE=true",
        '"$(readlink -f "$CARGO_HOME/config.toml")" != "${gateCargoConfig}"',
    ):
        assert cargo_command_section.count(cached_cargo_proof) == 1
    assert (
        hashlib.sha256(development_command_sections_text.encode()).hexdigest()
        == "cd113f31e69a87ec9a30bd7f7e5ad708933a27707dc9dd9149c185b31ff5b1ed"
    )
    development_provenance_assignment = "POKECON_RESOURCE_PROVENANCE=development"
    assert (
        development_command_sections_text.count(development_provenance_assignment)
        == len(development_command_section_boundaries) + 6
    )
    assert (
        development_command_sections_text.count("POKECON_RESOURCE_PROVENANCE")
        == len(development_command_section_boundaries) + 6
    )
    assert flake.count(development_provenance_assignment) == 20
    assert flake.count("POKECON_RESOURCE_PROVENANCE") == 24
    compatibility_cargo_build = (
        "cargo build --locked --jobs 1 --package pokecon "
        "--bin pokecon-worker --bin pokecon-compatibility"
    )
    flake_lines = flake.splitlines()
    compatibility_cargo_build_indices = tuple(
        line_index
        for line_index, line in enumerate(flake_lines)
        if compatibility_cargo_build in line
    )
    assert len(compatibility_cargo_build_indices) == 2
    for line_index in compatibility_cargo_build_indices:
        assert flake_lines[line_index - 1].strip() == (
            f"{development_provenance_assignment} " + "\\"
        )
    expected_development_cargo_invocations: dict[str, tuple[str, ...]] = {
        "cargo": (
            'POKECON_RESOURCE_PROVENANCE=development "${rustToolchain}/bin/cargo" "$@"',
        ),
        "rust-ci-core": (
            "POKECON_RESOURCE_PROVENANCE=development cargo clippy --locked "
            "--workspace --all-targets --all-features -- -D warnings",
            "cargo build --locked --workspace --all-features",
            "POKECON_RESOURCE_PROVENANCE=development cargo test --locked "
            "--workspace --all-features",
        ),
        "clippy": (
            "POKECON_RESOURCE_PROVENANCE=development cargo clippy --locked "
            "--workspace --all-targets --all-features -- -D warnings",
        ),
        "build-rust": ("cargo build --locked --workspace --all-features",),
        "cargo-test": (
            "POKECON_RESOURCE_PROVENANCE=development cargo test --locked "
            "--workspace --all-features",
        ),
        "check": (
            "POKECON_RESOURCE_PROVENANCE=development cargo test --locked "
            "--workspace --all-features",
            "cargo build --locked --workspace --all-features --jobs 1",
            "POKECON_RESOURCE_PROVENANCE=development cargo clippy --locked "
            "--workspace --all-targets --all-features -- -D warnings",
        ),
        "tauri-check": (
            "POKECON_RESOURCE_PROVENANCE=development cargo tauri build "
            "--debug --no-bundle --ci -- --locked",
        ),
    }
    assert set(development_command_sections) == set(
        expected_development_cargo_invocations
    )
    actual_development_cargo_invocations: dict[str, tuple[str, ...]] = {}
    for section_name, command_section in development_command_sections.items():
        actual_development_cargo_invocations[section_name] = tuple(
            line.strip()
            for line in command_section.splitlines()
            if (
                '"${rustToolchain}/bin/cargo" "$@"' in line
                or "--all-features" in line
                or "cargo tauri build --debug --no-bundle --ci -- --locked" in line
            )
        )
    assert (
        actual_development_cargo_invocations == expected_development_cargo_invocations
    )
    for command_section_name, build_command_index in (
        ("rust-ci-core", 1),
        ("build-rust", 0),
    ):
        command_section = development_command_sections[command_section_name]
        build_commands = expected_development_cargo_invocations[command_section_name]
        build_command = build_commands[build_command_index]
        build_with_local_provenance = (
            "POKECON_RESOURCE_PROVENANCE=development \\\n                "
            + build_command
        )
        assert command_section.count(build_with_local_provenance) == 1
    rust_ci_core_commands = expected_development_cargo_invocations["rust-ci-core"]
    rust_ci_core_section = development_command_sections["rust-ci-core"]
    assert rust_ci_core_section.count("${setupWorkdir}") == 1
    assert rust_ci_core_section.count("${desktopEnvironment}") == 1
    assert rust_ci_core_section.count("export PYTHONDONTWRITEBYTECODE=1") == 1
    assert rust_ci_core_section.count('export PYTHONPATH="$PWD"') == 1
    compatibility_promotion = "python -m scripts.compatibility.promote --check"
    compatibility_runner = "python -m scripts.compatibility.runner"
    assert rust_ci_core_section.count(compatibility_promotion) == 1
    assert rust_ci_core_section.count(compatibility_runner) == 1
    for compatibility_input in (
        '--compatibility-binary "$CARGO_TARGET_DIR/debug/pokecon-compatibility"',
        '--worker "$CARGO_TARGET_DIR/debug/pokecon-worker"',
        '--site-packages "${pythonEnv}/${pkgs.python314.sitePackages}"',
    ):
        assert rust_ci_core_section.count(compatibility_input) == 1
    assert (
        rust_ci_core_section.index(rust_ci_core_commands[0])
        < rust_ci_core_section.index(rust_ci_core_commands[1])
        < rust_ci_core_section.index(rust_ci_core_commands[2])
        < rust_ci_core_section.index(compatibility_promotion)
        < rust_ci_core_section.index(compatibility_runner)
    )
    check_commands = development_command_sections["check"]
    assert check_commands.count('-m "not production_routing_mutation"') == 1
    assert (
        check_commands.count(
            '"${productionRoutingMutationAuditRunner}/bin/'
            'pokecon-production-routing-mutation-audit"'
        )
        == 1
    )
    parallel_checks_start = '                "${pythonEnv}/bin/python" -I \\\n'
    parallel_runner = '"${source}/scripts/quality/run_parallel_checks.py"'
    assert check_commands.count(parallel_checks_start) == 2
    assert check_commands.count(parallel_runner) == 2
    rust_lane = section(
        check_commands,
        "                  rust-and-contracts \\\n",
        "                  --next \\\n                  web-and-static \\\n",
    )
    web_lane = section(
        check_commands,
        "                  web-and-static \\\n",
        parallel_checks_start,
    )
    for rust_lane_command in (
        '"${pkgs.bash}/bin/bash" -euo pipefail -c',
        "cargo run --locked --package pokecon --bin generate_contracts",
        "check-jsonschema --check-metaschema generated/settings.schema.json",
        "python -m scripts.acceptance.records",
        "scripts/quality/generate-api-types.sh --check",
        "cargo test --locked --workspace --all-features",
        "cargo build --locked --workspace --all-features --jobs 1",
        "cargo clippy --locked --workspace --all-targets --all-features",
        "python -m scripts.compatibility.promote --check",
        "python -m scripts.compatibility.runner",
    ):
        assert rust_lane_command in rust_lane
    for web_lane_command in (
        '"${pkgs.bash}/bin/bash" -euo pipefail -c',
        'cp -R "${webBunDependencies}/node_modules" web/',
        "bun run --cwd web --bun lint",
        "bun run --cwd web --bun svelte-check",
        "bun run --cwd web --bun test",
        "bun run --cwd web --bun build",
        'bun --bun "${basedpyrightCli}"',
        "shellcheck scripts/*.sh scripts/*/*.sh",
        'bun --bun "${markdownlintCli}"',
        'bun --bun "${textlintCli}"',
        "typos",
    ):
        assert web_lane_command in web_lane
    assert "cargo " not in web_lane
    assert "bun run --cwd web" not in rust_lane
    pytest_and_mutation_wave = (
        '                "${pythonEnv}/bin/python" -I \\\n'
        '                  "${source}/scripts/quality/run_parallel_checks.py" \\\n'
        "                  pytest \\\n"
        "                  python -m pytest \\\n"
        "                  -p no:cacheprovider \\\n"
        '                  -m "not production_routing_mutation" \\\n'
        "                  tests \\\n"
        "                  -v \\\n"
        "                  --tb=short \\\n"
        "                  --next \\\n"
        "                  production-routing-mutation-audit \\\n"
        '                  "${productionRoutingMutationAuditRunner}/bin/'
        'pokecon-production-routing-mutation-audit" \\\n'
        '                  --workers "$aggregate_mutation_workers"\n'
    )
    assert check_commands.count(pytest_and_mutation_wave) == 1
    aggregate_mutation_worker_setup = (
        'aggregate_mutation_workers="$(nproc)"\n'
        '                if [ "$aggregate_mutation_workers" -gt 1 ]; then\n'
        '                  aggregate_mutation_workers="$((aggregate_mutation_workers - 1))"\n'
        "                fi\n"
        '                if [ "$aggregate_mutation_workers" -gt 3 ]; then\n'
        "                  aggregate_mutation_workers=3\n"
        "                fi"
    )
    assert check_commands.count(aggregate_mutation_worker_setup) == 1
    shared_linker_flags = (
        "${lib.optionalString pkgs.stdenv.isLinux \"export RUSTFLAGS='-C "
        "link-arg=-Wl,--threads=1'\"}"
    )
    dev_debug_flags = "export CARGO_PROFILE_DEV_DEBUG=line-tables-only"
    test_debug_flags = "export CARGO_PROFILE_TEST_DEBUG=line-tables-only"
    contract_generator = (
        "cargo run --locked --package pokecon --bin generate_contracts "
        "--features contract-generator -- --check"
    )
    contract_generator_with_provenance = (
        f"{development_provenance_assignment} {contract_generator}"
    )
    api_type_generation_with_provenance = (
        f"{development_provenance_assignment} "
        "scripts/quality/generate-api-types.sh --check"
    )
    targeted_contract_test = (
        "cargo test --locked --package pokecon --test contract_sync"
    )
    assert "reclaimPerRunCargoTarget" not in flake
    assert "cargo clean" not in flake
    assert check_commands.count(shared_linker_flags) == 1
    assert check_commands.count(dev_debug_flags) == 1
    assert check_commands.count(test_debug_flags) == 1
    assert check_commands.count(contract_generator_with_provenance) == 1
    assert check_commands.count(api_type_generation_with_provenance) == 1
    assert check_commands.count(targeted_contract_test) == 0
    assert flake.count(targeted_contract_test) == 1
    check_invocations = expected_development_cargo_invocations["check"]
    assert len(check_invocations) == 3
    check_test = check_invocations[0]
    check_build = check_invocations[1]
    check_clippy = check_invocations[2]
    check_build_with_local_provenance = (
        "POKECON_RESOURCE_PROVENANCE=development \\\n                    " + check_build
    )
    assert check_commands.count(check_build_with_local_provenance) == 1
    assert (
        check_commands.index(dev_debug_flags)
        < check_commands.index(test_debug_flags)
        < check_commands.index(shared_linker_flags)
        < check_commands.index(contract_generator_with_provenance)
        < check_commands.index(api_type_generation_with_provenance)
        < check_commands.index(check_test)
        < check_commands.index(check_build)
        < check_commands.index(check_clippy)
    )
    treefmt_check = (
        '${config.treefmt.build.wrapper}/bin/treefmt --ci --working-dir "$PWD"'
    )
    assert check_commands.count(treefmt_check) == 1
    assert (
        check_commands.index("python -m scripts.release.gate")
        < check_commands.index(treefmt_check)
        < check_commands.index("rust-and-contracts")
        < check_commands.index(pytest_and_mutation_wave)
    )

    editor_section = section(
        flake,
        "            editor = mkTask {\n",
        "            editor-smoke = mkTask {\n",
    )
    editor_provenance = "export POKECON_RESOURCE_PROVENANCE=development"
    assert editor_section.count(editor_provenance) == 1
    assert (
        editor_section.index("${desktopEnvironment}")
        < editor_section.index(editor_provenance)
        < editor_section.index("export RUST_ANALYZER_PATH=")
    )

    contract_check_section = section(
        flake,
        "            contract-check = mkTask {\n",
        "            generate-contracts = mkTask {\n",
    )
    assert contract_check_section.count(editor_provenance) == 1
    assert contract_check_section.index(
        editor_provenance
    ) < contract_check_section.index(
        "cargo run --locked --package pokecon --bin generate_contracts"
    )
    assert contract_check_section.index(
        editor_provenance
    ) < contract_check_section.index(
        "cargo test --locked --package pokecon --test contract_sync"
    )
    assert contract_check_section.index(
        editor_provenance
    ) < contract_check_section.index("scripts/quality/generate-api-types.sh --check")

    generate_contracts_section = section(
        flake,
        "            generate-contracts = mkTask {\n",
        "            compatibility-inventory = mkTask {\n",
    )
    assert (
        generate_contracts_section.count(
            "POKECON_RESOURCE_PROVENANCE=development cargo run --locked --package "
            'pokecon --bin generate_contracts --features contract-generator -- "$@"'
        )
        == 1
    )

    generate_api_types_section = section(
        flake,
        "            generate-api-types = mkTask {\n",
        "            check = mkTask {\n",
    )
    assert (
        generate_api_types_section.count(
            "POKECON_RESOURCE_PROVENANCE=development "
            'scripts/quality/generate-api-types.sh "$@"'
        )
        == 1
    )

    tauri_task_anchor = "\n            tauri-build =\n"
    tauri_start = flake.index(tauri_task_anchor) + len("\n            ")
    tauri_end = flake.index("package-smoke = mkTask", tauri_start)
    tauri_section = flake[tauri_start:tauri_end]
    assert (
        hashlib.sha256(tauri_section.strip().encode()).hexdigest()
        == "3dcb903b83598b96139da06a5ce937eb45192c5873c2a6406aee540aebf84669"
    )
    assert tauri_section.count("${installControlledCargoManifests}") == 0
    assert tauri_section.count("${prepareTauriCargoInvocation}") == 3
    assert tauri_section.count("${resetTauriCargoTarget}") == 1
    assert 'test -f "${productionRoutingAudit}/passed"' in tauri_section
    assert 'release_workdir="$gate_home/pokecon-release-workdir"' in tauri_section
    assert 'release_workdir="$CARGO_TARGET_DIR/' not in tauri_section
    assert tauri_section.count('"${pkgs.coreutils}/bin/env" -i') == 0
    assert tauri_section.count('if system != "x86_64-linux" then') == 1
    assert tauri_section.count("tauri-build supports only x86_64-linux") == 1
    assert tauri_section.count('release_python="${linuxReleaseRuntime}/python"') == 1
    assert (
        tauri_section.count('release_wheelhouse="${linuxReleaseRuntime}/wheelhouse"')
        == 1
    )
    assert tauri_section.count('--web "${webPackage}"') == 1
    assert 'bun" run --cwd web' not in tauri_section
    assert "build_runtime.py" not in tauri_section
    assert "pkgs.uv" not in tauri_section
    assert "PYTHONPATH=" not in tauri_section
    assert "portableUvExecutionBinary" not in tauri_section
    assert tauri_section.count('POKECON_BUILD_UV_PATH="${portableUvBinary}"') == 1
    assert tauri_section.count('--uv "${portableUvBinary}"') == 1
    package_smoke_start = tauri_end
    package_smoke_end = flake.index(
        "package-install-smoke = mkTask", package_smoke_start
    )
    package_smoke_section = flake[package_smoke_start:package_smoke_end]
    for package_smoke_library_proof in (
        "linuxReleaseRuntimeLibraries",
        "unset LD_LIBRARY_PATH",
        '--runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"',
    ):
        assert package_smoke_library_proof in package_smoke_section
    assert package_smoke_section.count("linuxReleaseRuntimeLibraries") == 2
    assert package_smoke_section.count("unset LD_LIBRARY_PATH") == 1
    assert "LD_LIBRARY_PATH=" not in package_smoke_section
    assert "linuxReleasePortaudio" not in package_smoke_section
    assert (
        flake.count('--runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"') == 2
    )
    assert '--runtime-library-path "${linuxReleasePortaudio}/lib"' not in flake
    argument_boundary_start = tauri_section.index('if [ "$#" -eq 0 ]; then')
    artifact_preflight_start = tauri_section.index(
        "${sanitizeGateEnvironment}", argument_boundary_start
    )
    setup_workdir_invocation = tauri_section.index("${setupWorkdir}")
    argument_boundary = tauri_section[argument_boundary_start:artifact_preflight_start]
    for argument_boundary_proof in (
        'elif [ "$#" -eq 2 ] && [ "$1" = --bundles ] && [ "$2" = deb ]; then',
        'echo "usage: nix run .#tauri-build -- [--bundles deb]" >&2',
        "exit 2",
        "bundle_args=(--bundles deb)",
    ):
        assert argument_boundary.count(argument_boundary_proof) == 1
    artifact_preflight = tauri_section[
        artifact_preflight_start:setup_workdir_invocation
    ]
    for artifact_preflight_proof in (
        "${discoverRustWorktree}",
        'exec {artifact_parent_fd}< "$artifact_parent"',
        'artifact_parent_anchor="/proc/self/fd/$artifact_parent_fd"',
        'artifact_parent_path_identity="$(',
        'artifact_parent_fd_identity="$(',
        "artifact_parent_anchor_identity_is_owned() {",
        "artifact_parent_identity_is_current() {",
        '"${pkgs.util-linux}/bin/flock" -x "$artifact_parent_fd"',
        'artifact_dir="$artifact_parent_anchor/tauri"',
        'artifact_backup_dir="$artifact_parent_anchor/.tauri-previous"',
        'artifact_publish_dir="$artifact_parent_anchor/.tauri-publish"',
        'artifact_legacy_lock="$artifact_parent_anchor/.tauri-build.lock"',
        "remove_saved_artifact_directory() {",
        'if [ "$artifact_cleanup_actual_identity" != "$artifact_cleanup_expected_identity" ]; then',
        '"${pkgs.coreutils}/bin/rm" -rf -- "$artifact_cleanup_target"',
        "pokecon_cleanup_task_artifacts() {",
        '"$artifact_committed_identity"',
        '"$artifact_publish_identity"',
        '"$artifact_stale_canonical_identity"',
        "fail_closed_tauri_artifact_preflight() {",
        "pokecon_cleanup_task_artifacts || artifact_cleanup_status=$?",
        "trap fail_closed_tauri_artifact_preflight EXIT",
        "for artifact_existing_name in tauri .tauri-previous .tauri-publish; do",
        "tauri-build refuses legacy persistent lock residue",
        '"stale publication staging artifact"',
        '"${pkgs.coreutils}/bin/mv" -T --',
        "tauri-build did not atomically hide the owned stale artifact",
        "tauri-build failed to establish a clean artifact preflight boundary",
    ):
        assert artifact_preflight_proof in artifact_preflight, artifact_preflight_proof
    assert "artifact_backup_dir_expected" not in tauri_section
    for artifact_public_path_ownership in (
        'artifact_dir_expected="$artifact_parent/tauri"',
        'artifact_publish_dir_expected="$artifact_parent/.tauri-publish"',
        '!= "$artifact_dir_expected"',
        '!= "$artifact_publish_dir_expected"',
    ):
        assert tauri_section.count(artifact_public_path_ownership) == 1
    assert tauri_section.count("artifact_dir_expected") == 2
    assert tauri_section.count("artifact_publish_dir_expected") == 2
    assert "exec {artifact_parent_fd}>>" not in artifact_preflight
    assert (
        'artifact_lock="$artifact_parent/.tauri-build.lock"' not in artifact_preflight
    )
    assert 'mktemp -d --tmpdir="$artifact_parent"' not in artifact_preflight
    assert "|| true" not in artifact_preflight
    assert (
        artifact_preflight.count("if ! artifact_parent_anchor_identity_is_owned; then")
        == 1
    )
    assert artifact_preflight.count('"$artifact_dir" "$artifact_backup_dir"') == 1
    artifact_hide_adjacency = (
        '"${pkgs.coreutils}/bin/mv" -T -- \\\n'
        '                        "$artifact_dir" "$artifact_backup_dir"'
    )
    assert artifact_preflight.count(artifact_hide_adjacency) == 1
    assert artifact_preflight.index(
        'exec {artifact_parent_fd}< "$artifact_parent"'
    ) < artifact_preflight.index('"${pkgs.util-linux}/bin/flock"')
    assert artifact_preflight.index(
        '"${pkgs.util-linux}/bin/flock"'
    ) < artifact_preflight.index("trap fail_closed_tauri_artifact_preflight EXIT")
    assert artifact_preflight.index(
        "trap fail_closed_tauri_artifact_preflight EXIT"
    ) < artifact_preflight.index(artifact_hide_adjacency)
    assert "${setupWorkdir}" not in artifact_preflight
    tauri_cleanup_start = tauri_section.index("cleanup_tauri_build() {")
    tauri_cleanup_end = tauri_section.index(
        "trap cleanup_tauri_build EXIT", tauri_cleanup_start
    )
    tauri_cleanup = tauri_section[tauri_cleanup_start:tauri_cleanup_end]
    for tauri_cleanup_proof in (
        "tauri_cleanup_status=0",
        "if ! restore_release_application; then",
        'if [ "$gate_status" -eq 0 ] && [ "$tauri_cleanup_status" -ne 0 ]; then',
        "&& ! artifact_parent_identity_is_current; then",
        "tauri-build artifact parent changed before successful cleanup finalization",
        'if [ "$gate_status" -ne 0 ]; then',
        "pokecon_cleanup_task_artifacts || tauri_cleanup_status=1",
        "artifact_replaced=0",
        "artifact_committed_identity=",
    ):
        assert tauri_cleanup.count(tauri_cleanup_proof) == 1
    assert tauri_cleanup.index(
        '"${pkgs.coreutils}/bin/rm" -rf -- "$gate_home"'
    ) < tauri_cleanup.index("&& ! artifact_parent_identity_is_current; then")
    assert tauri_cleanup.index(
        "&& ! artifact_parent_identity_is_current; then"
    ) < tauri_cleanup.index('if [ "$gate_status" -ne 0 ]; then')
    assert tauri_cleanup.index(
        'if [ "$gate_status" -ne 0 ]; then'
    ) < tauri_cleanup.index("pokecon_cleanup_task_artifacts")
    assert tauri_cleanup.index("pokecon_cleanup_task_artifacts") < tauri_cleanup.index(
        "artifact_replaced=0"
    )
    assert "appimage" not in tauri_section.casefold()
    assert "rpm" not in tauri_section.casefold()
    assert 'bundle_args=("$@")' not in tauri_section
    assert tauri_section.count('cd "${cargoInvocationRoot}"') == 2
    assert tauri_section.count("${assertNoCargoConfigAncestors}") == 2
    assert tauri_section.count('--manifest-path "$cargo_source_root/Cargo.toml"') == 2
    worker_cargo_build = (
        "${prepareTauriCargoInvocation}\n"
        "                    (\n"
        '                      cd "${cargoInvocationRoot}"\n'
        "                      ${assertNoCargoConfigAncestors}\n"
        "                      POKECON_RESOURCE_PROVENANCE=development \\\n"
        '                        "${rustToolchain}/bin/cargo" build \\\n'
        '                        --manifest-path "$cargo_source_root/Cargo.toml" \\\n'
        "                        --locked \\\n"
        "                        --release \\\n"
        "                        --package pokecon \\\n"
        "                        --bin pokecon-worker\n"
        "                    )"
    )
    application_cargo_build = (
        "${prepareTauriCargoInvocation}\n"
        "                    (\n"
        '                      cd "${cargoInvocationRoot}"\n'
        "                      ${assertNoCargoConfigAncestors}\n"
        '                      export POKECON_RESOURCE_PROVENANCE="packaged:$packaged_resource_digest"\n'
        '                      "${rustToolchain}/bin/cargo" build \\\n'
        '                        --manifest-path "$cargo_source_root/Cargo.toml" \\\n'
        "                        --locked \\\n"
        "                        --release \\\n"
        "                        --package pokecon \\\n"
        "                        --bin pokecon\n"
        "                    )"
    )
    assert tauri_section.count(worker_cargo_build) == 1
    assert tauri_section.count(application_cargo_build) == 1
    cargo_bundle_adjacency = (
        "${prepareTauriCargoInvocation}\n"
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"\n'
        '                      "${pkgs.cargo-tauri}/bin/cargo-tauri" tauri bundle \\\n'
    )
    assert tauri_section.count(cargo_bundle_adjacency) == 1

    resource_provenance_clear = "unset POKECON_RESOURCE_PROVENANCE"
    worker_provenance = "POKECON_RESOURCE_PROVENANCE=development"
    worker_normalization = (
        '"${pythonEnv}/bin/python" -I "${source}/scripts/release/normalize_linux_elf.py" \\\n'
        '                      --worker "$normalized_worker" \\\n'
        '                      --python-root "$release_python" \\\n'
        '                      --patchelf "${pkgs.patchelf}/bin/patchelf" \\\n'
        '                      --strip "${pkgs.binutils}/bin/strip" \\\n'
        '                      --objdump "${pkgs.binutils}/bin/objdump" \\\n'
        '                      --ephemeral-build-root "$gate_home"'
    )
    stage_capture = 'if ! stage_report_json="$('
    stage_command = (
        '"${pythonEnv}/bin/python" -I "${source}/scripts/release/stage.py" \\\n'
        '                        --web "${webPackage}" \\\n'
        '                        --worker "$normalized_worker" \\\n'
        '                        --uv "${portableUvBinary}" \\\n'
        '                        --wheelhouse "$release_wheelhouse" \\\n'
        '                        --python "$release_python" \\\n'
        '                        --output "$bundle_root" \\\n'
        '                        --config-output "$bundle_config"'
    )
    stage_report_parse = 'if ! packaged_resource_digest="$('
    provenance_export = (
        'export POKECON_RESOURCE_PROVENANCE="packaged:$packaged_resource_digest"'
    )
    application_build = (
        '"${rustToolchain}/bin/cargo" build \\\n'
        '                        --manifest-path "$cargo_source_root/Cargo.toml" \\\n'
        "                        --locked \\\n"
        "                        --release \\\n"
        "                        --package pokecon \\\n"
        "                        --bin pokecon\n"
        "                    )"
    )
    application_normalization = (
        '"${pythonEnv}/bin/python" -I "${source}/scripts/release/normalize_linux_elf.py" \\\n'
        '                      --application "$normalized_application" \\\n'
        '                      --patchelf "${pkgs.patchelf}/bin/patchelf" \\\n'
        '                      --strip "${pkgs.binutils}/bin/strip" \\\n'
        '                      --objdump "${pkgs.binutils}/bin/objdump" \\\n'
        '                      --ephemeral-build-root "$gate_home"'
    )
    bundle_invocation = '"${pkgs.cargo-tauri}/bin/cargo-tauri" tauri bundle \\\n'
    assert tauri_section.count(resource_provenance_clear) == 1
    assert tauri_section.count(worker_provenance) == 1
    assert tauri_section.count("POKECON_RESOURCE_PROVENANCE") == 3
    provenance_lines = tuple(
        line.strip()
        for line in tauri_section.splitlines()
        if "POKECON_RESOURCE_PROVENANCE" in line
    )
    assert provenance_lines == (
        resource_provenance_clear,
        f"{worker_provenance} \\",
        provenance_export,
    )
    assert "$POKECON_RESOURCE_PROVENANCE" not in tauri_section
    assert "${POKECON_RESOURCE_PROVENANCE" not in tauri_section
    assert tauri_section.count("normalize_linux_elf.py") == 2
    assert tauri_section.count(worker_normalization) == 1
    assert tauri_section.count(application_normalization) == 1
    assert tauri_section.count('--python-root "$release_python"') == 1
    assert tauri_section.count('--application "$normalized_application"') == 1
    assert tauri_section.count("scripts/release/stage.py") == 1
    assert tauri_section.count(stage_capture) == 1
    assert tauri_section.count(stage_command) == 1
    assert tauri_section.count('echo "release resource staging failed" >&2') == 1
    assert tauri_section.count("stage_report_json") == 2
    assert tauri_section.count(stage_report_parse) == 1
    assert tauri_section.count("packaged_resource_digest") == 2
    assert tauri_section.count(provenance_export) == 1

    stage_report_parse_start = tauri_section.index(stage_report_parse)
    stage_report_parse_end = tauri_section.index(
        "${prepareTauriCargoInvocation}", stage_report_parse_start
    )
    stage_report_parser = tauri_section[stage_report_parse_start:stage_report_parse_end]
    for stage_report_proof in (
        '"${pythonEnv}/bin/python" -I -S -c',
        "report = json.loads(",
        "sys.argv[1], object_pairs_hook=reject_duplicate_keys",
        "if type(report) is not dict:",
        'if set(report) != {"content_sha256", "file_count", "platform"}:',
        'if type(platform) is not str or platform != "unix":',
        're.fullmatch(r"[0-9a-f]{64}", content_sha256) is None',
        "if type(file_count) is not int or file_count <= 0:",
        "print(content_sha256)",
        '"release stage report is not valid strict JSON"',
        '"release stage report must be exactly one JSON object"',
        '"release stage report has unexpected keys"',
        '"release stage report platform must be exactly unix"',
        '"release stage report content_sha256 must be exactly 64 lowercase hexadecimal characters"',
        '"release stage report file_count must be a positive integer"',
        'echo "release stage report validation failed" >&2',
    ):
        assert stage_report_parser.count(stage_report_proof) == 1, stage_report_proof
    assert stage_report_parser.count("print(") == 1
    assert "jq" not in stage_report_parser
    assert " python " not in stage_report_parser

    causal_statements = (
        resource_provenance_clear,
        worker_cargo_build,
        worker_normalization,
        stage_capture,
        stage_report_parse,
        provenance_export,
        application_build,
        application_normalization,
        bundle_invocation,
    )
    causal_positions = tuple(
        tauri_section.index(statement) for statement in causal_statements
    )
    assert causal_positions == tuple(sorted(causal_positions))

    bundle_boundary_start = tauri_section.index('if ! bundle_config_json="$(')
    tauri_compile_start = tauri_section.index(
        'export POKECON_RUST_REMAP_SOURCE="${controlledCargoSource}"'
    )
    tauri_compile_end = bundle_boundary_start
    tauri_compile_boundary = tauri_section[tauri_compile_start:tauri_compile_end]
    assert (
        hashlib.sha256(tauri_compile_boundary.strip().encode()).hexdigest()
        == "b0e77a4383bd3bc65829c610bdb0598dd558a3e9a465da1906a7de5c0bcb43da"
    )
    bundle_boundary_end = tauri_section.index(
        'while IFS= read -r -d "" package; do', bundle_boundary_start
    )
    bundle_boundary = tauri_section[bundle_boundary_start:bundle_boundary_end]
    assert (
        hashlib.sha256(bundle_boundary.strip().encode()).hexdigest()
        == "0ab9c994b15d97ca6d85d84e53ba13411f816aa30793c57545e6ea144c4cdab2"
    )
    for bundle_boundary_proof in (
        '"generated Tauri bundle config contains unexpected settings"',
        '"build": {"frontendDist": str(frontend_root)}',
        "canonical Tauri frontend distribution is incomplete",
        '--config "$bundle_config_json"',
        'cd "$cargo_source_root/rust/pokecon"',
    ):
        assert bundle_boundary.count(bundle_boundary_proof) == 1
    assert tauri_section.count('"$CARGO_TARGET_DIR/release/bundle"') == 1
    artifact_publication_start = tauri_section.index(
        "validate_private_tauri_inventory() {"
    )
    artifact_publication_end_marker = "                  '';\n"
    artifact_publication_end = tauri_section.index(
        artifact_publication_end_marker, artifact_publication_start
    ) + len(artifact_publication_end_marker)
    artifact_publication = tauri_section[
        artifact_publication_start:artifact_publication_end
    ]
    for artifact_publication_proof in (
        "validate_private_tauri_inventory() {",
        '"$gate_home"/*) ;;',
        '"${pkgs.coreutils}/bin/stat" -c %a -- "$inventory_path"',
        "remove_private_tauri_inventory() {",
        "pokecon-tauri-bundle-debs.XXXXXXXX.nul",
        "pokecon-tauri-publish.XXXXXXXX.nul",
        "pokecon-tauri-published.XXXXXXXX.nul",
        "if [ \"''${#bundle_deb_entries[@]}\" -ne 1 ]; then",
        'if [ -L "$package" ] || [ ! -f "$package" ]; then',
        '"${pkgs.coreutils}/bin/mkdir" -- "$artifact_publish_dir"',
        'artifact_publish_identity="$(',
        "if [ \"''${#artifact_publish_entries[@]}\" -ne 1 ]",
        "artifact_committed_identity=$artifact_publish_identity",
        "artifact_replaced=1",
        '"${pkgs.coreutils}/bin/mv" -T --',
        '"$artifact_publish_dir" "$artifact_dir"',
        '!= "$artifact_committed_identity"',
        "|| [ \"''${#artifact_published_entries[@]}\" -ne 1 ]",
        '"retained previous artifact"',
        "tauri-build success left transient artifact state in dist",
    ):
        assert artifact_publication_proof in artifact_publication, (
            artifact_publication_proof
        )
    assert artifact_publication.count('if ! "${pkgs.findutils}/bin/find" -P') == 3
    for inventory_name in (
        "bundle_deb_inventory",
        "artifact_publish_inventory",
        "artifact_published_inventory",
    ):
        assert artifact_publication.count(f'> "${inventory_name}"') == 1
        assert artifact_publication.count(f'done < "${inventory_name}"') == 1
        assert artifact_publication.count(f'"${inventory_name}" "Tauri ') >= 1
    assert "< <(" not in artifact_publication
    assert '--tmpdir="$artifact_parent"' not in artifact_publication
    assert ".tauri-publish.XXXXXXXX" not in artifact_publication
    assert "                    artifact_replaced=0\n" not in artifact_publication
    assert (
        "                    artifact_committed_identity=\n" not in artifact_publication
    )
    assert artifact_publication.count('"$artifact_publish_dir" "$artifact_dir"') == 1
    assert artifact_publication.index(
        "artifact_committed_identity=$artifact_publish_identity"
    ) < artifact_publication.index("artifact_replaced=1")
    assert artifact_publication.index(
        "artifact_replaced=1"
    ) < artifact_publication.index('"$artifact_publish_dir" "$artifact_dir"')
    assert artifact_publication.index(
        '"$artifact_publish_dir" "$artifact_dir"'
    ) < artifact_publication.rindex("                    artifact_publish_identity=\n")
    assert '"$artifact_backup_dir" "$artifact_dir"' not in tauri_section


def module_source_items(
    sources: dict[str, str], module: str
) -> tuple[tuple[str, str], ...]:
    prefix = f"{module}/"
    return tuple(
        (source_name, source)
        for source_name, source in sources.items()
        if source_name == f"{module}.rs" or source_name.startswith(prefix)
    )


def assert_forbidden_root_module_references(
    sources: dict[str, str], modules: tuple[str, ...], forbidden: frozenset[str]
) -> None:
    for module in modules:
        for source_name, source in module_source_items(sources, module):
            source_mask = rust_lexical_mask(source)
            for forbidden_module in forbidden:
                direct_reference = re.compile(
                    rf"\b(?:crate|pokecon)\s*::\s*{re.escape(forbidden_module)}\b"
                )
                grouped_reference = re.compile(
                    rf"\b(?:crate|pokecon)\s*::\s*\{{[^;]*"
                    rf"\b{re.escape(forbidden_module)}\b",
                    re.DOTALL,
                )
                parent_reference = re.compile(
                    rf"\b(?:super\s*::\s*)+{re.escape(forbidden_module)}\b"
                )
                assert direct_reference.search(source_mask) is None, (
                    source_name,
                    forbidden_module,
                )
                assert grouped_reference.search(source_mask) is None, (
                    source_name,
                    forbidden_module,
                )
                assert parent_reference.search(source_mask) is None, (
                    source_name,
                    forbidden_module,
                )


def assert_internal_module_dependency_and_ownership_boundaries(
    sources: dict[str, str],
) -> None:
    runtime_modules = frozenset(
        {
            "application_backend",
            "camera",
            "command_service",
            "desktop",
            "device",
            "diagnostics",
            "dynamic",
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
            "worker_binary",
        }
    )
    assert_forbidden_root_module_references(sources, ("contracts",), runtime_modules)
    assert_forbidden_root_module_references(
        sources, ("runtime",), frozenset({"desktop", "server"})
    )
    assert_forbidden_root_module_references(
        sources,
        (
            "camera",
            "contracts",
            "device",
            "diagnostics",
            "dynamic",
            "platform",
            "settings",
            "worker",
        ),
        frozenset({"runtime"}),
    )

    adapter_owner_tokens = frozenset(
        {
            "CameraManager",
            "CameraSession",
            "ControllerState",
            "InputArbiter",
            "NativeCameraBackend",
            "NativeSerialBackend",
            "SerialManager",
            "gilrs",
            "mlua",
            "pyo3",
            "tokio_serial",
            "v4l",
        }
    )
    for module in ("desktop", "server"):
        for source_name, source in module_source_items(sources, module):
            source_mask = rust_lexical_mask(source)
            for owner_token in adapter_owner_tokens:
                assert (
                    re.search(rf"\b{re.escape(owner_token)}\b", source_mask) is None
                ), (
                    source_name,
                    owner_token,
                )

    worker_owner_tokens = frozenset(
        {
            "ApplicationBackend",
            "CameraManager",
            "CameraSession",
            "InputArbiter",
            "NativeCameraBackend",
            "NativeSerialBackend",
            "SerialManager",
            "StateHub",
        }
    )
    for module in ("dynamic", "worker", "worker_binary"):
        for source_name, source in module_source_items(sources, module):
            source_mask = rust_lexical_mask(source)
            for owner_token in worker_owner_tokens:
                assert (
                    re.search(rf"\b{re.escape(owner_token)}\b", source_mask) is None
                ), (
                    source_name,
                    owner_token,
                )

    assert sources["server/state.rs"].startswith(
        "//! Atomic UI-visible state snapshots and revisioned change publication.\n"
    )
    application_backend = rust_lexical_mask(sources["application_backend.rs"])
    for application_owner in (
        r"\bhub\s*:\s*StateHub\b",
        r"\bcamera\s*:\s*CameraManager\b",
        r"\bserial\s*:\s*SerialManager\b",
        r"\barbiter\s*:\s*Arc\s*<\s*ParkingMutex\s*<\s*InputArbiter\s*>\s*>",
    ):
        assert re.search(application_owner, application_backend) is not None, (
            application_owner
        )


def assert_canonical_routing_wiring(sources: dict[str, str]) -> None:
    public_router_source = compact_rust(
        rust_without_comments(sources["server/router.rs"])
    )
    assert public_router_source == compact_rust(
        """
        use axum::Router;

        use crate::server::security::{RequestSecurity, secure_router};
        use crate::server::static_files::StaticFiles;

        pub fn public_router(api: Router, static_files: StaticFiles, security: RequestSecurity) -> Router {
            secure_router(api.fallback_service(static_files.router()), security)
        }
        """
    )

    manifest = tomllib.loads(sources[POKECON_MANIFEST_SOURCE])
    assert manifest["package"]["name"] == "pokecon"
    assert manifest["package"]["build"] == "build.rs"
    assert manifest["lints"] == {
        "rust": {
            "unsafe_code": "deny",
            "unsafe_op_in_unsafe_fn": "deny",
        },
        "clippy": {
            "all": {"level": "deny", "priority": -1},
            "pedantic": {"level": "deny", "priority": -1},
            "module_name_repetitions": "allow",
            "must_use_candidate": "allow",
        },
    }
    assert "autolib" not in manifest["package"]
    assert "lib" not in manifest
    assert manifest["bin"] == [
        {"name": "pokecon", "path": "src/main.rs"},
        {"name": "pokecon-worker", "path": "src/bin/worker.rs"},
        {"name": "pokecon-compatibility", "path": "src/bin/compatibility.rs"},
        {
            "name": "pokecon-worker-fault-fixture",
            "path": "tests/fixtures/fault_worker.rs",
            "test": False,
            "bench": False,
        },
        {
            "name": "generate_contracts",
            "path": "src/bin/generate_contracts.rs",
            "required-features": ["contract-generator"],
        },
        {
            "name": "generate_openapi",
            "path": "src/bin/generate_openapi.rs",
            "required-features": ["contract-generator"],
        },
    ]
    dependencies = manifest["dependencies"]
    assert isinstance(dependencies, dict)
    assert "pokecon-camera" not in dependencies
    assert "pokecon-contracts" not in dependencies
    assert "pokecon-dynamic" not in dependencies
    assert "pokecon-core" not in dependencies
    assert "pokecon-device" not in dependencies
    assert "pokecon-server" not in dependencies
    assert "pokecon-settings" not in dependencies
    assert "pokecon-worker" not in dependencies
    assert dependencies["axum"] == {"workspace": True, "features": ["ws"]}
    for server_dependency in (
        "futures-util",
        "mime_guess",
        "openh264",
        "utoipa",
        "webrtc",
    ):
        assert dependencies[server_dependency] == {"workspace": True}
    for worker_dependency in ("atomic-write-file", "mlua", "pyo3", "rmp-serde"):
        assert dependencies[worker_dependency] == {"workspace": True}
    dev_dependencies = manifest["dev-dependencies"]
    assert isinstance(dev_dependencies, dict)
    for server_test_dependency in ("tokio-tungstenite", "tower"):
        assert dev_dependencies[server_test_dependency] == {"workspace": True}
    assert (
        re.search(
            r'\bpackage\s*=\s*"pokecon-server"',
            sources[POKECON_MANIFEST_SOURCE],
        )
        is None
    )

    main_source = compact_rust(rust_without_comments(sources["main.rs"]))
    assert main_source == compact_rust(
        """
        #[tokio::main]
        async fn main() -> Result<(), pokecon::MainError> {
            pokecon::run_cli().await
        }
        """
    )
    openapi_bridge_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\bpub\s+fn\s+generate_openapi_document_json\s*\(\s*\)\s*->\s*"
        r"Result\s*<\s*String\s*,\s*Box\s*<\s*dyn\s+std\s*::\s*error\s*::\s*Error\s*>\s*>",
        attributes=(
            '#[cfg(feature = "contract-generator")]',
            "#[doc(hidden)]",
        ),
    )
    assert compact_rust(openapi_bridge_body) == compact_rust(
        "server::openapi::document_json().map_err(Into::into)"
    )
    generate_openapi_source = rust_lexical_mask(sources["bin/generate_openapi.rs"])
    assert (
        generate_openapi_source.count("pokecon::generate_openapi_document_json()") == 1
    )
    for retired_crate_identifier in (
        "pokecon_camera",
        "pokecon_contracts",
        "pokecon_core",
        "pokecon_device",
        "pokecon_dynamic",
        "pokecon_server",
        "pokecon_settings",
    ):
        assert all(
            retired_crate_identifier not in rust_lexical_mask(source)
            for source_name, source in sources.items()
            if source_name.endswith(".rs")
        )

    unsafe_sites = {
        source_name: len(re.findall(r"\bunsafe\b", rust_lexical_mask(source)))
        for source_name, source in sources.items()
        if source_name.endswith(".rs")
        and re.search(r"\bunsafe\b", rust_lexical_mask(source)) is not None
    }
    assert unsafe_sites == {"camera/shared_ring.rs": 8}
    shared_ring_source = rust_lexical_mask(sources["camera/shared_ring.rs"])
    assert shared_ring_source.count("#[allow(unsafe_code)]") == 1
    assert "#[allow(unsafe_code)]\nmod mapping {" in shared_ring_source

    server_module = sources["server/mod.rs"]
    server_module_code = rust_without_comments(server_module).lstrip()
    expected_server_module_block = """
pub mod api;
pub mod backend;
pub mod openapi;
pub mod paths;
pub mod realtime;
pub mod realtime_connection;
pub mod rest;
pub mod router;
pub mod security;
pub mod state;
pub mod static_files;
pub mod webrtc;
pub mod websocket;
""".lstrip()
    assert server_module_code.startswith(f"{expected_server_module_block}\nuse ")
    server_module_declarations = rust_top_level_matches(
        server_module,
        r"(?m)^[ \t]*(?:pub\s+)?mod\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*)\s*;$",
    )
    assert tuple(
        declaration.group(1) for declaration in server_module_declarations
    ) == (
        "api",
        "backend",
        "openapi",
        "paths",
        "realtime",
        "realtime_connection",
        "rest",
        "router",
        "security",
        "state",
        "static_files",
        "webrtc",
        "websocket",
    )

    bound_server_methods = {
        r"\bpub\s+async\s+fn\s+bind\s*\(\s*address\s*:\s*SocketAddr\s*\)"
        r"\s*->\s*io\s*::\s*Result\s*<\s*Self\s*>": (
            (),
            """
            Self::bind_with_router(address, Router::new()).await
        """,
        ),
        r"\bpub\s+async\s+fn\s+bind_with_router\s*\(\s*address\s*:\s*SocketAddr\s*,"
        r"\s*router\s*:\s*Router\s*\)\s*->\s*io\s*::\s*Result\s*<\s*Self\s*>": (
            (),
            """
            let listener = TcpListener::bind(address).await?;
            let local_addr = listener.local_addr()?;
            Ok(Self {
                listener,
                local_addr,
                router,
            })
        """,
        ),
        r"\bpub\s+fn\s+with_router\s*\(\s*mut\s+self\s*,\s*router\s*:\s*Router\s*\)"
        r"\s*->\s*Self": (
            ("#[must_use]",),
            """
            self.router = router;
            self
        """,
        ),
        r"\bpub\s+async\s+fn\s+serve\s*\(\s*self\s*,"
        r"\s*shutdown\s*:\s*CancellationToken\s*\)\s*->\s*io\s*::\s*Result\s*<\s*\(\s*\)\s*>": (
            (),
            """
            axum::serve(self.listener, self.router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        """,
        ),
    }
    for signature, (attributes, expected_body) in bound_server_methods.items():
        assert compact_rust(
            rust_impl_method_body(
                server_module,
                "BoundServer",
                signature,
                attributes=attributes,
            )
        ) == compact_rust(expected_body)

    production_source = rust_lexical_mask(sources["production.rs"])
    assert (
        len(
            rust_top_level_matches(
                sources["production.rs"],
                r"(?m)^[ \t]*use\s+crate\s*::\s*server\s*::\s*rest\s*;$",
            )
        )
        == 1
    )
    assert (
        len(
            rust_top_level_matches(
                sources["production.rs"],
                r"(?m)^[ \t]*use\s+axum\s*::\s*Router\s*;$",
            )
        )
        == 1
    )
    websocket_imports = tuple(
        match.group(1)
        for match in rust_top_level_matches(
            sources["production.rs"],
            r"(?ms)^[ \t]*use\s+crate\s*::\s*server\s*::\s*websocket\s*::"
            r"\s*\{(.*?)\}\s*;",
        )
    )
    assert len(websocket_imports) == 1
    assert compact_rust(websocket_imports[0]) == (
        "MotionJpegFeed, WebSocketBackend, WebSocketConfig, WebSocketTransport,"
    )
    assert len(re.findall(r"\brest\b", production_source)) == 2
    assert len(re.findall(r"\bWebSocketTransport\b", production_source)) == 2
    assert len(re.findall(r"\bRouter\b", production_source)) == 3

    production_build = rust_lexical_mask(
        rust_impl_method_body(
            sources["production.rs"],
            "ProductionRuntime",
            r"\bpub\s*\(\s*crate\s*\)\s+async\s+fn\s+build\s*\(",
            attributes=("#[allow(clippy::too_many_lines)]",),
        )
    )
    assert {
        identifier: len(re.findall(rf"\b{identifier}\b", production_build))
        for identifier in (
            "WebSocketConfig",
            "WebSocketTransport",
            "rest",
            "rest_backend",
            "router",
            "websocket",
            "websocket_backend",
        )
    } == {
        "WebSocketConfig": 1,
        "WebSocketTransport": 1,
        "rest": 1,
        "rest_backend": 2,
        "router": 4,
        "websocket": 3,
        "websocket_backend": 2,
    }

    production = compact_rust(production_source)
    assert (
        production.count(
            "let websocket_backend: Arc<dyn WebSocketBackend> = backend.clone();"
        )
        == 1
    )
    assert (
        production.count(
            "let websocket = WebSocketTransport::new(websocket_backend, WebSocketConfig::default())"
        )
        == 1
    )
    assert (
        production.count("let rest_backend: Arc<dyn RestBackend> = backend.clone();")
        == 1
    )
    assert (
        production.count(
            "let router = rest::router(rest_backend).merge(websocket.router());"
        )
        == 1
    )
    assert len(re.findall(r"\blet\s+(?:mut\s+)?websocket\b", production_source)) == 1
    assert len(re.findall(r"\blet\s+(?:mut\s+)?rest_backend\b", production_source)) == 1
    assert len(re.findall(r"\blet\s+(?:mut\s+)?router\b", production_source)) == 1
    assert (
        len(re.findall(r"(?m)^\s{4}router\s*:\s*Router\s*,\s*$", production_source))
        == 1
    )
    assert len(re.findall(r"(?m)^\s{12}router\s*,\s*$", production_source)) == 1
    assert len(re.findall(r"\brouter\s*:", production_source)) == 1

    application_with_literals = rust_without_comments(sources["lib.rs"])
    application_source = rust_lexical_mask(sources["lib.rs"])
    expected_application_prelude = """
mod application_backend;
#[doc(hidden)]
pub mod camera;
mod command_service;
#[doc(hidden)]
pub mod contracts;
mod desktop;
#[doc(hidden)]
pub mod device;
#[doc(hidden)]
pub mod diagnostics;
#[allow(
    dead_code,
    reason = "engine-only helpers are consumed by the worker binary's private copy"
)]
#[doc(hidden)]
pub mod dynamic;
pub(crate) use dynamic as dynamic_domain;
mod dynamic_host;
mod dynamic_runtime;
mod entrypoint;
#[doc(hidden)]
pub mod platform;
mod production;
mod profile_service;
#[doc(hidden)]
pub mod runtime;
mod script_host;
mod script_runtime;
#[allow(dead_code, reason = "retained internal server and OpenAPI contracts")]
#[allow(clippy::option_option, reason = "wire patch fields are three-state")]
mod server;
#[doc(hidden)]
pub mod settings;
mod settings_runtime;
#[doc(hidden)]
pub mod worker;

#[doc(hidden)]
pub use diagnostics::{
    APP_STARTING, APP_STOPPED, SHUTDOWN_REQUESTED, SIGNAL_HANDLER_FAILED, TracingInitError,
    init_tracing, init_tracing_to_stderr,
};
pub use entrypoint::{MainError, run_cli};
#[doc(hidden)]
pub use runtime::{
    OsSignal, RuntimeContext, ShutdownCoordinator, ShutdownReason, install_os_signal_forwarder,
};
""".lstrip()
    assert application_with_literals.lstrip().startswith(
        f"{expected_application_prelude}\nuse std::future::Future;\nuse std::io;"
    )
    application_modules = rust_top_level_matches(
        sources["lib.rs"],
        r"(?m)^[ \t]*(?:pub\s+)?mod\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*)\s*;$",
    )
    assert tuple(module.group(1) for module in application_modules) == (
        "application_backend",
        "camera",
        "command_service",
        "contracts",
        "desktop",
        "device",
        "diagnostics",
        "dynamic",
        "dynamic_host",
        "dynamic_runtime",
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
    server_module_paths = tuple(
        match.group(1)
        for match in rust_top_level_matches(
            sources["lib.rs"],
            r'(?m)^[ \t]*#\s*\[\s*path\s*=\s*"([^"]+)"\s*\]\s*'
            r"mod\s+server\s*;",
            search_view=application_with_literals,
        )
    )
    assert server_module_paths == ()
    assert "pokecon_server" not in application_source
    application_server_imports = tuple(
        compact_rust(imported)
        for imported in (
            match.group(1)
            for match in rust_top_level_matches(
                sources["lib.rs"],
                r"(?m)^[ \t]*(use\s+crate\s*::\s*server\s*::[^;]+;)$",
            )
        )
    )
    assert application_server_imports == (
        "use crate::server::BoundServer;",
        "use crate::server::router::public_router;",
        "use crate::server::security::RequestSecurity;",
        "use crate::server::static_files::{StaticFiles, StaticRootError};",
    )
    assert application_source.count("use crate::production::ProductionRuntime;") == 1
    assert len(re.findall(r"\bBoundServer\b", application_source)) == 2
    assert len(re.findall(r"\bpublic_router\b", application_source)) == 2

    entrypoint_source = sources["entrypoint.rs"]
    controlled_runner_imports = rust_top_level_matches(
        entrypoint_source,
        r"(?ms)^[ \t]*use\s+crate\s*::\s*\{([^;]+\brun_configured_controlled\b[^;]*)\}\s*;",
    )
    assert len(controlled_runner_imports) == 1
    assert compact_rust(controlled_runner_imports[0].group(1)) == (
        "AppError, AppOptions, RunControl, UiMode, run_configured_controlled"
    )

    packaged_resource_root_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+packaged_resource_root\s*\(\s*current\s*:\s*&\s*Path\s*\)\s*"
        r"->\s*Result\s*<\s*SelectedResourceRoot\s*,\s*ResourceManifestError\s*>",
    )
    assert compact_rust(packaged_resource_root_body) == compact_rust(
        """
        let provenance = compiled_resource_provenance()?;
        let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
        let origin_platform = ResourceOriginPlatform::current();
        let platform = origin_platform.resource_platform()?;
        select_resource_root_for_provenance(
            current,
            &context.package_info().name,
            origin_platform,
            provenance,
            has_exact_nix_resource_layout(current),
            platform == ResourcePlatform::Unix
                && has_exact_cargo_output_layout(current, current),
        )
        """
    )
    assert (
        compact_rust(entrypoint_source).count(
            compact_rust(
                """
            fn is_packager_owned_top_level_file(self, relative: &str) -> bool {
                match self {
                    Self::Macos => relative == "icon.icns",
                    Self::Windows => matches!(relative, "pokecon.exe" | "uninstall.exe"),
                    Self::Linux | Self::Unsupported => false,
                }
            }
            """
            )
        )
        == 1
    )
    provenance_matrix_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bconst\s+fn\s+provenance_accepts_origin\s*\(",
    )
    assert compact_rust(provenance_matrix_body) == compact_rust(
        """
        matches!(
            (provenance, origin),
            (ResourceProvenance::Development, ResourceOrigin::Cargo)
                | (ResourceProvenance::NixExact, ResourceOrigin::NixExact)
                | (ResourceProvenance::Packaged(_), ResourceOrigin::Packaged)
        )
        """
    )
    provenance_selection_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+select_resource_root_for_provenance\s*\(",
    )
    assert compact_rust(provenance_selection_body) == compact_rust(
        """
        let platform = origin_platform.resource_platform()?;
        if platform == ResourcePlatform::Unix && exact_nix_layout {
            require_resource_origin(provenance, ResourceOrigin::NixExact)?;
            return Ok(SelectedResourceRoot::borrowed(current));
        }
        if platform == ResourcePlatform::Unix && exact_cargo_layout {
            require_resource_origin(provenance, ResourceOrigin::Cargo)?;
            return Ok(SelectedResourceRoot::borrowed(current));
        }
        require_resource_origin(provenance, ResourceOrigin::Packaged)?;
        let ResourceProvenance::Packaged(required_content_sha256) = provenance else {
            return Err(ResourceManifestError::InvalidProvenance);
        };
        let candidate = packaged_resource_candidate(
            current,
            product_name,
            origin_platform,
            MissingManifestPolicy::Reject,
        )?;
        materialize_resource_manifest(&candidate, origin_platform, Some(required_content_sha256))?
            .ok_or_else(|| ResourceManifestError::Missing {
                path: candidate.join("resource-manifest.json"),
            })
        """
    )
    assert (
        packaged_resource_root_body.index("compiled_resource_provenance()?")
        < packaged_resource_root_body.index("origin_platform.resource_platform()?")
        < packaged_resource_root_body.index("select_resource_root_for_provenance(")
        < packaged_resource_root_body.index("has_exact_nix_resource_layout(current)")
        < packaged_resource_root_body.index(
            "has_exact_cargo_output_layout(current, current)"
        )
    )
    assert entrypoint_source.count('env!("POKECON_RESOURCE_PROVENANCE")') == 1
    assert "cfg!(debug_assertions)" not in entrypoint_source
    assert (
        re.search(
            r'(?:std\s*::\s*)?env\s*::\s*var(?:_os)?\s*\(\s*"POKECON_RESOURCE_PROVENANCE"',
            entrypoint_source,
        )
        is None
    )
    assert "preselected_exact_nix_resource_root" not in entrypoint_source
    assert (
        provenance_selection_body.count("SelectedResourceRoot::borrowed(current)") == 2
    )
    assert provenance_selection_body.count("require_resource_origin(") == 3
    assert provenance_selection_body.count("materialize_resource_manifest(") == 1
    assert provenance_selection_body.count("MissingManifestPolicy::Reject") == 1

    materialize_manifest_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+materialize_resource_manifest\s*\(",
    )
    verify_snapshot_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+verify_materialized_snapshot\s*\(",
    )
    inventory_resource_files_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+inventory_resource_files\s*\(",
    )
    packager_owned_regular_skip = inventory_resource_files_body.index(
        "if packager_owned {",
        inventory_resource_files_body.index("read_stable_resource_file("),
    )
    assert (
        inventory_resource_files_body.index("std::fs::symlink_metadata(&path)")
        < inventory_resource_files_body.index(
            "metadata_is_link_or_reparse_point(&metadata)"
        )
        < inventory_resource_files_body.index(
            "ensure_canonical_resource_containment(&path"
        )
        < inventory_resource_files_body.index("relative_resource_path(")
        < inventory_resource_files_body.index("let packager_owned =")
        < inventory_resource_files_body.index(
            "packager_owned && !metadata.file_type().is_file()"
        )
        < inventory_resource_files_body.index("read_stable_resource_file(")
        < packager_owned_regular_skip
        < inventory_resource_files_body.index("inventory.insert(")
        < inventory_resource_files_body.index("write_snapshot_file(")
    )
    assert (
        inventory_resource_files_body.count(
            "origin.is_packager_owned_top_level_file(&relative)"
        )
        == 1
    )
    assert compact_rust(
        """
        inventory_resource_files(
            resource_root,
            &canonical_root,
            &manifest_path,
            platform,
            Some(origin_platform),
            &expected.files,
            Some(&snapshot.root),
        )?
        """
    ) in compact_rust(materialize_manifest_body)
    assert compact_rust(
        """
        inventory_resource_files(
            snapshot_root,
            &canonical_root,
            &manifest_path,
            platform,
            None,
            &expected.files,
            None,
        )?
        """
    ) in compact_rust(verify_snapshot_body)
    assert "is_installer_owned" not in entrypoint_source
    assert materialize_manifest_body.count("require_packaged_resource_identity(") == 1
    assert verify_snapshot_body.count("require_packaged_resource_identity(") == 1
    assert (
        materialize_manifest_body.index("read_stable_resource_file(")
        < materialize_manifest_body.index("expected_resource_files(")
        < materialize_manifest_body.index("require_packaged_resource_identity(")
        < materialize_manifest_body.index("ResourceSnapshot::create()")
        < materialize_manifest_body.index("manifest.verify_stable(")
        < materialize_manifest_body.index("write_snapshot_file(")
        < materialize_manifest_body.index("verify_materialized_snapshot(")
        < materialize_manifest_body.index("seal_snapshot_directories(")
    )
    assert (
        verify_snapshot_body.index("manifest.contents != manifest_contents")
        < verify_snapshot_body.index("expected_resource_files(")
        < verify_snapshot_body.index("inventory_resource_files(")
        < verify_snapshot_body.index("ensure_resource_inventory(")
        < verify_snapshot_body.index("require_packaged_resource_identity(")
        < verify_snapshot_body.index("manifest.verify_stable(")
    )

    pre_dynamic_final_boolean_body = rust_impl_method_body(
        sources["settings/pipeline.rs"],
        "LoadedSettings",
        r"\bpub\s+fn\s+pre_dynamic_final_boolean_with_cli\s*\("
        r"\s*&\s*self\s*,\s*id\s*:\s*&\s*str\s*,?\s*\)\s*"
        r"->\s*Result\s*<\s*bool\s*,\s*PipelineError\s*>",
    )
    assert compact_rust(pre_dynamic_final_boolean_body) == compact_rust(
        """
        let setting = setting_by_id(&self.recipe.registry, id)?;
        if !pre_dynamic_final_selector(setting) {
            return Err(PipelineError::NotPreDynamicFinal(id.to_owned()));
        }
        let Some(raw) = self.recipe.parsed_cli.assignments.get(id) else {
            return self.settings.boolean(id);
        };
        let value = parse_wire_value(setting, raw, SettingSource::CommandLine)?;
        let value = normalize_value(
            setting,
            value,
            SettingSource::CommandLine,
            &self.roots,
            &self.recipe.request,
        )?;
        value
            .as_bool()
            .ok_or_else(|| PipelineError::TypedLookup(id.to_owned()))
        """
    )

    resource_root_retarget_body = rust_impl_method_body(
        sources["settings/pipeline.rs"],
        "LoadedSettings",
        r"\bpub\s+fn\s+with_resource_root\s*\(\s*mut\s+self\s*,"
        r"\s*resource_root\s*:\s*PathBuf\s*,?\s*\)\s*"
        r"->\s*Result\s*<\s*Self\s*,\s*PipelineError\s*>",
    )
    assert compact_rust(resource_root_retarget_body) == compact_rust(
        """
        self.recipe.request.resource_root = resource_root;
        for setting in &self.recipe.registry.settings {
            if !matches!(&setting.default, DefaultValue::ResourcePath { .. }) {
                continue;
            }
            let resolved = self
                .settings
                .values
                .get_mut(&setting.id)
                .ok_or_else(|| PipelineError::MissingCanonicalSetting(setting.id.clone()))?;
            if resolved.source == SettingSource::Default {
                resolved.value = default_value(
                    setting,
                    Some(&self.roots),
                    &self.recipe.request.resource_root,
                    SettingSource::Default,
                )?;
            }
        }
        validate_snapshot(&self.settings.values)?;
        Ok(self)
        """
    )

    pre_dynamic_final_selector_body = rust_top_level_function_body(
        sources["settings/pipeline.rs"],
        r"\bfn\s+pre_dynamic_final_selector\s*\(\s*setting\s*:\s*&\s*Setting\s*,?\s*\)"
        r"\s*->\s*bool",
    )
    assert compact_rust(pre_dynamic_final_selector_body) == compact_rust(
        """
        setting.mutability == Mutability::StartupOnly
            && setting.surfaces.dynamic.name.is_none()
        """
    )

    run_cli_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bpub\s+async\s+fn\s+run_cli\s*\(\s*\)\s*"
        r"->\s*Result\s*<\s*\(\s*\)\s*,\s*MainError\s*>",
    )
    assert compact_rust(run_cli_body) == compact_rust(
        """
        let request = PipelineRequest::current()?;
        let before_dynamic = SettingsPipeline::new(request.clone()).load_before_dynamic()?;
        let cli = Cli::parse_from(&before_dynamic.remaining_arguments);

        #[cfg(target_os = "linux")]
        {
            let disable_compositing =
                before_dynamic.pre_dynamic_final_boolean_with_cli("ui.desktop.disable_compositing")?;
            let already_reexecuted = std::env::var_os(COMPOSITING_REEXEC_MARKER).is_some();
            if should_reexec_for_linux_compositing(
                cli.ui,
                cli.exit_after_startup,
                disable_compositing,
                already_reexecuted,
            ) {
                drop((cli, before_dynamic, request));
                return reexec_for_linux_compositing();
            }
        }

        init_tracing("info")?;
        ScaffoldManager::new(before_dynamic.roots.clone())
            .ensure(before_dynamic.active_profile.as_str())?;

        if cli.ui == UiArgument::Desktop && !cli.exit_after_startup {
            return run_desktop(request, before_dynamic).await;
        }

        run_packaged_backend(
            request,
            before_dynamic,
            cli.ui.into(),
            cli.exit_after_startup,
            RunControl::new(ShutdownCoordinator::new()),
            None,
        )
        .await
        """
    )

    assert entrypoint_source.count("use std::os::unix::process::CommandExt;") == 1
    assert "CompositingChild" not in entrypoint_source
    compositing_reexec_predicate_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+should_reexec_for_linux_compositing\s*\(\s*ui\s*:\s*UiArgument\s*,"
        r"\s*exit_after_startup\s*:\s*bool\s*,\s*disable_compositing\s*:\s*bool\s*,"
        r"\s*already_reexecuted\s*:\s*bool\s*,?\s*\)\s*->\s*bool",
        attributes=('#[cfg(target_os = "linux")]',),
    )
    assert compact_rust(compositing_reexec_predicate_body) == compact_rust(
        """
        ui == UiArgument::Desktop
            && !exit_after_startup
            && disable_compositing
            && !already_reexecuted
        """
    )
    compositing_reexec_body = rust_top_level_function_body(
        entrypoint_source,
        r"\bfn\s+reexec_for_linux_compositing\s*\(\s*\)\s*"
        r"->\s*Result\s*<\s*\(\s*\)\s*,\s*MainError\s*>",
        attributes=('#[cfg(target_os = "linux")]',),
    )
    assert compact_rust(compositing_reexec_body) == compact_rust(
        """
        let executable = std::env::current_exe().map_err(MainError::CompositingRelaunch)?;
        let error = std::process::Command::new(executable)
            .args(std::env::args_os().skip(1))
            .env(COMPOSITING_REEXEC_MARKER, "1")
            .env("WEBKIT_DISABLE_COMPOSITING_MODE", "1")
            .exec();
        Err(MainError::CompositingRelaunch(error))
        """
    )
    assert compositing_reexec_body.count(".exec()") == 1
    for forbidden_wrapper_call in (".status()", ".spawn()", ".wait()"):
        assert forbidden_wrapper_call not in compositing_reexec_body
    reexec_predicate_call = "if should_reexec_for_linux_compositing("
    pre_exec_release = "drop((cli, before_dynamic, request));"
    reexec_return = "return reexec_for_linux_compositing();"
    assert (
        run_cli_body.index(reexec_predicate_call)
        < run_cli_body.index(pre_exec_release)
        < run_cli_body.index(reexec_return)
    )

    run_backend_body = rust_top_level_function_body(
        entrypoint_source,
        r"\basync\s+fn\s+run_backend\s*\(\s*request\s*:\s*PipelineRequest\s*,"
        r"\s*before_dynamic\s*:\s*LoadedSettings\s*,\s*ui_mode\s*:\s*UiMode\s*,"
        r"\s*exit_after_startup\s*:\s*bool\s*,\s*control\s*:\s*RunControl\s*,"
        r"\s*desktop_settings\s*:\s*Option\s*<\s*DesktopRuntimeSettings\s*>\s*,?"
        r"\s*\)\s*->\s*Result\s*<\s*\(\s*\)\s*,\s*MainError\s*>",
    )
    assert compact_rust(run_backend_body) == compact_rust(
        """
        let bootstrap = bootstrap_dynamic(request.clone(), before_dynamic).await?;
        if let Some(error) = bootstrap.startup_failure.as_ref() {
            tracing::error!(
                error = %error,
                "dynamic configuration is unavailable; continuing with static settings"
            );
        }
        let loaded = bootstrap.loaded;
        if let Some(settings) = desktop_settings {
            settings.set_close_behavior(
                loaded
                    .settings
                    .string("ui.desktop.close_behavior")?
                    .parse::<CloseBehavior>()?,
            );
        }
        let bind_address = loaded
            .settings
            .string("server.bind_address")?
            .parse::<IpAddr>()?;
        let port = u16::try_from(loaded.settings.integer("server.port")?)?;
        let web_root = PathBuf::from(loaded.settings.string("server.web_dir")?);
        run_configured_controlled(
            AppOptions {
                listen_address: SocketAddr::new(bind_address, port),
                ui_mode,
                web_root,
                exit_after_startup,
            },
            request,
            loaded,
            bootstrap.host,
            bootstrap.runtime,
            control,
        )
        .await?;
        Ok(())
        """
    )

    run_packaged_backend_body = rust_top_level_function_body(
        entrypoint_source,
        r"\basync\s+fn\s+run_packaged_backend\s*\(\s*mut\s+request\s*:\s*PipelineRequest\s*,"
        r"\s*before_dynamic\s*:\s*LoadedSettings\s*,\s*ui_mode\s*:\s*UiMode\s*,"
        r"\s*exit_after_startup\s*:\s*bool\s*,\s*control\s*:\s*RunControl\s*,"
        r"\s*desktop_settings\s*:\s*Option\s*<\s*DesktopRuntimeSettings\s*>\s*,?"
        r"\s*\)\s*->\s*Result\s*<\s*\(\s*\)\s*,\s*MainError\s*>",
    )
    assert compact_rust(run_packaged_backend_body) == compact_rust(
        """
        let resource_root_guard = packaged_resource_root(&request.resource_root)?;
        request.resource_root = resource_root_guard.path().to_path_buf();
        let before_dynamic =
            before_dynamic.with_resource_root(request.resource_root.clone())?;
        let result = run_backend(
            request,
            before_dynamic,
            ui_mode,
            exit_after_startup,
            control,
            desktop_settings,
        )
        .await;
        drop(resource_root_guard);
        result
        """
    )
    assert (
        run_packaged_backend_body.index(
            "packaged_resource_root(&request.resource_root)?"
        )
        < run_packaged_backend_body.index("before_dynamic.with_resource_root(")
        < run_packaged_backend_body.index("run_backend(")
        < run_packaged_backend_body.index(".await")
        < run_packaged_backend_body.index("drop(resource_root_guard)")
    )

    run_desktop_body = rust_top_level_function_body(
        entrypoint_source,
        r"\basync\s+fn\s+run_desktop\s*\(\s*request\s*:\s*PipelineRequest\s*,"
        r"\s*before_dynamic\s*:\s*LoadedSettings\s*,?\s*\)\s*"
        r"->\s*Result\s*<\s*\(\s*\)\s*,\s*MainError\s*>",
    )
    assert compact_rust(run_desktop_body) == compact_rust(
        """
        let runtime_settings = DesktopRuntimeSettings::new(
            before_dynamic
                .settings
                .string("ui.desktop.close_behavior")?
                .parse::<CloseBehavior>()?,
        );
        let disable_compositing =
            before_dynamic.pre_dynamic_final_boolean_with_cli("ui.desktop.disable_compositing")?;
        let shell_config = DesktopShellConfig {
            config_directory: before_dynamic.roots.config.clone(),
            runtime_settings: runtime_settings.clone(),
            disable_compositing,
        };
        let shutdown = ShutdownCoordinator::new();
        let lifecycle = DesktopLifecycle::new(shutdown.clone());
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let readiness_guard = ready_sender.clone();
        let backend_stopped_before_readiness = Arc::new(AtomicBool::new(false));
        let startup_failure_marker = Arc::clone(&backend_stopped_before_readiness);
        let (task_sender, task_receiver) =
            mpsc::sync_channel::<tokio::task::JoinHandle<Result<(), MainError>>>(1);
        let runtime = tokio::runtime::Handle::current();
        let task_shutdown = shutdown.clone();
        let control = RunControl::new(shutdown.clone())
            .with_ready_sender(ready_sender)
            .with_desktop_settings(runtime_settings.clone());

        let shell_result = tokio::task::block_in_place(|| {
            run_tauri_shell(
                tauri::generate_context!(),
                shell_config,
                &lifecycle,
                move || {
                    let inner_task = runtime.spawn(async move {
                        run_packaged_backend(
                            request,
                            before_dynamic,
                            UiMode::Desktop,
                            false,
                            control,
                            Some(runtime_settings),
                        )
                        .await
                    });
                    let supervisor_task = runtime.spawn(supervise_desktop_backend_startup(
                        inner_task,
                        task_shutdown,
                        readiness_guard,
                    ));
                    task_sender.send(supervisor_task).map_err(|_error| {
                        DesktopError::BackendStartup("backend task receiver was dropped".to_owned())
                    })?;
                    let actual_address = ready_receiver.recv().map_err(|_error| {
                        startup_failure_marker.store(true, Ordering::Release);
                        DesktopError::BackendAddressUnavailable
                    })?;
                    Ok(actual_address)
                },
            )
        });

        finish_desktop_run(
            shell_result,
            task_receiver,
            &shutdown,
            backend_stopped_before_readiness.load(Ordering::Acquire),
        )
        .await
        """
    )
    desktop_backend_supervisor_body = rust_top_level_function_body(
        entrypoint_source,
        r"\basync\s+fn\s+supervise_desktop_backend_task\s*\(",
    )
    assert compact_rust(desktop_backend_supervisor_body) == compact_rust(
        """
        match inner_task.await {
            Ok(result) => {
                if let Err(error) = result.as_ref() {
                    let _fatal_claimed =
                        shutdown.request(ShutdownReason::FatalError(error.to_string()));
                }
                result
            }
            Err(error) => {
                let error = MainError::BackendTask(error);
                let _fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
                Err(error)
            }
        }
        """
    )
    compact_desktop_backend_supervisor = compact_rust(desktop_backend_supervisor_body)
    assert compact_desktop_backend_supervisor.count("inner_task.await") == 1

    desktop_backend_startup_supervisor_body = rust_top_level_function_body(
        entrypoint_source,
        r"\basync\s+fn\s+supervise_desktop_backend_startup\s*<ReadinessGuard>\s*\(",
    )
    assert compact_rust(desktop_backend_startup_supervisor_body) == compact_rust(
        """
        let result = supervise_desktop_backend_task(inner_task, shutdown).await;
        drop(readiness_guard);
        result
        """
    )
    assert desktop_backend_startup_supervisor_body.index(
        "supervise_desktop_backend_task(inner_task, shutdown).await"
    ) < desktop_backend_startup_supervisor_body.index("drop(readiness_guard)")

    finish_desktop_body = rust_top_level_function_body(
        entrypoint_source,
        r"\basync\s+fn\s+finish_desktop_run\s*\(",
    )
    assert compact_rust(finish_desktop_body) == compact_rust(
        """
        if !backend_stopped_before_readiness
            && let Err(error) = shell_result.as_ref()
        {
            shutdown.request(ShutdownReason::FatalError(error.to_string()));
        }
        let backend_task = task_receiver.try_recv().ok();
        let backend_result = if let Some(task) = backend_task {
            match task.await {
                Ok(result) => result,
                Err(error) => Err(MainError::BackendTask(error)),
            }
        } else {
            Err(MainError::BackendNotStarted)
        };
        if backend_stopped_before_readiness {
            return if let Err(error) = backend_result {
                Err(error)
            } else {
                let error = shell_result
                    .err()
                    .unwrap_or(DesktopError::BackendAddressUnavailable);
                shutdown.request(ShutdownReason::FatalError(error.to_string()));
                Err(MainError::Desktop(error))
            };
        }
        shell_result?;
        backend_result
        """
    )
    assert compact_rust(finish_desktop_body).count("task.await") == 1
    assert finish_desktop_body.index("shell_result?;") < finish_desktop_body.rindex(
        "backend_result"
    )
    compact_run_desktop = compact_rust(run_desktop_body)
    assert compact_run_desktop.count("let inner_task = runtime.spawn(async move {") == 1
    assert (
        run_desktop_body.count("runtime.spawn(supervise_desktop_backend_startup(") == 1
    )
    assert compact_run_desktop.count("let readiness_guard = ready_sender.clone();") == 1
    assert (
        compact_run_desktop.count(
            "startup_failure_marker.store(true, Ordering::Release);"
        )
        == 1
    )
    assert (
        compact_run_desktop.count(
            "backend_stopped_before_readiness.load(Ordering::Acquire)"
        )
        == 1
    )
    assert compact_run_desktop.count("task_sender.send(supervisor_task)") == 1
    assert "task_sender.send(inner_task)" not in compact_run_desktop
    assert (
        compact_run_desktop.index("let inner_task = runtime.spawn(async move {")
        < compact_run_desktop.index("let supervisor_task = runtime.spawn(")
        < compact_run_desktop.index("task_sender.send(supervisor_task)")
        < compact_run_desktop.index("let actual_address = ready_receiver")
        < compact_run_desktop.index("finish_desktop_run(")
    )
    assert "packaged_resource_root" not in run_cli_body
    assert run_cli_body.count("load_before_dynamic()?") == 1
    assert run_cli_body.count("run_packaged_backend(") == 1
    assert entrypoint_source.count("run_packaged_backend(") == 3
    assert run_desktop_body.count("run_packaged_backend(") == 1
    assert run_desktop_body.index("move || {") < run_desktop_body.index(
        "run_packaged_backend("
    )
    assert "run_backend(" not in run_desktop_body
    assert "tauri::utils::Env::default()" not in entrypoint_source
    assert "APPDIR" not in entrypoint_source
    assert "APPIMAGE" not in entrypoint_source
    assert "tauri::utils::platform::resource_dir" not in entrypoint_source
    assert (
        re.search(r"\bresource_dir\s*(?:::\s*<[^>]*>\s*)?\(", entrypoint_source) is None
    )
    assert "select_resolved_tauri_resource_root" not in entrypoint_source
    assert "SettingsPipeline::new" not in run_desktop_body
    assert "select_tauri_resource_root" not in run_desktop_body
    assert "resource_root_guard" not in run_desktop_body

    application_run_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\basync\s+fn\s+run_configured_controlled\s*\(",
    )
    application_run = rust_lexical_mask(application_run_body)
    ui_router_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\bfn\s+ui_router\s*\(",
    )
    assert compact_rust(rust_lexical_mask(ui_router_body)) == compact_rust(
        """
        let security = RequestSecurity::new(listen_address, ui.allow_tauri_origin);
        public_router(api, static_files, security)
        """
    )
    readiness_barrier_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\basync\s+fn\s+serve_with_readiness_barrier\s*<",
    )
    assert compact_rust(readiness_barrier_body) == compact_rust(
        """
        tokio::pin!(serve);
        let request = tokio::select! {
            biased;
            result = &mut serve => return result,
            request = readiness => request,
        };
        let Ok(request) = request else {
            return serve.await;
        };
        if request.prepared.send(()).is_err() {
            return serve.await;
        }
        let _published = request.published.await;
        serve.await
        """
    )
    readiness_preparation_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\basync\s+fn\s+prepare_server_readiness\s*\(",
    )
    assert compact_rust(readiness_preparation_body) == compact_rust(
        """
        let (prepared_sender, prepared_receiver) = oneshot::channel();
        let (published_sender, published_receiver) = oneshot::channel();
        let request = ServerReadinessRequest {
            prepared: prepared_sender,
            published: published_receiver,
        };
        if readiness.send(request).is_err() {
            let result = (&mut *server_task).await;
            return Err(completed_server_task_result(shutdown, result));
        }
        if prepared_receiver.await.is_err() {
            let result = (&mut *server_task).await;
            return Err(completed_server_task_result(shutdown, result));
        }
        Ok(ServerReadinessPermit {
            published: published_sender,
        })
        """
    )
    readiness_publish_body = rust_impl_method_body(
        sources["lib.rs"],
        "ServerReadinessPermit",
        r"\bfn\s+publish\s*\(\s*self\s*\)",
    )
    assert compact_rust(readiness_publish_body) == compact_rust(
        """
        let _published = self.published.send(());
        """
    )
    runtime_wait_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\basync\s+fn\s+wait_for_shutdown_or_runtime_task\s*\(",
    )
    assert compact_rust(runtime_wait_body) == compact_rust(
        """
        tokio::select! {
            biased;
            _reason = shutdown.cancelled() => RuntimeTaskResult {
                early_task_error: None,
                server_task_consumed: false,
                signal_task_consumed: false,
            },
            result = &mut *server_task => completed_server_task_result(shutdown, result),
            result = &mut *signal_task => completed_signal_task_result(shutdown, result),
        }
        """
    )
    application_server_result_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\bfn\s+completed_server_task_result\s*\(",
    )
    assert compact_rust(application_server_result_body) == compact_rust(
        """
        let error = early_server_task_error(result);
        shutdown.request(ShutdownReason::FatalError(error.to_string()));
        RuntimeTaskResult {
            early_task_error: Some(EarlyRuntimeTaskError::Server(error)),
            server_task_consumed: true,
            signal_task_consumed: false,
        }
        """
    )
    listener_error_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\bfn\s+early_server_task_error\s*\(",
    )
    assert compact_rust(listener_error_body) == compact_rust(
        """
        match result {
            Ok(Ok(())) => AppError::ServerStopped,
            Ok(Err(error)) => AppError::Serve(error),
            Err(error) => AppError::Task(error),
        }
        """
    )
    application_signal_result_body = rust_top_level_function_body(
        sources["lib.rs"],
        r"\bfn\s+completed_signal_task_result\s*\(",
    )
    assert compact_rust(application_signal_result_body) == compact_rust(
        """
        let early_task_error = match result {
            Ok(()) => {
                let error = AppError::SignalStopped;
                let fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
                fatal_claimed.then_some(EarlyRuntimeTaskError::Signal(error))
            }
            Err(error) => {
                let error = AppError::Task(error);
                let _fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
                Some(EarlyRuntimeTaskError::Signal(error))
            }
        };
        RuntimeTaskResult {
            early_task_error,
            server_task_consumed: false,
            signal_task_consumed: true,
        }
        """
    )
    assert {
        identifier: len(re.findall(rf"\b{identifier}\b", application_run))
        for identifier in (
            "BoundServer",
            "ProductionRuntime",
            "RequestSecurity",
            "StaticFiles",
            "app",
            "production",
            "public_router",
            "router",
            "security",
            "server",
            "static_files",
            "ui",
            "ui_router",
        )
    } == {
        "BoundServer": 1,
        "ProductionRuntime": 1,
        "RequestSecurity": 0,
        "StaticFiles": 1,
        "app": 2,
        "production": 6,
        "public_router": 0,
        "router": 1,
        "security": 0,
        "server": 7,
        "static_files": 4,
        "ui": 3,
        "ui_router": 1,
    }

    application = compact_rust(application_source)
    for canonical_statement in (
        "let static_files = match StaticFiles::new(&options.web_root) {",
        "let ui = options.ui_mode.capabilities();",
        "let mut production = match ProductionRuntime::build(",
        "ui.screenshot_mode,",
        "let server = match BoundServer::bind(options.listen_address).await {",
        "let listen_address = server.local_addr();",
        "let app = ui_router(production.router(), static_files, listen_address, ui);",
        "let server = server.with_router(app);",
        "let mut signal_task = install_os_signal_forwarder(shutdown.clone()).await;",
        "let (server_readiness_sender, server_readiness_receiver) = oneshot::channel();",
        "let mut server_task = tokio::spawn(serve_with_readiness_barrier(",
        "server.serve(server_shutdown.clone()),",
        "server_readiness_receiver,",
    ):
        assert application.count(canonical_statement) == 1
    assert (
        len(re.findall(r"\blet\s+(?:mut\s+)?static_files\b", application_source)) == 1
    )
    assert len(re.findall(r"\blet\s+(?:mut\s+)?production\b", application_source)) == 1
    assert len(re.findall(r"\blet\s+(?:mut\s+)?security\b", application_source)) == 1
    assert len(re.findall(r"\blet\s+(?:mut\s+)?app\b", application_source)) == 1
    assert len(re.findall(r"\blet\s+(?:mut\s+)?ui\b", application_source)) == 1
    assert len(re.findall(r"\blet\s+(?:mut\s+)?server\b", application_source)) == 2
    readiness_preparation_call = compact_rust(
        """
        let task_result = match prepare_server_readiness(
            &shutdown,
            &mut server_task,
            server_readiness_sender,
        )
        .await
        {
        """
    )
    runtime_wait_call = (
        "wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, "
        "&mut signal_task).await"
    )
    conditional_server_completion = compact_rust(
        """
        let server_result = if task_result.server_task_consumed {
            Ok(())
        } else {
            finish_server_task(server_task).await
        };
        """
    )
    conditional_signal_completion = compact_rust(
        """
        let signal_result = if task_result.signal_task_consumed {
            Ok(())
        } else {
            signal_task.await.map_err(AppError::Task)
        };
        """
    )
    initiating_error_propagation = compact_rust(
        """
        if let Some(error) = task_result.early_task_error {
            return Err(match error {
                EarlyRuntimeTaskError::Server(error) |
                EarlyRuntimeTaskError::Signal(error) => error,
            });
        }
        """
    )
    compact_application_run = compact_rust(application_run_body)
    for exact_runtime_supervision in (
        readiness_preparation_call,
        runtime_wait_call,
        "Ok(readiness_permit) => {",
        "Err(task_result) => task_result,",
        "readiness_permit.publish();",
        conditional_server_completion,
        conditional_signal_completion,
        initiating_error_propagation,
        "server_result?;",
        "signal_result?;",
    ):
        assert compact_application_run.count(exact_runtime_supervision) == 1
    prepared_branch = compact_application_run.index("Ok(readiness_permit) => {")
    failed_preparation_branch = compact_application_run.index(
        "Err(task_result) => task_result,"
    )
    assert (
        compact_application_run.index("emit_startup_post(dynamic.as_ref()).await;")
        < compact_application_run.index(readiness_preparation_call)
        < prepared_branch
        < compact_application_run.index("diagnostic_id = APP_STARTING")
        < compact_application_run.index("ready.send(listen_address)")
        < compact_application_run.index("readiness_permit.publish();")
        < compact_application_run.index(runtime_wait_call)
        < failed_preparation_branch
        < compact_application_run.index(
            "let shutdown_reason = shutdown.cancelled().await;"
        )
    )
    assert (
        compact_application_run.index(runtime_wait_call)
        < compact_application_run.index(
            "let shutdown_reason = shutdown.cancelled().await;"
        )
        < compact_application_run.rindex(
            "shutdown_production(&mut production, dynamic.take()).await;"
        )
        < compact_application_run.index("server_shutdown.cancel();")
        < compact_application_run.index(conditional_server_completion)
        < compact_application_run.index(conditional_signal_completion)
        < compact_application_run.index(initiating_error_propagation)
        < compact_application_run.index("server_result?;")
        < compact_application_run.index("signal_result?;")
        < compact_application_run.rindex("tracing::info!(")
    )

    worker_run_body = rust_top_level_function_body(
        sources["worker_binary/mod.rs"],
        r"\basync\s+fn\s+run\s*\(\s*kind\s*:\s*WorkerKind\s*,",
    )
    worker_supervision_body = rust_top_level_function_body(
        sources["worker_binary/mod.rs"],
        r"\basync\s+fn\s+supervise_worker_tasks\s*\(",
    )
    assert compact_rust(worker_supervision_body) == compact_rust(
        """
        tokio::pin!(protocol);
        let observation = tokio::select! {
            biased;
            _reason = shutdown.cancelled() => WorkerTaskObservation::Shutdown,
            result = &mut *signal_task => {
                WorkerTaskObservation::Signal(completed_signal_task_result(shutdown, result))
            }
            result = &mut protocol => WorkerTaskObservation::Protocol(result),
        };
        match observation {
            WorkerTaskObservation::Shutdown => WorkerTaskResult {
                protocol_result: protocol.await,
                early_signal_error: None,
                signal_task_consumed: false,
            },
            WorkerTaskObservation::Signal(result) => WorkerTaskResult {
                protocol_result: protocol.await,
                early_signal_error: result.early_signal_error,
                signal_task_consumed: result.signal_task_consumed,
            },
            WorkerTaskObservation::Protocol(result) => WorkerTaskResult {
                protocol_result: result,
                early_signal_error: None,
                signal_task_consumed: false,
            },
        }
        """
    )
    worker_signal_result_body = rust_top_level_function_body(
        sources["worker_binary/mod.rs"],
        r"\bfn\s+completed_signal_task_result\s*\(",
    )
    assert compact_rust(worker_signal_result_body) == compact_rust(
        """
        let early_signal_error = match result {
            Ok(()) => {
                let error = WorkerError::SignalStopped;
                let fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
                fatal_claimed.then_some(error)
            }
            Err(error) => {
                let error = WorkerError::SignalTask(error);
                let _fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
                Some(error)
            }
        };
        WorkerSignalTaskResult {
            early_signal_error,
            signal_task_consumed: true,
        }
        """
    )
    worker_supervision_call = compact_rust(
        """
        let task_result = supervise_worker_tasks(
            &shutdown,
            &mut signal_task,
            run_protocol(kind, &connection, &shutdown),
        )
        .await;
        """
    )
    worker_connection_completion = "connection.close().await;"
    worker_signal_completion = compact_rust(
        """
        let signal_result = if signal_task_consumed {
            Ok(())
        } else {
            signal_task.await.map_err(WorkerError::SignalTask)
        };
        """
    )
    worker_initiating_error_propagation = compact_rust(
        """
        if let Some(error) = early_signal_error {
            return Err(error);
        }
        """
    )
    compact_worker_run = compact_rust(worker_run_body)
    for exact_worker_supervision in (
        "let mut signal_task = install_os_signal_forwarder(shutdown.clone()).await;",
        "let _signal_result = signal_task.await;",
        worker_supervision_call,
        worker_connection_completion,
        worker_signal_completion,
        worker_initiating_error_propagation,
        "protocol_result?;",
        "signal_result?;",
    ):
        assert compact_worker_run.count(exact_worker_supervision) == 1
    assert (
        compact_worker_run.index(worker_supervision_call)
        < compact_worker_run.index("if let Err(error) = &task_result.protocol_result {")
        < compact_worker_run.index(worker_connection_completion)
        < compact_worker_run.index("let shutdown_reason = shutdown.cancelled().await;")
        < compact_worker_run.index(worker_signal_completion)
        < compact_worker_run.index(worker_initiating_error_propagation)
        < compact_worker_run.index("protocol_result?;")
        < compact_worker_run.index("signal_result?;")
        < compact_worker_run.rindex("tracing::info!(")
    )


def assert_closed_production_routing(sources: dict[str, str]) -> None:
    assert_canonical_cargo_provenance(sources)
    assert_internal_module_dependency_and_ownership_boundaries(sources)
    production_macro_definition = re.compile(r"\bmacro_rules\s*!")
    production_include_invocation = re.compile(r"\b(?:r#)?include\s*!")
    top_level_bang_macro = re.compile(
        r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?"
        r"(?P<name>(?:r#)?[A-Za-z_][A-Za-z0-9_]*"
        r"(?:::\s*(?:r#)?[A-Za-z_][A-Za-z0-9_]*)*)\s*!"
    )
    expected_top_level_bang_macros = {
        "worker_binary/dynamic/engine.rs": ("thread_local",),
    }
    actual_top_level_bang_macros: dict[str, tuple[str, ...]] = {}
    for source_name, source in sources.items():
        if source_name.startswith("@") or not source_name.endswith(".rs"):
            continue
        source_mask = rust_lexical_mask(source)
        assert production_macro_definition.search(source_mask) is None, source_name
        assert production_include_invocation.search(source_mask) is None, source_name
        source_depths = rust_delimiter_depths(source_mask)
        source_top_level_macros = tuple(
            macro.group("name").replace(" ", "")
            for macro in top_level_bang_macro.finditer(source_mask)
            if source_depths[0][macro.start()] == 0
            and source_depths[1][macro.start()] == 0
            and source_depths[2][macro.start()] == 0
            and source_mask[: macro.start()].rstrip()[-1:] != "="
        )
        if source_top_level_macros:
            actual_top_level_bang_macros[source_name] = source_top_level_macros
    assert actual_top_level_bang_macros == expected_top_level_bang_macros
    raw_critical_identifier = re.compile(
        r"\br#(?:bind|bind_with_router|build|public_router|router|run_backend|"
        r"run_cli|run_configured_controlled|run_desktop|secure_router|serve|"
        r"with_router)\b"
    )
    assert not {
        source_name
        for source_name, source in sources.items()
        if not source_name.startswith("@")
        and source_name.endswith(".rs")
        and raw_critical_identifier.search(source) is not None
    }
    assert_canonical_routing_wiring(sources)
    expected_rest_sources = set(REST_ROUTE_PATHS_BY_SOURCE)
    actual_rest_sources = {
        source_name
        for source_name in sources
        if source_name.startswith("server/rest/") and source_name.endswith(".rs")
    }
    assert actual_rest_sources == expected_rest_sources
    assert ROUTER_FACTORY_SIGNATURES.keys() == ROUTER_FACTORY_BODIES.keys()
    for source_name, signature in ROUTER_FACTORY_SIGNATURES.items():
        if source_name == "server/websocket.rs":
            actual_body = rust_impl_method_body(
                sources[source_name], "WebSocketTransport", signature
            )
        elif source_name == "server/static_files.rs":
            actual_body = rust_impl_method_body(
                sources[source_name], "StaticFiles", signature
            )
        else:
            actual_body = rust_top_level_function_body(sources[source_name], signature)
        assert compact_rust(actual_body) == compact_rust(
            ROUTER_FACTORY_BODIES[source_name]
        ), source_name

    canonical_route_builder_imports = {
        "@rust/pokecon/src/lib.rs": ("use axum::Router;",),
        "lib.rs": ("use axum::Router;",),
        "production.rs": ("use axum::Router;",),
        "server/mod.rs": ("use axum::Router;",),
        "server/rest/commands.rs": (
            "use axum::Router;",
            "use axum::routing::post;",
        ),
        "server/rest/devices.rs": (
            "use axum::Router;",
            "use axum::routing::{MethodFilter, on, post};",
        ),
        "server/rest/dynamic_config.rs": (
            "use axum::Router;",
            "use axum::routing::post;",
        ),
        "server/rest/mod.rs": (
            "use axum::routing::any;",
            "use axum::{Json, Router};",
        ),
        "server/rest/notifications.rs": (
            "use axum::Router;",
            "use axum::routing::post;",
        ),
        "server/rest/profiles.rs": (
            "use axum::Router;",
            "use axum::routing::post;",
        ),
        "server/rest/script_ui.rs": (
            "use axum::Router;",
            "use axum::routing::post;",
        ),
        "server/rest/settings.rs": (
            "use axum::Router;",
            "use axum::routing::{MethodFilter, on};",
        ),
        "server/rest/state.rs": (
            "use axum::routing::{MethodFilter, on};",
            "use axum::{Json, Router};",
        ),
        "server/rest/update.rs": (
            "use axum::Router;",
            "use axum::routing::post;",
        ),
        "server/router.rs": ("use axum::Router;",),
        "server/security.rs": ("use axum::{Json, Router};",),
        "server/static_files.rs": ("use axum::Router;",),
        "server/websocket.rs": (
            "use axum::routing::{MethodFilter, on};",
            "use axum::{Json, Router};",
        ),
    }
    route_builder_identifier = re.compile(r"\b(?:Router|MethodFilter|on|post|any)\b")
    top_level_use = re.compile(
        r"(?ms)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?use\s+([^;]+)\s*;"
    )
    actual_route_builder_imports: dict[str, tuple[str, ...]] = {}
    rebound_axum_sources: set[str] = set()
    for source_name, source in sources.items():
        if not source_name.endswith(".rs"):
            continue
        mask = rust_lexical_mask(source)
        brace_depths, parenthesis_depths, bracket_depths = rust_delimiter_depths(mask)
        critical_imports: list[str] = []
        for imported in top_level_use.finditer(mask):
            if not (
                brace_depths[imported.start()] == 0
                and parenthesis_depths[imported.start()] == 0
                and bracket_depths[imported.start()] == 0
            ):
                continue
            body = compact_rust(imported.group(1))
            if route_builder_identifier.search(body) is not None:
                critical_imports.append(compact_rust(imported.group(0)))
            if (
                re.search(r"\bas\s+(?:r#)?axum\b", body) is not None
                or re.search(r"(?:^|::|\{)\s*(?:r#)?axum\s*(?:,|\}|$)", body)
                is not None
            ):
                rebound_axum_sources.add(source_name)
        if critical_imports:
            actual_route_builder_imports[source_name] = tuple(critical_imports)
    assert actual_route_builder_imports == canonical_route_builder_imports
    assert not rebound_axum_sources

    critical_definition = re.compile(
        r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?"
        r"(?:(?:async|const|unsafe)\s+)*(?:fn|struct|enum|union|trait|type|mod|"
        r"const|static)\s+(?:r#)?(?:axum|Router|MethodFilter|on|post|any)\b"
    )
    extern_axum_rebinding = re.compile(
        r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?extern\s+crate\s+"
        r"[^;]*(?:\bas\s+)?(?:r#)?axum\s*;"
    )
    for source_name, source in sources.items():
        if not source_name.endswith(".rs"):
            continue
        mask = rust_lexical_mask(source)
        brace_depths, parenthesis_depths, bracket_depths = rust_delimiter_depths(mask)
        for forbidden_definition in (
            *critical_definition.finditer(mask),
            *extern_axum_rebinding.finditer(mask),
        ):
            assert not (
                brace_depths[forbidden_definition.start()] == 0
                and parenthesis_depths[forbidden_definition.start()] == 0
                and bracket_depths[forbidden_definition.start()] == 0
            ), source_name

    route_builder_sources = set(canonical_route_builder_imports)
    assert not {
        source_name
        for source_name in route_builder_sources
        if re.search(
            r"\b(?:Method|MethodFilter)\s*::\s*CONNECT\b",
            rust_lexical_mask(sources[source_name]),
        )
        is not None
    }

    security = sources["server/security.rs"]
    exact_security_imports = (
        r"(?m)^[ \t]*use\s+std\s*::\s*collections\s*::\s*BTreeSet\s*;$",
        r"(?m)^[ \t]*use\s+std\s*::\s*net\s*::\s*SocketAddr\s*;$",
        r"(?m)^[ \t]*use\s+axum\s*::\s*extract\s*::\s*"
        r"\{\s*Request\s*,\s*State\s*,?\s*\}\s*;$",
        r"(?ms)^[ \t]*use\s+axum\s*::\s*http\s*::\s*header\s*::\s*\{"
        r"\s*ACCESS_CONTROL_ALLOW_HEADERS\s*,\s*ACCESS_CONTROL_ALLOW_METHODS\s*,"
        r"\s*ACCESS_CONTROL_ALLOW_ORIGIN\s*,\s*ACCESS_CONTROL_REQUEST_HEADERS\s*,"
        r"\s*ACCESS_CONTROL_REQUEST_METHOD\s*,\s*CONTENT_TYPE\s*,\s*HOST\s*,"
        r"\s*ORIGIN\s*,\s*VARY\s*,?\s*\}\s*;$",
        r"(?m)^[ \t]*use\s+axum\s*::\s*http\s*::\s*\{"
        r"\s*HeaderMap\s*,\s*HeaderName\s*,\s*HeaderValue\s*,\s*Method\s*,"
        r"\s*StatusCode\s*,\s*Uri\s*,?\s*\}\s*;$",
        r"(?m)^[ \t]*use\s+axum\s*::\s*response\s*::\s*\{"
        r"\s*IntoResponse\s*,\s*Response\s*,?\s*\}\s*;$",
        r"(?m)^[ \t]*use\s+crate\s*::\s*server\s*::\s*api\s*::\s*"
        r"\{\s*ApiError\s*,\s*ApiErrorCode\s*,\s*ErrorEnvelope\s*,?\s*\}\s*;$",
    )
    for import_pattern in exact_security_imports:
        assert len(rust_top_level_matches(security, import_pattern)) == 1
    assert (
        len(
            rust_top_level_matches(
                security,
                r"(?m)^[ \t]*use\s+axum\s*::\s*middleware\s*::\s*"
                r"\{\s*self\s*,\s*Next\s*\}\s*;",
            )
        )
        == 1
    )
    assert (
        len(
            rust_top_level_matches(
                security,
                r"(?m)^[ \t]*use\s+axum\s*::\s*"
                r"\{\s*Json\s*,\s*Router\s*\}\s*;",
            )
        )
        == 1
    )
    security_mask = rust_lexical_mask(security)
    security_depths = rust_delimiter_depths(security_mask)
    security_top_level_uses = tuple(
        imported
        for imported in top_level_use.finditer(security_mask)
        if security_depths[0][imported.start()] == 0
        and security_depths[1][imported.start()] == 0
        and security_depths[2][imported.start()] == 0
    )
    assert len(security_top_level_uses) == len(exact_security_imports) + 2
    assert not rust_top_level_matches(
        security,
        r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?extern\s+crate\s+[^;]+;",
    )
    security_function_names = tuple(
        function.group(1)
        for function in re.finditer(
            r"\bfn\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\b",
            security_mask,
        )
    )
    assert security_function_names == (
        "new",
        "validate",
        "into_response",
        "optional_header",
        "required_header",
        "is_mutating",
        "validate_preflight",
        "add_cors_headers",
        "enforce_security",
        "secure_router",
    )
    security_definition_pattern = re.compile(
        r"(?m)^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?"
        r"(?:(?:async|const|unsafe)\s+)*(?:extern\s+(?:\"[^\"]+\"\s+)?)?"
        r"(?P<kind>fn|struct|enum|union|trait|type|const|static|mod)\s+"
        r"(?:r#)?(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b"
    )
    security_top_level_definitions = tuple(
        (definition.group("kind"), definition.group("name"))
        for definition in security_definition_pattern.finditer(security_mask)
        if security_depths[0][definition.start()] == 0
        and security_depths[1][definition.start()] == 0
        and security_depths[2][definition.start()] == 0
    )
    assert security_top_level_definitions == (
        ("const", "REQUEST_MARKER"),
        ("const", "ALLOWED_METHODS"),
        ("const", "ALLOWED_HEADERS"),
        ("struct", "RequestSecurity"),
        ("enum", "SecurityDecision"),
        ("enum", "SecurityError"),
        ("fn", "optional_header"),
        ("fn", "required_header"),
        ("fn", "is_mutating"),
        ("fn", "validate_preflight"),
        ("fn", "add_cors_headers"),
        ("fn", "enforce_security"),
        ("fn", "secure_router"),
    )
    security_impl_pattern = re.compile(
        r"(?m)^[ \t]*(?:unsafe\s+)?impl\s+([^{}\r\n]+?)\s*\{"
    )
    security_top_level_impls = tuple(
        compact_rust(implementation.group(0))
        for implementation in security_impl_pattern.finditer(security_mask)
        if security_depths[0][implementation.start()] == 0
        and security_depths[1][implementation.start()] == 0
        and security_depths[2][implementation.start()] == 0
    )
    assert security_top_level_impls == (
        "impl RequestSecurity {",
        "impl IntoResponse for SecurityError {",
    )
    security_macro_definition_pattern = re.compile(
        r"(?<![\w#])(?:r#)?macro_rules(?!\w)\s*!"
    )
    assert not tuple(security_macro_definition_pattern.finditer(security_mask))
    security_macro_invocation_pattern = re.compile(r"!\s*(?P<delimiter>[({\[])")
    security_macro_invocations: list[tuple[str, str, int, int, int]] = []
    for invocation in security_macro_invocation_pattern.finditer(security_mask):
        line_start = security.rfind("\n", 0, invocation.start()) + 1
        line_end = security.find("\n", invocation.end())
        if line_end == -1:
            line_end = len(security)
        security_macro_invocations.append(
            (
                security[line_start:line_end].strip(),
                invocation.group("delimiter"),
                security_depths[0][invocation.start()],
                security_depths[1][invocation.start()],
                security_depths[2][invocation.start()],
            )
        )
    assert tuple(security_macro_invocations) == (
        (
            'let mut allowed_origins = BTreeSet::from([::std::format!("http://{authority}")]);',
            "(",
            2,
            1,
            1,
        ),
        (
            'let localhost = ::std::format!("localhost:{}", address.port());',
            "(",
            3,
            0,
            0,
        ),
        (
            'allowed_origins.insert(::std::format!("http://{localhost}"));',
            "(",
            3,
            1,
            0,
        ),
    )
    assert len(re.findall(r"\bsecure_router\b", security_mask)) == 1
    exact_security_constants = (
        r"(?m)^[ \t]*const\s+REQUEST_MARKER\s*:\s*HeaderName\s*=\s*"
        r'HeaderName\s*::\s*from_static\s*\(\s*"x-pokecon-request"\s*\)\s*;$',
        r"(?m)^[ \t]*const\s+ALLOWED_METHODS\s*:\s*&\s*str\s*=\s*"
        r'"GET, PATCH, POST, OPTIONS"\s*;$',
        r"(?m)^[ \t]*const\s+ALLOWED_HEADERS\s*:\s*&\s*str\s*=\s*"
        r'"Content-Type, X-Pokecon-Request"\s*;$',
    )
    security_without_comments = rust_without_comments(security)
    for constant_pattern in exact_security_constants:
        constant_matches = rust_top_level_matches(
            security,
            constant_pattern,
            search_view=security_without_comments,
        )
        assert len(constant_matches) == 1
        assert (
            rust_item_attributes(
                security,
                security_mask,
                constant_matches[0].start(),
            )
            == ()
        )
    security_code = compact_rust(rust_without_comments(security))
    assert (
        security_code.count(
            'const ALLOWED_METHODS: &str = "GET, PATCH, POST, OPTIONS";'
        )
        == 1
    )
    assert (
        security_code.count(
            'const ALLOWED_HEADERS: &str = "Content-Type, X-Pokecon-Request";'
        )
        == 1
    )
    assert {
        identifier: len(re.findall(rf"\b{identifier}\b", rust_lexical_mask(security)))
        for identifier in (
            "ALLOWED_HEADERS",
            "ALLOWED_METHODS",
            "REQUEST_MARKER",
            "validate_preflight",
        )
    } == {
        "ALLOWED_HEADERS": 2,
        "ALLOWED_METHODS": 2,
        "REQUEST_MARKER": 3,
        "validate_preflight": 2,
    }
    request_security_fields = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*pub\s+struct\s+RequestSecurity\b",
        attributes=("#[derive(Clone, Debug)]",),
    )
    assert exact_rust(request_security_fields) == exact_rust(
        """
        allowed_hosts: BTreeSet<String>,
        allowed_origins: BTreeSet<String>,
        """
    )
    security_decision_variants = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*enum\s+SecurityDecision\b",
        attributes=("#[derive(Clone, Debug)]",),
    )
    assert exact_rust(security_decision_variants) == exact_rust(
        """
        Continue { origin: Option<HeaderValue> },
        Preflight { origin: HeaderValue },
        """
    )
    security_error_variants = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*enum\s+SecurityError\b",
        attributes=("#[derive(Clone, Copy, Debug)]",),
    )
    assert exact_rust(security_error_variants) == exact_rust(
        """
        Forbidden,
        UnsupportedMediaType,
        """
    )
    request_security_new_body = rust_impl_method_body(
        security,
        "RequestSecurity",
        r"(?m)^[ \t]*pub\s+fn\s+new\s*\(\s*address\s*:\s*SocketAddr\s*,"
        r"\s*desktop_mode\s*:\s*bool\s*,?\s*\)\s*->\s*Self\s*(?=\{)",
        attributes=("#[must_use]",),
    )
    assert exact_rust(request_security_new_body) == exact_rust(
        """
        let authority = address.to_string();
        let mut allowed_hosts = BTreeSet::from([authority.clone()]);
        let mut allowed_origins = BTreeSet::from([::std::format!("http://{authority}")]);
        if address.ip().is_loopback() {
            let localhost = ::std::format!("localhost:{}", address.port());
            allowed_origins.insert(::std::format!("http://{localhost}"));
            allowed_hosts.insert(localhost);
        }
        if desktop_mode {
            allowed_origins.insert("tauri://localhost".to_owned());
        }
        Self {
            allowed_hosts,
            allowed_origins,
        }
        """
    )
    security_error_into_response_body = rust_impl_method_body(
        security,
        "SecurityError",
        r"(?m)^[ \t]*fn\s+into_response\s*\(\s*self\s*\)\s*"
        r"->\s*Response\s*(?=\{)",
        trait_name="IntoResponse",
    )
    assert exact_rust(security_error_into_response_body) == exact_rust(
        """
        let (status, code, message) = match self {
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                ApiErrorCode::RequestForbidden,
                "request validation failed",
            ),
            Self::UnsupportedMediaType => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                ApiErrorCode::UnsupportedMediaType,
                "Content-Type must be application/json",
            ),
        };
        (
            status,
            Json(ErrorEnvelope {
                error: ApiError {
                    code,
                    message: message.to_owned(),
                    fields: None,
                },
            }),
        )
            .into_response()
        """
    )
    exact_header_lookup_helpers = (
        (
            r"(?m)^[ \t]*fn\s+optional_header\s*<\s*'a\s*>\s*\("
            r"\s*headers\s*:\s*&\s*'a\s*HeaderMap\s*,"
            r"\s*name\s*:\s*&\s*HeaderName\s*,?\s*\)\s*"
            r"->\s*Result\s*<\s*Option\s*<\s*&\s*'a\s*HeaderValue\s*>\s*,"
            r"\s*SecurityError\s*>\s*(?=\{)",
            """
            let mut values = headers.get_all(name).iter();
            let value = values.next();
            if values.next().is_some() {
                return Err(SecurityError::Forbidden);
            }
            Ok(value)
            """,
        ),
        (
            r"(?m)^[ \t]*fn\s+required_header\s*<\s*'a\s*>\s*\("
            r"\s*headers\s*:\s*&\s*'a\s*HeaderMap\s*,"
            r"\s*name\s*:\s*&\s*HeaderName\s*,?\s*\)\s*"
            r"->\s*Result\s*<\s*&\s*'a\s*HeaderValue\s*,"
            r"\s*SecurityError\s*>\s*(?=\{)",
            """
            optional_header(headers, name)?.ok_or(SecurityError::Forbidden)
            """,
        ),
    )
    for signature, expected_body in exact_header_lookup_helpers:
        actual_body = rust_top_level_function_body(security, signature)
        assert exact_rust(actual_body) == exact_rust(expected_body)
    is_mutating_body = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*fn\s+is_mutating\s*\(\s*method\s*:\s*&\s*Method"
        r"\s*,?\s*\)\s*->\s*bool\s*(?=\{)",
    )
    assert exact_rust(is_mutating_body) == exact_rust(
        """
        method == Method::PATCH
            || method == Method::POST
            || method == Method::PUT
            || method == Method::DELETE
        """
    )
    request_validation_body = rust_impl_method_body(
        security,
        "RequestSecurity",
        r"(?m)^[ \t]*fn\s+validate\s*\(\s*&\s*self\s*,\s*method\s*:\s*&\s*Method\s*,"
        r"\s*uri\s*:\s*&\s*Uri\s*,\s*headers\s*:\s*&\s*HeaderMap\s*,?\s*\)"
        r"\s*->\s*Result\s*<\s*SecurityDecision\s*,\s*SecurityError\s*>"
        r"\s*(?=\{)",
    )
    assert exact_rust(request_validation_body) == exact_rust(
        """
        let host = required_header(headers, &HOST)?;
        let host = host.to_str().map_err(|_error| SecurityError::Forbidden)?;
        if !self
            .allowed_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host))
        {
            return Err(SecurityError::Forbidden);
        }
        if let Some(authority) = uri.authority()
            && !self
                .allowed_hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(authority.as_str()))
        {
            return Err(SecurityError::Forbidden);
        }

        let origin = optional_header(headers, &ORIGIN)?;
        let origin = origin
            .map(|value| {
                let text = value.to_str().map_err(|_error| SecurityError::Forbidden)?;
                self.allowed_origins
                    .contains(text)
                    .then(|| value.clone())
                    .ok_or(SecurityError::Forbidden)
            })
            .transpose()?;

        if method == Method::OPTIONS {
            let origin = origin.ok_or(SecurityError::Forbidden)?;
            validate_preflight(uri.path(), headers)?;
            return Ok(SecurityDecision::Preflight { origin });
        }
        if uri.path() == "/ws" && origin.is_none() {
            return Err(SecurityError::Forbidden);
        }
        if is_mutating(method) {
            let content_type = optional_header(headers, &CONTENT_TYPE)?
                .ok_or(SecurityError::UnsupportedMediaType)?;
            let content_type = content_type
                .to_str()
                .map_err(|_error| SecurityError::UnsupportedMediaType)?;
            if !content_type
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
            {
                return Err(SecurityError::UnsupportedMediaType);
            }
            let marker = required_header(headers, &REQUEST_MARKER)?;
            if marker.as_bytes() != b"1" {
                return Err(SecurityError::Forbidden);
            }
        }
        Ok(SecurityDecision::Continue { origin })
        """
    )
    assert security_code.count("validate_preflight(uri.path(), headers)?;") == 1
    validate_preflight_body = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*fn\s+validate_preflight\s*\(\s*path\s*:\s*&\s*str\s*,"
        r"\s*headers\s*:\s*&\s*HeaderMap\s*\)\s*"
        r"->\s*Result\s*<\s*\(\s*\)\s*,\s*SecurityError\s*>\s*(?=\{)",
    )
    assert exact_rust(validate_preflight_body) == exact_rust(
        """
        let requested_method = required_header(headers, &ACCESS_CONTROL_REQUEST_METHOD)?;
        let requested_method = Method::from_bytes(requested_method.as_bytes())
            .map_err(|_error| SecurityError::Forbidden)?;
        let advertised = match path {
            "/api/settings" => requested_method == Method::GET || requested_method == Method::PATCH,
            "/api/devices/cameras" | "/api/devices/serial-ports" | "/api/state" | "/ws" => {
                requested_method == Method::GET
            }
            "/api/camera/retry"
            | "/api/camera/screenshot"
            | "/api/commands/control"
            | "/api/commands/reload"
            | "/api/dynamic-config/control"
            | "/api/notifications/test"
            | "/api/profiles/generate-launcher"
            | "/api/script-ui/action"
            | "/api/serial/control"
            | "/api/update/check" => requested_method == Method::POST,
            _ => false,
        };
        if !advertised {
            return Err(SecurityError::Forbidden);
        }
        let Some(requested_headers) = optional_header(headers, &ACCESS_CONTROL_REQUEST_HEADERS)? else {
            return Ok(());
        };
        let requested_headers = requested_headers
            .to_str()
            .map_err(|_error| SecurityError::Forbidden)?;
        requested_headers
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .all(|name| {
                name.eq_ignore_ascii_case(CONTENT_TYPE.as_str())
                    || name.eq_ignore_ascii_case(REQUEST_MARKER.as_str())
            })
            .then_some(())
            .ok_or(SecurityError::Forbidden)
        """
    )
    add_cors_headers_body = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*fn\s+add_cors_headers\s*\(\s*response\s*:\s*&\s*mut\s*"
        r"Response\s*,\s*origin\s*:\s*HeaderValue\s*,?\s*\)\s*(?=\{)",
    )
    assert exact_rust(add_cors_headers_body) == exact_rust(
        """
        response
            .headers_mut()
            .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
        response
            .headers_mut()
            .append(VARY, HeaderValue::from_static("Origin"));
        """
    )
    enforce_security_body = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*async\s+fn\s+enforce_security\s*\(\s*State\s*\(\s*security\s*\)"
        r"\s*:\s*State\s*<\s*RequestSecurity\s*>\s*,\s*request\s*:\s*Request"
        r"\s*,\s*next\s*:\s*Next\s*,?\s*\)\s*->\s*Response\s*(?=\{)",
    )
    assert exact_rust(enforce_security_body) == exact_rust(
        """
        match security.validate(request.method(), request.uri(), request.headers()) {
            Ok(SecurityDecision::Continue { origin }) => {
                let mut response = next.run(request).await;
                if let Some(origin) = origin {
                    add_cors_headers(&mut response, origin);
                }
                response
            }
            Ok(SecurityDecision::Preflight { origin }) => {
                let mut response = StatusCode::NO_CONTENT.into_response();
                add_cors_headers(&mut response, origin);
                response.headers_mut().insert(
                    ACCESS_CONTROL_ALLOW_METHODS,
                    HeaderValue::from_static(ALLOWED_METHODS),
                );
                response.headers_mut().insert(
                    ACCESS_CONTROL_ALLOW_HEADERS,
                    HeaderValue::from_static(ALLOWED_HEADERS),
                );
                response
            }
            Err(error) => error.into_response(),
        }
        """
    )
    secure_router_body = rust_top_level_function_body(
        security,
        r"(?m)^[ \t]*pub\s+fn\s+secure_router\s*\(\s*router\s*:\s*Router\s*,"
        r"\s*security\s*:\s*RequestSecurity\s*\)\s*->\s*Router\s*(?=\{)",
    )
    assert exact_rust(secure_router_body) == exact_rust(
        """
        Router::new()
            .fallback_service(router)
            .layer(middleware::from_fn_with_state(security, enforce_security))
        """
    )
    assert security_code.count(".fallback_service(router)") == 1
    assert (
        security_code.count(
            "middleware::from_fn_with_state(security, enforce_security)"
        )
        == 1
    )
    assert "router.layer(middleware::from_fn_with_state" not in security_code
    assert secure_router_body.index("fallback_service(router)") < (
        secure_router_body.index("middleware::from_fn_with_state")
    )

    exact_response_imports = {
        "server/rest/mod.rs": (
            r"(?m)^[ \t]*use\s+axum\s*::\s*http\s*::\s*header\s*::\s*"
            r"\{\s*ALLOW\s*,\s*CONTENT_DISPOSITION\s*,\s*CONTENT_TYPE\s*\}\s*;$",
            r"(?m)^[ \t]*use\s+axum\s*::\s*http\s*::\s*"
            r"\{\s*HeaderValue\s*,\s*StatusCode\s*,\s*Uri\s*\}\s*;$",
            r"(?m)^[ \t]*use\s+axum\s*::\s*response\s*::\s*"
            r"\{\s*IntoResponse\s*,\s*Response\s*\}\s*;$",
            r"(?m)^[ \t]*use\s+axum\s*::\s*"
            r"\{\s*Json\s*,\s*Router\s*\}\s*;$",
            r"(?m)^[ \t]*use\s+crate\s*::\s*server\s*::\s*api\s*::\s*"
            r"\{\s*ApiError\s*,\s*ApiErrorCode\s*,\s*ErrorEnvelope\s*\}\s*;$",
        ),
        "server/websocket.rs": (
            r"(?m)^[ \t]*use\s+axum\s*::\s*http\s*::\s*header\s*::\s*ALLOW\s*;$",
            r"(?m)^[ \t]*use\s+axum\s*::\s*http\s*::\s*"
            r"\{\s*HeaderValue\s*,\s*StatusCode\s*\}\s*;$",
            r"(?m)^[ \t]*use\s+axum\s*::\s*response\s*::\s*"
            r"\{\s*IntoResponse\s*,\s*Response\s*\}\s*;$",
            r"(?m)^[ \t]*use\s+axum\s*::\s*"
            r"\{\s*Json\s*,\s*Router\s*\}\s*;$",
            r"(?ms)^[ \t]*use\s+crate\s*::\s*server\s*::\s*api\s*::\s*\{"
            r"(?=[^;]*\bApiError\s*,)(?=[^;]*\bApiErrorCode\s*,)"
            r"(?=[^;]*\bErrorEnvelope\s*,)[^;]+\}\s*;$",
        ),
    }
    for source_name, import_patterns in exact_response_imports.items():
        for import_pattern in import_patterns:
            assert (
                len(rust_top_level_matches(sources[source_name], import_pattern)) == 1
            ), (source_name, import_pattern)

    rest_error_new = rust_impl_method_body(
        sources["server/rest/mod.rs"],
        "RestError",
        r"(?m)^[ \t]*fn\s+new\s*\(\s*status\s*:\s*StatusCode\s*,"
        r"\s*code\s*:\s*ApiErrorCode\s*,\s*message\s*:\s*impl\s+Into\s*"
        r"<\s*String\s*>\s*,?\s*\)\s*->\s*Self\s*(?=\{)",
    )
    assert compact_rust(rest_error_new) == compact_rust(
        """
        Self {
            status,
            error: ApiError {
                code,
                message: message.into(),
                fields: None,
            },
        }
        """
    )
    rest_error_into_response = rust_impl_method_body(
        sources["server/rest/mod.rs"],
        "RestError",
        r"(?m)^[ \t]*fn\s+into_response\s*\(\s*self\s*\)\s*"
        r"->\s*Response\s*(?=\{)",
        trait_name="IntoResponse",
    )
    assert compact_rust(rest_error_into_response) == compact_rust(
        """
        (self.status, Json(ErrorEnvelope { error: self.error })).into_response()
        """
    )
    websocket_http_error = rust_top_level_function_body(
        sources["server/websocket.rs"],
        r"(?m)^[ \t]*fn\s+http_error\s*\(\s*status\s*:\s*StatusCode\s*,"
        r"\s*code\s*:\s*ApiErrorCode\s*,\s*message\s*:\s*&\s*str\s*,?\s*\)"
        r"\s*->\s*Response\s*(?=\{)",
    )
    assert compact_rust(websocket_http_error) == compact_rust(
        """
        (
            status,
            Json(ErrorEnvelope {
                error: ApiError {
                    code,
                    message: message.to_owned(),
                    fields: None,
                },
            }),
        )
            .into_response()
        """
    )

    exact_catchall_handlers = (
        (
            "server/rest/mod.rs",
            r"\basync\s+fn\s+not_found\s*\(\s*\)\s*->\s*RestError",
            """
            RestError::new(
                StatusCode::NOT_FOUND,
                ApiErrorCode::ResourceNotFound,
                "API resource was not found",
            )
            """,
        ),
        (
            "server/rest/mod.rs",
            r"(?m)^[ \t]*async\s+fn\s+method_not_allowed\s*"
            r"\(\s*uri\s*:\s*Uri\s*\)\s*->\s*Response\s*(?=\{)",
            """
            let allow = match uri.path() {
                "/api/settings" => "GET, PATCH",
                "/api/devices/cameras" | "/api/devices/serial-ports" | "/api/state" => "GET",
                "/api/camera/retry"
                | "/api/camera/screenshot"
                | "/api/commands/control"
                | "/api/commands/reload"
                | "/api/dynamic-config/control"
                | "/api/notifications/test"
                | "/api/profiles/generate-launcher"
                | "/api/script-ui/action"
                | "/api/serial/control"
                | "/api/update/check" => "POST",
                _ => return not_found().await.into_response(),
            };
            let mut response = RestError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                ApiErrorCode::MethodNotAllowed,
                "HTTP method is not allowed for this resource",
            )
            .into_response();
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static(allow));
            response
            """,
        ),
        (
            "server/websocket.rs",
            r"\basync\s+fn\s+websocket_method_not_allowed\s*\(\s*\)\s*"
            r"->\s*Response",
            """
            let mut response = http_error(
                StatusCode::METHOD_NOT_ALLOWED,
                ApiErrorCode::MethodNotAllowed,
                "HTTP method is not allowed for this resource",
            );
            response
                .headers_mut()
                .insert(ALLOW, HeaderValue::from_static("GET"));
            response
            """,
        ),
    )
    for source_name, signature, expected_body in exact_catchall_handlers:
        actual_body = rust_top_level_function_body(sources[source_name], signature)
        assert compact_rust(actual_body) == compact_rust(expected_body), source_name

    route_call_pattern = re.compile(r"\.\s*route\s*\(")
    literal_route_pattern = re.compile(r'\.\s*route\s*\(\s*"([^"]+)"')
    expected_route_calls = {
        source_name: len(paths)
        for source_name, paths in REST_ROUTE_PATHS_BY_SOURCE.items()
    }
    expected_route_calls |= {
        "server/realtime_connection.rs": 2,
        "server/websocket.rs": 1,
    }
    actual_route_calls = {
        source_name: len(route_call_pattern.findall(source))
        for source_name, source in sources.items()
        if route_call_pattern.search(source) is not None
    }
    assert actual_route_calls == expected_route_calls

    registered_rest_paths: list[str] = []
    for source_name, expected_paths in REST_ROUTE_PATHS_BY_SOURCE.items():
        source = sources[source_name]
        literal_paths = tuple(literal_route_pattern.findall(source))
        assert len(route_call_pattern.findall(source)) == len(literal_paths), (
            source_name
        )
        assert literal_paths == expected_paths, source_name
        registered_rest_paths.extend(literal_paths)

    catchalls = {"/api", "/api/{*path}"}
    normative_rest_paths = [
        path for path in registered_rest_paths if path not in catchalls
    ]
    documented_rest_paths = {
        path
        for path, _method, _operation_id in EXPECTED_OPENAPI_OPERATIONS
        if path != "/ws"
    }
    assert len(normative_rest_paths) == len(set(normative_rest_paths)) == 14
    assert set(normative_rest_paths) == documented_rest_paths

    websocket = sources["server/websocket.rs"]
    assert tuple(literal_route_pattern.findall(websocket)) == ("/ws",)
    realtime_connection = sources["server/realtime_connection.rs"]
    assert realtime_connection.count("self.controller.route()") == 2

    alternative_registration = re.compile(
        r"\.\s*(?:route_service|nest|nest_service)\s*\("
    )
    assert not {
        source_name
        for source_name, source in sources.items()
        if alternative_registration.search(source) is not None
    }
    assert not {
        source_name
        for source_name, source in sources.items()
        if re.search(r"\bRouter\s+as\s+", source) is not None
    }
    assert not {
        source_name
        for source_name, source in sources.items()
        if re.search(
            r"(?i)\b[A-Za-z_][A-Za-z0-9_]*(?:route|router)[A-Za-z0-9_]*\s*!",
            source,
        )
        is not None
    }
    assert not {
        source_name
        for source_name, source in sources.items()
        if re.search(r"(?i)#\s*\[[^\]]*\b(?:route|router)\b", source) is not None
    }

    angle_ufcs_registration = re.compile(
        r"<[^;{}]*?>\s*::\s*(?:route|route_service|nest|nest_service|merge|"
        r"fallback|fallback_service|method_not_allowed_fallback)\s*\("
    )
    assert not {
        source_name
        for source_name, source in sources.items()
        if angle_ufcs_registration.search(source) is not None
        and angle_ufcs_registration.search(rust_lexical_mask(source)) is not None
    }

    router_associated_call_pattern = re.compile(
        r"\bRouter(?:\s*::\s*<[^>]+>)?\s*::\s*"
        r"([A-Za-z_][A-Za-z0-9_]*)\s*\("
    )
    expected_router_associated_calls = {
        source_name: ("new",) for source_name in expected_rest_sources
    }
    expected_router_associated_calls |= {
        "server/mod.rs": ("new",),
        "server/security.rs": ("new",),
        "server/static_files.rs": ("new",),
        "server/websocket.rs": ("new",),
    }
    actual_router_associated_calls = {
        source_name: tuple(router_associated_call_pattern.findall(source))
        for source_name, source in sources.items()
        if router_associated_call_pattern.search(source) is not None
    }
    assert actual_router_associated_calls == expected_router_associated_calls

    merge_pattern = re.compile(r"\.\s*merge\s*\(")
    actual_merge_calls = {
        source_name: len(merge_pattern.findall(source))
        for source_name, source in sources.items()
        if merge_pattern.search(source) is not None
    }
    assert actual_merge_calls == {
        "production.rs": 1,
        "server/rest/mod.rs": 9,
    }

    rest_module = sources["server/rest/mod.rs"]
    expected_rest_module_block = """
mod commands;
mod devices;
mod dynamic_config;
mod notifications;
mod profiles;
mod script_ui;
mod settings;
mod state;
mod update;
""".lstrip()
    assert (
        rust_without_comments(rest_module)
        .lstrip()
        .startswith(f"{expected_rest_module_block}\nuse ")
    )
    rest_module_declarations = rust_top_level_matches(
        rest_module,
        r"(?m)^[ \t]*mod\s+((?:r#)?[A-Za-z_][A-Za-z0-9_]*)\s*;$",
    )
    assert (
        tuple(declaration.group(1) for declaration in rest_module_declarations)
        == REST_ROUTER_MODULES
    )
    assert (
        tuple(
            re.findall(
                r"\.\s*merge\s*\(\s*([a-z_][a-z0-9_]*)\s*::\s*router\s*\(\s*\)\s*\)",
                rest_module,
            )
        )
        == REST_ROUTER_MERGES
    )

    production = compact_rust(sources["production.rs"])
    assert (
        production.count(
            "let router = rest::router(rest_backend).merge(websocket.router());"
        )
        == 1
    )
    production_router_getter = rust_impl_method_body(
        sources["production.rs"],
        "ProductionRuntime",
        r"\bpub\s*\(\s*crate\s*\)\s+fn\s+router\s*\(\s*&\s*self\s*\)"
        r"\s*->\s*Router",
    )
    assert compact_rust(production_router_getter) == "self.router.clone()"

    fallback_pattern = re.compile(r"\.\s*fallback\s*\(")
    fallback_service_pattern = re.compile(r"\.\s*fallback_service\s*\(")
    assert {
        source_name: len(fallback_pattern.findall(source))
        for source_name, source in sources.items()
        if fallback_pattern.search(source) is not None
    } == {
        "server/static_files.rs": 1,
        "server/websocket.rs": 1,
    }
    assert {
        source_name: len(fallback_service_pattern.findall(source))
        for source_name, source in sources.items()
        if fallback_service_pattern.search(source) is not None
    } == {
        "server/router.rs": 1,
        "server/security.rs": 1,
    }
    assert ".fallback(serve_static)" in compact_rust(sources["server/static_files.rs"])
    assert ".fallback(websocket_method_not_allowed)" in compact_rust(websocket)

    method_fallback_pattern = re.compile(r"\.\s*method_not_allowed_fallback\s*\(")
    assert {
        source_name: len(method_fallback_pattern.findall(source))
        for source_name, source in sources.items()
        if method_fallback_pattern.search(source) is not None
    } == {"server/rest/mod.rs": 1}
    method_router_on_pattern = re.compile(r"\.\s*on\s*\(")
    assert {
        source_name: len(method_router_on_pattern.findall(source))
        for source_name, source in sources.items()
        if source_name in route_builder_sources
        and method_router_on_pattern.search(source) is not None
    } == {
        "server/rest/devices.rs": 2,
        "server/rest/settings.rs": 1,
        "server/rest/state.rs": 1,
        "server/websocket.rs": 1,
    }
    with_state_pattern = re.compile(r"\.\s*with_state\s*\(")
    assert {
        source_name: len(with_state_pattern.findall(source))
        for source_name, source in sources.items()
        if source_name in route_builder_sources
        and with_state_pattern.search(source) is not None
    } == {
        "server/rest/mod.rs": 1,
        "server/static_files.rs": 1,
        "server/websocket.rs": 1,
    }
    layer_pattern = re.compile(r"\.\s*layer\s*\(")
    assert {
        source_name: len(layer_pattern.findall(source))
        for source_name, source in sources.items()
        if source_name in route_builder_sources
        and layer_pattern.search(source) is not None
    } == {"server/security.rs": 1}

    public_router_calls = {
        source_name: len(re.findall(r"\bpublic_router\s*\(", source))
        for source_name, source in sources.items()
        if re.search(r"\bpublic_router\s*\(", source) is not None
    }
    assert public_router_calls == {
        "@rust/pokecon/src/lib.rs": 1,
        "lib.rs": 1,
        "server/router.rs": 1,
    }
    with_router_calls = {
        source_name: len(re.findall(r"\.\s*with_router\s*\(", source))
        for source_name, source in sources.items()
        if re.search(r"\.\s*with_router\s*\(", source) is not None
    }
    assert with_router_calls == {
        "@rust/pokecon/src/lib.rs": 1,
        "lib.rs": 1,
    }
    compact_application_source = compact_rust(sources["lib.rs"])
    for exact_ui_composition in (
        "let security = RequestSecurity::new(listen_address, ui.allow_tauri_origin);",
        "public_router(api, static_files, security)",
        "let app = ui_router(production.router(), static_files, listen_address, ui);",
        "let server = server.with_router(app);",
    ):
        assert compact_application_source.count(exact_ui_composition) == 1


def section(document: str, start: str, end: str) -> str:
    _prefix, separator, tail = document.partition(start)
    assert separator, f"missing section start: {start}"
    body, separator, _suffix = tail.partition(end)
    assert separator, f"missing section end after: {start}"
    return body


def test_packaged_worker_harness_is_store_cached_and_runtime_compile_free() -> None:
    flake = (REPOSITORY / "flake.nix").read_text()
    harness = section(
        flake,
        "workerPackageTestHarness =",
        "\n          cliHelpCheck =",
    )
    gate = section(
        flake,
        "workerPackageCheck =",
        "\n          uiPackageSoftwareRenderer =",
    )
    lifecycle = (REPOSITORY / "rust/pokecon/tests/lifecycle.rs").read_text()

    for required in (
        "rustPlatform.buildRustPackage",
        "dontCargoBuild = true;",
        "doCheck = true;",
        'checkType = "debug";',
        'POKECON_RESOURCE_PROVENANCE = "development";',
        "${installControlledCargoManifests}",
        "${sanitizeCargoCompilerEnvironment}",
        'RUSTC = "${rustToolchain}/bin/rustc";',
        'RUSTC_WRAPPER = "${pinnedRustcWrapper}";',
        '"--no-run"',
        '"worker_startup"',
        '"lifecycle"',
        '"script_runtime"',
        '"$out/bin/pokecon-worker-fault-fixture"',
    ):
        assert required in harness
    assert harness.count('"--test"') == 3
    assert harness.count("cargoTestFlags = [") == 1
    assert harness.count("cargoBuildFlags = [") == 0

    for required in (
        "${setupSourceGateEnvironment}",
        'harness_output="${workerPackageTestHarness}"',
        'canonical_harness="$(readlink -f -- "$harness_output")"',
        'export POKECON_TEST_WORKER_BINARY="$worker_binary"',
        'export POKECON_TEST_FAULT_WORKER_BINARY="$fault_worker_binary"',
        '"$worker_startup_harness" --nocapture',
        '"$lifecycle_harness" --nocapture',
        '"$script_runtime_harness"',
        "--exact --nocapture",
    ):
        assert required in gate
    for forbidden in (
        "${setupWorkdir}",
        "${desktopEnvironment}",
        "rustTaskInputs",
        "cargo test",
        "cargo build",
    ):
        assert forbidden not in gate
    assert gate.index('package_output="${self\'.packages.pokecon}"') < gate.index(
        'harness_output="${workerPackageTestHarness}"'
    )
    assert gate.index(
        'export POKECON_TEST_WORKER_BINARY="$worker_binary"'
    ) < gate.index('"$worker_startup_harness" --nocapture')
    assert "POKECON_TEST_FAULT_WORKER_BINARY" in lifecycle
    assert (
        len(
            re.findall(
                r'env!\(\s*"CARGO_BIN_EXE_pokecon-worker-fault-fixture"\s*\)',
                lifecycle,
            )
        )
        == 2
    )


def test_gate_keeps_real_packaged_modes_and_forbidden_bypasses_out() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()

    for forbidden in (
        "--exit-after-startup",
        "POKECON_WEB_DIR",
        "--web-dir",
        "EGL_PLATFORM",
        "GDK_GL",
        "GALLIUM_DRIVER",
        "MESA_LOADER_DRIVER_OVERRIDE",
        "WEBKIT_SKIA_ENABLE_CPU_RENDERING",
        "xdotool windowclose",
        "xdotool windowkill",
        'kill -TERM "$active_pid"',
        'kill "-$signal"',
        "--disable-compositing false",
    ):
        assert forbidden not in gate
    for required in (
        'application="$(readlink -f -- "$canonical_package/bin/pokecon")"',
        'run_mode web "$web_root" "$web_port"',
        'run_mode desktop "$desktop_root" "$desktop_port"',
        "env -i",
        'cd "$mode_root"',
        "--dynamic-config-language none",
        "--disable-compositing true",
        "--ui-desktop-close-behavior keep_backend",
        "dbus-run-session",
        '--config-file="$canonical_dbus_session_config"',
        "xvfb-run",
        "GSETTINGS_BACKEND=memory",
        "      WEBKIT_DISABLE_DMABUF_RENDERER=1",
        "WEBKIT_DISABLE_COMPOSITING_MODE=1",
        "LIBGL_ALWAYS_SOFTWARE=1",
        'LIBGL_DRIVERS_PATH="$mesa_renderer/lib/dri"',
        '__EGL_VENDOR_LIBRARY_FILENAMES="$mesa_renderer/share/glvnd/egl_vendor.d/50_mesa.json"',
        '"$project_python" -I -S "$canonical_pidfd_signal"',
        "timeout --signal=TERM --kill-after=1s 2s",
        "application.log",
        "POKECON-RUNTIME-0003.*Signal\\(Terminate\\)",
    ):
        assert required in gate
    assert "/etc/dbus-1/session.conf" not in gate
    identity_validation = 'fail "$mode published application identity is not live"'
    private_group_validation = '|| [ "$live_pgid" = "$gate_pgid" ]; then'
    publication = 'active_pgid="$live_pgid"'
    assert (
        gate.index(identity_validation)
        < gate.index(private_group_validation)
        < gate.index(publication)
    )
    assert 'active_pgid="$(ps -o pgid=' not in gate
    assert gate.count("dbus-run-session") == 1
    assert gate.count("xvfb-run") == 1
    assert gate.count("kill -0") == 4
    assert (
        gate.count('*) fail "EWMH close relay is outside an immutable store output" ;;')
        == 1
    )


def test_gate_publishes_and_guards_exact_live_application_identity() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    stat_parser = section(
        gate,
        "parse_process_stat() {",
        "\n}\n\nread_process_identity() {",
    )
    identity_reader = section(
        gate,
        "read_process_identity() {",
        "\n}\n\nlaunch_product() {",
    )
    launcher = section(
        gate,
        "launch_product() {",
        '\n}\n\nif [ "${1:-}" = __launch_product ]; then',
    )
    parent_launch = section(
        gate,
        "launch_mode() {",
        "\n}\n\nfetch() {",
    )
    identity_matcher = section(
        gate,
        "active_identity_matches() {",
        "\n}\n\nvalidate_process_identity() {",
    )
    identity_validator = section(
        gate,
        "validate_process_identity() {",
        "\n}\n\nvalidate_http_contract() {",
    )
    normal_stop = section(
        gate,
        "stop_mode_normally() {",
        "\n}\n\nrun_mode() {",
    )
    run_mode = section(gate, "run_mode() {", '\n}\n\nweb_root="$gate_root/web-mode"')

    for required in (
        'stat_tail="${stat_line##*) }"',
        'candidate_state="${stat_fields[0]}"',
        'candidate_pgid="${stat_fields[2]}"',
        'candidate_start_ticks="${stat_fields[19]}"',
        '[[ ! "$candidate_state" =~ ^[A-Za-z]$ ]]',
        "X | x | Z) return 1",
        '[[ ! "$candidate_pgid" =~ ^[0-9]+$ ]]',
        '[[ ! "$candidate_start_ticks" =~ ^[0-9]+$ ]]',
        '[ -n "$state_output" ]',
    ):
        assert required in stat_parser
    assert identity_reader.count('readlink -- "/proc/$requested_pid/exe"') == 2
    assert identity_reader.count('2>/dev/null <"/proc/$requested_pid/stat"') == 2
    assert '<"/proc/$requested_pid/stat" 2>/dev/null' not in identity_reader
    for required in (
        'first_executable" != "$first_canonical_executable',
        '[ ! -f "$first_canonical_executable" ]',
        'second_executable" != "$second_canonical_executable',
        '[ ! -f "$second_canonical_executable" ]',
        '"$second_pgid" != "$first_pgid"',
        '"$second_start_ticks" != "$first_start_ticks"',
        '"$second_executable" != "$first_executable"',
        '[ -z "$first_state" ]',
        '[ -n "$state_output" ]',
    ):
        assert required in identity_reader

    assert 'if [ "$#" -ne 12 ]' in launcher
    assert 'local identity_file="${12}"' in launcher
    assert "for _attempt in {1..100}" in launcher
    assert '"$identity_executable" = "$application"' in launcher
    assert "umask 077" in launcher
    assert 'chmod 0600 "$identity_temporary"' in launcher
    assert "WEBKIT_DISABLE_COMPOSITING_MODE" not in launcher
    assert gate.count("WEBKIT_DISABLE_COMPOSITING_MODE=1") == 1
    identity_rename = 'mv -f -- "$identity_temporary" "$identity_file"'
    pid_publication = 'printf \'%s\\n\' "$application_pid" >"$pid_file"'
    assert (
        launcher.index('"start_ticks=$identity_start_ticks"')
        < launcher.index(identity_rename)
        < launcher.index(pid_publication)
    )
    assert (
        gate.count('"$root/runtime" "$canonical_mesa_renderer" "$identity_file"') == 3
    )

    for required in (
        'local identity_file="$root/application.identity"',
        'mapfile -t published_identity <"$identity_file"',
        '"${published_identity[0]}" != pid=*',
        '"${published_identity[1]}" != state=*',
        '"${published_identity[2]}" != pgid=*',
        '"${published_identity[3]}" != start_ticks=*',
        '"${published_identity[4]}" != executable=*',
        '"$pid_record" != "$published_pid"',
        '"$live_pgid" != "$published_pgid"',
        '"$live_start_ticks" != "$published_start_ticks"',
        '"$live_executable" != "$published_executable"',
        '"$live_executable" != "$application"',
        '"$live_pgid" = "$gate_pgid"',
        'active_pid="$live_pid"',
        'active_pgid="$live_pgid"',
        'active_start_ticks="$live_start_ticks"',
        'active_executable="$live_executable"',
    ):
        assert required in parent_launch

    for required in (
        "read_process_identity",
        'case "$live_state" in',
        "X | x | Z) return 1",
        '"$live_pid" = "$active_pid"',
        '"$live_pgid" = "$active_pgid"',
        '"$live_start_ticks" = "$active_start_ticks"',
        '"$live_executable" = "$active_executable"',
        '"$live_executable" = "$application"',
    ):
        assert required in identity_matcher

    for required in (
        "local compositing_reexec_marker_count=0",
        "PCME_DESKTOP_COMPOSITING_CONFIGURED=1)",
        "PCME_DESKTOP_COMPOSITING_CONFIGURED=*)",
        'done <"/proc/$active_pid/environ"',
        'if [ "$mode" = desktop ]; then',
        'if [ "$compositing_reexec_marker_count" -ne 1 ]; then',
        'elif [ "$compositing_reexec_marker_count" -ne 0 ]; then',
        "desktop application did not preserve exactly one compositing reexec marker",
        "web application unexpectedly inherited a compositing reexec marker",
    ):
        assert identity_validator.count(required) == 1
    assert identity_validator.index(
        "active_identity_matches"
    ) < identity_validator.index('done <"/proc/$active_pid/environ"')
    assert (
        run_mode.index('launch_mode "$mode" "$root" "$port"')
        < run_mode.index('wait_for_readiness "$mode" "$port" "$root"')
        < run_mode.index('validate_process_identity "$mode"')
        < run_mode.index('validate_http_contract "$mode" "$port" "$root"')
    )

    signal_call = "if ! signal_process_if_identity_matches"
    assert normal_stop.count(signal_call) == 1
    assert 'TERM "$mode application"' in normal_stop
    assert '"$active_pid" "$active_pgid" "$active_start_ticks"' in normal_stop
    assert '"$active_executable"' in normal_stop
    assert "active_identity_matches" not in normal_stop
    assert "ps -o stat=" not in normal_stop
    assert normal_stop.count("process_identity_matches") == 4
    first_identity_poll = normal_stop.index("if ! process_identity_matches")
    assert normal_stop.index(signal_call) < first_identity_poll
    assert "reset_primary_identity_cache" in normal_stop[first_identity_poll:]
    assert "reset_active_supervisor_identity_cache" in normal_stop[first_identity_poll:]


def test_gate_failure_cleanup_revalidates_exact_process_identities() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    identity_matcher = section(
        gate,
        "process_identity_matches() {",
        "\n}\n\nsignal_process_if_identity_matches() {",
    )
    signal_guard = section(
        gate,
        "signal_process_if_identity_matches() {",
        "\n}\n\ncapture_expected_process_identity() {",
    )
    capture_helper = section(
        gate,
        "capture_expected_process_identity() {",
        "\n}\n\nreset_primary_identity_cache() {",
    )
    anchors = section(
        gate,
        "primary_group_anchor_matches() {",
        "\n}\n\nshow_failure_evidence() {",
    )
    launch = section(gate, "launch_mode() {", "\n}\n\nfetch() {")
    failure_cleanup = section(gate, "failure_cleanup() {", "\n}\n\non_exit() {")
    relay_start = section(
        gate,
        "start_close_request_relay() {",
        "\n}\n\nreap_close_relay_if_stopped() {",
    )
    relay_reap = section(
        gate,
        "reap_close_relay_if_stopped() {",
        "\n}\n\nclose_initial_window_and_wait() {",
    )
    relay_close = section(
        gate,
        "close_initial_window_and_wait() {",
        "\n}\n\ncapture_post_close_socket_baseline() {",
    )
    pidfd_input_validation = section(
        gate,
        'case "$pidfd_signal_input" in',
        '\ncase "$ewmh_close_relay_input" in',
    )

    for cached_field in (
        "active_supervisor_pgid=",
        "active_supervisor_start_ticks=",
        "active_supervisor_executable=",
        "active_group_leader=",
        "active_group_leader_pgid=",
        "active_group_leader_start_ticks=",
        "active_group_leader_executable=",
        "close_relay_supervisor_pgid=",
        "close_relay_supervisor_start_ticks=",
        "close_relay_supervisor_executable=",
    ):
        assert cached_field in gate
    for exact_field_match in (
        '"$live_pid" = "$expected_pid"',
        '"$live_pgid" = "$expected_pgid"',
        '"$live_start_ticks" = "$expected_start_ticks"',
        '"$live_executable" = "$expected_executable"',
    ):
        assert exact_field_match in identity_matcher
    assert "read_process_identity" in identity_matcher
    helper_invocation = '"$project_python" -I -S "$canonical_pidfd_signal"'
    assert helper_invocation in signal_guard
    assert (
        '"$signal" "$expected_pid" "$expected_pgid" "$expected_start_ticks"'
        in signal_guard
    )
    assert '"$expected_executable"' in signal_guard
    assert "process_identity_matches" not in signal_guard
    assert 'kill "-$signal"' not in signal_guard
    assert 'case "$helper_status" in' in signal_guard
    assert "75)" in signal_guard
    for immutable_helper_proof in (
        '[ ! -f "$pidfd_signal_input" ]',
        '[ -L "$pidfd_signal_input" ]',
        'canonical_pidfd_signal="$(readlink -f -- "$pidfd_signal_input")"',
        'expected_pidfd_signal="$(dirname -- "$script_path")/pidfd_signal.py"',
        '"$canonical_pidfd_signal" != "$pidfd_signal_input"',
        '"$canonical_pidfd_signal" != "$expected_pidfd_signal"',
        '"$store_directory"/*/*',
        "readonly canonical_pidfd_signal expected_pidfd_signal",
    ):
        assert immutable_helper_proof in pidfd_input_validation
    assert "TERM | KILL" in signal_guard
    assert "for ((_attempt = 0; _attempt < attempts; _attempt++))" in capture_helper
    assert '"$live_start_ticks" != "$observed_start_ticks"' in capture_helper
    assert '"$live_executable" = "$expected_executable"' in capture_helper

    assert 'setsid_executable="$(command -v setsid' in gate
    assert 'readlink -f -- "$setsid_executable"' in gate
    assert "readonly setsid_executable" in gate
    assert 'timeout_command="$(command -v timeout' in gate
    assert 'timeout_executable="$(readlink -f -- "$timeout_command"' in gate
    assert (
        '"$timeout_command" != "$store_directory/$timeout_store_output_name/bin/timeout"'
        in gate
    )
    assert '[ ! -f "$timeout_command" ]' in gate
    assert '[ ! -x "$timeout_command" ]' in gate
    assert "readonly timeout_command timeout_executable" in gate
    assert '"$store_directory"/*/*' in gate
    assert gate.count('"$setsid_executable" --fork --wait') == 2
    assert '"$spawned_supervisor" "$setsid_executable" "$mode supervisor"' in launch
    for assignment in (
        'active_supervisor="$captured_supervisor"',
        'active_supervisor_pgid="$captured_supervisor_pgid"',
        'active_supervisor_start_ticks="$captured_supervisor_start_ticks"',
        'active_supervisor_executable="$captured_supervisor_executable"',
    ):
        assert assignment in launch
    assert launch.index('active_executable="$live_executable"') < launch.index(
        '"$active_pgid" \\\n        leader_pid leader_state'
    )
    for group_leader_proof in (
        '[ -n "$leader_state" ]',
        '"$leader_pid" = "$active_pgid"',
        '"$leader_pgid" = "$active_pgid"',
        'active_group_leader="$leader_pid"',
        'active_group_leader_pgid="$leader_pgid"',
        'active_group_leader_start_ticks="$leader_start_ticks"',
        'active_group_leader_executable="$leader_executable"',
    ):
        assert group_leader_proof in launch

    assert anchors.count("primary_group_anchor_matches") == 2
    for anchor_proof in (
        '"$active_pid" "$active_pgid" "$active_start_ticks"',
        '"$active_group_leader" = "$active_pgid"',
        '"$active_group_leader_pgid" = "$active_pgid"',
        '"$active_group_leader_start_ticks" "$active_group_leader_executable"',
        "ps -e -o pid= -o pgid=",
        '"$live_pgid" = "$active_pgid"',
        'pid_output+=("$live_pid")',
        'pgid_output+=("$live_pgid")',
        'start_ticks_output+=("$live_start_ticks")',
        'executable_output+=("$live_executable")',
    ):
        assert anchor_proof in anchors

    guarded_cleanup = failure_cleanup
    assert "kill -TERM" not in guarded_cleanup
    assert "kill -KILL" not in guarded_cleanup
    assert 'kill -TERM -- "-$active_pgid"' not in failure_cleanup
    assert 'kill -KILL -- "-$active_pgid"' not in failure_cleanup
    assert "kill -0" not in guarded_cleanup
    for guarded_signal in (
        'TERM "X11 close-request relay supervisor"',
        'TERM "primary process-group member"',
        'KILL "primary process-group member"',
        'TERM "main setsid supervisor"',
        'KILL "main setsid supervisor"',
    ):
        assert guarded_signal in guarded_cleanup
    assert guarded_cleanup.count("process_identity_matches") >= 5
    assert (
        failure_cleanup.index("snapshot_primary_group_identities")
        < failure_cleanup.index("reset_primary_identity_cache")
        < failure_cleanup.index('TERM "main setsid supervisor"')
    )
    assert "reset_close_relay_identity_cache" in guarded_cleanup
    assert "reset_active_supervisor_identity_cache" in guarded_cleanup

    assert '"$timeout_command" --signal=TERM --kill-after=1s 10s' in relay_start
    assert '"$timeout_executable" --signal=TERM' not in relay_start
    assert '"$spawned_relay_supervisor" "$timeout_executable"' in relay_start
    for relay_cache in (
        'close_relay_supervisor="$captured_relay_supervisor"',
        'close_relay_supervisor_pgid="$captured_relay_pgid"',
        'close_relay_supervisor_start_ticks="$captured_relay_start_ticks"',
        'close_relay_supervisor_executable="$captured_relay_executable"',
    ):
        assert relay_cache in relay_start
    assert relay_start.count("reset_close_relay_identity_cache") == 2
    assert relay_reap.count("reset_close_relay_identity_cache") == 1
    assert relay_close.count("reset_close_relay_identity_cache") == 1


def test_secondary_cleanup_revalidates_exact_process_tree_identities() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    snapshot_inventory = section(
        gate,
        "capture_resource_snapshot_inventory() {",
        "\n}\n\nlaunch_mode() {",
    )
    parent_reader = section(
        gate,
        "read_process_parent_identity() {",
        "\n}\n\nlaunch_product() {",
    )
    cache_reset = section(
        gate,
        "reset_secondary_identity_cache() {",
        "\n}\n\nconsume_published_secondary_identity() {",
    )
    identity_consumer = section(
        gate,
        "consume_published_secondary_identity() {",
        "\n}\n\ncollect_descendants() {",
    )
    tree_snapshot = section(
        gate,
        "snapshot_secondary_tree_identities() {",
        "\n}\n\ncleanup_secondary_process_tree() {",
    )
    tree_cleanup = section(
        gate,
        "cleanup_secondary_process_tree() {",
        "\n}\n\nfailure_cleanup() {",
    )
    failure_cleanup = section(gate, "failure_cleanup() {", "\n}\n\non_exit() {")
    terminate_secondary = section(
        gate,
        "terminate_secondary_if_live() {",
        "\n}\n\nlaunch_same_session_secondary() {",
    )
    secondary_launch = section(
        gate,
        "launch_same_session_secondary() {",
        "\n}\n\nwait_for_reopened_surface_and_socket() {",
    )

    for snapshot_proof in (
        'local temporary_root="$root/tmp"',
        '[ ! -d "$temporary_root" ]',
        '[ -L "$temporary_root" ]',
        'canonical_temporary_root="$(readlink -f -- "$temporary_root")"',
        ': >"$output"',
        '[ ! -d "$snapshot" ]',
        '[ -L "$snapshot" ]',
        'canonical_snapshot="$(readlink -f -- "$snapshot")"',
        '"$(dirname -- "$canonical_snapshot")" != "$canonical_temporary_root"',
        '"$canonical_snapshot" != "$snapshot"',
        "-mindepth 1 -maxdepth 1 -name 'pokecon-verified-resources-*' -print",
        "| LC_ALL=C sort",
    ):
        assert snapshot_proof in snapshot_inventory
    assert snapshot_inventory.count("pokecon-verified-resources-*") == 1
    assert snapshot_inventory.count("LC_ALL=C sort") == 1
    assert snapshot_inventory.count("printf '%s\\n' \"${snapshot##*/}\"") == 1

    assert parent_reader.count("read_process_identity") == 2
    assert parent_reader.count('done 2>/dev/null <"/proc/$requested_pid/status"') == 2
    assert 'done <"/proc/$requested_pid/status" 2>/dev/null' not in parent_reader
    assert parent_reader.count("PPid:*)") == 2
    for parent_proof in (
        "((parent_identity_snapshot_one_ppid_count += 1))",
        "((parent_identity_snapshot_two_ppid_count += 1))",
        '[[ "$status_line" =~ ^PPid:[[:space:]]+([0-9]+)[[:space:]]*$ ]]',
        '[ "$parent_identity_snapshot_one_ppid_count" -ne 1 ]',
        '[ -z "$parent_identity_snapshot_one_state" ]',
        '[ "$parent_identity_snapshot_two_ppid_count" -ne 1 ]',
        '[ -z "$parent_identity_snapshot_two_state" ]',
        '"$parent_identity_snapshot_two_pid" != "$parent_identity_snapshot_one_pid"',
        '"$parent_identity_snapshot_two_ppid" != "$parent_identity_snapshot_one_ppid"',
        '"$parent_identity_snapshot_two_pgid" != "$parent_identity_snapshot_one_pgid"',
        '"$parent_identity_snapshot_two_start_ticks" != "$parent_identity_snapshot_one_start_ticks"',
        '"$parent_identity_snapshot_two_executable" != "$parent_identity_snapshot_one_executable"',
        'pid_output="$parent_identity_snapshot_two_pid"',
        'ppid_output="$parent_identity_snapshot_two_ppid"',
        'pgid_output="$parent_identity_snapshot_two_pgid"',
        'start_ticks_output="$parent_identity_snapshot_two_start_ticks"',
        'executable_output="$parent_identity_snapshot_two_executable"',
        '[ -n "$ppid_output" ]',
    ):
        assert parent_proof in parent_reader

    for cache_field in (
        "secondary_pid=",
        "secondary_pgid=",
        "secondary_start_ticks=",
        "secondary_executable=",
    ):
        assert cache_field in cache_reset
    for consumption_proof in (
        '[ ! -f "$identity_file" ]',
        '[ -L "$identity_file" ]',
        '"$(stat -c \'%a\' -- "$identity_file")" != 600',
        '[ ! -f "$pid_file" ]',
        '[ -L "$pid_file" ]',
        '"${#published_identity[@]}" -ne 5',
        '"${published_identity[0]}" != pid=*',
        '"${published_identity[1]}" != state=*',
        '"${published_identity[2]}" != pgid=*',
        '"${published_identity[3]}" != start_ticks=*',
        '"${published_identity[4]}" != executable=*',
        '"${#published_pid_record[@]}" -ne 1',
        '"${published_pid_record[0]}" != "$published_pid"',
        '"$published_pid" = "$active_pid"',
        '"$published_executable" != "$application"',
        'secondary_pid="$published_pid"',
        'secondary_pgid="$published_pgid"',
        'secondary_start_ticks="$published_start_ticks"',
        'secondary_executable="$published_executable"',
        'desktop_secondary_pid="$published_pid"',
    ):
        assert consumption_proof in identity_consumer

    assert tree_snapshot.count("process_identity_matches") == 4
    assert (
        tree_snapshot.count('"$parent_pid" "$parent_pgid" "$parent_start_ticks"') == 2
    )
    for snapshot_proof in (
        "local -A seen_pids=()",
        'pid_output+=("$secondary_pid")',
        'pgid_output+=("$secondary_pgid")',
        'start_ticks_output+=("$secondary_start_ticks")',
        'executable_output+=("$secondary_executable")',
        'seen_pids["$secondary_pid"]=1',
        "for proc_path in /proc/[0-9]*",
        "read_process_parent_identity",
        '[ "$candidate_ppid" != "$parent_pid" ]',
        '"${seen_pids[$candidate_pid]+present}"',
        '[ "${#pid_output[@]}" -ge 256 ]',
        'seen_pids["$candidate_pid"]=1',
        "the secondary root identity changed during tree discovery",
        'echo "ui-package-check: skipped secondary tree snapshot because $unsafe_reason"',
    ):
        assert snapshot_proof in tree_snapshot
    gate_lines = gate.splitlines()
    shellcheck_directive_indices = tuple(
        line_index
        for line_index, line in enumerate(gate_lines)
        if re.search(r"#\s*shellcheck\b", line, flags=re.IGNORECASE)
    )
    assert len(shellcheck_directive_indices) == 4
    canonical_sc2178_directive = "  # shellcheck disable=SC2178"
    assert (
        tuple(gate_lines[line_index] for line_index in shellcheck_directive_indices)
        == (canonical_sc2178_directive,) * 4
    )

    tree_snapshot_start = gate_lines.index("snapshot_secondary_tree_identities() {")
    tree_snapshot_end = gate_lines.index("cleanup_secondary_process_tree() {")
    expected_sc2178_targets = (
        '  local -n pid_output="$1"',
        '  local -n pgid_output="$2"',
        '  local -n start_ticks_output="$3"',
        '  local -n executable_output="$4"',
    )
    expected_sc2178_target_indices: list[int] = []
    for target in expected_sc2178_targets:
        target_indices = tuple(
            line_index
            for line_index in range(tree_snapshot_start + 1, tree_snapshot_end)
            if gate_lines[line_index] == target
        )
        assert len(target_indices) == 1
        expected_sc2178_target_indices.append(target_indices[0])
    assert tuple(
        line_index + 1 for line_index in shellcheck_directive_indices
    ) == tuple(expected_sc2178_target_indices)
    assert (
        "These caller-owned namerefs are indexed arrays initialized immediately below."
        in tree_snapshot
    )
    assert tree_snapshot.index(
        "a queued secondary parent identity changed before child discovery"
    ) < tree_snapshot.index("for proc_path in /proc/[0-9]*")
    assert tree_snapshot.index("for proc_path in /proc/[0-9]*") < tree_snapshot.index(
        "a queued secondary parent identity changed during child discovery"
    )
    assert tree_snapshot.rindex(
        '"$secondary_pid" "$secondary_pgid" "$secondary_start_ticks"'
    ) > tree_snapshot.index('while [ -z "$unsafe_reason" ]')

    term_signal = 'TERM "secondary process-tree member"'
    tuple_poll = "if process_identity_matches"
    kill_signal = 'KILL "secondary process-tree member"'
    assert tree_cleanup.count("signal_process_if_identity_matches") == 2
    assert "for _attempt in {1..30}" in tree_cleanup
    assert tree_cleanup.index(term_signal) < tree_cleanup.index(tuple_poll)
    assert tree_cleanup.index(tuple_poll) < tree_cleanup.index(kill_signal)
    assert tree_cleanup.count("reset_secondary_identity_cache") == 2
    assert "kill " not in tree_cleanup

    assert "cleanup_secondary_process_tree" in failure_cleanup
    assert "collect_descendants" not in failure_cleanup
    assert 'readlink -f -- "/proc/$secondary_pid/exe"' not in failure_cleanup
    assert 'kill -TERM "${processes[@]}"' not in failure_cleanup
    assert 'kill -KILL "${processes[@]}"' not in failure_cleanup
    assert terminate_secondary.strip() == "cleanup_secondary_process_tree"

    helper_evidence = (
        'printf \'%s\\n\' "$desktop_secondary_helper_status" >"$helper_status_file"'
    )
    consume_identity = (
        'consume_published_secondary_identity "$identity_file" "$pid_file"'
    )
    helper_status_decision = 'if [ "$desktop_secondary_helper_status" -ne 0 ]'
    assert (
        secondary_launch.index(helper_evidence)
        < secondary_launch.index(consume_identity)
        < secondary_launch.index(helper_status_decision)
    )
    clean_liveness = secondary_launch.rindex("if process_identity_matches")
    clean_failure = secondary_launch.index(
        'fail "same-session secondary remained live after its synchronous launch"'
    )
    exact_liveness = secondary_launch[clean_liveness:clean_failure]
    for tuple_field in (
        '"$secondary_pid"',
        '"$secondary_pgid"',
        '"$secondary_start_ticks"',
        '"$secondary_executable"',
        "terminate_secondary_if_live",
    ):
        assert tuple_field in exact_liveness
    assert secondary_launch.rindex("reset_secondary_identity_cache") > clean_failure
    assert (
        'local helper_status_file="$root/secondary.helper.status"' in secondary_launch
    )
    assert 'local identity_file="$root/secondary.identity"' in secondary_launch
    for snapshot_proof in (
        'local snapshots_before="$root/resource-snapshots.before-secondary"',
        'local snapshots_after="$root/resource-snapshots.after-secondary"',
        'capture_resource_snapshot_inventory "$root" "$snapshots_before"',
        'mapfile -t primary_snapshots <"$snapshots_before"',
        '"${#primary_snapshots[@]}" -ne 0',
        "exact-Nix desktop primary unexpectedly materialized a private resource snapshot",
        'capture_resource_snapshot_inventory "$root" "$snapshots_after"',
        'cmp -s -- "$snapshots_before" "$snapshots_after"',
        "--label resource-snapshots-before-secondary",
        "--label resource-snapshots-after-secondary",
        "same-session secondary changed the private resource snapshot inventory",
    ):
        assert snapshot_proof in secondary_launch
    assert secondary_launch.count("capture_resource_snapshot_inventory") == 2
    assert (
        secondary_launch.count('cmp -s -- "$snapshots_before" "$snapshots_after"') == 1
    )
    baseline_capture = secondary_launch.index(
        'capture_resource_snapshot_inventory "$root" "$snapshots_before"'
    )
    baseline_mapfile = secondary_launch.index(
        'mapfile -t primary_snapshots <"$snapshots_before"'
    )
    baseline_count = secondary_launch.index('"${#primary_snapshots[@]}" -ne 0')
    after_capture = secondary_launch.index(
        'capture_resource_snapshot_inventory "$root" "$snapshots_after"'
    )
    inventory_compare = secondary_launch.index(
        'cmp -s -- "$snapshots_before" "$snapshots_after"'
    )
    assert (
        baseline_capture
        < baseline_mapfile
        < baseline_count
        < secondary_launch.index("reset_secondary_identity_cache")
        < secondary_launch.index("timeout --signal=TERM --kill-after=2s 20s")
        < clean_liveness
        < after_capture
        < inventory_compare
        < secondary_launch.rindex("reset_secondary_identity_cache")
        < secondary_launch.index("validate_primary_continuity")
    )
    assert 'secondary_state="$(ps -o stat=' not in secondary_launch


def test_parent_identity_reader_executes_without_nameref_collisions() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    process_helpers, separator, _remainder = gate.partition("\n\nlaunch_product() {")
    assert separator
    fixture = r"""

fixture_pid=
fixture_write_fd=
cleanup_fixture() {
  trap - EXIT
  if [[ "$fixture_write_fd" =~ ^[0-9]+$ ]]; then
    exec {fixture_write_fd}>&-
    fixture_write_fd=
  fi
  if [[ "$fixture_pid" =~ ^[0-9]+$ ]]; then
    wait "$fixture_pid" 2>/dev/null || true
    fixture_pid=
  fi
}
trap cleanup_fixture EXIT

coproc STABLE_CHILD { IFS= read -r _fixture_stop; }
fixture_pid="$STABLE_CHILD_PID"
fixture_write_fd="${STABLE_CHILD[1]}"

expected_stat_line=
expected_state=
expected_pid="$fixture_pid"
expected_pgid=
expected_start_ticks=
expected_ppid=
expected_ppid_count=0
expected_executable=
IFS= read -r expected_stat_line 2>/dev/null <"/proc/$fixture_pid/stat"
parse_process_stat \
  "$fixture_pid" "$expected_stat_line" \
  expected_state expected_pgid expected_start_ticks
while IFS= read -r status_line; do
  case "$status_line" in
    PPid:*)
      ((expected_ppid_count += 1))
      [[ "$status_line" =~ ^PPid:[[:space:]]+([0-9]+)[[:space:]]*$ ]]
      expected_ppid="${BASH_REMATCH[1]}"
      ;;
  esac
done 2>/dev/null <"/proc/$fixture_pid/status"
[ "$expected_ppid_count" -eq 1 ]
expected_executable="$(readlink -f -- "/proc/$fixture_pid/exe")"

observed_pid=
observed_ppid=
observed_pgid=
observed_start_ticks=
observed_executable=
repeated_pid=
repeated_ppid=
repeated_pgid=
repeated_start_ticks=
repeated_executable=
read_process_parent_identity \
  "$fixture_pid" \
  observed_pid observed_ppid observed_pgid observed_start_ticks \
  observed_executable
read_process_parent_identity \
  "$fixture_pid" \
  repeated_pid repeated_ppid repeated_pgid repeated_start_ticks \
  repeated_executable

[ -n "$observed_pid" ]
[ -n "$observed_ppid" ]
[ -n "$observed_pgid" ]
[ -n "$observed_start_ticks" ]
[ -n "$observed_executable" ]
[ "$observed_pid" = "$fixture_pid" ]
[ "$observed_pid" = "$expected_pid" ]
[ "$observed_ppid" = "$expected_ppid" ]
[ "$observed_pgid" = "$expected_pgid" ]
[ "$observed_start_ticks" = "$expected_start_ticks" ]
[ "$observed_executable" = "$expected_executable" ]
[ "$repeated_pid" = "$observed_pid" ]
[ "$repeated_ppid" = "$observed_ppid" ]
[ "$repeated_pgid" = "$observed_pgid" ]
[ "$repeated_start_ticks" = "$observed_start_ticks" ]
[ "$repeated_executable" = "$observed_executable" ]

printf '%s\n' \
  "pid=$observed_pid" \
  "ppid=$observed_ppid" \
  "pgid=$observed_pgid" \
  "start_ticks=$observed_start_ticks" \
  "executable=$observed_executable"
"""
    bash = shutil.which("bash")
    assert bash is not None
    bash_executable = Path(bash).resolve(strict=True)
    # The repository-owned Bash helpers are the executable subject of this test.
    completed = subprocess.run(  # noqa: S603
        [str(bash_executable), "-c", process_helpers + fixture],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        text=True,
        timeout=10,
    )

    assert completed.returncode == 0, completed.stderr
    assert completed.stderr == ""
    identity = dict(
        line.split("=", maxsplit=1) for line in completed.stdout.splitlines()
    )
    assert identity.keys() == {"pid", "ppid", "pgid", "start_ticks", "executable"}
    assert all(identity.values())
    assert identity["executable"] == str(
        Path(identity["executable"]).resolve(strict=True)
    )


def test_api_evidence_matches_exact_continuity_and_normalized_parity_checks() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    continuity = section(
        gate,
        "validate_primary_continuity() {",
        "\n}\n\ncapture_initial_socket_baseline() {",
    )
    output = gate[gate.rindex("jq -n \\") :]
    state_fetch = 'fetch "$port" /api/state "$prefix-state" "$root"'
    state_http = 'require_json_response 200 "desktop $prefix state continuity endpoint"'
    state_pid = ".data.pid == $pid"
    settings_fetch = 'fetch "$port" /api/settings "$prefix-settings" "$root"'
    settings_http = (
        'require_json_response 200 "desktop $prefix settings continuity endpoint"'
    )
    assert continuity.index(state_fetch) < continuity.index(state_http)
    assert continuity.index(state_http) < continuity.index(state_pid)
    assert continuity.index(settings_fetch) < continuity.index(settings_http)
    for canonical_launch_check in (
        '.data.values["server.port"] == $port',
        '.data.values["server.bind_address"] == "127.0.0.1"',
        '.data.values["ui.desktop.close_behavior"] == "keep_backend"',
    ):
        assert continuity.index(settings_http) < continuity.index(
            canonical_launch_check
        )

    continuity_shape = """api_continuity: {
          state_endpoint: {
            json_http_200: true,
            original_primary_pid_reported: true
          },
          settings_endpoint: {
            json_http_200: true,
            canonical_launch_values_preserved: true
          }
        }"""
    parity_shape = """api_parity: {
      normalized_state_payload: true,
      normalized_settings_payload: true
    }"""
    assert output.count(continuity_shape) == 2
    assert output.count(parity_shape) == 1

    for location in (
        ".desktop.keep_backend_close.api_continuity",
        ".desktop.reopened.api_continuity",
    ):
        for exact_validation in (
            f"(({location} | keys)",
            f"(({location}.state_endpoint | keys)",
            f"{location}.state_endpoint.json_http_200 == true",
            f"{location}.state_endpoint.original_primary_pid_reported == true",
            f"(({location}.settings_endpoint | keys)",
            f"{location}.settings_endpoint.json_http_200 == true",
            f"{location}.settings_endpoint.canonical_launch_values_preserved == true",
        ):
            assert exact_validation in output
    assert output.count('== ["settings_endpoint", "state_endpoint"]') == 2
    assert output.count('== ["json_http_200", "original_primary_pid_reported"]') == 2
    assert (
        output.count('== ["canonical_launch_values_preserved", "json_http_200"]') == 2
    )
    assert "((.api_parity | keys)" in output
    assert '== ["normalized_settings_payload", "normalized_state_payload"]' in output
    assert ".api_parity.normalized_state_payload == true" in output
    assert ".api_parity.normalized_settings_payload == true" in output

    for normalization_proof in (
        "del(.data.pid, .data.revision)",
        '.data.values["server.port"]',
        '.data.values["server.bind_address"]',
        '.data.values["server.web_dir"]',
        '"$web_root/state.normalized.json"',
        '"$desktop_root/state.normalized.json"',
        '"$web_root/settings.normalized.json"',
        '"$desktop_root/settings.normalized.json"',
    ):
        assert normalization_proof in gate
    assert "api_unchanged" not in gate
    assert "api_parity: {state: true, settings: true}" not in gate
    assert ".api_parity.state" not in output
    assert ".api_parity.settings" not in output


def test_openapi_runtime_inventory_is_canonical_complete_and_valid_bash() -> None:
    gate_path = REPOSITORY / "scripts/integration/ui_package_check.sh"
    gate = gate_path.read_text()
    flake = (REPOSITORY / "flake.nix").read_text()
    openapi = json.loads((REPOSITORY / "api/openapi.json").read_text())
    operations = sorted(
        (path, method, operation["operationId"])
        for path, path_item in openapi["paths"].items()
        for method, operation in path_item.items()
    )
    assert operations == list(EXPECTED_OPENAPI_OPERATIONS)
    assert len(openapi["paths"]) == 15
    assert len(operations) == 16
    assert {method for _path, method, _operation_id in operations} == {
        "get",
        "patch",
        "post",
    }

    task_start = flake.index("uiPackageCheck = mkTask {")
    task_end = flake.index("\n        in\n", task_start)
    ui_task = flake[task_start:task_end]
    assert ui_task.count('"${source}/api/openapi.json"') == 1
    assert ui_task.index('"${source}/scripts/integration/ewmh_close_relay.py"') < (
        ui_task.index('"${source}/api/openapi.json"')
    )
    assert gate.count('if [ "$#" -ne 12 ]') == 2
    assert (
        "usage: ui_package_check.sh PACKAGE PYTHON STORE GATE_ROOT BASH "
        "CHILD_PATH DBUS_SESSION_CONFIG MESA_RENDERER PROC_SOCKET_EVIDENCE "
        "PIDFD_SIGNAL EWMH_CLOSE_RELAY OPENAPI" in gate
    )
    assert 'readonly pidfd_signal_input="${10}"' in gate
    assert 'readonly ewmh_close_relay_input="${11}"' in gate
    assert 'readonly openapi_input="${12}"' in gate

    unique_json_decoder_body = section(
        gate,
        "validate_unique_json_object_keys() {",
        "\n}\nreadonly -f validate_unique_json_object_keys"
        "\n\nbuild_openapi_operation_inventory() {",
    )
    canonical_unique_json_decoder_body = (
        "\n"
        '  if [ "$#" -ne 1 ]; then\n'
        "    return 2\n"
        "  fi\n"
        '  "$project_python" -I -S -c \'\n'
        "import json\n"
        "import sys\n"
        "\n"
        "\n"
        "def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:\n"
        "    result: dict[str, object] = {}\n"
        "    for key, value in pairs:\n"
        "        if key in result:\n"
        '            raise ValueError(f"duplicate JSON object key: {key}")\n'
        "        result[key] = value\n"
        "    return result\n"
        "\n"
        "\n"
        'with open(sys.argv[1], encoding="utf-8") as document:\n'
        "    json.load(document, object_pairs_hook=unique_object)\n"
        '\' "$1"'
    )
    assert unique_json_decoder_body == canonical_unique_json_decoder_body
    canonical_unique_json_decoder_definition = (
        "validate_unique_json_object_keys() {"
        + canonical_unique_json_decoder_body
        + "\n}"
    )
    canonical_unique_json_decoder_lock = (
        canonical_unique_json_decoder_definition
        + "\nreadonly -f validate_unique_json_object_keys"
    )
    unique_json_decoder_definition_pattern = re.compile(
        r"(?m)^[ \t]*(?:"
        r"validate_unique_json_object_keys[ \t]*\([ \t]*\)[ \t\r\n]*\{"
        r"|function[ \t]+validate_unique_json_object_keys[ \t\r\n]+\{"
        r"|function[ \t]+validate_unique_json_object_keys[ \t]*\([ \t]*\)"
        r"[ \t\r\n]*\{"
        r")"
    )
    unique_json_decoder_readonly_pattern = re.compile(
        r"(?m)^[ \t]*readonly[ \t]+-f[ \t]+"
        r"validate_unique_json_object_keys[ \t]*$"
    )

    def assert_single_locked_unique_json_decoder(candidate_gate: str) -> None:
        assert len(unique_json_decoder_definition_pattern.findall(candidate_gate)) == 1
        assert len(unique_json_decoder_readonly_pattern.findall(candidate_gate)) == 1
        assert candidate_gate.count(canonical_unique_json_decoder_lock) == 1
        assert (
            candidate_gate.count(
                canonical_unique_json_decoder_lock
                + "\n\nbuild_openapi_operation_inventory() {"
            )
            == 1
        )

    assert_single_locked_unique_json_decoder(gate)
    locked_unique_json_decoder_anchor = (
        canonical_unique_json_decoder_lock + "\n\nbuild_openapi_operation_inventory() {"
    )
    same_line_unique_json_decoder_override = gate.replace(
        locked_unique_json_decoder_anchor,
        canonical_unique_json_decoder_lock
        + "\n\nvalidate_unique_json_object_keys() { :; }\n\n"
        + "build_openapi_operation_inventory() {",
        1,
    )
    assert same_line_unique_json_decoder_override != gate
    assert (
        len(
            unique_json_decoder_definition_pattern.findall(
                same_line_unique_json_decoder_override
            )
        )
        == 2
    )
    try:
        assert_single_locked_unique_json_decoder(same_line_unique_json_decoder_override)
    except AssertionError:
        pass
    else:
        diagnostic = "same-line unique-JSON decoder override passed the audit"
        raise AssertionError(diagnostic)
    real_mode_call_anchor = (
        'run_mode web "$web_root" "$web_port"\n'
        'run_mode desktop "$desktop_root" "$desktop_port"'
    )
    distant_multiline_unique_json_decoder_override = gate.replace(
        real_mode_call_anchor,
        "function validate_unique_json_object_keys()\n{\n  :\n}\n\n"
        + real_mode_call_anchor,
        1,
    )
    assert distant_multiline_unique_json_decoder_override != gate
    assert (
        len(
            unique_json_decoder_definition_pattern.findall(
                distant_multiline_unique_json_decoder_override
            )
        )
        == 2
    )
    try:
        assert_single_locked_unique_json_decoder(
            distant_multiline_unique_json_decoder_override
        )
    except AssertionError:
        pass
    else:
        diagnostic = "distant multiline unique-JSON decoder override passed the audit"
        raise AssertionError(diagnostic)
    inventory_builder = section(
        gate,
        "build_openapi_operation_inventory() {",
        '\n}\n\nif [ ! -x "$project_python" ]; then',
    )
    input_validation = section(
        gate,
        'case "$openapi_input" in',
        '\ncase "$dbus_session_config" in',
    )
    for duplicate_proof in (
        "if key in result:",
        "raise ValueError",
        "json.load(document, object_pairs_hook=unique_object)",
    ):
        assert duplicate_proof in unique_json_decoder_body
    for immutable_proof in (
        '[ ! -f "$openapi_input" ]',
        '[ -L "$openapi_input" ]',
        '"$(dirname -- "$gate_source_output")" != "$store_directory"',
        '"$script_path" != "$gate_source_output/scripts/integration/ui_package_check.sh"',
        'canonical_openapi="$(readlink -f -- "$openapi_input")"',
        'expected_openapi="$gate_source_output/api/openapi.json"',
        '"$canonical_openapi" != "$openapi_input"',
        '"$canonical_openapi" != "$expected_openapi"',
        'validate_unique_json_object_keys "$canonical_openapi"',
        "build_openapi_operation_inventory",
        'sha256sum "$canonical_openapi"',
    ):
        assert immutable_proof in input_validation
    for extraction_proof in (
        ".paths",
        "to_entries[]",
        "safe_api_path",
        "supported_method",
        'error("OpenAPI contains an unsafe public path")',
        'error("OpenAPI path item contains an unsupported or malformed operation")',
        "sort_by([.path, .method])",
        "length == 16",
        "unique_by([.path, .method])",
        "unique_by(.path)",
        "unique_by(.operation_id)",
        'select(.path | startswith("/api/"))',
        'select(.path == "/ws" and .method == "get")',
        'select(.method == "get")',
        'select(.method == "post")',
        'select(.method == "patch")',
    ):
        assert extraction_proof in inventory_builder

    bash = shutil.which("bash")
    assert bash is not None
    bash_executable = Path(bash).resolve(strict=True)
    syntax = subprocess.run(  # noqa: S603
        [str(bash_executable), "-n", str(gate_path)],
        check=False,
        capture_output=True,
        text=True,
        timeout=10,
    )
    assert syntax.returncode == 0, syntax.stderr
    assert syntax.stderr == ""


def test_openapi_inventory_extractor_executes_and_rejects_unsafe_fixtures(
    tmp_path: Path,
) -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    unique_keys_function = (
        "validate_unique_json_object_keys() {"
        + section(
            gate,
            "validate_unique_json_object_keys() {",
            "\n}\nreadonly -f validate_unique_json_object_keys"
            "\n\nbuild_openapi_operation_inventory() {",
        )
        + "\n}\nreadonly -f validate_unique_json_object_keys"
    )
    inventory_function = (
        "build_openapi_operation_inventory() {"
        + section(
            gate,
            "build_openapi_operation_inventory() {",
            '\n}\n\nif [ ! -x "$project_python" ]; then',
        )
        + "\n}"
    )
    program = (
        "set -euo pipefail\n"
        'project_python="$1"\n'
        "fail() { printf '%s\\n' \"$*\" >&2; return 1; }\n"
        + unique_keys_function
        + "\n\n"
        + inventory_function
        + "\n\n"
        + 'validate_unique_json_object_keys "$2"\n'
        + 'build_openapi_operation_inventory "$2" "$3"\n'
    )
    bash = shutil.which("bash")
    jq = shutil.which("jq")
    assert bash is not None
    assert jq is not None
    bash_executable = Path(bash).resolve(strict=True)
    jq_executable = Path(jq).resolve(strict=True)
    python_executable = Path(sys.executable).resolve(strict=True)

    def run_fixture(
        document: Path,
        inventory: Path,
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(  # noqa: S603
            [
                str(bash_executable),
                "-c",
                program,
                "--",
                str(python_executable),
                str(document),
                str(inventory),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={"LC_ALL": "C", "PATH": str(jq_executable.parent)},
            text=True,
            timeout=10,
        )

    canonical = REPOSITORY / "api/openapi.json"
    canonical_inventory = tmp_path / "canonical-inventory.json"
    completed = run_fixture(canonical, canonical_inventory)
    assert completed.returncode == 0, completed.stderr
    assert completed.stderr == ""
    assert json.loads(canonical_inventory.read_text()) == [
        {"method": method, "operation_id": operation_id, "path": path}
        for path, method, operation_id in EXPECTED_OPENAPI_OPERATIONS
    ]

    duplicate = tmp_path / "duplicate.json"
    duplicate.write_text(
        '{"paths":{"/api/state":{"get":{"operationId":"one"},'
        '"get":{"operationId":"two"}}}}'
    )
    duplicate_result = run_fixture(duplicate, tmp_path / "duplicate-inventory.json")
    assert duplicate_result.returncode != 0
    assert "duplicate JSON object key: get" in duplicate_result.stderr

    unsafe_document = json.loads(canonical.read_text())
    unsafe_document["paths"]["/api/../state"] = unsafe_document["paths"].pop(
        "/api/state"
    )
    unsafe = tmp_path / "unsafe.json"
    unsafe.write_text(json.dumps(unsafe_document))
    unsafe_result = run_fixture(unsafe, tmp_path / "unsafe-inventory.json")
    assert unsafe_result.returncode != 0
    assert "OpenAPI contains an unsafe public path" in unsafe_result.stderr

    unsupported_document = json.loads(canonical.read_text())
    unsupported_document["paths"]["/api/state"]["parameters"] = []
    unsupported = tmp_path / "unsupported.json"
    unsupported.write_text(json.dumps(unsupported_document))
    unsupported_result = run_fixture(
        unsupported,
        tmp_path / "unsupported-inventory.json",
    )
    assert unsupported_result.returncode != 0
    assert (
        "OpenAPI path item contains an unsupported or malformed operation"
        in unsupported_result.stderr
    )


def test_ui_gate_matches_openapi_routes_and_methods() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    cors_success_forbidden_inventory = section(
        gate,
        "readonly -a cors_success_forbidden_response_headers=(\n",
        "\n)\nreadonly -a advertised_bare_options_forbidden_response_headers=(",
    )
    assert cors_success_forbidden_inventory.splitlines() == [
        "  Allow",
        "  Content-Length",
        "  Transfer-Encoding",
        "  Access-Control-Allow-Credentials",
        "  Access-Control-Expose-Headers",
        "  Access-Control-Max-Age",
    ]
    advertised_bare_options_forbidden_inventory = section(
        gate,
        "readonly -a advertised_bare_options_forbidden_response_headers=(\n",
        "\n)\nadvertised_bare_options_forbidden_response_headers_json=",
    )
    assert advertised_bare_options_forbidden_inventory.splitlines() == [
        "  Access-Control-Allow-Headers",
        "  Access-Control-Allow-Methods",
        "  Access-Control-Allow-Origin",
        "  Allow",
        "  Vary",
    ]
    cors_rejection_forbidden_inventory = section(
        gate,
        "readonly -a cors_rejection_forbidden_response_headers=(\n",
        "\n)\ncors_rejection_forbidden_response_headers_json=",
    )
    assert cors_rejection_forbidden_inventory.splitlines() == [
        "  Access-Control-Allow-Credentials",
        "  Access-Control-Allow-Headers",
        "  Access-Control-Allow-Methods",
        "  Access-Control-Allow-Origin",
        "  Access-Control-Expose-Headers",
        "  Access-Control-Max-Age",
        "  Allow",
        "  Vary",
    ]
    cors_rejection_forbidden_inventory_declaration = (
        "readonly -a cors_rejection_forbidden_response_headers=(\n"
        + cors_rejection_forbidden_inventory
        + "\n)"
    )
    cors_rejection_forbidden_inventory_serialization = (
        'cors_rejection_forbidden_response_headers_json="$(\n'
        "  printf '%s\\n' \"${cors_rejection_forbidden_response_headers[@]}\" \\\n"
        "    | jq -Rsc 'split(\"\\n\")[:-1]'\n"
        ')"\n'
        "readonly cors_rejection_forbidden_response_headers_json"
    )
    assert gate.count(cors_rejection_forbidden_inventory_declaration) == 1
    assert gate.count(cors_rejection_forbidden_inventory_serialization) == 1
    assert (
        gate.count(
            "readonly advertised_bare_options_forbidden_response_headers_json\n"
            + cors_rejection_forbidden_inventory_declaration
            + "\n"
            + cors_rejection_forbidden_inventory_serialization
            + "\nreadonly -a openapi_method_matrix=("
        )
        == 1
    )
    assert gate.count("readonly -a cors_success_forbidden_response_headers=(") == 1
    assert (
        gate.count("readonly -a advertised_bare_options_forbidden_response_headers=(")
        == 1
    )
    forbidden_header_inventories = {
        tuple(line.strip() for line in inventory.splitlines())
        for inventory in (
            cors_success_forbidden_inventory,
            advertised_bare_options_forbidden_inventory,
            cors_rejection_forbidden_inventory,
        )
    }
    assert len(forbidden_header_inventories) == 3
    assert gate.count("cors_rejection_forbidden_response_headers_json=") == 1
    assert gate.count("readonly cors_rejection_forbidden_response_headers_json") == 1
    assert (
        "advertised_bare_options_forbidden_response_headers"
        not in cors_rejection_forbidden_inventory_serialization
    )
    method_fetch = section(
        gate,
        "fetch_openapi_method_probe() {",
        "\n}\n\nrequire_head_method_rejection() {",
    )
    head_rejection = section(
        gate,
        "require_head_method_rejection() {",
        "\n}\n\nrequire_single_exact_header() {",
    )
    single_exact_header = section(
        gate,
        "require_single_exact_header() {",
        "\n}\n\nrequire_single_positive_decimal_header() {",
    )
    single_positive_decimal_header = section(
        gate,
        "require_single_positive_decimal_header() {",
        "\n}\n\nrequire_cors_preflight() {",
    )
    cors_preflight = section(
        gate,
        "require_cors_preflight() {",
        "\n}\nreadonly -f require_cors_preflight\n\nrequire_forbidden_preflight() {",
    )
    forbidden_preflight = section(
        gate,
        "require_forbidden_preflight() {",
        "\n}\nreadonly -f require_forbidden_preflight"
        "\n\nrequire_exact_allow_header() {",
    )
    exact_allow_header = section(
        gate,
        "require_exact_allow_header() {",
        "\n}\n\nprobe_cors_preflight_policy() {",
    )
    preflight_policy_probe = section(
        gate,
        "probe_cors_preflight_policy() {",
        "\n}\n\nprobe_openapi_method_matrix() {",
    )
    method_probe = section(
        gate,
        "probe_openapi_method_matrix() {",
        "\n}\n\nwait_for_readiness() {",
    )
    unknown_options_fetch = section(
        gate,
        "fetch_unknown_api_options_probe() {",
        "\n}\n\nrequire_resource_not_found_response() {",
    )
    resource_not_found_response = section(
        gate,
        "require_resource_not_found_response() {",
        "\n}\n\nrequire_head_resource_not_found_response() {",
    )
    head_resource_not_found_response = section(
        gate,
        "require_head_resource_not_found_response() {",
        "\n}\n\nprobe_unknown_api_boundary_matrix() {",
    )
    unknown_api_boundary_probe = section(
        gate,
        "probe_unknown_api_boundary_matrix() {",
        "\n}\n\nfetch_advertised_bare_options_probe() {",
    )
    advertised_bare_options_fetch = section(
        gate,
        "fetch_advertised_bare_options_probe() {",
        "\n}\n\nrequire_absent_response_header() {",
    )
    absent_response_header = section(
        gate,
        "require_absent_response_header() {",
        "\n}\n\nrequire_advertised_bare_options_rejection() {",
    )
    advertised_bare_options_rejection = section(
        gate,
        "require_advertised_bare_options_rejection() {",
        "\n}\n\nprobe_advertised_bare_options_security_boundary() {",
    )
    advertised_bare_options_probe = section(
        gate,
        "probe_advertised_bare_options_security_boundary() {",
        "\n}\n\nactive_identity_matches() {",
    )
    http_contract = section(
        gate,
        "validate_http_contract() {",
        "\n}\n\ncapture_socket_snapshot() {",
    )
    run_mode = section(gate, "run_mode() {", '\n}\n\nweb_root="$gate_root/web-mode"')
    mode_results = gate[
        gate.index('run_mode web "$web_root" "$web_port"') : gate.rindex("jq -n \\")
    ]
    output = gate[gate.rindex("jq -n \\") :]
    options_branch_start = method_fetch.index("    options)\n")
    options_branch_end = method_fetch.index(
        "    trace | connect)\n", options_branch_start
    )
    options_branch = method_fetch[options_branch_start:options_branch_end]
    non_options_branches = (
        method_fetch[:options_branch_start] + method_fetch[options_branch_end:]
    )

    for non_mutating_request_proof in (
        'case "$method" in',
        "get)",
        "head)",
        "post | put | patch | delete)",
        "options)",
        "trace | connect)",
        "--request HEAD",
        "--ignore-content-length",
        "--header 'Connection: close'",
        '--header "Origin: http://127.0.0.1:$port"',
        '--request "${method^^}"',
        "--header 'Content-Type: application/json'",
        "--header 'X-Pokecon-Request: 1'",
        "--data-binary '{'",
        "Access-Control-Request-Method: ${preflight_for^^}",
        "Access-Control-Request-Headers: content-type,x-pokecon-request",
    ):
        assert non_mutating_request_proof in method_fetch
    cross_origin_header = '--header "Origin: http://localhost:$port"'
    same_origin_header = '--header "Origin: http://127.0.0.1:$port"'
    compact_acrh = "Access-Control-Request-Headers: content-type,x-pokecon-request"
    spaced_acrh = "Access-Control-Request-Headers: content-type, x-pokecon-request"
    assert options_branch.count(cross_origin_header) == 1
    assert same_origin_header not in options_branch
    assert cross_origin_header not in non_options_branches
    assert non_options_branches.count(same_origin_header) == 4
    assert options_branch.count(compact_acrh) == 1
    assert spaced_acrh not in options_branch
    assert method_fetch.count('"http://127.0.0.1:$port$path"') == 1
    decoy_condition = 'if [ "$path:$preflight_for" = /api/state:connect ]; then'
    decoy_header = "--header 'X-Pokecon-Preflight-Method: GET'"
    assert method_fetch.count(decoy_condition) == 1
    assert method_fetch.count(decoy_header) == 1
    assert (
        method_fetch.index("Access-Control-Request-Method")
        < method_fetch.index(decoy_condition)
        < method_fetch.index(decoy_header)
    )
    assert re.search(r"(?m)^\s*--head\s*$", method_fetch) is None
    for forbidden_upgrade_header in (
        "Upgrade:",
        "Connection: Upgrade",
        "Sec-WebSocket",
    ):
        assert forbidden_upgrade_header not in method_fetch
    head_status_guard = '[ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 405 ]'
    head_content_type_guard = (
        'require_single_exact_header Content-Type application/json "$label"'
    )
    head_content_length_guard = (
        'require_single_positive_decimal_header Content-Length "$label"'
    )
    head_body_guard = '[ -s "$fetch_body" ]'
    for head_rejection_proof in (
        head_status_guard,
        head_content_type_guard,
        head_content_length_guard,
        head_body_guard,
        "returned response bytes after the HEAD headers",
    ):
        assert head_rejection.count(head_rejection_proof) == 1
    assert (
        head_rejection.index(head_status_guard)
        < head_rejection.index(head_content_type_guard)
        < head_rejection.index(head_content_length_guard)
        < head_rejection.index(head_body_guard)
    )
    assert "grep" not in head_rejection
    canonical_cors_preflight_body = (
        "\n"
        '  if [ "$#" -ne 2 ]; then\n'
        "    return 2\n"
        "  fi\n"
        '  local port="$1"\n'
        '  local label="$2"\n'
        '  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 204 ]; then\n'
        '    fail "$label returned curl status $fetch_status and HTTP $fetch_code"\n'
        "  fi\n"
        '  if [ -s "$fetch_body" ]; then\n'
        '    fail "$label returned a body"\n'
        "  fi\n"
        "  require_single_exact_header \\\n"
        '    Access-Control-Allow-Origin "http://localhost:$port" "$label"\n'
        "  require_single_exact_header \\\n"
        '    Access-Control-Allow-Methods "GET, PATCH, POST, OPTIONS" "$label"\n'
        "  require_single_exact_header \\\n"
        '    Access-Control-Allow-Headers "Content-Type, X-Pokecon-Request" "$label"\n'
        '  require_single_exact_header Vary Origin "$label"\n'
        "  local forbidden_name\n"
        '  for forbidden_name in "${cors_success_forbidden_response_headers[@]}"; do\n'
        '    require_absent_response_header "$forbidden_name" "$label"\n'
        "  done"
    )

    def assert_closed_cors_preflight_body(candidate: str) -> None:
        assert candidate == canonical_cors_preflight_body

    assert_closed_cors_preflight_body(cors_preflight)
    canonical_cors_preflight_definition = (
        "require_cors_preflight() {" + canonical_cors_preflight_body + "\n}"
    )
    canonical_cors_preflight_lock = (
        canonical_cors_preflight_definition + "\nreadonly -f require_cors_preflight"
    )
    cors_preflight_definition_pattern = re.compile(
        r"(?m)^[ \t]*(?:"
        r"require_cors_preflight[ \t]*\([ \t]*\)[ \t\r\n]*\{"
        r"|function[ \t]+require_cors_preflight[ \t\r\n]+\{"
        r"|function[ \t]+require_cors_preflight[ \t]*\([ \t]*\)"
        r"[ \t\r\n]*\{"
        r")"
    )
    cors_preflight_readonly_pattern = re.compile(
        r"(?m)^[ \t]*readonly[ \t]+-f[ \t]+require_cors_preflight[ \t]*$"
    )

    def assert_single_locked_cors_preflight(candidate_gate: str) -> None:
        assert len(cors_preflight_definition_pattern.findall(candidate_gate)) == 1
        assert len(cors_preflight_readonly_pattern.findall(candidate_gate)) == 1
        assert candidate_gate.count(canonical_cors_preflight_lock) == 1
        assert (
            candidate_gate.count(
                canonical_cors_preflight_lock + "\n\nrequire_forbidden_preflight() {"
            )
            == 1
        )

    assert_single_locked_cors_preflight(gate)
    locked_cors_preflight_anchor = (
        canonical_cors_preflight_lock + "\n\nrequire_forbidden_preflight() {"
    )
    late_noop_cors_preflight_override = gate.replace(
        locked_cors_preflight_anchor,
        canonical_cors_preflight_lock
        + "\n\nrequire_cors_preflight() {\n  :\n}\n\n"
        + "require_forbidden_preflight() {",
        1,
    )
    assert late_noop_cors_preflight_override != gate
    assert (
        len(
            cors_preflight_definition_pattern.findall(late_noop_cors_preflight_override)
        )
        == 2
    )
    try:
        assert_single_locked_cors_preflight(late_noop_cors_preflight_override)
    except AssertionError:
        pass
    else:
        diagnostic = "late no-op CORS preflight override passed the whole-gate audit"
        raise AssertionError(diagnostic)
    real_mode_calls = (
        'run_mode web "$web_root" "$web_port"\n'
        'run_mode desktop "$desktop_root" "$desktop_port"'
    )
    distant_multiline_cors_preflight_override = gate.replace(
        real_mode_calls,
        "require_cors_preflight()\n{\n  :\n}\n\n" + real_mode_calls,
        1,
    )
    assert distant_multiline_cors_preflight_override != gate
    assert (
        len(
            cors_preflight_definition_pattern.findall(
                distant_multiline_cors_preflight_override
            )
        )
        == 2
    )
    try:
        assert_single_locked_cors_preflight(distant_multiline_cors_preflight_override)
    except AssertionError:
        pass
    else:
        diagnostic = (
            "distant multiline no-op CORS preflight override passed "
            "the whole-gate audit"
        )
        raise AssertionError(diagnostic)
    later_route_positional_bypass = cors_preflight.replace(
        '  for forbidden_name in "${cors_success_forbidden_response_headers[@]}"; do\n',
        '  for forbidden_name in "${cors_success_forbidden_response_headers[@]}"; do\n'
        '    case "$2" in "web advertised CORS preflight OPTIONS /api/settings '
        'for GET" | "desktop advertised CORS preflight OPTIONS /api/settings '
        'for GET") continue ;; esac\n',
        1,
    )
    assert later_route_positional_bypass != cors_preflight
    try:
        assert_closed_cors_preflight_body(later_route_positional_bypass)
    except AssertionError:
        pass
    else:
        diagnostic = "later-route positional CORS bypass passed the audit"
        raise AssertionError(diagnostic)
    exact_cors_header_checks = (
        "require_single_exact_header \\\n"
        '    Access-Control-Allow-Origin "http://localhost:$port" "$label"',
        "require_single_exact_header \\\n"
        '    Access-Control-Allow-Methods "GET, PATCH, POST, OPTIONS" "$label"',
        "require_single_exact_header \\\n"
        '    Access-Control-Allow-Headers "Content-Type, X-Pokecon-Request" "$label"',
        'require_single_exact_header Vary Origin "$label"',
    )
    cors_transport_status_guard = (
        'if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 204 ]; then'
    )
    cors_body_guard = 'if [ -s "$fetch_body" ]; then'
    assert cors_preflight.count(cors_transport_status_guard) == 1
    assert cors_preflight.count(cors_body_guard) == 1
    assert (
        cors_preflight.index(cors_transport_status_guard)
        < cors_preflight.index(cors_body_guard)
        < cors_preflight.index(exact_cors_header_checks[0])
    )
    for exact_cors_header in exact_cors_header_checks:
        assert cors_preflight.count(exact_cors_header) == 1
    assert [
        cors_preflight.index(check) for check in exact_cors_header_checks
    ] == sorted(cors_preflight.index(check) for check in exact_cors_header_checks)
    cors_arity_guard = 'if [ "$#" -ne 2 ]; then\n    return 2\n  fi'
    assert cors_preflight.lstrip().startswith(cors_arity_guard)
    assert re.findall(
        r"(?m)^[ \t]*return(?:[ \t]+[^\n]*)?$",
        cors_preflight,
    ) == ["    return 2"]
    assert re.findall(r"(?m)^  local[^\n]*$", cors_preflight) == [
        '  local port="$1"',
        '  local label="$2"',
        "  local forbidden_name",
    ]
    assert (
        re.search(
            r"(?m)^[ \t]*(?:declare|readonly)\b",
            cors_preflight,
        )
        is None
    )
    assert (
        re.search(
            r"(?m)^[ \t]*[A-Za-z_][A-Za-z0-9_]*[+]?=\(",
            cors_preflight,
        )
        is None
    )
    canonical_cors_forbidden_inventory_reference = (
        '"${cors_success_forbidden_response_headers[@]}"'
    )
    assert cors_preflight.count(canonical_cors_forbidden_inventory_reference) == 1
    assert cors_preflight.count("cors_success_forbidden_response_headers") == 1
    label_reference = re.compile(r"\$(?:label\b|\{label\})")
    assert [
        line.strip()
        for line in cors_preflight.splitlines()
        if label_reference.search(line) is not None
    ] == [
        'fail "$label returned curl status $fetch_status and HTTP $fetch_code"',
        'fail "$label returned a body"',
        'Access-Control-Allow-Origin "http://localhost:$port" "$label"',
        'Access-Control-Allow-Methods "GET, PATCH, POST, OPTIONS" "$label"',
        'Access-Control-Allow-Headers "Content-Type, X-Pokecon-Request" "$label"',
        'require_single_exact_header Vary Origin "$label"',
        'require_absent_response_header "$forbidden_name" "$label"',
    ]
    cors_control_flow_lines = [
        line
        for line in cors_preflight.splitlines()
        if re.match(r"^[ \t]*(?:if|elif|while|until|for|case)\b", line) is not None
    ]
    assert all(label_reference.search(line) is None for line in cors_control_flow_lines)
    cors_forbidden_local = "local forbidden_name"
    cors_forbidden_loop = (
        'for forbidden_name in "${cors_success_forbidden_response_headers[@]}"; do'
    )
    cors_forbidden_check = 'require_absent_response_header "$forbidden_name" "$label"'
    for cors_forbidden_proof in (
        cors_forbidden_local,
        cors_forbidden_loop,
        cors_forbidden_check,
    ):
        assert cors_preflight.count(cors_forbidden_proof) == 1
    assert cors_preflight.count("require_single_exact_header") == 4
    assert (
        cors_preflight.index(exact_cors_header_checks[-1])
        < cors_preflight.index(cors_forbidden_local)
        < cors_preflight.index(cors_forbidden_loop)
        < cors_preflight.index(cors_forbidden_check)
    )
    for forbidden_method in ("HEAD", "PUT", "DELETE", "CONNECT"):
        assert forbidden_method not in cors_preflight
    canonical_forbidden_preflight_body = (
        "\n"
        '  if [ "$#" -ne 1 ]; then\n'
        "    return 2\n"
        "  fi\n"
        '  local label="$1"\n'
        '  if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 403 ]; then\n'
        '    fail "$label returned curl status $fetch_status and HTTP $fetch_code"\n'
        "  fi\n"
        '  require_single_exact_header Content-Type application/json "$label"\n'
        '  if ! jq -e . "$fetch_body" >/dev/null 2>&1; then\n'
        '    fail "$label did not return valid JSON"\n'
        "  fi\n"
        '  if ! validate_unique_json_object_keys "$fetch_body" 2>/dev/null; then\n'
        '    fail "$label did not return exactly one JSON document with unique object keys"\n'
        "  fi\n"
        "  if ! jq -e '\n"
        '    keys == ["error"]\n'
        '    and (.error | keys) == ["code", "fields", "message"]\n'
        '    and .error.code == "request_forbidden"\n'
        "    and .error.fields == null\n"
        '    and .error.message == "request validation failed"\n'
        '  \' "$fetch_body" >/dev/null; then\n'
        '    fail "$label did not return the canonical request_forbidden rejection"\n'
        "  fi\n"
        "  local forbidden_name\n"
        '  for forbidden_name in "${cors_rejection_forbidden_response_headers[@]}"; do\n'
        '    require_absent_response_header "$forbidden_name" "$label"\n'
        "  done"
    )

    def assert_closed_forbidden_preflight_body(candidate: str) -> None:
        assert candidate == canonical_forbidden_preflight_body

    assert_closed_forbidden_preflight_body(forbidden_preflight)
    canonical_forbidden_preflight_definition = (
        "require_forbidden_preflight() {" + canonical_forbidden_preflight_body + "\n}"
    )
    canonical_forbidden_preflight_lock = (
        canonical_forbidden_preflight_definition
        + "\nreadonly -f require_forbidden_preflight"
    )
    forbidden_preflight_definition_pattern = re.compile(
        r"(?m)^[ \t]*(?:"
        r"require_forbidden_preflight[ \t]*\([ \t]*\)[ \t\r\n]*\{"
        r"|function[ \t]+require_forbidden_preflight[ \t\r\n]+\{"
        r"|function[ \t]+require_forbidden_preflight[ \t]*\([ \t]*\)"
        r"[ \t\r\n]*\{"
        r")"
    )
    forbidden_preflight_readonly_pattern = re.compile(
        r"(?m)^[ \t]*readonly[ \t]+-f[ \t]+"
        r"require_forbidden_preflight[ \t]*$"
    )

    def assert_single_locked_forbidden_preflight(candidate_gate: str) -> None:
        assert len(forbidden_preflight_definition_pattern.findall(candidate_gate)) == 1
        assert len(forbidden_preflight_readonly_pattern.findall(candidate_gate)) == 1
        assert candidate_gate.count(canonical_forbidden_preflight_lock) == 1
        assert (
            candidate_gate.count(
                canonical_forbidden_preflight_lock
                + "\n\nrequire_exact_allow_header() {"
            )
            == 1
        )

    assert_single_locked_forbidden_preflight(gate)
    locked_forbidden_preflight_anchor = (
        canonical_forbidden_preflight_lock + "\n\nrequire_exact_allow_header() {"
    )
    late_noop_forbidden_preflight_override = gate.replace(
        locked_forbidden_preflight_anchor,
        canonical_forbidden_preflight_lock
        + "\n\nrequire_forbidden_preflight() {\n  :\n}\n\n"
        + "require_exact_allow_header() {",
        1,
    )
    assert late_noop_forbidden_preflight_override != gate
    assert (
        len(
            forbidden_preflight_definition_pattern.findall(
                late_noop_forbidden_preflight_override
            )
        )
        == 2
    )
    try:
        assert_single_locked_forbidden_preflight(late_noop_forbidden_preflight_override)
    except AssertionError:
        pass
    else:
        diagnostic = "late no-op forbidden-preflight override passed the audit"
        raise AssertionError(diagnostic)
    multiline_forbidden_preflight_override = gate.replace(
        real_mode_calls,
        "function require_forbidden_preflight()\n{\n  :\n}\n\n" + real_mode_calls,
        1,
    )
    assert multiline_forbidden_preflight_override != gate
    assert (
        len(
            forbidden_preflight_definition_pattern.findall(
                multiline_forbidden_preflight_override
            )
        )
        == 2
    )
    try:
        assert_single_locked_forbidden_preflight(multiline_forbidden_preflight_override)
    except AssertionError:
        pass
    else:
        diagnostic = "multiline forbidden-preflight override passed the audit"
        raise AssertionError(diagnostic)
    forbidden_preflight_transport_guard = (
        'if [ "$fetch_status" -ne 0 ] || [ "$fetch_code" != 403 ]; then'
    )
    forbidden_preflight_content_type_check = (
        'require_single_exact_header Content-Type application/json "$label"'
    )
    forbidden_preflight_json_syntax_check = (
        'if ! jq -e . "$fetch_body" >/dev/null 2>&1; then'
    )
    strict_body_decoder_call = (
        'validate_unique_json_object_keys "$fetch_body" 2>/dev/null'
    )
    strict_body_decoder_diagnostic = (
        'fail "$label did not return exactly one JSON document with unique object keys"'
    )
    forbidden_preflight_semantic_check = "if ! jq -e '\n"
    forbidden_preflight_semantic_diagnostic = (
        'fail "$label did not return the canonical request_forbidden rejection"'
    )
    forbidden_preflight_header_local = "local forbidden_name"
    forbidden_preflight_header_inventory_reference = (
        '"${cors_rejection_forbidden_response_headers[@]}"'
    )
    forbidden_preflight_header_loop = (
        "for forbidden_name in "
        + forbidden_preflight_header_inventory_reference
        + "; do"
    )
    forbidden_preflight_header_check = (
        'require_absent_response_header "$forbidden_name" "$label"'
    )
    for forbidden_preflight_singleton in (
        forbidden_preflight_transport_guard,
        forbidden_preflight_content_type_check,
        forbidden_preflight_json_syntax_check,
        strict_body_decoder_call,
        forbidden_preflight_semantic_check,
        forbidden_preflight_header_local,
        forbidden_preflight_header_inventory_reference,
        forbidden_preflight_header_loop,
        forbidden_preflight_header_check,
        'fail "$label returned curl status $fetch_status and HTTP $fetch_code"',
        'fail "$label did not return valid JSON"',
        strict_body_decoder_diagnostic,
        forbidden_preflight_semantic_diagnostic,
    ):
        assert forbidden_preflight.count(forbidden_preflight_singleton) == 1
    assert (
        forbidden_preflight.index(forbidden_preflight_transport_guard)
        < forbidden_preflight.index(forbidden_preflight_content_type_check)
        < forbidden_preflight.index(forbidden_preflight_json_syntax_check)
        < forbidden_preflight.index(strict_body_decoder_call)
        < forbidden_preflight.index(forbidden_preflight_semantic_check)
        < forbidden_preflight.index(forbidden_preflight_semantic_diagnostic)
        < forbidden_preflight.index(forbidden_preflight_header_local)
        < forbidden_preflight.index(forbidden_preflight_header_loop)
        < forbidden_preflight.index(forbidden_preflight_header_check)
    )
    forbidden_preflight_arity_guard = 'if [ "$#" -ne 1 ]; then\n    return 2\n  fi'
    assert forbidden_preflight.lstrip().startswith(forbidden_preflight_arity_guard)
    assert re.findall(
        r"(?m)^[ \t]*return(?:[ \t]+[^\n]*)?$",
        forbidden_preflight,
    ) == ["    return 2"]
    assert re.findall(r"(?m)^  local[^\n]*$", forbidden_preflight) == [
        '  local label="$1"',
        "  local forbidden_name",
    ]
    assert "require_json_response" not in forbidden_preflight
    for forbidden_preflight_excluded_responsibility in (
        "sha256",
        "body_sha256",
        "--slurpfile",
        "results_jsonl",
        "jq -cnS",
    ):
        assert forbidden_preflight_excluded_responsibility not in forbidden_preflight
    assert forbidden_preflight.count("cors_rejection_forbidden_response_headers") == 1
    assert forbidden_preflight.count("require_absent_response_header") == 1
    assert (
        "advertised_bare_options_forbidden_response_headers" not in forbidden_preflight
    )
    assert forbidden_preflight.count("jq -e") == 2
    assert '.error.code == "request_forbidden"' in forbidden_preflight
    assert '.error.message == "request validation failed"' in forbidden_preflight
    assert "(.error.message | length) > 0" not in forbidden_preflight
    for singleton_header_proof in (
        'local expected_name="$1"',
        'local expected_value="$2"',
        "local -a observed_values=()",
        'while IFS= read -r header_line || [ -n "$header_line" ]',
        "header_line=\"${header_line%$'\\r'}\"",
        '"${observed_name,,}" = "${expected_name,,}"',
        'observed_values+=("$observed_value")',
        '"${#observed_values[@]}" -ne 1',
        '"${observed_values[0]}" != "$expected_value"',
        "did not return exactly one $expected_name header with value $expected_value",
    ):
        assert singleton_header_proof in single_exact_header
    assert "grep" not in single_exact_header
    for positive_decimal_header_proof in (
        'local expected_name="$1"',
        'local label="$2"',
        "local -a observed_values=()",
        'while IFS= read -r header_line || [ -n "$header_line" ]',
        "header_line=\"${header_line%$'\\r'}\"",
        '"${observed_name,,}" = "${expected_name,,}"',
        'observed_values+=("$observed_value")',
        '"${#observed_values[@]}" -ne 1',
        '[[ ! "${observed_values[0]}" =~ ^[1-9][0-9]*$ ]]',
        "did not return exactly one $expected_name header with a strict positive "
        "base-10 integer value",
    ):
        assert positive_decimal_header_proof in single_positive_decimal_header
    assert "grep" not in single_positive_decimal_header
    assert "Content-Length" not in single_positive_decimal_header
    assert 'require_single_exact_header Allow "$1" "$2"' in exact_allow_header

    for preflight_policy_proof in (
        'local results="$root/cors-preflight-policy-results.json"',
        "sort_by([.path, .method])[]",
        "advertised_pair_count",
        "known_path_wrong_method",
        "unknown_path",
        "/api/not-in-openapi",
        "/api/settings:post",
        "/api/camera/retry:get",
        "/api/state:head",
        "/api/settings:put",
        "/api/state:delete",
        "/api/state:connect",
        "require_cors_preflight",
        "require_forbidden_preflight",
        'classification == "advertised_pair"',
        'classification == "known_path_wrong_method"',
        'classification == "unknown_path"',
        'response_class == "request_forbidden"',
        "closed-world CORS preflight results are incomplete or malformed",
    ):
        assert preflight_policy_proof in preflight_policy_probe
    assert preflight_policy_probe.count("fetch_openapi_method_probe") == 3
    assert preflight_policy_probe.count("require_cors_preflight") == 1
    assert preflight_policy_probe.count("require_forbidden_preflight") == 2
    advertised_preflight_emitter = section(
        preflight_policy_probe,
        "  while IFS=$'\\t' read -r path requested_method operation_id; do\n",
        '  done <"$pairs_tsv"',
    )
    known_wrong_preflight_emitter = section(
        preflight_policy_probe,
        '  for probe in "${known_wrong_preflights[@]}"; do\n',
        '  done\n\n  path="/api/not-in-openapi"',
    )
    unknown_path_preflight_emitter = section(
        preflight_policy_probe,
        '  path="/api/not-in-openapi"\n',
        "\n\n  if ! jq -e -S -s \\\n",
    )
    rejected_preflight_emitters = (
        known_wrong_preflight_emitter,
        unknown_path_preflight_emitter,
    )
    rejected_artifact_fields = (
        "body_sha256",
        "content_type",
        "error",
        "forbidden_response_headers_absent",
    )
    for rejected_preflight_emitter in rejected_preflight_emitters:
        for artifact_field in rejected_artifact_fields:
            assert artifact_field in rejected_preflight_emitter
        forbidden_validation = rejected_preflight_emitter.index(
            "require_forbidden_preflight"
        )
        body_digest = rejected_preflight_emitter.index(
            'body_sha256="$(sha256sum "$fetch_body" | cut -d \' \' -f 1)"'
        )
        response_slurp = rejected_preflight_emitter.index(
            '--slurpfile response "$fetch_body"'
        )
        row_emission = rejected_preflight_emitter.index("jq -cnS")
        assert forbidden_validation < body_digest < row_emission < response_slurp
        assert (
            rejected_preflight_emitter.count('--slurpfile response "$fetch_body"') == 1
        )
        assert rejected_preflight_emitter.count("error: $response[0].error") == 1
        assert (
            rejected_preflight_emitter.count(
                '"$cors_rejection_forbidden_response_headers_json"'
            )
            == 1
        )
    for artifact_field in rejected_artifact_fields:
        assert artifact_field not in advertised_preflight_emitter
    assert (
        preflight_policy_probe.count(
            'body_sha256="$(sha256sum "$fetch_body" | cut -d \' \' -f 1)"'
        )
        == 2
    )
    assert preflight_policy_probe.count('--slurpfile response "$fetch_body"') == 2
    assert preflight_policy_probe.count("error: $response[0].error") == 2
    cors_result_materialization_start = preflight_policy_probe.index(
        "  if ! jq -e -S -s \\\n"
    )
    cors_result_materialization = preflight_policy_probe[
        cors_result_materialization_start:
    ]
    canonical_cors_result_sort = (
        "      sort_by([.path, .requested_method, .classification])\n"
    )
    canonical_cors_result_materialization_prefix = (
        "  if ! jq -e -S -s \\\n"
        '    --slurpfile advertised "$openapi_operation_inventory" \\\n'
        '    --argjson advertised_count "$openapi_operation_count" \\\n'
        "    --argjson forbidden_response_headers_absent \\\n"
        '    "$cors_rejection_forbidden_response_headers_json" \\\n'
        '    --argjson known_wrong_count "$known_wrong_count" \'\n'
        + canonical_cors_result_sort
    )
    canonical_cors_result_materialization_suffix = (
        '    \' "$results_jsonl" >"$results"; then\n'
        '    fail "$mode closed-world CORS preflight evidence is incomplete or '
        'malformed"\n'
        "  fi"
    )

    def assert_canonical_cors_result_materialization(candidate: str) -> None:
        assert candidate.startswith(canonical_cors_result_materialization_prefix)
        assert candidate.count(canonical_cors_result_sort) == 1
        assert candidate.count("  if ! jq -e -S -s \\\n") == 1
        assert "jq -c" not in candidate
        assert candidate.endswith(canonical_cors_result_materialization_suffix)

    assert_canonical_cors_result_materialization(cors_result_materialization)
    reversed_cors_result_sort = (
        "      sort_by([.classification, .requested_method, .path])\n"
    )
    reversed_cors_result_materialization = cors_result_materialization.replace(
        canonical_cors_result_sort,
        reversed_cors_result_sort,
        1,
    )
    assert reversed_cors_result_materialization != cors_result_materialization
    assert reversed_cors_result_materialization.count(reversed_cors_result_sort) == 1
    assert canonical_cors_result_sort not in reversed_cors_result_materialization
    try:
        assert_canonical_cors_result_materialization(
            reversed_cors_result_materialization
        )
    except AssertionError:
        pass
    else:
        diagnostic = "reversed CORS result sort key passed the materialization audit"
        raise AssertionError(diagnostic)

    advertised_cors_result_keys = (
        "keys == [\n"
        '                "classification", "operation_id", "path", '
        '"requested_method",\n'
        '                "response_class", "status"\n'
        "              ]"
    )
    rejected_cors_result_keys = (
        "keys == [\n"
        '                "body_sha256", "classification", "content_type", '
        '"error",\n'
        '                "forbidden_response_headers_absent", "operation_id", '
        '"path",\n'
        '                "requested_method", "response_class", "status"\n'
        "              ]"
    )
    assert cors_result_materialization.count(advertised_cors_result_keys) == 1
    assert cors_result_materialization.count(rejected_cors_result_keys) == 1
    for rejection_schema_proof in (
        'select(.classification != "advertised_pair")\n'
        "            | .body_sha256] | unique | length) == 1",
        '(.body_sha256 | test("^[0-9a-f]{64}$"))',
        '.content_type == "application/json"',
        ".error == {\n"
        '                code: "request_forbidden",\n'
        "                fields: null,\n"
        '                message: "request validation failed"\n'
        "              }",
        ".forbidden_response_headers_absent\n"
        "                == $forbidden_response_headers_absent",
    ):
        assert cors_result_materialization.count(rejection_schema_proof) == 1
    assert (
        cors_result_materialization.count(
            '"$cors_rejection_forbidden_response_headers_json"'
        )
        == 1
    )

    for result_proof in (
        'local results="$root/openapi-method-results.json"',
        "group_by(.path)[]",
        'for method in "${openapi_method_matrix[@]}"',
        "classification=advertised",
        "classification=rejected",
        "classification=cors_preflight",
        "require_head_method_rejection",
        "require_exact_allow_header",
        "require_cors_preflight",
        "require_json_response 405",
        "expected_status=200",
        "expected_status=400",
        "expected_response_class=success",
        "expected_response_class=malformed_json",
        "expected_response_class=invalid_request",
        'keys == ["data"]',
        'keys == ["error"]',
        '(.error | keys) == ["code", "fields", "message"]',
        ".error.code == $expected_code",
        '.error.code == "method_not_allowed"',
        ".error.fields == null",
        "sort_by([.path, .method])",
        "unique_by([.path, .method])",
        'select(.classification == "advertised")',
        'select(.classification == "rejected")',
        'select(.classification == "cors_preflight")',
        '"method_not_allowed_head"',
        '"method_not_allowed"',
        '"cors_preflight"',
        '"allow", "classification", "method", "operation_id", "path"',
        "$result.allow == ([",
        "| ascii_upcase",
        '] | sort | join(", "))',
    ):
        assert result_proof in method_probe
    assert method_probe.index("group_by(.path)[]") < method_probe.index(
        "while IFS=$'\\t' read -r path advertised_methods preflight_for"
    )
    assert method_probe.count("fetch_openapi_method_probe") == 1
    assert method_probe.count("require_json_response") == 2
    for options_boundary_fetch_proof in (
        "--request OPTIONS",
        "bare)",
        "origin_only)",
        "requested_method_only)",
        '--header "Origin: http://127.0.0.1:$port"',
        "--header 'Access-Control-Request-Method: GET'",
        "unknown API OPTIONS probe escaped its fixed boundary variants",
    ):
        assert options_boundary_fetch_proof in unknown_options_fetch
    assert "complete_preflight" not in unknown_options_fetch
    for canonical_not_found_proof in (
        "require_json_response 404",
        'keys == ["error"]',
        '(.error | keys) == ["code", "fields", "message"]',
        '.error.code == "resource_not_found"',
        ".error.fields == null",
        '.error.message == "API resource was not found"',
        "grep -iE '<(!doctype[[:space:]]+html|html([[:space:]>]))'",
        "leaked the packaged SPA document",
    ):
        assert canonical_not_found_proof in resource_not_found_response
    for head_not_found_proof in (
        '"$fetch_code" != 404',
        "require_single_exact_header Content-Type application/json",
        'require_single_exact_header Content-Length "$expected_content_length"',
        '[ -s "$fetch_body" ]',
        "returned response bytes after the HEAD headers",
    ):
        assert head_not_found_proof in head_resource_not_found_response
    for unknown_boundary_proof in (
        'local path="/api/not-in-openapi"',
        'local results="$root/unknown-api-boundary-results.json"',
        'local expected_routed_count="$((openapi_method_count - 1))"',
        "local expected_options_count=4",
        'for method in "${openapi_method_matrix[@]}"',
        'if [ "$method" = options ]',
        "fetch_openapi_method_probe",
        "require_resource_not_found_response",
        "require_head_resource_not_found_response",
        "fetch_unknown_api_options_probe",
        "require_forbidden_preflight",
        "bare origin_only requested_method_only",
        '"complete_preflight"',
        '"routed_not_found"',
        '"security_rejected_options"',
        '"resource_not_found_head"',
        '"request_forbidden"',
        "unique_by([.method, .request_variant])",
        '.classification == "unknown_path"',
        '"unknown API boundary results are incomplete or malformed"',
    ):
        assert unknown_boundary_proof in unknown_api_boundary_probe
    assert unknown_api_boundary_probe.count("fetch_openapi_method_probe") == 1
    assert unknown_api_boundary_probe.count("fetch_unknown_api_options_probe") == 1
    assert unknown_api_boundary_probe.count("require_forbidden_preflight") == 1
    assert (
        unknown_api_boundary_probe.count('request_variant: "complete_preflight"') == 1
    )
    unknown_cors_projection = section(
        unknown_api_boundary_probe,
        "  if ! jq -ceS '\n",
        '\n  \' "$root/cors-preflight-policy-results.json" >>"$results_jsonl"; then',
    )
    assert (
        unknown_cors_projection.count(
            "    | {\n"
            '        classification: "security_rejected_options",\n'
            "        content_length: null,\n"
            '        method: "options",\n'
            "        path,\n"
            '        request_variant: "complete_preflight",\n'
            "        response_class,\n"
            "        status\n"
            "      }"
        )
        == 1
    )
    for cors_rejection_only_field in rejected_artifact_fields:
        assert cors_rejection_only_field not in unknown_cors_projection
    for bare_options_fetch_proof in (
        "--request OPTIONS",
        "--dump-header",
        "--output",
        '"http://127.0.0.1:$port$path"',
    ):
        assert bare_options_fetch_proof in advertised_bare_options_fetch
    for forbidden_bare_request_proof in (
        "--header",
        "--data",
        "Origin:",
        "Access-Control-Request-Method",
        "Access-Control-Request-Headers",
        "X-Pokecon-Preflight-Method",
    ):
        assert forbidden_bare_request_proof not in advertised_bare_options_fetch
    for absent_header_proof in (
        'local forbidden_name="$1"',
        'while IFS= read -r header_line || [ -n "$header_line" ]',
        "header_line=\"${header_line%$'\\r'}\"",
        '"${observed_name,,}" = "${forbidden_name,,}"',
        "returned forbidden $forbidden_name response header",
    ):
        assert absent_header_proof in absent_response_header
    for bare_options_rejection_proof in (
        '"$fetch_code" != 403',
        "require_single_exact_header Content-Type application/json",
        '[ ! -s "$fetch_body" ]',
        "did not return valid JSON",
        'if ! validate_unique_json_object_keys "$fetch_body" 2>/dev/null; then',
        "did not return exactly one JSON document with unique object keys",
        'keys == ["error"]',
        '(.error | keys) == ["code", "fields", "message"]',
        '.error.code == "request_forbidden"',
        ".error.fields == null",
        '.error.message == "request validation failed"',
        '"${advertised_bare_options_forbidden_response_headers[@]}"',
        "require_absent_response_header",
    ):
        assert bare_options_rejection_proof in advertised_bare_options_rejection
    assert advertised_bare_options_rejection.count(strict_body_decoder_call) == 1
    assert forbidden_preflight.count(strict_body_decoder_call) == 1
    assert gate.count(strict_body_decoder_call) == 2
    assert advertised_bare_options_rejection.count(strict_body_decoder_diagnostic) == 1
    assert forbidden_preflight.count(strict_body_decoder_diagnostic) == 1
    assert gate.count(strict_body_decoder_diagnostic) == 2
    assert (
        advertised_bare_options_rejection.index(
            'if ! jq -e . "$fetch_body" >/dev/null 2>&1; then'
        )
        < advertised_bare_options_rejection.index(strict_body_decoder_call)
        < advertised_bare_options_rejection.index('keys == ["error"]')
    )
    for bare_options_probe_proof in (
        'local paths="$root/advertised-bare-options-paths.txt"',
        'local results="$root/advertised-bare-options-results.json"',
        "'[.[].path] | unique[]'",
        '"$openapi_operation_inventory" >"$paths"',
        'while IFS= read -r path || [ -n "$path" ]',
        "fetch_advertised_bare_options_probe",
        "require_advertised_bare_options_rejection",
        '"$mode advertised bare OPTIONS $path"',
        "sort_by(.path)",
        "($operations[0] | map(.path) | unique) as $canonical_paths",
        "map(.path) == $canonical_paths",
        "unique_by(.path)",
        '.classification == "advertised_path_security_rejected"',
        '.request_variant == "bare"',
        ".access_control_request_method_header_sent == false",
        ".origin_header_sent == false",
        ".request_body_bytes == 0",
        'select(.path == "/ws")',
        'select(.path == "/api/settings")',
        "forbidden_response_headers_absent: $forbidden_response_headers_absent",
        "advertised bare OPTIONS results are incomplete or malformed",
    ):
        assert bare_options_probe_proof in advertised_bare_options_probe
    assert (
        advertised_bare_options_probe.count("fetch_advertised_bare_options_probe") == 1
    )
    assert (
        advertised_bare_options_probe.count("require_advertised_bare_options_rejection")
        == 1
    )
    assert "/api/not-in-openapi" not in advertised_bare_options_probe
    assert (
        '"$advertised_bare_options_forbidden_response_headers_json"'
        in advertised_bare_options_probe
    )
    assert "cors_rejection_forbidden_response_headers_json" not in (
        advertised_bare_options_probe
    )
    assert "advertised_bare_options_forbidden_response_headers_json" not in (
        preflight_policy_probe
    )
    assert "readonly -a openapi_method_matrix=(" in gate
    assert "get head post put patch delete options trace connect" in gate
    assert (
        'openapi_method_probe_count="$((openapi_path_count * openapi_method_count))"'
        in gate
    )
    assert (
        "openapi_rest_path_count=\"$(\n  jq -er '[.[].path "
        '| select(startswith("/api/"))] | unique | length\'' in gate
    )
    assert (
        "openapi_websocket_path_count=\"$(\n  jq -er '[.[].path "
        '| select(. == "/ws")] | unique | length\'' in gate
    )
    assert (
        http_contract.count('probe_openapi_method_matrix "$mode" "$port" "$root"') == 1
    )
    assert (
        http_contract.count('probe_cors_preflight_policy "$mode" "$port" "$root"') == 1
    )
    assert (
        http_contract.count('probe_unknown_api_boundary_matrix "$mode" "$port" "$root"')
        == 1
    )
    assert (
        http_contract.count(
            'probe_advertised_bare_options_security_boundary "$mode" "$port" "$root"'
        )
        == 1
    )
    assert run_mode.count('validate_http_contract "$mode" "$port" "$root"') == 1
    assert gate.count('run_mode web "$web_root" "$web_port"') == 1
    assert gate.count('run_mode desktop "$desktop_root" "$desktop_port"') == 1

    for parity_proof in (
        '"$web_root/openapi-method-results.json"',
        '"$desktop_root/openapi-method-results.json"',
        "cmp -s",
        "diff -u --label web-openapi-methods --label desktop-openapi-methods",
        'fail "Web and desktop OpenAPI exact method-result inventories differ"',
        "openapi_method_results_sha256",
        '"$web_root/cors-preflight-policy-results.json"',
        '"$desktop_root/cors-preflight-policy-results.json"',
        "diff -u --label web-cors-preflight-policy --label desktop-cors-preflight-policy",
        'fail "Web and desktop closed-world CORS preflight results differ"',
        "cors_preflight_policy_results",
        "cors_preflight_policy_results_sha256",
        '"$web_root/unknown-api-boundary-results.json"',
        '"$desktop_root/unknown-api-boundary-results.json"',
        "diff -u --label web-unknown-api-boundary --label desktop-unknown-api-boundary",
        'fail "Web and desktop unknown API boundary results differ"',
        "unknown_api_boundary_results",
        '"$web_root/advertised-bare-options-results.json"',
        '"$desktop_root/advertised-bare-options-results.json"',
        "--label web-advertised-bare-options",
        "--label desktop-advertised-bare-options",
        'fail "Web and desktop advertised bare OPTIONS security results differ"',
        "advertised_bare_options_results",
        "advertised_bare_options_results_sha256",
        "web_advertised_bare_options_probe_count",
        "desktop_advertised_bare_options_probe_count",
    ):
        assert parity_proof in mode_results
    cors_policy_parity_block = (
        "if ! cmp -s \\\n"
        '  "$web_root/cors-preflight-policy-results.json" \\\n'
        '  "$desktop_root/cors-preflight-policy-results.json"; then\n'
        "  diff -u --label web-cors-preflight-policy "
        "--label desktop-cors-preflight-policy \\\n"
        '    "$web_root/cors-preflight-policy-results.json" \\\n'
        '    "$desktop_root/cors-preflight-policy-results.json" >&2 || true\n'
        '  fail "Web and desktop closed-world CORS preflight results differ"\n'
        "fi\n"
    )
    cors_policy_digest_command = (
        'cors_preflight_policy_results_sha256="$(\n'
        '  sha256sum "$web_root/cors-preflight-policy-results.json" '
        "| cut -d ' ' -f 1\n"
        ')"\n'
    )
    assert mode_results.count(cors_policy_parity_block) == 1
    assert mode_results.count(cors_policy_digest_command) == 1
    assert mode_results.index(cors_policy_parity_block) + len(
        cors_policy_parity_block
    ) <= mode_results.index(cors_policy_digest_command)
    assert (
        'sha256sum "$desktop_root/cors-preflight-policy-results.json"'
        not in mode_results
    )
    cors_policy_final_args = (
        "  --argjson cors_preflight_policy_results "
        '"$cors_preflight_policy_results" \\\n'
        "  --arg cors_preflight_policy_results_sha256 "
        '"$cors_preflight_policy_results_sha256" \\\n'
    )
    assert output.count(cors_policy_final_args) == 1
    cors_rejection_inventory_final_arg = (
        "  --argjson cors_rejection_forbidden_response_headers \\\n"
        '  "$cors_rejection_forbidden_response_headers_json" \\\n'
    )
    assert output.count(cors_rejection_inventory_final_arg) == 1
    assert output.index(cors_rejection_inventory_final_arg) < output.index(
        cors_policy_final_args
    )
    assert (
        "advertised_bare_options_forbidden_response_headers_json"
        not in cors_rejection_inventory_final_arg
    )
    assert (
        output.count(
            "  --arg cors_preflight_policy_results_sha256 "
            '"$cors_preflight_policy_results_sha256" \\\n'
        )
        == 1
    )
    cors_policy_output = section(
        output,
        "      cors_preflight_policy: {\n",
        "\n      },\n      bare_options_security_boundary: {",
    )
    cors_policy_result_fields = (
        "        results: $cors_preflight_policy_results,\n"
        "        results_sha256: $cors_preflight_policy_results_sha256,\n"
        "        unknown_path_count: $cors_preflight_unknown_path_count"
    )
    assert cors_policy_output.count(cors_policy_result_fields) == 1
    assert (
        cors_policy_output.count(
            "results_sha256: $cors_preflight_policy_results_sha256"
        )
        == 1
    )
    assert output.count("$cors_preflight_policy_results_sha256") == 2
    cors_policy_schema_keys = (
        "      and ((.api_inventory.cors_preflight_policy | keys) == [\n"
        '        "advertised_pair_count", "allowed_headers", "allowed_methods",\n'
        '        "known_path_wrong_method_count", "mode_results_byte_identical",\n'
        '        "probe_count_per_mode", "results", "results_sha256", '
        '"unknown_path_count"\n'
        "      ])\n"
    )
    assert output.count(cors_policy_schema_keys) == 1
    assert (
        output.count(
            "      and (.api_inventory.cors_preflight_policy.results_sha256\n"
            '        | test("^[0-9a-f]{64}$"))\n'
        )
        == 1
    )
    final_cors_result_validator = section(
        output,
        "      and ([.api_inventory.cors_preflight_policy.results[]\n"
        '        | select(.classification == "advertised_pair")] | length) == 16',
        "\n      and ((.api_inventory.unknown_api_boundary | keys) == [",
    )
    final_advertised_cors_result_keys = (
        "keys == [\n"
        '            "classification", "operation_id", "path", '
        '"requested_method",\n'
        '            "response_class", "status"\n'
        "          ]"
    )
    final_rejected_cors_result_keys = (
        "keys == [\n"
        '            "body_sha256", "classification", "content_type", '
        '"error",\n'
        '            "forbidden_response_headers_absent", "operation_id", '
        '"path",\n'
        '            "requested_method", "response_class", "status"\n'
        "          ]"
    )
    assert final_cors_result_validator.count(final_advertised_cors_result_keys) == 1
    assert final_cors_result_validator.count(final_rejected_cors_result_keys) == 1
    for final_rejection_schema_proof in (
        'select(.classification != "advertised_pair")\n'
        "        | .body_sha256] | unique | length) == 1",
        '(.body_sha256 | test("^[0-9a-f]{64}$"))',
        '.content_type == "application/json"',
        ".error == {\n"
        '            code: "request_forbidden",\n'
        "            fields: null,\n"
        '            message: "request validation failed"\n'
        "          }",
        ".forbidden_response_headers_absent\n"
        "            == $cors_rejection_forbidden_response_headers",
    ):
        assert final_cors_result_validator.count(final_rejection_schema_proof) == 1

    for final_evidence_proof in (
        "api_inventory: {",
        "canonical_openapi_sha256: $canonical_openapi_sha256",
        "advertised_operation_count: $advertised_operation_count",
        "advertised_rest_operation_count: $advertised_rest_operation_count",
        "advertised_websocket_operation_count: $advertised_websocket_operation_count",
        "canonical_path_count: $canonical_path_count",
        "standard_method_count: $standard_method_count",
        "method_matrix_probe_count: $method_matrix_probe_count",
        "rejected_method_count: $rejected_method_count",
        "cors_preflight_count: $cors_preflight_count",
        "cors_preflight_policy: {",
        'allowed_headers: ["Content-Type", "X-Pokecon-Request"]',
        'allowed_methods: ["GET", "PATCH", "POST", "OPTIONS"]',
        "advertised_pair_count: $cors_preflight_advertised_pair_count",
        "known_path_wrong_method_count: $cors_preflight_known_wrong_count",
        "probe_count_per_mode: $cors_preflight_policy_probe_count",
        "results: $cors_preflight_policy_results",
        "results_sha256: $cors_preflight_policy_results_sha256",
        "unknown_path_count: $cors_preflight_unknown_path_count",
        "bare_options_security_boundary: {",
        "advertised_path_count: ($advertised_bare_options_results | length)",
        "desktop_probe_count: $desktop_advertised_bare_options_probe_count",
        "rest_path_count: $advertised_bare_options_rest_path_count",
        "results: $advertised_bare_options_results",
        "results_sha256: $advertised_bare_options_results_sha256",
        "web_probe_count: $web_advertised_bare_options_probe_count",
        "websocket_path_count: $advertised_bare_options_websocket_path_count",
        "unknown_api_boundary: {",
        "desktop_probe_count: $desktop_unknown_api_boundary_probe_count",
        "options_security_rejection_count: $unknown_api_options_rejection_count",
        'path: "/api/not-in-openapi"',
        "probe_count_per_mode: ($unknown_api_boundary_results | length)",
        "results: $unknown_api_boundary_results",
        "routed_method_count: $unknown_api_routed_method_count",
        "web_probe_count: $web_unknown_api_boundary_probe_count",
        "web_method_probe_count: $web_method_probe_count",
        "desktop_method_probe_count: $desktop_method_probe_count",
        "method_results_sha256: $method_results_sha256",
        "mode_results_byte_identical: true",
        'allow_policy: "openapi-advertised-methods-only-cors-options-excluded"',
        'implicit_head_policy: "explicit-route-405-empty-body"',
        'options_policy: "advertised-pair-preflight-204-other-preflight-403"',
        'probe_scope: "bidirectional-exact-method-complement-non-mutating"',
        "((.api_inventory | keys)",
        ".api_inventory.advertised_operation_count == 16",
        ".api_inventory.advertised_rest_operation_count == 15",
        ".api_inventory.advertised_websocket_operation_count == 1",
        ".api_inventory.canonical_path_count == 15",
        ".api_inventory.standard_method_count == 9",
        ".api_inventory.method_matrix_probe_count == 135",
        ".api_inventory.rejected_method_count == 104",
        ".api_inventory.cors_preflight_count == 15",
        ".api_inventory.cors_preflight_policy.advertised_pair_count == 16",
        ".api_inventory.cors_preflight_policy.known_path_wrong_method_count == 6",
        ".api_inventory.cors_preflight_policy.unknown_path_count == 1",
        ".api_inventory.cors_preflight_policy.probe_count_per_mode == 23",
        ".api_inventory.cors_preflight_policy.results_sha256",
        ".api_inventory.bare_options_security_boundary.advertised_path_count == 15",
        ".api_inventory.bare_options_security_boundary.desktop_probe_count == 15",
        ".api_inventory.bare_options_security_boundary.rest_path_count == 14",
        ".api_inventory.bare_options_security_boundary.web_probe_count == 15",
        ".api_inventory.bare_options_security_boundary.websocket_path_count == 1",
        'select(.path_kind == "websocket" and .path == "/ws")',
        'select(.path == "/api/settings")',
        '.classification == "advertised_path_security_rejected"',
        ".forbidden_response_headers_absent == [",
        ".access_control_request_method_header_sent == false",
        ".origin_header_sent == false",
        ".request_body_bytes == 0",
        ".api_inventory.unknown_api_boundary.desktop_probe_count == 12",
        ".api_inventory.unknown_api_boundary.options_security_rejection_count == 4",
        ".api_inventory.unknown_api_boundary.probe_count_per_mode == 12",
        ".api_inventory.unknown_api_boundary.routed_method_count == 8",
        ".api_inventory.unknown_api_boundary.web_probe_count == 12",
        'select(.classification == "routed_not_found")',
        'select(.classification == "security_rejected_options")',
        'and .request_variant == "complete_preflight"',
        ".api_inventory.web_method_probe_count == 135",
        ".api_inventory.desktop_method_probe_count == 135",
        ".api_inventory.mode_results_byte_identical == true",
        ".api_inventory.allow_policy",
        ".api_inventory.bare_options_policy",
        '"outer-security-403-before-inner-rest-websocket-dispatch"',
    ):
        assert final_evidence_proof in output
    assert '(keys == [\n        "api_inventory", "api_parity"' in output
    assert output.count("api_parity: {") == 1
    assert "api_unchanged" not in output
    assert "response_schema_conformance" not in output


def test_openapi_preflight_request_builder_executes_browser_shaped_cross_origin(
    tmp_path: Path,
) -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    assert gate.count("fetch_openapi_method_probe() {") == 1
    fetch_function = (
        "fetch_openapi_method_probe() {"
        + section(
            gate,
            "fetch_openapi_method_probe() {",
            "\n}\n\nrequire_head_method_rejection() {",
        )
        + "\n}"
    )
    assert "FAKE_CURL_" not in fetch_function

    bash = shutil.which("bash")
    assert bash is not None
    fixture_bin = tmp_path / "bin"
    fixture_bin.mkdir()
    fake_curl = fixture_bin / "curl"
    fake_curl_source = (
        f"#!{bash}\n"
        r"""set -euo pipefail
argv_log="${FAKE_CURL_ARGV_LOG:?missing fake curl argv log}"
printf '%s\0' "$@" >"$argv_log"
case "${FAKE_CURL_RESPONSE_KIND:-canonical}" in
  canonical) ;;
  *) exit 64 ;;
esac
dump_header=
output=
write_out=
target_count=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --silent | --show-error | --globoff | --ignore-content-length)
      shift
      ;;
    --noproxy | --connect-timeout | --max-time | --request | --header | --data-binary)
      [ "$#" -ge 2 ] || exit 64
      shift 2
      ;;
    --dump-header)
      [ "$#" -ge 2 ] || exit 64
      dump_header="$2"
      shift 2
      ;;
    --output)
      [ "$#" -ge 2 ] || exit 64
      output="$2"
      shift 2
      ;;
    --write-out)
      [ "$#" -ge 2 ] || exit 64
      write_out="$2"
      shift 2
      ;;
    http://127.0.0.1:*)
      ((target_count += 1))
      shift
      ;;
    *) exit 64 ;;
  esac
done
if [ "$target_count" -ne 1 ] || [ -z "$dump_header" ] || [ -z "$output" ] \
  || [ "$write_out" != '%{http_code}' ]; then
  exit 64
fi
: >"$dump_header"
: >"$output"
printf '%s' 204
"""
    )
    for forbidden_fake_curl_substitute in (
        "localhost",
        "Access-Control-Request-Method",
        "Access-Control-Request-Headers",
        "content-type",
        "/api/settings",
        "/api/camera/retry",
    ):
        assert forbidden_fake_curl_substitute not in fake_curl_source
    fake_curl.write_text(fake_curl_source)
    fake_curl.chmod(fake_curl.stat().st_mode | stat.S_IXUSR)

    program = (
        "set -euo pipefail\n"
        "fail() { printf '%s\\n' \"$*\" >&2; exit 1; }\n"
        + fetch_function
        + "\n"
        + 'fetch_openapi_method_probe "$2" options "$3" "$4" "$5" "$1"\n'
        + 'printf \'%s\\n\' "$fetch_status" "$fetch_code" "$fetch_body" "$fetch_headers"\n'
    )
    assert program.count("fetch_openapi_method_probe() {") == 1
    fixture_root = tmp_path / "results"
    fixture_root.mkdir()
    port = 48123

    def invoke_preflight(
        path: str,
        preflight_for: str,
        prefix: str,
        *,
        response_kind: str = "canonical",
    ) -> tuple[subprocess.CompletedProcess[str], list[str]]:
        argv_log = fixture_root / f"{prefix}.argv"
        result = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                "fixture",
                str(fixture_root),
                str(port),
                path,
                preflight_for,
                prefix,
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "FAKE_CURL_ARGV_LOG": str(argv_log),
                "FAKE_CURL_RESPONSE_KIND": response_kind,
                "LC_ALL": "C",
                "PATH": str(fixture_bin),
            },
            text=True,
            timeout=10,
        )
        raw_argv = argv_log.read_bytes()
        assert raw_argv.endswith(b"\0")
        argv = [item.decode() for item in raw_argv[:-1].split(b"\0")]
        return result, argv

    for path, preflight_for, prefix, expected_acrh in (
        ("/api/settings", "get", "settings-get", None),
        (
            "/api/settings",
            "patch",
            "settings-patch",
            "Access-Control-Request-Headers: content-type,x-pokecon-request",
        ),
        (
            "/api/camera/retry",
            "post",
            "camera-retry-post",
            "Access-Control-Request-Headers: content-type,x-pokecon-request",
        ),
    ):
        result, argv = invoke_preflight(path, preflight_for, prefix)
        expected_body = fixture_root / f"{prefix}.body"
        expected_headers = fixture_root / f"{prefix}.headers"
        expected_request_headers = [
            f"Origin: http://localhost:{port}",
            f"Access-Control-Request-Method: {preflight_for.upper()}",
        ]
        if expected_acrh is not None:
            expected_request_headers.append(expected_acrh)
        expected_argv = [
            "--silent",
            "--show-error",
            "--globoff",
            "--noproxy",
            "*",
            "--connect-timeout",
            "1",
            "--max-time",
            "3",
            "--dump-header",
            str(expected_headers),
            "--output",
            str(expected_body),
            "--write-out",
            "%{http_code}",
            "--request",
            "OPTIONS",
        ]
        for request_header in expected_request_headers:
            expected_argv.extend(("--header", request_header))
        expected_argv.append(f"http://127.0.0.1:{port}{path}")

        assert result.returncode == 0, result.stderr
        assert result.stderr == ""
        assert result.stdout.splitlines() == [
            "0",
            "204",
            str(expected_body),
            str(expected_headers),
        ]
        assert argv == expected_argv
        observed_request_headers = [
            argv[index + 1] for index, value in enumerate(argv) if value == "--header"
        ]
        assert observed_request_headers == expected_request_headers
        assert (
            sum(header.startswith("Origin:") for header in observed_request_headers)
            == 1
        )
        assert (
            sum(
                header.startswith("Access-Control-Request-Method:")
                for header in observed_request_headers
            )
            == 1
        )
        assert sum(
            header.startswith("Access-Control-Request-Headers:")
            for header in observed_request_headers
        ) == (expected_acrh is not None)
        assert expected_body.read_bytes() == b""
        assert expected_headers.read_bytes() == b""
        assert (fixture_root / f"{prefix}.curl.stderr").read_bytes() == b""

    rejected_prefix = "unknown-fake-curl-control"
    rejected, _argv = invoke_preflight(
        "/api/settings",
        "get",
        rejected_prefix,
        response_kind="unsupported",
    )
    assert rejected.returncode == 0
    assert rejected.stderr == ""
    assert rejected.stdout.splitlines() == [
        "64",
        "",
        str(fixture_root / f"{rejected_prefix}.body"),
        str(fixture_root / f"{rejected_prefix}.headers"),
    ]
    assert not (fixture_root / f"{rejected_prefix}.body").exists()
    assert not (fixture_root / f"{rejected_prefix}.headers").exists()


def test_openapi_method_matrix_fixture_executes_exact_complement(
    tmp_path: Path,
) -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    cors_success_forbidden_inventory_body = section(
        gate,
        "readonly -a cors_success_forbidden_response_headers=(\n",
        "\n)\nreadonly -a advertised_bare_options_forbidden_response_headers=(",
    )
    cors_success_forbidden_inventory_declaration = (
        "readonly -a cors_success_forbidden_response_headers=(\n"
        + cors_success_forbidden_inventory_body
        + "\n)"
    )
    cors_rejection_forbidden_inventory_body = section(
        gate,
        "readonly -a cors_rejection_forbidden_response_headers=(\n",
        "\n)\ncors_rejection_forbidden_response_headers_json=",
    )
    cors_rejection_forbidden_inventory_declaration = (
        "readonly -a cors_rejection_forbidden_response_headers=(\n"
        + cors_rejection_forbidden_inventory_body
        + "\n)"
    )
    cors_rejection_forbidden_inventory_serialization = (
        'cors_rejection_forbidden_response_headers_json="$(\n'
        "  printf '%s\\n' \"${cors_rejection_forbidden_response_headers[@]}\" \\\n"
        "    | jq -Rsc 'split(\"\\n\")[:-1]'\n"
        ')"\n'
        "readonly cors_rejection_forbidden_response_headers_json"
    )
    assert gate.count(cors_rejection_forbidden_inventory_serialization) == 1

    def shell_function(name: str, following: str) -> str:
        return (
            f"{name}() {{"
            + section(gate, f"{name}() {{", f"\n}}\n\n{following}() {{")
            + "\n}"
        )

    strict_json_decoder = (
        "validate_unique_json_object_keys() {"
        + section(
            gate,
            "validate_unique_json_object_keys() {",
            "\n}\nreadonly -f validate_unique_json_object_keys"
            "\n\nbuild_openapi_operation_inventory() {",
        )
        + "\n}\nreadonly -f validate_unique_json_object_keys"
    )
    require_json = shell_function("require_json_response", "fetch_openapi_method_probe")
    require_head = shell_function(
        "require_head_method_rejection",
        "require_single_exact_header",
    )
    require_single_exact_header = shell_function(
        "require_single_exact_header",
        "require_single_positive_decimal_header",
    )
    require_single_positive_decimal_header = shell_function(
        "require_single_positive_decimal_header",
        "require_cors_preflight",
    )
    require_preflight = (
        "require_cors_preflight() {"
        + section(
            gate,
            "require_cors_preflight() {",
            "\n}\nreadonly -f require_cors_preflight"
            "\n\nrequire_forbidden_preflight() {",
        )
        + "\n}\nreadonly -f require_cors_preflight"
    )
    require_forbidden_preflight = (
        "require_forbidden_preflight() {"
        + section(
            gate,
            "require_forbidden_preflight() {",
            "\n}\nreadonly -f require_forbidden_preflight"
            "\n\nrequire_exact_allow_header() {",
        )
        + "\n}\nreadonly -f require_forbidden_preflight"
    )
    require_exact_allow_header = shell_function(
        "require_exact_allow_header",
        "probe_cors_preflight_policy",
    )
    preflight_policy_probe = shell_function(
        "probe_cors_preflight_policy",
        "probe_openapi_method_matrix",
    )
    probe = shell_function("probe_openapi_method_matrix", "wait_for_readiness")
    require_resource_not_found = shell_function(
        "require_resource_not_found_response",
        "require_head_resource_not_found_response",
    )
    require_head_resource_not_found = shell_function(
        "require_head_resource_not_found_response",
        "probe_unknown_api_boundary_matrix",
    )
    unknown_api_boundary_probe = shell_function(
        "probe_unknown_api_boundary_matrix",
        "fetch_advertised_bare_options_probe",
    )
    absent_response_header = shell_function(
        "require_absent_response_header",
        "require_advertised_bare_options_rejection",
    )
    advertised_bare_options_rejection = shell_function(
        "require_advertised_bare_options_rejection",
        "probe_advertised_bare_options_security_boundary",
    )
    advertised_bare_options_probe = shell_function(
        "probe_advertised_bare_options_security_boundary",
        "active_identity_matches",
    )
    fake_fetch = r"""
fetch_openapi_method_probe() {
  local port="$1"
  local method="$2"
  local path="$3"
  local preflight_for="$4"
  local prefix="$5"
  local root="$6"
  printf '%s\t%s\t%s\n' "$path" "$method" "$preflight_for" \
    >>"$root/request-invocations.tsv"
  if [ "$method" = options ] \
    && [ "$path:$preflight_for" = /api/state:connect ]; then
    printf '%s\t%s\t%s\t%s\t%s\n' \
      "$path" "$method" "$preflight_for" \
      X-Pokecon-Preflight-Method GET \
      >>"$root/request-header-invocations.tsv"
  fi
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
  : >"$fetch_body"
  : >"$fetch_headers"
  fetch_status=0
  local unknown_body
  unknown_body='{"error":{"code":"resource_not_found","fields":null,"message":"API resource was not found"}}'
  if [ "$method" = options ]; then
    local advertised_preflight=false
    if jq -e --arg path "$path" --arg method "$preflight_for" \
      'any(.[]; .path == $path and .method == $method)' \
      "$openapi_operation_inventory" >/dev/null; then
      advertised_preflight=true
    fi
    if [ "$advertised_preflight" = true ] \
      || [ "${FAIL_OPEN_PREFLIGHTS:-0}" = 1 ]; then
      fetch_code=204
      printf '%s\n' \
        'HTTP/1.1 204 No Content' \
        "Access-Control-Allow-Origin: http://localhost:$port" \
        'Access-Control-Allow-Methods: GET, PATCH, POST, OPTIONS' \
        'Access-Control-Allow-Headers: Content-Type, X-Pokecon-Request' \
        'Vary: Origin' >"$fetch_headers"
      if [ "${DUPLICATE_CORS_HEADER:-0}" = 1 ]; then
        printf '%s\n' 'Access-Control-Allow-Origin: https://conflict.invalid' \
          >>"$fetch_headers"
      fi
      if [ "$advertised_preflight" = true ] \
        && [ "$path:$preflight_for" = /api/settings:get ]; then
        local -a cors_success_response_headers=()
        mapfile -t cors_success_response_headers <"$fetch_headers"
        case "${CORS_SUCCESS_REQUIRED_HEADER_FAULT:-none}" in
          none) ;;
          origin_missing)
            unset 'cors_success_response_headers[1]'
            ;;
          origin_wrong)
            cors_success_response_headers[1]='Access-Control-Allow-Origin: https://wrong.invalid'
            ;;
          origin_duplicate_conflicting)
            cors_success_response_headers+=(
              'Access-Control-Allow-Origin: https://conflict.invalid'
            )
            ;;
          methods_missing)
            unset 'cors_success_response_headers[2]'
            ;;
          methods_wrong)
            cors_success_response_headers[2]='Access-Control-Allow-Methods: GET, OPTIONS'
            ;;
          methods_duplicate_conflicting)
            cors_success_response_headers+=(
              'Access-Control-Allow-Methods: GET, OPTIONS'
            )
            ;;
          headers_missing)
            unset 'cors_success_response_headers[3]'
            ;;
          headers_wrong)
            cors_success_response_headers[3]='Access-Control-Allow-Headers: Content-Type'
            ;;
          headers_duplicate_conflicting)
            cors_success_response_headers+=(
              'Access-Control-Allow-Headers: Content-Type'
            )
            ;;
          vary_missing)
            unset 'cors_success_response_headers[4]'
            ;;
          vary_wrong)
            cors_success_response_headers[4]='Vary: Accept-Encoding'
            ;;
          vary_duplicate_conflicting)
            cors_success_response_headers+=(
              'Vary: Accept-Encoding'
            )
            ;;
          *) return 2 ;;
        esac
        printf '%s\n' "${cors_success_response_headers[@]}" >"$fetch_headers"
        case "${CORS_SUCCESS_PREFLIGHT_ENVELOPE_KIND:-canonical}" in
          canonical) ;;
          transport_failure)
            fetch_status=7
            fetch_code=000
            ;;
          wrong_http_status)
            fetch_code=200
            ;;
          nonempty_body)
            printf '%s' 'forbidden successful CORS preflight response body' \
              >"$fetch_body"
            ;;
          *) return 2 ;;
        esac
        case "${CORS_SUCCESS_FORBIDDEN_HEADER_KIND:-none}" in
          none) ;;
          allow)
            printf '%s\n' 'aLlOw: GET' >>"$fetch_headers"
            ;;
          content_length)
            printf '%s\n' 'Content-Length: 0' >>"$fetch_headers"
            ;;
          transfer_encoding)
            printf '%s\n' 'Transfer-Encoding: chunked' >>"$fetch_headers"
            ;;
          allow_credentials)
            printf '%s\n' 'Access-Control-Allow-Credentials: true' \
              >>"$fetch_headers"
            ;;
          expose_headers)
            printf '%s\n' 'Access-Control-Expose-Headers: X-Pokecon-Secret' \
              >>"$fetch_headers"
            ;;
          max_age)
            printf '%s\n' 'Access-Control-Max-Age: 600' >>"$fetch_headers"
            ;;
          *) return 2 ;;
        esac
      fi
    else
      fetch_code=403
      printf '%s\n' \
        'HTTP/1.1 403 Forbidden' \
        'Content-Type: application/json' >"$fetch_headers"
      printf '%s\n' \
        "{\"error\":{\"code\":\"request_forbidden\",\"fields\":null,\"message\":\"${CORS_PREFLIGHT_FORBIDDEN_MESSAGE:-request validation failed}\"}}" \
        >"$fetch_body"
      if [ "$path:$preflight_for" = /api/settings:put ]; then
        case "${CORS_PREFLIGHT_FORBIDDEN_ENVELOPE_FAULT:-none}" in
          none) ;;
          transport_status)
            fetch_status=7
            ;;
          http_status)
            fetch_code=400
            ;;
          content_type_missing)
            printf '%s\n' 'HTTP/1.1 403 Forbidden' >"$fetch_headers"
            ;;
          content_type_wrong)
            printf '%s\n' \
              'HTTP/1.1 403 Forbidden' \
              'Content-Type: text/plain' >"$fetch_headers"
            ;;
          content_type_duplicate_conflicting)
            printf '%s\n' 'Content-Type: text/plain' >>"$fetch_headers"
            ;;
          *) return 2 ;;
        esac
      fi
      if [ "$path:$preflight_for" = /api/state:delete ]; then
        case "${CORS_PREFLIGHT_FORBIDDEN_DOCUMENT_FAULT:-none}" in
          none) ;;
          non_json)
            printf '%s' '<!doctype html><html><body>fixture</body></html>' \
              >"$fetch_body"
            ;;
          multiple_documents)
            printf '%s%s' \
              '{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"}}' \
              '{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"}}' \
              >"$fetch_body"
            ;;
          duplicate_top_level_error)
            printf '%s' \
              '{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"},"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"}}' \
              >"$fetch_body"
            ;;
          duplicate_nested_code)
            printf '%s' \
              '{"error":{"code":"request_forbidden","code":"request_forbidden","fields":null,"message":"request validation failed"}}' \
              >"$fetch_body"
            ;;
          *) return 2 ;;
        esac
      fi
      if [ "$path:$preflight_for" = /api/state:connect ]; then
        case "${CORS_PREFLIGHT_FORBIDDEN_SEMANTIC_FAULT:-none}" in
          none) ;;
          top_level_extra)
            printf '%s' \
              '{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"},"extra":null}' \
              >"$fetch_body"
            ;;
          nested_error_extra)
            printf '%s' \
              '{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed","extra":null}}' \
              >"$fetch_body"
            ;;
          code_wrong_type)
            printf '%s' \
              '{"error":{"code":null,"fields":null,"message":"request validation failed"}}' \
              >"$fetch_body"
            ;;
          fields_non_null)
            printf '%s' \
              '{"error":{"code":"request_forbidden","fields":{},"message":"request validation failed"}}' \
              >"$fetch_body"
            ;;
          message_wrong_type)
            printf '%s' \
              '{"error":{"code":"request_forbidden","fields":null,"message":null}}' \
              >"$fetch_body"
            ;;
          *) return 2 ;;
        esac
      fi
      if [ "$path:$preflight_for" = /api/not-in-openapi:get ]; then
        case "${CORS_PREFLIGHT_FORBIDDEN_HEADER_FAULT:-none}" in
          none) ;;
          allow_credentials)
            printf '%s\n' 'Access-Control-Allow-Credentials: true' \
              >>"$fetch_headers"
            ;;
          allow_headers)
            printf '%s\n' 'Access-Control-Allow-Headers: Content-Type' \
              >>"$fetch_headers"
            ;;
          allow_methods)
            printf '%s\n' 'Access-Control-Allow-Methods: GET' \
              >>"$fetch_headers"
            ;;
          allow_origin)
            printf '%s\n' \
              'Access-Control-Allow-Origin: https://conflict.invalid' \
              >>"$fetch_headers"
            ;;
          expose_headers)
            printf '%s\n' 'Access-Control-Expose-Headers: X-Pokecon-Secret' \
              >>"$fetch_headers"
            ;;
          max_age)
            printf '%s\n' 'Access-Control-Max-Age: 600' >>"$fetch_headers"
            ;;
          allow)
            printf '%s\n' 'Allow: GET' >>"$fetch_headers"
            ;;
          vary)
            printf '%s\n' 'Vary: Origin' >>"$fetch_headers"
            ;;
          *) return 2 ;;
        esac
      fi
    fi
    return
  fi
  local advertised_allow
  advertised_allow="$(
    jq -r --arg path "$path" '
      [.[] | select(.path == $path) | .method | ascii_upcase]
      | sort | join(", ")
    ' "$openapi_operation_inventory"
  )"
  if [ "${MISLEADING_ALLOW:-0}" = 1 ]; then
    advertised_allow='GET, HEAD, OPTIONS'
  fi
  if [ "$method" = head ]; then
    if [ "$path" = /api/not-in-openapi ]; then
      fetch_code="${UNKNOWN_ROUTED_STATUS:-404}"
      local unknown_head_length="${#unknown_body}"
      if [ "${UNKNOWN_HEAD_LENGTH:-canonical}" != canonical ]; then
        unknown_head_length="$UNKNOWN_HEAD_LENGTH"
      fi
      printf '%s\n' \
        'HTTP/1.1 404 Not Found' \
        "Content-Type: ${UNKNOWN_HEAD_CONTENT_TYPE:-application/json}" \
        "Content-Length: $unknown_head_length" >"$fetch_headers"
      if [ "${UNKNOWN_HEAD_BODY_KIND:-empty}" = nonempty ]; then
        printf '%s' 'forbidden unknown HEAD response bytes' >"$fetch_body"
      fi
    else
      fetch_code="${ADVERTISED_HEAD_STATUS:-405}"
      printf 'HTTP/1.1 %s Fixture\n' "$fetch_code" >"$fetch_headers"
      case "${ADVERTISED_HEAD_CONTENT_TYPE_KIND:-canonical}" in
        canonical)
          printf '%s\n' 'Content-Type: application/json' >>"$fetch_headers"
          ;;
        wrong)
          printf '%s\n' 'Content-Type: text/plain' >>"$fetch_headers"
          ;;
        duplicate_conflicting)
          printf '%s\n' \
            'Content-Type: application/json' \
            'Content-Type: text/plain' >>"$fetch_headers"
          ;;
        missing) ;;
        *) return 2 ;;
      esac
      printf '%s\n' "Allow: $advertised_allow" >>"$fetch_headers"
      case "${ADVERTISED_HEAD_CONTENT_LENGTH_KIND:-canonical}" in
        canonical)
          printf '%s\n' 'Content-Length: 123' >>"$fetch_headers"
          ;;
        zero)
          printf '%s\n' 'Content-Length: 0' >>"$fetch_headers"
          ;;
        non_numeric)
          printf '%s\n' 'Content-Length: one' >>"$fetch_headers"
          ;;
        duplicate_conflicting)
          printf '%s\n' \
            'Content-Length: 123' \
            'Content-Length: 0' >>"$fetch_headers"
          ;;
        missing) ;;
        *) return 2 ;;
      esac
      case "${ADVERTISED_HEAD_BODY_KIND:-empty}" in
        empty) ;;
        nonempty)
          printf '%s' 'forbidden HEAD response bytes' >"$fetch_body"
          ;;
        *) return 2 ;;
      esac
    fi
    return
  fi
  local advertised=false
  if jq -e --arg path "$path" --arg method "$method" \
    'any(.[]; .path == $path and .method == $method)' \
    "$openapi_operation_inventory" >/dev/null; then
    advertised=true
  fi
  printf '%s\n' 'Content-Type: application/json' >"$fetch_headers"
  if [ "$path" = /api/not-in-openapi ]; then
    fetch_code="${UNKNOWN_ROUTED_STATUS:-404}"
    if [ "${UNKNOWN_NON_HEAD_BODY_KIND:-canonical}" = spa ]; then
      printf '%s' '<!doctype html><html><body>fixture SPA</body></html>' \
        >"$fetch_body"
    else
      printf '%s' \
        "{\"error\":{\"code\":\"${UNKNOWN_NON_HEAD_CODE:-resource_not_found}\",\"fields\":null,\"message\":\"${UNKNOWN_NON_HEAD_MESSAGE:-API resource was not found}\"}}" \
        >"$fetch_body"
    fi
  elif [ "$advertised" = true ] && [ "$method" = get ] && [ "$path" != /ws ]; then
    fetch_code=200
    printf '%s\n' '{"data":{}}' >"$fetch_body"
  elif [ "$advertised" = true ] && [ "$method" = get ]; then
    fetch_code=400
    printf '%s\n' \
      '{"error":{"code":"invalid_request","fields":null,"message":"fixture"}}' \
      >"$fetch_body"
  elif [ "$advertised" = true ]; then
    fetch_code=400
    printf '%s\n' \
      '{"error":{"code":"malformed_json","fields":null,"message":"fixture"}}' \
      >"$fetch_body"
  else
    fetch_code=405
    printf '%s\n' "Allow: $advertised_allow" >>"$fetch_headers"
    printf '%s\n' \
      '{"error":{"code":"method_not_allowed","fields":null,"message":"fixture"}}' \
      >"$fetch_body"
  fi
  : "$preflight_for"
}
"""
    fake_unknown_options_fetch = r"""
fetch_unknown_api_options_probe() {
  local port="$1"
  local path="$2"
  local request_variant="$3"
  local prefix="$4"
  local root="$5"
  printf '%s\t%s\t%s\n' "$path" options "$request_variant" \
    >>"$root/request-invocations.tsv"
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
  fetch_status=0
  fetch_code="${UNKNOWN_OPTIONS_STATUS:-403}"
  printf '%s\n' \
    'HTTP/1.1 403 Forbidden' \
    'Content-Type: application/json' >"$fetch_headers"
  printf '%s\n' \
    "{\"error\":{\"code\":\"${UNKNOWN_OPTIONS_CODE:-request_forbidden}\",\"fields\":null,\"message\":\"${UNKNOWN_OPTIONS_FORBIDDEN_MESSAGE:-request validation failed}\"}}" \
    >"$fetch_body"
  : "$port"
}
"""
    fake_advertised_bare_options_fetch = r"""
fetch_advertised_bare_options_probe() {
  local port="$1"
  local path="$2"
  local prefix="$3"
  local root="$4"
  printf '%s\toptions\tbare\t0\tfalse\tfalse\n' "$path" \
    >>"$root/advertised-bare-options-invocations.tsv"
  fetch_body="$root/$prefix.body"
  fetch_headers="$root/$prefix.headers"
  fetch_status=0
  fetch_code="${ADVERTISED_BARE_OPTIONS_STATUS:-403}"
  printf '%s\n' \
    'HTTP/1.1 403 Forbidden' \
    "Content-Type: ${ADVERTISED_BARE_OPTIONS_CONTENT_TYPE:-application/json}" \
    >"$fetch_headers"
  case "${ADVERTISED_BARE_OPTIONS_LEAKED_HEADER_KIND:-none}" in
    none) ;;
    allow) printf '%s\n' 'Allow: GET' >>"$fetch_headers" ;;
    cors)
      printf '%s\n' 'Access-Control-Allow-Origin: http://127.0.0.1:8020' \
        >>"$fetch_headers"
      ;;
    *) return 2 ;;
  esac
  local canonical_error
  canonical_error="{\"code\":\"${ADVERTISED_BARE_OPTIONS_CODE:-request_forbidden}\",\"fields\":null,\"message\":\"${ADVERTISED_BARE_OPTIONS_MESSAGE:-request validation failed}\"}"
  case "${ADVERTISED_BARE_OPTIONS_BODY_KIND:-canonical}" in
    canonical)
      printf '{"error":%s}' "$canonical_error" >"$fetch_body"
      ;;
    non_json)
      printf '%s' '<!doctype html><html><body>fixture</body></html>' >"$fetch_body"
      ;;
    multiple_documents)
      printf '{"error":%s}{"error":%s}' \
        "$canonical_error" "$canonical_error" >"$fetch_body"
      ;;
    duplicate_keys)
      printf '{"error":%s,"error":%s}' \
        "$canonical_error" "$canonical_error" >"$fetch_body"
      ;;
    *) return 2 ;;
  esac
  : "$port"
}
"""
    cors_success_required_header_faults: tuple[CorsSuccessRequiredHeaderFault, ...] = (
        (
            "origin_missing",
            "Access-Control-Allow-Origin",
            "http://localhost:8020",
            "missing",
        ),
        (
            "origin_wrong",
            "Access-Control-Allow-Origin",
            "http://localhost:8020",
            "wrong",
        ),
        (
            "origin_duplicate_conflicting",
            "Access-Control-Allow-Origin",
            "http://localhost:8020",
            "duplicate_conflicting",
        ),
        (
            "methods_missing",
            "Access-Control-Allow-Methods",
            "GET, PATCH, POST, OPTIONS",
            "missing",
        ),
        (
            "methods_wrong",
            "Access-Control-Allow-Methods",
            "GET, PATCH, POST, OPTIONS",
            "wrong",
        ),
        (
            "methods_duplicate_conflicting",
            "Access-Control-Allow-Methods",
            "GET, PATCH, POST, OPTIONS",
            "duplicate_conflicting",
        ),
        (
            "headers_missing",
            "Access-Control-Allow-Headers",
            "Content-Type, X-Pokecon-Request",
            "missing",
        ),
        (
            "headers_wrong",
            "Access-Control-Allow-Headers",
            "Content-Type, X-Pokecon-Request",
            "wrong",
        ),
        (
            "headers_duplicate_conflicting",
            "Access-Control-Allow-Headers",
            "Content-Type, X-Pokecon-Request",
            "duplicate_conflicting",
        ),
        ("vary_missing", "Vary", "Origin", "missing"),
        ("vary_wrong", "Vary", "Origin", "wrong"),
        (
            "vary_duplicate_conflicting",
            "Vary",
            "Origin",
            "duplicate_conflicting",
        ),
    )
    required_header_specs = (
        ("origin", "Access-Control-Allow-Origin", "http://localhost:8020"),
        ("methods", "Access-Control-Allow-Methods", "GET, PATCH, POST, OPTIONS"),
        (
            "headers",
            "Access-Control-Allow-Headers",
            "Content-Type, X-Pokecon-Request",
        ),
        ("vary", "Vary", "Origin"),
    )
    required_header_fault_kinds = ("missing", "wrong", "duplicate_conflicting")
    assert len(cors_success_required_header_faults) == 12
    assert tuple(
        selector
        for selector, _header_name, _expected_value, _fault_kind in (
            cors_success_required_header_faults
        )
    ) == (
        "origin_missing",
        "origin_wrong",
        "origin_duplicate_conflicting",
        "methods_missing",
        "methods_wrong",
        "methods_duplicate_conflicting",
        "headers_missing",
        "headers_wrong",
        "headers_duplicate_conflicting",
        "vary_missing",
        "vary_wrong",
        "vary_duplicate_conflicting",
    )
    assert {
        (header_name, expected_value, fault_kind)
        for _selector, header_name, expected_value, fault_kind in (
            cors_success_required_header_faults
        )
    } == {
        (header_name, expected_value, fault_kind)
        for _selector_prefix, header_name, expected_value in required_header_specs
        for fault_kind in required_header_fault_kinds
    }
    assert all(
        selector == f"{selector_prefix}_{fault_kind}"
        for selector_prefix, header_name, expected_value in required_header_specs
        for fault_kind in required_header_fault_kinds
        for selector, observed_header_name, observed_expected_value, observed_fault_kind in (
            cors_success_required_header_faults
        )
        if observed_header_name == header_name
        and observed_expected_value == expected_value
        and observed_fault_kind == fault_kind
    )
    cors_success_preflight_envelope_faults = (
        ("transport_failure", "returned curl status 7 and HTTP 000"),
        ("wrong_http_status", "returned curl status 0 and HTTP 200"),
        ("nonempty_body", "returned a body"),
    )
    assert tuple(
        selector for selector, _diagnostic in cors_success_preflight_envelope_faults
    ) == (
        "transport_failure",
        "wrong_http_status",
        "nonempty_body",
    )
    cors_preflight_forbidden_envelope_faults: tuple[tuple[str, str], ...] = (
        ("transport_status", "returned curl status 7 and HTTP 403"),
        ("http_status", "returned curl status 0 and HTTP 400"),
        (
            "content_type_missing",
            "did not return exactly one Content-Type header with value application/json",
        ),
        (
            "content_type_wrong",
            "did not return exactly one Content-Type header with value application/json",
        ),
        (
            "content_type_duplicate_conflicting",
            "did not return exactly one Content-Type header with value application/json",
        ),
    )
    assert tuple(
        selector for selector, _diagnostic in cors_preflight_forbidden_envelope_faults
    ) == (
        "transport_status",
        "http_status",
        "content_type_missing",
        "content_type_wrong",
        "content_type_duplicate_conflicting",
    )
    cors_preflight_forbidden_document_faults: tuple[tuple[str, str], ...] = (
        ("non_json", "did not return valid JSON"),
        (
            "multiple_documents",
            "did not return exactly one JSON document with unique object keys",
        ),
        (
            "duplicate_top_level_error",
            "did not return exactly one JSON document with unique object keys",
        ),
        (
            "duplicate_nested_code",
            "did not return exactly one JSON document with unique object keys",
        ),
    )
    assert tuple(
        selector for selector, _diagnostic in cors_preflight_forbidden_document_faults
    ) == (
        "non_json",
        "multiple_documents",
        "duplicate_top_level_error",
        "duplicate_nested_code",
    )
    cors_preflight_forbidden_semantic_faults: tuple[str, ...] = (
        "top_level_extra",
        "nested_error_extra",
        "code_wrong_type",
        "fields_non_null",
        "message_wrong_type",
    )
    assert cors_preflight_forbidden_semantic_faults == (
        "top_level_extra",
        "nested_error_extra",
        "code_wrong_type",
        "fields_non_null",
        "message_wrong_type",
    )
    forbidden_semantic_diagnostic = (
        "did not return the canonical request_forbidden rejection"
    )
    cors_preflight_forbidden_header_faults: tuple[tuple[str, str, str], ...] = (
        (
            "allow_credentials",
            "Access-Control-Allow-Credentials",
            "Access-Control-Allow-Credentials: true",
        ),
        (
            "allow_headers",
            "Access-Control-Allow-Headers",
            "Access-Control-Allow-Headers: Content-Type",
        ),
        (
            "allow_methods",
            "Access-Control-Allow-Methods",
            "Access-Control-Allow-Methods: GET",
        ),
        (
            "allow_origin",
            "Access-Control-Allow-Origin",
            "Access-Control-Allow-Origin: https://conflict.invalid",
        ),
        (
            "expose_headers",
            "Access-Control-Expose-Headers",
            "Access-Control-Expose-Headers: X-Pokecon-Secret",
        ),
        ("max_age", "Access-Control-Max-Age", "Access-Control-Max-Age: 600"),
        ("allow", "Allow", "Allow: GET"),
        ("vary", "Vary", "Vary: Origin"),
    )
    assert tuple(
        selector
        for selector, _header_name, _response_header in (
            cors_preflight_forbidden_header_faults
        )
    ) == (
        "allow_credentials",
        "allow_headers",
        "allow_methods",
        "allow_origin",
        "expose_headers",
        "max_age",
        "allow",
        "vary",
    )
    assert tuple(
        header_name
        for _selector, header_name, _response_header in (
            cors_preflight_forbidden_header_faults
        )
    ) == tuple(
        line.strip() for line in cors_rejection_forbidden_inventory_body.splitlines()
    )
    cors_success_forbidden_header_faults = (
        ("allow", "Allow", "aLlOw: GET"),
        ("content_length", "Content-Length", "Content-Length: 0"),
        (
            "transfer_encoding",
            "Transfer-Encoding",
            "Transfer-Encoding: chunked",
        ),
        (
            "allow_credentials",
            "Access-Control-Allow-Credentials",
            "Access-Control-Allow-Credentials: true",
        ),
        (
            "expose_headers",
            "Access-Control-Expose-Headers",
            "Access-Control-Expose-Headers: X-Pokecon-Secret",
        ),
        ("max_age", "Access-Control-Max-Age", "Access-Control-Max-Age: 600"),
    )
    assert tuple(
        selector
        for selector, _header_name, _response_header in cors_success_forbidden_header_faults
    ) == (
        "allow",
        "content_length",
        "transfer_encoding",
        "allow_credentials",
        "expose_headers",
        "max_age",
    )
    assert tuple(
        header_name
        for _selector, header_name, _response_header in cors_success_forbidden_header_faults
    ) == (
        "Allow",
        "Content-Length",
        "Transfer-Encoding",
        "Access-Control-Allow-Credentials",
        "Access-Control-Expose-Headers",
        "Access-Control-Max-Age",
    )
    envelope_selector_control = "${CORS_SUCCESS_PREFLIGHT_ENVELOPE_KIND:-canonical}"
    selector_control = "${CORS_SUCCESS_FORBIDDEN_HEADER_KIND:-none}"
    required_header_selector_control = "${CORS_SUCCESS_REQUIRED_HEADER_FAULT:-none}"
    forbidden_envelope_selector_control = (
        "${CORS_PREFLIGHT_FORBIDDEN_ENVELOPE_FAULT:-none}"
    )
    forbidden_document_selector_control = (
        "${CORS_PREFLIGHT_FORBIDDEN_DOCUMENT_FAULT:-none}"
    )
    forbidden_semantic_selector_control = (
        "${CORS_PREFLIGHT_FORBIDDEN_SEMANTIC_FAULT:-none}"
    )
    forbidden_semantic_selector_name = "CORS_PREFLIGHT_FORBIDDEN_SEMANTIC_FAULT"
    forbidden_header_selector_control = "${CORS_PREFLIGHT_FORBIDDEN_HEADER_FAULT:-none}"
    forbidden_header_selector_name = "CORS_PREFLIGHT_FORBIDDEN_HEADER_FAULT"
    preflight_block_start = fake_fetch.index('  if [ "$method" = options ]; then\n')
    preflight_block_end = fake_fetch.index(
        "  local advertised_allow\n",
        preflight_block_start,
    )
    preflight_block = fake_fetch[preflight_block_start:preflight_block_end]
    successful_preflight_branch_start = preflight_block.index(
        '    if [ "$advertised_preflight" = true ]'
    )
    successful_preflight_branch_end = preflight_block.index(
        "    else\n",
        successful_preflight_branch_start,
    )
    successful_preflight_branch = preflight_block[
        successful_preflight_branch_start:successful_preflight_branch_end
    ]
    rejected_preflight_branch = preflight_block[successful_preflight_branch_end:]
    later_advertised_fault_target = '[ "$path:$preflight_for" = /api/settings:get ]'
    advertised_success_guard = (
        '      if [ "$advertised_preflight" = true ] \\\n'
        f"        && {later_advertised_fault_target}; then\n"
    )
    required_header_case_marker = (
        f'        case "{required_header_selector_control}" in\n'
    )
    envelope_case_marker = f'        case "{envelope_selector_control}" in\n'
    selector_case_marker = f'        case "{selector_control}" in\n'
    assert fake_fetch.count(envelope_selector_control) == 1
    assert fake_fetch.count(selector_control) == 1
    assert fake_fetch.count(required_header_selector_control) == 1
    assert fake_fetch.count(later_advertised_fault_target) == 1
    assert successful_preflight_branch.count(envelope_selector_control) == 1
    assert successful_preflight_branch.count(selector_control) == 1
    assert successful_preflight_branch.count(required_header_selector_control) == 1
    assert successful_preflight_branch.count(later_advertised_fault_target) == 1
    assert successful_preflight_branch.count(advertised_success_guard) == 1
    advertised_success_only_start = successful_preflight_branch.index(
        advertised_success_guard
    )
    advertised_success_only_end = successful_preflight_branch.index(
        "      fi\n",
        advertised_success_only_start,
    )
    advertised_success_only_branch = successful_preflight_branch[
        advertised_success_only_start:advertised_success_only_end
    ]
    assert advertised_success_only_branch.count(envelope_selector_control) == 1
    assert advertised_success_only_branch.count(selector_control) == 1
    assert advertised_success_only_branch.count(required_header_selector_control) == 1
    preflight_outside_advertised_success_target = (
        successful_preflight_branch[:advertised_success_only_start]
        + successful_preflight_branch[advertised_success_only_end:]
    )
    assert envelope_selector_control not in preflight_outside_advertised_success_target
    assert selector_control not in preflight_outside_advertised_success_target
    assert (
        required_header_selector_control
        not in preflight_outside_advertised_success_target
    )
    non_options_fetch = (
        fake_fetch[:preflight_block_start] + fake_fetch[preflight_block_end:]
    )
    for selector_isolation_target in (
        rejected_preflight_branch,
        fake_unknown_options_fetch,
        fake_advertised_bare_options_fetch,
        non_options_fetch,
    ):
        assert envelope_selector_control not in selector_isolation_target
        assert selector_control not in selector_isolation_target
        assert required_header_selector_control not in selector_isolation_target
    assert fake_fetch.count(forbidden_envelope_selector_control) == 1
    assert rejected_preflight_branch.count(forbidden_envelope_selector_control) == 1
    for forbidden_envelope_isolation_target in (
        successful_preflight_branch,
        fake_unknown_options_fetch,
        fake_advertised_bare_options_fetch,
        non_options_fetch,
    ):
        assert (
            forbidden_envelope_selector_control
            not in forbidden_envelope_isolation_target
        )
    assert fake_fetch.count(forbidden_document_selector_control) == 1
    assert rejected_preflight_branch.count(forbidden_document_selector_control) == 1
    for forbidden_document_isolation_target in (
        successful_preflight_branch,
        fake_unknown_options_fetch,
        fake_advertised_bare_options_fetch,
        non_options_fetch,
    ):
        assert (
            forbidden_document_selector_control
            not in forbidden_document_isolation_target
        )
    assert fake_fetch.count(forbidden_semantic_selector_control) == 1
    assert rejected_preflight_branch.count(forbidden_semantic_selector_control) == 1
    assert fake_fetch.count(forbidden_semantic_selector_name) == 1
    assert rejected_preflight_branch.count(forbidden_semantic_selector_name) == 1
    for forbidden_semantic_isolation_target in (
        successful_preflight_branch,
        fake_unknown_options_fetch,
        fake_advertised_bare_options_fetch,
        non_options_fetch,
    ):
        assert (
            forbidden_semantic_selector_control
            not in forbidden_semantic_isolation_target
        )
        assert (
            forbidden_semantic_selector_name not in forbidden_semantic_isolation_target
        )
    assert fake_fetch.count(forbidden_header_selector_control) == 1
    assert rejected_preflight_branch.count(forbidden_header_selector_control) == 1
    assert fake_fetch.count(forbidden_header_selector_name) == 1
    assert rejected_preflight_branch.count(forbidden_header_selector_name) == 1
    for forbidden_header_isolation_target in (
        successful_preflight_branch,
        fake_unknown_options_fetch,
        fake_advertised_bare_options_fetch,
        non_options_fetch,
    ):
        assert (
            forbidden_header_selector_control not in forbidden_header_isolation_target
        )
        assert forbidden_header_selector_name not in forbidden_header_isolation_target
    forbidden_envelope_target = '[ "$path:$preflight_for" = /api/settings:put ]'
    forbidden_envelope_guard = f"      if {forbidden_envelope_target}; then\n"
    assert fake_fetch.count(forbidden_envelope_target) == 1
    assert rejected_preflight_branch.count(forbidden_envelope_guard) == 1
    forbidden_envelope_guard_start = rejected_preflight_branch.index(
        forbidden_envelope_guard
    )
    forbidden_envelope_guard_end = rejected_preflight_branch.index(
        "      fi\n",
        forbidden_envelope_guard_start,
    )
    forbidden_envelope_branch = rejected_preflight_branch[
        forbidden_envelope_guard_start:forbidden_envelope_guard_end
    ]
    forbidden_envelope_case_marker = (
        f'        case "{forbidden_envelope_selector_control}" in\n'
    )
    forbidden_envelope_case_start = forbidden_envelope_branch.index(
        forbidden_envelope_case_marker
    )
    forbidden_envelope_case_end = forbidden_envelope_branch.index(
        "        esac\n",
        forbidden_envelope_case_start,
    ) + len("        esac\n")
    forbidden_envelope_case = forbidden_envelope_branch[
        forbidden_envelope_case_start:forbidden_envelope_case_end
    ]
    assert forbidden_envelope_case == (
        '        case "${CORS_PREFLIGHT_FORBIDDEN_ENVELOPE_FAULT:-none}" in\n'
        "          none) ;;\n"
        "          transport_status)\n"
        "            fetch_status=7\n"
        "            ;;\n"
        "          http_status)\n"
        "            fetch_code=400\n"
        "            ;;\n"
        "          content_type_missing)\n"
        "            printf '%s\\n' 'HTTP/1.1 403 Forbidden' "
        '>"$fetch_headers"\n'
        "            ;;\n"
        "          content_type_wrong)\n"
        "            printf '%s\\n' \\\n"
        "              'HTTP/1.1 403 Forbidden' \\\n"
        "              'Content-Type: text/plain' >\"$fetch_headers\"\n"
        "            ;;\n"
        "          content_type_duplicate_conflicting)\n"
        "            printf '%s\\n' 'Content-Type: text/plain' "
        '>>"$fetch_headers"\n'
        "            ;;\n"
        "          *) return 2 ;;\n"
        "        esac\n"
    )
    forbidden_envelope_case_kinds = (
        "none",
        "transport_status",
        "http_status",
        "content_type_missing",
        "content_type_wrong",
        "content_type_duplicate_conflicting",
    )
    forbidden_envelope_case_indices: list[int] = []
    for forbidden_envelope_case_kind in forbidden_envelope_case_kinds:
        forbidden_envelope_kind_marker = f"          {forbidden_envelope_case_kind})"
        assert forbidden_envelope_case.count(forbidden_envelope_kind_marker) == 1
        forbidden_envelope_case_indices.append(
            forbidden_envelope_case.index(forbidden_envelope_kind_marker)
        )
    assert forbidden_envelope_case_indices == sorted(forbidden_envelope_case_indices)
    forbidden_envelope_unknown_marker = "          *) return 2 ;;"
    assert forbidden_envelope_case.count(forbidden_envelope_unknown_marker) == 1
    assert forbidden_envelope_case_indices[-1] < forbidden_envelope_case.index(
        forbidden_envelope_unknown_marker
    )
    for forbidden_envelope_bypass in (
        '"$mode"',
        '"$label"',
        "fixture",
        '"$root"',
    ):
        assert forbidden_envelope_bypass not in forbidden_envelope_case
    forbidden_document_target = '[ "$path:$preflight_for" = /api/state:delete ]'
    forbidden_document_guard = f"      if {forbidden_document_target}; then\n"
    assert fake_fetch.count(forbidden_document_target) == 1
    assert rejected_preflight_branch.count(forbidden_document_guard) == 1
    forbidden_document_guard_start = rejected_preflight_branch.index(
        forbidden_document_guard
    )
    forbidden_document_guard_end = rejected_preflight_branch.index(
        "      fi\n",
        forbidden_document_guard_start,
    )
    forbidden_document_branch = rejected_preflight_branch[
        forbidden_document_guard_start:forbidden_document_guard_end
    ]
    assert forbidden_envelope_guard_start < forbidden_document_guard_start
    assert forbidden_document_selector_control not in forbidden_envelope_branch
    assert forbidden_envelope_selector_control not in forbidden_document_branch
    forbidden_document_case_marker = (
        f'        case "{forbidden_document_selector_control}" in\n'
    )
    forbidden_document_case_start = forbidden_document_branch.index(
        forbidden_document_case_marker
    )
    forbidden_document_case_end = forbidden_document_branch.index(
        "        esac\n",
        forbidden_document_case_start,
    ) + len("        esac\n")
    forbidden_document_case = forbidden_document_branch[
        forbidden_document_case_start:forbidden_document_case_end
    ]
    assert forbidden_document_case == (
        '        case "${CORS_PREFLIGHT_FORBIDDEN_DOCUMENT_FAULT:-none}" in\n'
        "          none) ;;\n"
        "          non_json)\n"
        "            printf '%s' '<!doctype html><html><body>fixture</body></html>' \\\n"
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          multiple_documents)\n"
        "            printf '%s%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"}}\' \\\n'
        '              \'{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          duplicate_top_level_error)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"},"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          duplicate_nested_code)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","code":"request_forbidden","fields":null,"message":"request validation failed"}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          *) return 2 ;;\n"
        "        esac\n"
    )
    forbidden_document_case_kinds = (
        "none",
        "non_json",
        "multiple_documents",
        "duplicate_top_level_error",
        "duplicate_nested_code",
    )
    forbidden_document_case_indices: list[int] = []
    for forbidden_document_case_kind in forbidden_document_case_kinds:
        forbidden_document_kind_marker = f"          {forbidden_document_case_kind})"
        assert forbidden_document_case.count(forbidden_document_kind_marker) == 1
        forbidden_document_case_indices.append(
            forbidden_document_case.index(forbidden_document_kind_marker)
        )
    assert forbidden_document_case_indices == sorted(forbidden_document_case_indices)
    forbidden_document_unknown_marker = "          *) return 2 ;;"
    assert forbidden_document_case.count(forbidden_document_unknown_marker) == 1
    assert forbidden_document_case_indices[-1] < forbidden_document_case.index(
        forbidden_document_unknown_marker
    )
    forbidden_semantic_target = '[ "$path:$preflight_for" = /api/state:connect ]'
    forbidden_semantic_guard = f"      if {forbidden_semantic_target}; then\n"
    request_header_evidence_guard = (
        f'  if [ "$method" = options ] \\\n    && {forbidden_semantic_target}; then\n'
    )
    assert fake_fetch.count(forbidden_semantic_target) == 2
    assert fake_fetch.count(request_header_evidence_guard) == 1
    assert rejected_preflight_branch.count(forbidden_semantic_guard) == 1
    forbidden_semantic_guard_start = rejected_preflight_branch.index(
        forbidden_semantic_guard
    )
    forbidden_semantic_guard_end = rejected_preflight_branch.index(
        "      fi\n",
        forbidden_semantic_guard_start,
    )
    forbidden_semantic_branch = rejected_preflight_branch[
        forbidden_semantic_guard_start:forbidden_semantic_guard_end
    ]
    assert forbidden_document_guard_start < forbidden_semantic_guard_start
    assert forbidden_semantic_selector_control not in forbidden_document_branch
    assert forbidden_document_selector_control not in forbidden_semantic_branch
    forbidden_semantic_case_marker = (
        f'        case "{forbidden_semantic_selector_control}" in\n'
    )
    forbidden_semantic_case_start = forbidden_semantic_branch.index(
        forbidden_semantic_case_marker
    )
    forbidden_semantic_case_end = forbidden_semantic_branch.index(
        "        esac\n",
        forbidden_semantic_case_start,
    ) + len("        esac\n")
    forbidden_semantic_case = forbidden_semantic_branch[
        forbidden_semantic_case_start:forbidden_semantic_case_end
    ]
    assert forbidden_semantic_case == (
        '        case "${CORS_PREFLIGHT_FORBIDDEN_SEMANTIC_FAULT:-none}" in\n'
        "          none) ;;\n"
        "          top_level_extra)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed"},"extra":null}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          nested_error_extra)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","fields":null,"message":"request validation failed","extra":null}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          code_wrong_type)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":null,"fields":null,"message":"request validation failed"}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          fields_non_null)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","fields":{},"message":"request validation failed"}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          message_wrong_type)\n"
        "            printf '%s' \\\n"
        '              \'{"error":{"code":"request_forbidden","fields":null,"message":null}}\' \\\n'
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          *) return 2 ;;\n"
        "        esac\n"
    )
    forbidden_semantic_case_kinds = (
        "none",
        *cors_preflight_forbidden_semantic_faults,
    )
    assert forbidden_semantic_case_kinds == (
        "none",
        "top_level_extra",
        "nested_error_extra",
        "code_wrong_type",
        "fields_non_null",
        "message_wrong_type",
    )
    forbidden_semantic_case_indices: list[int] = []
    for forbidden_semantic_case_kind in forbidden_semantic_case_kinds:
        forbidden_semantic_kind_marker = f"          {forbidden_semantic_case_kind})"
        assert forbidden_semantic_case.count(forbidden_semantic_kind_marker) == 1
        forbidden_semantic_case_indices.append(
            forbidden_semantic_case.index(forbidden_semantic_kind_marker)
        )
    assert forbidden_semantic_case_indices == sorted(forbidden_semantic_case_indices)
    forbidden_semantic_unknown_marker = "          *) return 2 ;;"
    assert forbidden_semantic_case.count(forbidden_semantic_unknown_marker) == 1
    assert forbidden_semantic_case_indices[-1] < forbidden_semantic_case.index(
        forbidden_semantic_unknown_marker
    )
    for forbidden_semantic_envelope_mutation in (
        "fetch_status",
        "fetch_code",
        "fetch_headers",
        "Content-Type",
    ):
        assert forbidden_semantic_envelope_mutation not in forbidden_semantic_case
    forbidden_header_target = '[ "$path:$preflight_for" = /api/not-in-openapi:get ]'
    forbidden_header_guard = f"      if {forbidden_header_target}; then\n"
    assert fake_fetch.count(forbidden_header_target) == 1
    assert rejected_preflight_branch.count(forbidden_header_guard) == 1
    assert forbidden_header_target not in fake_unknown_options_fetch
    forbidden_header_guard_start = rejected_preflight_branch.index(
        forbidden_header_guard
    )
    forbidden_header_guard_end = rejected_preflight_branch.index(
        "      fi\n",
        forbidden_header_guard_start,
    )
    forbidden_header_branch = rejected_preflight_branch[
        forbidden_header_guard_start:forbidden_header_guard_end
    ]
    assert forbidden_semantic_guard_start < forbidden_header_guard_start
    assert forbidden_header_selector_control not in forbidden_semantic_branch
    assert forbidden_semantic_selector_control not in forbidden_header_branch
    forbidden_header_case_marker = (
        f'        case "{forbidden_header_selector_control}" in\n'
    )
    forbidden_header_case_start = forbidden_header_branch.index(
        forbidden_header_case_marker
    )
    forbidden_header_case_end = forbidden_header_branch.index(
        "        esac\n",
        forbidden_header_case_start,
    ) + len("        esac\n")
    forbidden_header_case = forbidden_header_branch[
        forbidden_header_case_start:forbidden_header_case_end
    ]
    assert forbidden_header_case == (
        '        case "${CORS_PREFLIGHT_FORBIDDEN_HEADER_FAULT:-none}" in\n'
        "          none) ;;\n"
        "          allow_credentials)\n"
        "            printf '%s\\n' 'Access-Control-Allow-Credentials: true' \\\n"
        '              >>"$fetch_headers"\n'
        "            ;;\n"
        "          allow_headers)\n"
        "            printf '%s\\n' 'Access-Control-Allow-Headers: Content-Type' \\\n"
        '              >>"$fetch_headers"\n'
        "            ;;\n"
        "          allow_methods)\n"
        "            printf '%s\\n' 'Access-Control-Allow-Methods: GET' \\\n"
        '              >>"$fetch_headers"\n'
        "            ;;\n"
        "          allow_origin)\n"
        "            printf '%s\\n' \\\n"
        "              'Access-Control-Allow-Origin: https://conflict.invalid' \\\n"
        '              >>"$fetch_headers"\n'
        "            ;;\n"
        "          expose_headers)\n"
        "            printf '%s\\n' 'Access-Control-Expose-Headers: X-Pokecon-Secret' \\\n"
        '              >>"$fetch_headers"\n'
        "            ;;\n"
        "          max_age)\n"
        "            printf '%s\\n' 'Access-Control-Max-Age: 600' "
        '>>"$fetch_headers"\n'
        "            ;;\n"
        "          allow)\n"
        "            printf '%s\\n' 'Allow: GET' >>\"$fetch_headers\"\n"
        "            ;;\n"
        "          vary)\n"
        "            printf '%s\\n' 'Vary: Origin' >>\"$fetch_headers\"\n"
        "            ;;\n"
        "          *) return 2 ;;\n"
        "        esac\n"
    )
    forbidden_header_case_kinds = (
        "none",
        *(
            selector
            for selector, _header_name, _response_header in (
                cors_preflight_forbidden_header_faults
            )
        ),
    )
    forbidden_header_case_indices: list[int] = []
    for forbidden_header_case_kind in forbidden_header_case_kinds:
        forbidden_header_kind_marker = f"          {forbidden_header_case_kind})"
        assert forbidden_header_case.count(forbidden_header_kind_marker) == 1
        forbidden_header_case_indices.append(
            forbidden_header_case.index(forbidden_header_kind_marker)
        )
    assert forbidden_header_case_indices == sorted(forbidden_header_case_indices)
    forbidden_header_unknown_marker = "          *) return 2 ;;"
    assert forbidden_header_case.count(forbidden_header_unknown_marker) == 1
    assert forbidden_header_case_indices[-1] < forbidden_header_case.index(
        forbidden_header_unknown_marker
    )
    for (
        _selector,
        _header_name,
        response_header,
    ) in cors_preflight_forbidden_header_faults:
        assert forbidden_header_case.count(response_header) == 1
    assert forbidden_header_case.count('>>"$fetch_headers"') == 8
    for forbidden_header_case_excluded_mutation in (
        "fetch_status",
        "fetch_code",
        "fetch_body",
    ):
        assert forbidden_header_case_excluded_mutation not in forbidden_header_case
    legacy_duplicate_cors_header_block = (
        '      if [ "${DUPLICATE_CORS_HEADER:-0}" = 1 ]; then\n'
        "        printf '%s\\n' "
        "'Access-Control-Allow-Origin: https://conflict.invalid' \\\n"
        '          >>"$fetch_headers"\n'
        "      fi\n"
    )
    assert fake_fetch.count(legacy_duplicate_cors_header_block) == 1
    assert successful_preflight_branch.count(legacy_duplicate_cors_header_block) == 1
    assert successful_preflight_branch.index(legacy_duplicate_cors_header_block) < (
        advertised_success_only_start
    )
    assert required_header_selector_control not in legacy_duplicate_cors_header_block
    canonical_cors_success_header_block = (
        "      printf '%s\\n' \\\n"
        "        'HTTP/1.1 204 No Content' \\\n"
        '        "Access-Control-Allow-Origin: http://localhost:$port" \\\n'
        "        'Access-Control-Allow-Methods: GET, PATCH, POST, OPTIONS' \\\n"
        "        'Access-Control-Allow-Headers: Content-Type, X-Pokecon-Request' \\\n"
        "        'Vary: Origin' >\"$fetch_headers\"\n"
    )
    assert fake_fetch.count(canonical_cors_success_header_block) == 1
    assert successful_preflight_branch.index(canonical_cors_success_header_block) < (
        successful_preflight_branch.index(legacy_duplicate_cors_header_block)
    )
    required_header_array_load = (
        "        local -a cors_success_response_headers=()\n"
        '        mapfile -t cors_success_response_headers <"$fetch_headers"\n'
    )
    required_header_array_write = (
        "        printf '%s\\n' \"${cors_success_response_headers[@]}\" "
        '>"$fetch_headers"\n'
    )
    assert advertised_success_only_branch.count(required_header_array_load) == 1
    assert advertised_success_only_branch.count(required_header_array_write) == 1
    required_header_case_start = advertised_success_only_branch.index(
        required_header_case_marker
    )
    required_header_case_end = advertised_success_only_branch.index(
        "        esac\n",
        required_header_case_start,
    ) + len("        esac\n")
    required_header_case = advertised_success_only_branch[
        required_header_case_start:required_header_case_end
    ]
    assert required_header_case == (
        '        case "${CORS_SUCCESS_REQUIRED_HEADER_FAULT:-none}" in\n'
        "          none) ;;\n"
        "          origin_missing)\n"
        "            unset 'cors_success_response_headers[1]'\n"
        "            ;;\n"
        "          origin_wrong)\n"
        "            cors_success_response_headers[1]="
        "'Access-Control-Allow-Origin: https://wrong.invalid'\n"
        "            ;;\n"
        "          origin_duplicate_conflicting)\n"
        "            cors_success_response_headers+=(\n"
        "              'Access-Control-Allow-Origin: https://conflict.invalid'\n"
        "            )\n"
        "            ;;\n"
        "          methods_missing)\n"
        "            unset 'cors_success_response_headers[2]'\n"
        "            ;;\n"
        "          methods_wrong)\n"
        "            cors_success_response_headers[2]="
        "'Access-Control-Allow-Methods: GET, OPTIONS'\n"
        "            ;;\n"
        "          methods_duplicate_conflicting)\n"
        "            cors_success_response_headers+=(\n"
        "              'Access-Control-Allow-Methods: GET, OPTIONS'\n"
        "            )\n"
        "            ;;\n"
        "          headers_missing)\n"
        "            unset 'cors_success_response_headers[3]'\n"
        "            ;;\n"
        "          headers_wrong)\n"
        "            cors_success_response_headers[3]="
        "'Access-Control-Allow-Headers: Content-Type'\n"
        "            ;;\n"
        "          headers_duplicate_conflicting)\n"
        "            cors_success_response_headers+=(\n"
        "              'Access-Control-Allow-Headers: Content-Type'\n"
        "            )\n"
        "            ;;\n"
        "          vary_missing)\n"
        "            unset 'cors_success_response_headers[4]'\n"
        "            ;;\n"
        "          vary_wrong)\n"
        "            cors_success_response_headers[4]='Vary: Accept-Encoding'\n"
        "            ;;\n"
        "          vary_duplicate_conflicting)\n"
        "            cors_success_response_headers+=(\n"
        "              'Vary: Accept-Encoding'\n"
        "            )\n"
        "            ;;\n"
        "          *) return 2 ;;\n"
        "        esac\n"
    )
    required_header_case_kinds = (
        "none",
        *(
            selector
            for selector, _header_name, _expected_value, _fault_kind in (
                cors_success_required_header_faults
            )
        ),
    )
    required_header_case_indices: list[int] = []
    for required_header_case_kind in required_header_case_kinds:
        required_header_kind_marker = f"          {required_header_case_kind})"
        assert required_header_case.count(required_header_kind_marker) == 1
        required_header_case_indices.append(
            required_header_case.index(required_header_kind_marker)
        )
    required_header_unknown_marker = "          *) return 2 ;;"
    assert required_header_case_indices == sorted(required_header_case_indices)
    assert required_header_case.count(required_header_unknown_marker) == 1
    assert required_header_case_indices[-1] < required_header_case.index(
        required_header_unknown_marker
    )
    for forbidden_required_header_bypass in (
        '"$mode"',
        '"$label"',
        "fixture",
        "/api/settings",
        '"$path"',
        '"$preflight_for"',
    ):
        assert forbidden_required_header_bypass not in required_header_case
    envelope_case_start = advertised_success_only_branch.index(envelope_case_marker)
    envelope_case_end = advertised_success_only_branch.index(
        "        esac\n",
        envelope_case_start,
    ) + len("        esac\n")
    envelope_case = advertised_success_only_branch[
        envelope_case_start:envelope_case_end
    ]
    assert envelope_case == (
        '        case "${CORS_SUCCESS_PREFLIGHT_ENVELOPE_KIND:-canonical}" in\n'
        "          canonical) ;;\n"
        "          transport_failure)\n"
        "            fetch_status=7\n"
        "            fetch_code=000\n"
        "            ;;\n"
        "          wrong_http_status)\n"
        "            fetch_code=200\n"
        "            ;;\n"
        "          nonempty_body)\n"
        "            printf '%s' 'forbidden successful CORS preflight response body' \\\n"
        '              >"$fetch_body"\n'
        "            ;;\n"
        "          *) return 2 ;;\n"
        "        esac\n"
    )
    assert selector_control not in envelope_case
    assert required_header_selector_control not in envelope_case
    assert envelope_selector_control not in required_header_case
    assert selector_control not in required_header_case
    required_header_array_load_start = advertised_success_only_branch.index(
        required_header_array_load
    )
    required_header_array_write_start = advertised_success_only_branch.index(
        required_header_array_write
    )
    assert (
        required_header_array_load_start + len(required_header_array_load)
        <= required_header_case_start
        < required_header_case_end
        <= required_header_array_write_start
    )
    assert (
        required_header_array_write_start + len(required_header_array_write)
        <= envelope_case_start
    )
    selector_case_start = advertised_success_only_branch.index(selector_case_marker)
    selector_case_end = advertised_success_only_branch.index(
        "        esac\n",
        selector_case_start,
    )
    selector_case = advertised_success_only_branch[
        selector_case_start:selector_case_end
    ]
    assert envelope_selector_control not in selector_case
    assert required_header_selector_control not in selector_case
    assert envelope_case_end <= selector_case_start
    selector_case_kinds = (
        "none",
        *(
            selector
            for selector, _header_name, _response_header in cors_success_forbidden_header_faults
        ),
    )
    selector_case_indices: list[int] = []
    for selector_case_kind in selector_case_kinds:
        selector_case_marker = f"          {selector_case_kind})"
        assert selector_case.count(selector_case_marker) == 1
        selector_case_indices.append(selector_case.index(selector_case_marker))
    assert selector_case_indices == sorted(selector_case_indices)
    assert selector_case.count("          *) return 2 ;;") == 1
    assert selector_case_indices[-1] < selector_case.index("          *) return 2 ;;")
    for (
        _selector,
        _header_name,
        response_header,
    ) in cors_success_forbidden_header_faults:
        assert selector_case.count(response_header) == 1
    assert (
        fake_fetch.count(
            "${CORS_PREFLIGHT_FORBIDDEN_MESSAGE:-request validation failed}"
        )
        == 1
    )
    assert (
        fake_unknown_options_fetch.count(
            "${UNKNOWN_OPTIONS_FORBIDDEN_MESSAGE:-request validation failed}"
        )
        == 1
    )
    assert "CORS_PREFLIGHT_FORBIDDEN_MESSAGE" not in fake_unknown_options_fetch
    assert "UNKNOWN_OPTIONS_FORBIDDEN_MESSAGE" not in fake_fetch
    for advertised_bare_options_control in (
        "${ADVERTISED_BARE_OPTIONS_STATUS:-403}",
        "${ADVERTISED_BARE_OPTIONS_CODE:-request_forbidden}",
        "${ADVERTISED_BARE_OPTIONS_MESSAGE:-request validation failed}",
        "${ADVERTISED_BARE_OPTIONS_BODY_KIND:-canonical}",
        "${ADVERTISED_BARE_OPTIONS_CONTENT_TYPE:-application/json}",
        "${ADVERTISED_BARE_OPTIONS_LEAKED_HEADER_KIND:-none}",
    ):
        assert (
            fake_advertised_bare_options_fetch.count(advertised_bare_options_control)
            == 1
        )
        assert advertised_bare_options_control not in fake_fetch
        assert advertised_bare_options_control not in fake_unknown_options_fetch
    body_kind_case_start = fake_advertised_bare_options_fetch.index(
        '  case "${ADVERTISED_BARE_OPTIONS_BODY_KIND:-canonical}" in\n'
    )
    body_kind_case = fake_advertised_bare_options_fetch[
        body_kind_case_start : fake_advertised_bare_options_fetch.index(
            "  esac\n", body_kind_case_start
        )
    ]
    for advertised_bare_options_body_kind in (
        "canonical",
        "non_json",
        "multiple_documents",
        "duplicate_keys",
    ):
        assert body_kind_case.count(f"    {advertised_bare_options_body_kind})") == 1
    assert body_kind_case.count("    *) return 2 ;;") == 1
    for forbidden_advertised_bare_request_proof in (
        "--header",
        "--data",
        "Access-Control-Request-Method",
        "Access-Control-Request-Headers",
        "X-Pokecon-Preflight-Method",
    ):
        assert (
            forbidden_advertised_bare_request_proof
            not in fake_advertised_bare_options_fetch
        )
    assert r"\tbare\t0\tfalse\tfalse\n" in fake_advertised_bare_options_fetch
    head_block_start = fake_fetch.index('  if [ "$method" = head ]; then\n')
    head_block_end = fake_fetch.index(
        "  local advertised=false\n",
        head_block_start,
    )
    head_block = fake_fetch[head_block_start:head_block_end]
    assert envelope_selector_control not in head_block
    assert selector_control not in head_block
    assert required_header_selector_control not in head_block
    unknown_head_branch_marker = '    if [ "$path" = /api/not-in-openapi ]; then\n'
    assert head_block.count(unknown_head_branch_marker) == 1
    unknown_head_branch_start = head_block.index(unknown_head_branch_marker)
    unknown_head_branch_end = head_block.index(
        "    else\n",
        unknown_head_branch_start,
    )
    unknown_head_branch = head_block[unknown_head_branch_start:unknown_head_branch_end]
    for unknown_head_control in (
        "${UNKNOWN_HEAD_CONTENT_TYPE:-application/json}",
        "${UNKNOWN_HEAD_BODY_KIND:-empty}",
    ):
        assert fake_fetch.count(unknown_head_control) == 1
        assert unknown_head_control in unknown_head_branch
        assert unknown_head_control not in fake_unknown_options_fetch
    advertised_head_branch_start = unknown_head_branch_end
    advertised_head_branch_end = head_block.index(
        "    fi\n    return\n",
        advertised_head_branch_start,
    )
    advertised_head_branch = head_block[
        advertised_head_branch_start:advertised_head_branch_end
    ]
    for advertised_head_control in (
        "${ADVERTISED_HEAD_STATUS:-405}",
        "${ADVERTISED_HEAD_CONTENT_TYPE_KIND:-canonical}",
        "${ADVERTISED_HEAD_CONTENT_LENGTH_KIND:-canonical}",
        "${ADVERTISED_HEAD_BODY_KIND:-empty}",
    ):
        assert fake_fetch.count(advertised_head_control) == 1
        assert advertised_head_control in advertised_head_branch
        assert advertised_head_control not in unknown_head_branch
        assert advertised_head_control not in fake_unknown_options_fetch
    assert "${UNKNOWN_HEAD_BODY_KIND:-empty}" not in advertised_head_branch
    assert "HEAD_FIXTURE_BODY" not in fake_fetch
    content_type_case_start = advertised_head_branch.index(
        '      case "${ADVERTISED_HEAD_CONTENT_TYPE_KIND:-canonical}" in\n'
    )
    content_type_case = advertised_head_branch[
        content_type_case_start : advertised_head_branch.index(
            "      esac\n", content_type_case_start
        )
    ]
    for content_type_kind in (
        "canonical",
        "wrong",
        "duplicate_conflicting",
        "missing",
    ):
        assert content_type_case.count(f"        {content_type_kind})") == 1
    assert content_type_case.count("        *) return 2 ;;") == 1
    assert (
        "'Content-Type: application/json' \\\n"
        "            'Content-Type: text/plain'" in content_type_case
    )
    content_length_case_start = advertised_head_branch.index(
        '      case "${ADVERTISED_HEAD_CONTENT_LENGTH_KIND:-canonical}" in\n'
    )
    content_length_case = advertised_head_branch[
        content_length_case_start : advertised_head_branch.index(
            "      esac\n", content_length_case_start
        )
    ]
    for content_length_kind in (
        "canonical",
        "zero",
        "non_numeric",
        "duplicate_conflicting",
        "missing",
    ):
        assert content_length_case.count(f"        {content_length_kind})") == 1
    assert content_length_case.count("        *) return 2 ;;") == 1
    assert (
        "'Content-Length: 123' \\\n"
        "            'Content-Length: 0'" in content_length_case
    )
    body_case_start = advertised_head_branch.index(
        '      case "${ADVERTISED_HEAD_BODY_KIND:-empty}" in\n'
    )
    body_case = advertised_head_branch[
        body_case_start : advertised_head_branch.index("      esac\n", body_case_start)
    ]
    for body_kind in ("empty", "nonempty"):
        assert body_case.count(f"        {body_kind})") == 1
    assert body_case.count("        *) return 2 ;;") == 1
    assert "forbidden HEAD response bytes" in body_case
    non_head_unknown_branch_start = fake_fetch.rindex(
        '  if [ "$path" = /api/not-in-openapi ]; then\n'
    )
    non_head_unknown_branch_end = fake_fetch.index(
        '  elif [ "$advertised" = true ]',
        non_head_unknown_branch_start,
    )
    non_head_unknown_branch = fake_fetch[
        non_head_unknown_branch_start:non_head_unknown_branch_end
    ]
    for unknown_non_head_control in (
        "${UNKNOWN_NON_HEAD_BODY_KIND:-canonical}",
        "${UNKNOWN_NON_HEAD_CODE:-resource_not_found}",
        "${UNKNOWN_NON_HEAD_MESSAGE:-API resource was not found}",
    ):
        assert fake_fetch.count(unknown_non_head_control) == 1
        assert unknown_non_head_control in non_head_unknown_branch
        assert unknown_non_head_control not in fake_unknown_options_fetch
    assert fake_fetch.index('>>"$root/request-invocations.tsv"') < fake_fetch.index(
        'fetch_body="$root/$prefix.body"'
    )
    assert fake_fetch.index('>>"$root/request-header-invocations.tsv"') < (
        fake_fetch.index('fetch_body="$root/$prefix.body"')
    )
    assert strict_json_decoder.count('"$project_python" -I -S -c') == 1
    assert strict_json_decoder.count("validate_unique_json_object_keys() {") == 1
    assert (
        strict_json_decoder.count("\n}\nreadonly -f validate_unique_json_object_keys")
        == 1
    )
    production_modes = ("web", "desktop")
    program = (
        "set -euo pipefail\n"
        'project_python="$0"\n'
        'openapi_operation_inventory="$1"\n'
        'mode="$3"\n'
        'probe_scope="$4"\n'
        "openapi_operation_count=16\nopenapi_path_count=15\n"
        "openapi_rest_path_count=14\nopenapi_websocket_path_count=1\n"
        "openapi_method_count=9\nopenapi_method_probe_count=135\n"
        "openapi_cors_preflight_count=15\nopenapi_rejected_method_count=104\n"
        + cors_success_forbidden_inventory_declaration
        + "\n"
        + "readonly -a advertised_bare_options_forbidden_response_headers=(Access-Control-Allow-Headers Access-Control-Allow-Methods Access-Control-Allow-Origin Allow Vary)\n"
        + 'advertised_bare_options_forbidden_response_headers_json=\'["Access-Control-Allow-Headers","Access-Control-Allow-Methods","Access-Control-Allow-Origin","Allow","Vary"]\'\n'
        + cors_rejection_forbidden_inventory_declaration
        + "\n"
        + cors_rejection_forbidden_inventory_serialization
        + "\n"
        + "readonly -a openapi_method_matrix=(get head post put patch delete options trace connect)\n"
        + "fail() { printf '%s\\n' \"$*\" >&2; exit 1; }\n"
        + strict_json_decoder
        + "\n\n"
        + require_json
        + "\n\n"
        + fake_fetch
        + "\n"
        + fake_unknown_options_fetch
        + "\n"
        + fake_advertised_bare_options_fetch
        + "\n"
        + require_head
        + "\n\n"
        + require_single_exact_header
        + "\n\n"
        + require_single_positive_decimal_header
        + "\n\n"
        + require_preflight
        + "\n\n"
        + require_forbidden_preflight
        + "\n\n"
        + require_exact_allow_header
        + "\n\n"
        + preflight_policy_probe
        + "\n\n"
        + probe
        + "\n\n"
        + require_resource_not_found
        + "\n\n"
        + require_head_resource_not_found
        + "\n\n"
        + unknown_api_boundary_probe
        + "\n\n"
        + absent_response_header
        + "\n\n"
        + advertised_bare_options_rejection
        + "\n\n"
        + advertised_bare_options_probe
        + "\n\n"
        + ': >"$2/request-invocations.tsv"\n'
        + ': >"$2/request-header-invocations.tsv"\n'
        + ': >"$2/advertised-bare-options-invocations.tsv"\n'
        + 'case "$probe_scope" in\n'
        + "  all)\n"
        + '    probe_openapi_method_matrix "$mode" 8020 "$2"\n'
        + '    probe_cors_preflight_policy "$mode" 8020 "$2"\n'
        + '    probe_unknown_api_boundary_matrix "$mode" 8020 "$2"\n'
        + '    probe_advertised_bare_options_security_boundary "$mode" 8020 "$2"\n'
        + "    ;;\n"
        + "  method)\n"
        + '    probe_openapi_method_matrix "$mode" 8020 "$2"\n'
        + "    ;;\n"
        + "  cors)\n"
        + '    probe_cors_preflight_policy "$mode" 8020 "$2"\n'
        + "    ;;\n"
        + "  unknown)\n"
        + '    probe_cors_preflight_policy "$mode" 8020 "$2"\n'
        + '    probe_unknown_api_boundary_matrix "$mode" 8020 "$2"\n'
        + "    ;;\n"
        + "  bare-options)\n"
        + '    probe_advertised_bare_options_security_boundary "$mode" 8020 "$2"\n'
        + "    ;;\n"
        + '  *) fail "unsupported OpenAPI matrix fixture probe scope: $probe_scope" ;;\n'
        + "esac\n"
    )
    assert production_modes == ("web", "desktop")
    assert program.count('mode="$3"') == 1
    assert program.count('probe_scope="$4"') == 1
    assert program.count('case "$probe_scope" in') == 1
    probe_scope_dispatcher = section(
        program,
        'case "$probe_scope" in\n',
        "\nesac",
    )
    for probe_scope in ("all", "method", "cors", "unknown", "bare-options"):
        assert probe_scope_dispatcher.count(f"  {probe_scope})\n") == 1
    assert (
        probe_scope_dispatcher.count(
            '*) fail "unsupported OpenAPI matrix fixture probe scope: $probe_scope" ;;'
        )
        == 1
    )
    assert program.count(cors_rejection_forbidden_inventory_declaration) == 1
    assert program.count(cors_rejection_forbidden_inventory_serialization) == 1
    assert (
        section(
            program,
            "readonly -a cors_rejection_forbidden_response_headers=(\n",
            "\n)",
        )
        == cors_rejection_forbidden_inventory_body
    )
    executable_forbidden_header_inventory_reference = (
        '"${cors_rejection_forbidden_response_headers[@]}"'
    )
    executable_forbidden_header_loop = (
        "for forbidden_name in "
        + executable_forbidden_header_inventory_reference
        + "; do"
    )
    executable_forbidden_header_check = (
        'require_absent_response_header "$forbidden_name" "$label"'
    )
    for executable_forbidden_header_dependency in (
        executable_forbidden_header_inventory_reference,
        executable_forbidden_header_loop,
        executable_forbidden_header_check,
    ):
        assert (
            require_forbidden_preflight.count(executable_forbidden_header_dependency)
            == 1
        )
    assert program.count(executable_forbidden_header_inventory_reference) == 2
    assert program.count('"$cors_rejection_forbidden_response_headers_json"') == 3
    assert program.count("cors_rejection_forbidden_response_headers_json=") == 1
    assert program.count("readonly cors_rejection_forbidden_response_headers_json") == 1
    cors_rejection_inventory_declaration_index = program.index(
        cors_rejection_forbidden_inventory_declaration
    )
    cors_rejection_inventory_serialization_index = program.index(
        cors_rejection_forbidden_inventory_serialization
    )
    assert (
        cors_rejection_inventory_declaration_index
        < cors_rejection_inventory_serialization_index
        < program.index("readonly -a openapi_method_matrix=(")
        < program.index("require_forbidden_preflight() {")
        < program.index("probe_cors_preflight_policy() {")
    )
    assert program.count(forbidden_semantic_selector_name) == 1
    assert program.count(forbidden_header_selector_name) == 1
    assert program.count("validate_unique_json_object_keys() {") == 1
    assert (
        program.count(
            "\n}\nreadonly -f validate_unique_json_object_keys"
            "\n\nrequire_json_response() {"
        )
        == 1
    )
    assert program.count("require_cors_preflight() {") == 1
    assert (
        program.count(
            "\n}\nreadonly -f require_cors_preflight\n\nrequire_forbidden_preflight() {"
        )
        == 1
    )
    assert program.count("require_forbidden_preflight() {") == 1
    assert (
        program.count(
            "\n}\nreadonly -f require_forbidden_preflight"
            "\n\nrequire_exact_allow_header() {"
        )
        == 1
    )
    expected_probe_scope_counts = {
        "probe_openapi_method_matrix": 2,
        "probe_cors_preflight_policy": 3,
        "probe_unknown_api_boundary_matrix": 2,
        "probe_advertised_bare_options_security_boundary": 2,
    }
    for probe_name, expected_count in expected_probe_scope_counts.items():
        assert (
            probe_scope_dispatcher.count(f'{probe_name} "$mode" 8020 "$2"')
            == expected_count
        )
        assert f"{probe_name} fixture " not in program
    cors_success_forbidden_declaration_start = (
        "readonly -a cors_success_forbidden_response_headers=(\n"
    )
    assert program.count(cors_success_forbidden_declaration_start) == 1
    assert program.count(cors_success_forbidden_inventory_declaration) == 1
    assert (
        section(
            program,
            cors_success_forbidden_declaration_start,
            "\n)",
        )
        == cors_success_forbidden_inventory_body
    )
    program_without_multiline_forbidden_inventories = program.replace(
        cors_success_forbidden_inventory_declaration,
        "",
        1,
    ).replace(
        cors_rejection_forbidden_inventory_declaration,
        "",
        1,
    )
    for inventory_entry in cors_success_forbidden_inventory_body.splitlines():
        assert (
            re.search(
                rf"(?m)^[ \t]*{re.escape(inventory_entry.strip())}[ \t]*$",
                program_without_multiline_forbidden_inventories,
            )
            is None
        )
    inventory = tmp_path / "inventory.json"
    inventory.write_text(
        json.dumps(
            [
                {"method": method, "operation_id": operation_id, "path": path}
                for path, method, operation_id in EXPECTED_OPENAPI_OPERATIONS
            ]
        )
    )
    result_roots = {mode: tmp_path / f"{mode}-result" for mode in production_modes}
    assert len(set(result_roots.values())) == len(production_modes)
    for baseline_root in result_roots.values():
        baseline_root.mkdir()
    bash = shutil.which("bash")
    jq = shutil.which("jq")
    grep = shutil.which("grep")
    stat = shutil.which("stat")
    sha256sum = shutil.which("sha256sum")
    cut = shutil.which("cut")
    python_executable = Path(sys.executable).resolve(strict=True)
    assert bash is not None
    assert jq is not None
    assert grep is not None
    assert stat is not None
    assert sha256sum is not None
    assert cut is not None
    fixture_path = ":".join(
        dict.fromkeys(
            str(Path(tool).parent) for tool in (jq, grep, stat, sha256sum, cut)
        )
    )
    fault_probe_scope_counts = {
        "bare-options": 0,
        "cors": 0,
        "method": 0,
        "unknown": 0,
    }

    def fault_probe_scope(scope: str) -> str:
        assert scope in fault_probe_scope_counts
        fault_probe_scope_counts[scope] += 1
        return scope

    def injected_fault_probe_scope(injected_environment: dict[str, str]) -> str:
        assert len(injected_environment) == 1
        injected_name = next(iter(injected_environment))
        matching_scopes = tuple(
            scope
            for prefix, scope in (
                ("ADVERTISED_BARE_OPTIONS_", "bare-options"),
                ("ADVERTISED_HEAD_", "method"),
                ("CORS_PREFLIGHT_", "cors"),
                ("UNKNOWN_", "unknown"),
            )
            if injected_name.startswith(prefix)
        )
        assert len(matching_scopes) == 1, injected_name
        return fault_probe_scope(matching_scopes[0])

    unique_json_decoder_program_lock_anchor = (
        "\n}\nreadonly -f validate_unique_json_object_keys\n\nrequire_json_response() {"
    )
    late_multiline_unique_json_decoder_program = program.replace(
        unique_json_decoder_program_lock_anchor,
        "\n}\nreadonly -f validate_unique_json_object_keys"
        "\n\nfunction validate_unique_json_object_keys()\n{\n  :\n}\n\n"
        "require_json_response() {",
        1,
    )
    assert late_multiline_unique_json_decoder_program != program
    decoder_locked_redefinition_root = tmp_path / "locked-unique-json-redefinition"
    decoder_locked_redefinition_root.mkdir()
    rejected_decoder_locked_redefinition = subprocess.run(  # noqa: S603
        [
            bash,
            "-c",
            late_multiline_unique_json_decoder_program,
            str(python_executable),
            str(inventory),
            str(decoder_locked_redefinition_root),
            production_modes[0],
            "all",
        ],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        env={
            "LC_ALL": "C",
            "PATH": fixture_path,
        },
        text=True,
        timeout=20,
    )
    assert rejected_decoder_locked_redefinition.returncode != 0
    assert rejected_decoder_locked_redefinition.stdout == ""
    assert len(rejected_decoder_locked_redefinition.stderr.splitlines()) == 1
    assert (
        re.fullmatch(
            rf"{re.escape(str(python_executable))}: line [0-9]+: "
            r"validate_unique_json_object_keys: readonly function",
            rejected_decoder_locked_redefinition.stderr.rstrip("\n"),
        )
        is not None
    )
    assert not decoder_locked_redefinition_root.joinpath(
        "request-invocations.tsv"
    ).exists()
    forbidden_preflight_program_lock_anchor = (
        "\n}\nreadonly -f require_forbidden_preflight\n\nrequire_exact_allow_header() {"
    )
    late_multiline_forbidden_preflight_program = program.replace(
        forbidden_preflight_program_lock_anchor,
        "\n}\nreadonly -f require_forbidden_preflight"
        "\n\nfunction require_forbidden_preflight()\n{\n  :\n}\n\n"
        "require_exact_allow_header() {",
        1,
    )
    assert late_multiline_forbidden_preflight_program != program
    locked_redefinition_root = tmp_path / "locked-forbidden-preflight-redefinition"
    locked_redefinition_root.mkdir()
    rejected_locked_redefinition = subprocess.run(  # noqa: S603
        [
            bash,
            "-c",
            late_multiline_forbidden_preflight_program,
            str(python_executable),
            str(inventory),
            str(locked_redefinition_root),
            production_modes[0],
            "all",
        ],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        env={
            "LC_ALL": "C",
            "PATH": fixture_path,
        },
        text=True,
        timeout=20,
    )
    assert rejected_locked_redefinition.returncode != 0
    assert rejected_locked_redefinition.stdout == ""
    assert len(rejected_locked_redefinition.stderr.splitlines()) == 1
    assert (
        re.fullmatch(
            rf"{re.escape(str(python_executable))}: line [0-9]+: "
            r"require_forbidden_preflight: readonly function",
            rejected_locked_redefinition.stderr.rstrip("\n"),
        )
        is not None
    )
    assert not locked_redefinition_root.joinpath("request-invocations.tsv").exists()
    completed_baseline_modes: list[str] = []
    for mode in production_modes:
        completed = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(result_roots[mode]),
                mode,
                "all",
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert completed.returncode == 0, completed.stderr
        assert completed.stdout == ""
        assert completed.stderr == ""
        completed_baseline_modes.append(mode)
    assert tuple(completed_baseline_modes) == production_modes
    result_root = result_roots["web"]

    methods_by_path: dict[str, list[str]] = {}
    for path, method, _operation_id in EXPECTED_OPENAPI_OPERATIONS:
        methods_by_path.setdefault(path, []).append(method)
    method_matrix = (
        "get",
        "head",
        "post",
        "put",
        "patch",
        "delete",
        "options",
        "trace",
        "connect",
    )
    expected_invocations: list[tuple[str, str, str]] = []
    for path in sorted(methods_by_path):
        advertised = methods_by_path[path]
        if "patch" in advertised:
            preflight_for = "patch"
        elif "post" in advertised:
            preflight_for = "post"
        else:
            preflight_for = "get"
        expected_invocations.extend(
            (path, method, preflight_for) for method in method_matrix
        )
    expected_invocations.extend(
        (path, "options", method)
        for path, method, _operation_id in sorted(
            EXPECTED_OPENAPI_OPERATIONS,
            key=lambda operation: (operation[0], operation[1]),
        )
    )
    expected_invocations.extend(
        (
            ("/api/settings", "options", "post"),
            ("/api/camera/retry", "options", "get"),
            ("/api/state", "options", "head"),
            ("/api/settings", "options", "put"),
            ("/api/state", "options", "delete"),
            ("/api/state", "options", "connect"),
            ("/api/not-in-openapi", "options", "get"),
        )
    )
    expected_invocations.extend(
        ("/api/not-in-openapi", method, "get")
        for method in method_matrix
        if method != "options"
    )
    expected_invocations.extend(
        ("/api/not-in-openapi", "options", request_variant)
        for request_variant in ("bare", "origin_only", "requested_method_only")
    )
    invocations: list[tuple[str, str, str]] = []
    for record in (result_root / "request-invocations.tsv").read_text().splitlines():
        fields = record.split("\t")
        assert len(fields) == 3
        invocations.append((fields[0], fields[1], fields[2]))
    assert len(invocations) == 169
    assert sorted(invocations) == sorted(expected_invocations)
    assert invocations == expected_invocations
    attached_request_headers = (
        (result_root / "request-header-invocations.tsv").read_text().splitlines()
    )
    assert attached_request_headers == [
        "/api/state\toptions\tconnect\tX-Pokecon-Preflight-Method\tGET"
    ]
    expected_advertised_paths = sorted(methods_by_path)
    advertised_bare_options_invocations = [
        tuple(record.split("\t"))
        for record in (result_root / "advertised-bare-options-invocations.tsv")
        .read_text()
        .splitlines()
    ]
    assert advertised_bare_options_invocations == [
        (path, "options", "bare", "0", "false", "false")
        for path in expected_advertised_paths
    ]
    assert len(advertised_bare_options_invocations) == 15
    assert (
        sum(
            invocation[0] == "/api/settings"
            for invocation in advertised_bare_options_invocations
        )
        == 1
    )
    assert (
        sum(
            invocation[0] == "/ws" for invocation in advertised_bare_options_invocations
        )
        == 1
    )

    advertised_bare_options_results = json.loads(
        (result_root / "advertised-bare-options-results.json").read_text()
    )
    assert [result["path"] for result in advertised_bare_options_results] == (
        expected_advertised_paths
    )
    assert len(advertised_bare_options_results) == 15
    assert len({result["path"] for result in advertised_bare_options_results}) == 15
    assert (
        sum(result["path_kind"] == "rest" for result in advertised_bare_options_results)
        == 14
    )
    assert [
        result["path"]
        for result in advertised_bare_options_results
        if result["path_kind"] == "websocket"
    ] == ["/ws"]
    assert (
        len({result["body_sha256"] for result in advertised_bare_options_results}) == 1
    )
    assert all(
        set(result)
        == {
            "access_control_request_method_header_sent",
            "body_sha256",
            "classification",
            "content_type",
            "error",
            "forbidden_response_headers_absent",
            "method",
            "origin_header_sent",
            "path",
            "path_kind",
            "request_body_bytes",
            "request_variant",
            "response_class",
            "status",
        }
        and result["access_control_request_method_header_sent"] is False
        and re.fullmatch(r"[0-9a-f]{64}", result["body_sha256"]) is not None
        and result["classification"] == "advertised_path_security_rejected"
        and result["content_type"] == "application/json"
        and result["error"]
        == {
            "code": "request_forbidden",
            "fields": None,
            "message": "request validation failed",
        }
        and result["forbidden_response_headers_absent"]
        == [
            "Access-Control-Allow-Headers",
            "Access-Control-Allow-Methods",
            "Access-Control-Allow-Origin",
            "Allow",
            "Vary",
        ]
        and result["method"] == "options"
        and result["origin_header_sent"] is False
        and result["request_body_bytes"] == 0
        and result["request_variant"] == "bare"
        and result["response_class"] == "request_forbidden"
        and result["status"] == 403
        for result in advertised_bare_options_results
    )

    expected_cors_rejection_forbidden_response_headers: list[JsonValue] = [
        line.strip() for line in cors_rejection_forbidden_inventory_body.splitlines()
    ]
    assert expected_cors_rejection_forbidden_response_headers == [
        "Access-Control-Allow-Credentials",
        "Access-Control-Allow-Headers",
        "Access-Control-Allow-Methods",
        "Access-Control-Allow-Origin",
        "Access-Control-Expose-Headers",
        "Access-Control-Max-Age",
        "Allow",
        "Vary",
    ]
    canonical_cors_rejection_error: dict[str, JsonValue] = {
        "code": "request_forbidden",
        "fields": None,
        "message": "request validation failed",
    }
    canonical_cors_rejection_body = (
        b'{"error":{"code":"request_forbidden","fields":null,'
        b'"message":"request validation failed"}}\n'
    )
    canonical_cors_rejection_body_sha256 = hashlib.sha256(
        canonical_cors_rejection_body
    ).hexdigest()
    assert canonical_cors_rejection_body_sha256 == (
        "aa3676e84888343c334c4f1ecdc6df640a6e1274d0e21ceaedd8dbdef4efda9b"
    )
    canonical_fake_cors_rejection_body = (
        "      printf '%s\\n' \\\n"
        '        "{\\"error\\":{\\"code\\":\\"request_forbidden\\",'
        '\\"fields\\":null,\\"message\\":'
        '\\"${CORS_PREFLIGHT_FORBIDDEN_MESSAGE:-request validation failed}\\"}}" \\\n'
        '        >"$fetch_body"'
    )
    assert fake_fetch.count(canonical_fake_cors_rejection_body) == 1

    known_wrong_cors_preflight_pairs = (
        ("/api/settings", "post"),
        ("/api/camera/retry", "get"),
        ("/api/state", "head"),
        ("/api/settings", "put"),
        ("/api/state", "delete"),
        ("/api/state", "connect"),
    )
    expected_preflight_results: list[dict[str, JsonValue]] = [
        {
            "classification": "advertised_pair",
            "operation_id": operation_id,
            "path": path,
            "requested_method": method,
            "response_class": "cors_preflight",
            "status": 204,
        }
        for path, method, operation_id in EXPECTED_OPENAPI_OPERATIONS
    ]
    expected_preflight_results.extend(
        {
            "body_sha256": canonical_cors_rejection_body_sha256,
            "classification": "known_path_wrong_method",
            "content_type": "application/json",
            "error": canonical_cors_rejection_error,
            "forbidden_response_headers_absent": (
                expected_cors_rejection_forbidden_response_headers
            ),
            "operation_id": None,
            "path": path,
            "requested_method": requested_method,
            "response_class": "request_forbidden",
            "status": 403,
        }
        for path, requested_method in known_wrong_cors_preflight_pairs
    )
    expected_preflight_results.append(
        {
            "body_sha256": canonical_cors_rejection_body_sha256,
            "classification": "unknown_path",
            "content_type": "application/json",
            "error": canonical_cors_rejection_error,
            "forbidden_response_headers_absent": (
                expected_cors_rejection_forbidden_response_headers
            ),
            "operation_id": None,
            "path": "/api/not-in-openapi",
            "requested_method": "get",
            "response_class": "request_forbidden",
            "status": 403,
        }
    )

    def cors_preflight_result_sort_key(
        result: dict[str, JsonValue],
    ) -> tuple[str, str, str]:
        path = result["path"]
        requested_method = result["requested_method"]
        classification = result["classification"]
        assert isinstance(path, str), result
        assert isinstance(requested_method, str), result
        assert isinstance(classification, str), result
        return path, requested_method, classification

    expected_preflight_results.sort(key=cors_preflight_result_sort_key)

    def classification_first_cors_preflight_result_sort_key(
        result: dict[str, JsonValue],
    ) -> tuple[str, str, str]:
        path, requested_method, classification = cors_preflight_result_sort_key(result)
        return classification, requested_method, path

    classification_first_preflight_results = sorted(
        expected_preflight_results,
        key=classification_first_cors_preflight_result_sort_key,
    )
    assert classification_first_preflight_results != expected_preflight_results
    assert len(expected_preflight_results) == 23
    cors_preflight_policy_raw_by_mode = {
        mode: result_roots[mode]
        .joinpath("cors-preflight-policy-results.json")
        .read_bytes()
        for mode in production_modes
    }
    assert len(set(cors_preflight_policy_raw_by_mode.values())) == 1
    expected_preflight_results_text = (
        json.dumps(expected_preflight_results, indent=2, sort_keys=True) + "\n"
    )
    expected_preflight_results_bytes = expected_preflight_results_text.encode()
    for raw_results in cors_preflight_policy_raw_by_mode.values():
        assert raw_results == expected_preflight_results_bytes
        assert raw_results.decode() == expected_preflight_results_text
        assert raw_results.endswith(b"\n")
        assert not raw_results.endswith(b"\n\n")
    preflight_results = json.loads(
        cors_preflight_policy_raw_by_mode[production_modes[0]]
    )
    assert preflight_results == expected_preflight_results
    advertised_preflight_results = [
        result
        for result in preflight_results
        if result["classification"] == "advertised_pair"
    ]
    rejected_preflight_results = [
        result
        for result in preflight_results
        if result["classification"] != "advertised_pair"
    ]
    assert len(advertised_preflight_results) == 16
    assert len(rejected_preflight_results) == 7
    cors_rejection_artifact_fields = (
        "body_sha256",
        "content_type",
        "error",
        "forbidden_response_headers_absent",
    )
    assert all(
        set(result)
        == {
            "classification",
            "operation_id",
            "path",
            "requested_method",
            "response_class",
            "status",
        }
        and not set(result).intersection(cors_rejection_artifact_fields)
        for result in advertised_preflight_results
    )
    assert all(
        set(result)
        == {
            "body_sha256",
            "classification",
            "content_type",
            "error",
            "forbidden_response_headers_absent",
            "operation_id",
            "path",
            "requested_method",
            "response_class",
            "status",
        }
        and result["body_sha256"] == canonical_cors_rejection_body_sha256
        and result["content_type"] == "application/json"
        and result["error"] == canonical_cors_rejection_error
        and result["forbidden_response_headers_absent"]
        == expected_cors_rejection_forbidden_response_headers
        for result in rejected_preflight_results
    )
    assert {result["body_sha256"] for result in rejected_preflight_results} == {
        canonical_cors_rejection_body_sha256
    }
    cors_preflight_policy_digest_by_mode = {
        mode: hashlib.sha256(raw_results).hexdigest()
        for mode, raw_results in cors_preflight_policy_raw_by_mode.items()
    }
    expected_cors_preflight_policy_results_sha256 = (
        "83da600ed7e6ad247f111c9ad41ce55295ce99e1599521346cc48ca2fb306dd9"
    )
    assert (
        re.fullmatch(
            r"[0-9a-f]{64}",
            expected_cors_preflight_policy_results_sha256,
        )
        is not None
    )
    assert len(set(cors_preflight_policy_digest_by_mode.values())) == 1
    assert set(cors_preflight_policy_digest_by_mode.values()) == {
        expected_cors_preflight_policy_results_sha256
    }

    unknown_boundary_results = json.loads(
        (result_root / "unknown-api-boundary-results.json").read_text()
    )
    assert len(unknown_boundary_results) == 12
    assert (
        len(
            {
                (result["method"], result["request_variant"])
                for result in unknown_boundary_results
            }
        )
        == 12
    )
    assert {result["method"] for result in unknown_boundary_results} == set(
        method_matrix
    )
    routed_unknown_results = [
        result
        for result in unknown_boundary_results
        if result["classification"] == "routed_not_found"
    ]
    assert len(routed_unknown_results) == 8
    assert {result["method"] for result in routed_unknown_results} == (
        set(method_matrix) - {"options"}
    )
    assert all(
        result["path"] == "/api/not-in-openapi"
        and result["request_variant"] == "valid_boundary_headers"
        and result["status"] == 404
        and result["content_length"] > 0
        and result["response_class"]
        == (
            "resource_not_found_head"
            if result["method"] == "head"
            else "resource_not_found"
        )
        for result in routed_unknown_results
    )
    assert len({result["content_length"] for result in routed_unknown_results}) == 1
    rejected_unknown_options = [
        result
        for result in unknown_boundary_results
        if result["classification"] == "security_rejected_options"
    ]
    assert len(rejected_unknown_options) == 4
    assert {result["request_variant"] for result in rejected_unknown_options} == {
        "bare",
        "origin_only",
        "requested_method_only",
        "complete_preflight",
    }
    assert all(
        result["content_length"] is None
        and result["method"] == "options"
        and result["path"] == "/api/not-in-openapi"
        and result["response_class"] == "request_forbidden"
        and result["status"] == 403
        for result in rejected_unknown_options
    )

    existing_fault_mode = production_modes[0]
    fail_open_root = tmp_path / "fail-open-preflight-result"
    fail_open_root.mkdir()
    fail_open = subprocess.run(  # noqa: S603
        [
            bash,
            "-c",
            program,
            str(python_executable),
            str(inventory),
            str(fail_open_root),
            existing_fault_mode,
            fault_probe_scope("cors"),
        ],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        env={
            "FAIL_OPEN_PREFLIGHTS": "1",
            "LC_ALL": "C",
            "PATH": fixture_path,
        },
        text=True,
        timeout=20,
    )
    assert fail_open.returncode != 0
    assert "HTTP 204" in fail_open.stderr
    assert "unadvertised CORS preflight" in fail_open.stderr

    duplicate_cors_root = tmp_path / "duplicate-cors-header-result"
    duplicate_cors_root.mkdir()
    rejected_duplicate_cors = subprocess.run(  # noqa: S603
        [
            bash,
            "-c",
            program,
            str(python_executable),
            str(inventory),
            str(duplicate_cors_root),
            existing_fault_mode,
            fault_probe_scope("cors"),
        ],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        env={
            "DUPLICATE_CORS_HEADER": "1",
            "LC_ALL": "C",
            "PATH": fixture_path,
        },
        text=True,
        timeout=20,
    )
    assert rejected_duplicate_cors.returncode != 0
    assert (
        "did not return exactly one Access-Control-Allow-Origin header"
        in rejected_duplicate_cors.stderr
    )

    later_forbidden_fault_invocation = "/api/settings\toptions\tput"
    forbidden_envelope_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for selector, expected_diagnostic in cors_preflight_forbidden_envelope_faults:
            forbidden_envelope_root = (
                tmp_path / f"{mode}-cors-forbidden-envelope-{selector}"
            )
            forbidden_envelope_root.mkdir()
            rejected_forbidden_envelope = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(forbidden_envelope_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_PREFLIGHT_FORBIDDEN_ENVELOPE_FAULT": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_forbidden_envelope.returncode != 0
            assert rejected_forbidden_envelope.stdout == ""
            assert rejected_forbidden_envelope.stderr.splitlines() == [
                f"{mode} unadvertised CORS preflight OPTIONS /api/settings for PUT "
                f"{expected_diagnostic}"
            ]
            assert (
                forbidden_envelope_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == later_forbidden_fault_invocation
            )
            forbidden_envelope_fault_coverage.add((mode, selector))
    assert forbidden_envelope_fault_coverage == {
        (mode, selector)
        for mode in production_modes
        for selector, _expected_diagnostic in (cors_preflight_forbidden_envelope_faults)
    }
    assert len(forbidden_envelope_fault_coverage) == len(production_modes) * 5 == 10

    completed_unknown_forbidden_envelope_modes: list[str] = []
    for mode in production_modes:
        unknown_forbidden_envelope_root = (
            tmp_path / f"{mode}-cors-forbidden-envelope-unknown"
        )
        unknown_forbidden_envelope_root.mkdir()
        rejected_unknown_forbidden_envelope = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_forbidden_envelope_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_PREFLIGHT_FORBIDDEN_ENVELOPE_FAULT": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_forbidden_envelope.returncode != 0
        assert rejected_unknown_forbidden_envelope.stdout == ""
        assert rejected_unknown_forbidden_envelope.stderr == ""
        assert (
            unknown_forbidden_envelope_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == later_forbidden_fault_invocation
        )
        completed_unknown_forbidden_envelope_modes.append(mode)
    assert tuple(completed_unknown_forbidden_envelope_modes) == production_modes

    later_forbidden_document_fault_invocation = "/api/state\toptions\tdelete"
    forbidden_document_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for selector, expected_diagnostic in cors_preflight_forbidden_document_faults:
            forbidden_document_root = (
                tmp_path / f"{mode}-cors-forbidden-document-{selector}"
            )
            forbidden_document_root.mkdir()
            rejected_forbidden_document = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(forbidden_document_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_PREFLIGHT_FORBIDDEN_DOCUMENT_FAULT": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_forbidden_document.returncode != 0
            assert rejected_forbidden_document.stdout == ""
            assert rejected_forbidden_document.stderr.splitlines() == [
                f"{mode} unadvertised CORS preflight OPTIONS /api/state for DELETE "
                f"{expected_diagnostic}"
            ]
            assert (
                forbidden_document_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == later_forbidden_document_fault_invocation
            )
            forbidden_document_fault_coverage.add((mode, selector))
    assert forbidden_document_fault_coverage == {
        (mode, selector)
        for mode in production_modes
        for selector, _expected_diagnostic in cors_preflight_forbidden_document_faults
    }
    assert len(forbidden_document_fault_coverage) == len(production_modes) * 4 == 8

    completed_unknown_forbidden_document_modes: list[str] = []
    for mode in production_modes:
        unknown_forbidden_document_root = (
            tmp_path / f"{mode}-cors-forbidden-document-unknown"
        )
        unknown_forbidden_document_root.mkdir()
        rejected_unknown_forbidden_document = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_forbidden_document_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_PREFLIGHT_FORBIDDEN_DOCUMENT_FAULT": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_forbidden_document.returncode != 0
        assert rejected_unknown_forbidden_document.stdout == ""
        assert rejected_unknown_forbidden_document.stderr == ""
        assert (
            unknown_forbidden_document_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == later_forbidden_document_fault_invocation
        )
        completed_unknown_forbidden_document_modes.append(mode)
    assert tuple(completed_unknown_forbidden_document_modes) == production_modes

    later_forbidden_semantic_fault_invocation = "/api/state\toptions\tconnect"
    forbidden_semantic_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for selector in cors_preflight_forbidden_semantic_faults:
            forbidden_semantic_root = (
                tmp_path / f"{mode}-cors-forbidden-semantic-{selector}"
            )
            forbidden_semantic_root.mkdir()
            rejected_forbidden_semantic = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(forbidden_semantic_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_PREFLIGHT_FORBIDDEN_SEMANTIC_FAULT": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_forbidden_semantic.returncode != 0
            assert rejected_forbidden_semantic.stdout == ""
            assert rejected_forbidden_semantic.stderr.splitlines() == [
                f"{mode} unadvertised CORS preflight OPTIONS /api/state for CONNECT "
                f"{forbidden_semantic_diagnostic}"
            ]
            assert (
                forbidden_semantic_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == later_forbidden_semantic_fault_invocation
            )
            forbidden_semantic_fault_coverage.add((mode, selector))
    assert forbidden_semantic_fault_coverage == {
        (mode, selector)
        for mode in production_modes
        for selector in cors_preflight_forbidden_semantic_faults
    }
    assert len(forbidden_semantic_fault_coverage) == len(production_modes) * 5 == 10

    completed_unknown_forbidden_semantic_modes: list[str] = []
    for mode in production_modes:
        unknown_forbidden_semantic_root = (
            tmp_path / f"{mode}-cors-forbidden-semantic-unknown"
        )
        unknown_forbidden_semantic_root.mkdir()
        rejected_unknown_forbidden_semantic = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_forbidden_semantic_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_PREFLIGHT_FORBIDDEN_SEMANTIC_FAULT": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_forbidden_semantic.returncode != 0
        assert rejected_unknown_forbidden_semantic.stdout == ""
        assert rejected_unknown_forbidden_semantic.stderr == ""
        assert (
            unknown_forbidden_semantic_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == later_forbidden_semantic_fault_invocation
        )
        completed_unknown_forbidden_semantic_modes.append(mode)
    assert tuple(completed_unknown_forbidden_semantic_modes) == production_modes

    terminal_forbidden_header_fault_invocation = "/api/not-in-openapi\toptions\tget"
    rejected_preflight_header_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for (
            selector,
            header_name,
            _response_header,
        ) in cors_preflight_forbidden_header_faults:
            forbidden_header_root = (
                tmp_path / f"{mode}-cors-forbidden-header-{selector}"
            )
            forbidden_header_root.mkdir()
            rejected_forbidden_header = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(forbidden_header_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_PREFLIGHT_FORBIDDEN_HEADER_FAULT": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_forbidden_header.returncode != 0
            assert rejected_forbidden_header.stdout == ""
            assert rejected_forbidden_header.stderr.splitlines() == [
                f"{mode} unknown-path CORS preflight OPTIONS /api/not-in-openapi "
                f"for GET returned forbidden {header_name} response header"
            ]
            assert (
                forbidden_header_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == terminal_forbidden_header_fault_invocation
            )
            rejected_preflight_header_fault_coverage.add((mode, selector))
    assert rejected_preflight_header_fault_coverage == {
        (mode, selector)
        for mode in production_modes
        for selector, _header_name, _response_header in (
            cors_preflight_forbidden_header_faults
        )
    }
    assert (
        len(rejected_preflight_header_fault_coverage) == len(production_modes) * 8 == 16
    )

    completed_unknown_forbidden_header_modes: list[str] = []
    for mode in production_modes:
        unknown_forbidden_header_root = (
            tmp_path / f"{mode}-cors-forbidden-header-unknown"
        )
        unknown_forbidden_header_root.mkdir()
        rejected_unknown_forbidden_header = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_forbidden_header_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_PREFLIGHT_FORBIDDEN_HEADER_FAULT": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_forbidden_header.returncode != 0
        assert rejected_unknown_forbidden_header.stdout == ""
        assert rejected_unknown_forbidden_header.stderr == ""
        assert (
            unknown_forbidden_header_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == terminal_forbidden_header_fault_invocation
        )
        completed_unknown_forbidden_header_modes.append(mode)
    assert tuple(completed_unknown_forbidden_header_modes) == production_modes

    later_advertised_fault_invocation = "/api/settings\toptions\tget"
    required_header_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for (
            selector,
            header_name,
            expected_value,
            _fault_kind,
        ) in cors_success_required_header_faults:
            required_header_root = tmp_path / f"{mode}-cors-success-required-{selector}"
            required_header_root.mkdir()
            rejected_required_header = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(required_header_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_SUCCESS_REQUIRED_HEADER_FAULT": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_required_header.returncode != 0
            assert rejected_required_header.stdout == ""
            assert rejected_required_header.stderr.splitlines() == [
                f"{mode} advertised CORS preflight OPTIONS /api/settings for GET "
                f"did not return exactly one {header_name} header with value "
                f"{expected_value}"
            ]
            assert (
                required_header_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == later_advertised_fault_invocation
            )
            required_header_fault_coverage.add((mode, selector))
    expected_required_header_fault_coverage = {
        (mode, selector)
        for mode in production_modes
        for selector, _header_name, _expected_value, _fault_kind in (
            cors_success_required_header_faults
        )
    }
    assert required_header_fault_coverage == expected_required_header_fault_coverage
    assert len(required_header_fault_coverage) == len(production_modes) * 12 == 24

    completed_unknown_required_header_modes: list[str] = []
    for mode in production_modes:
        unknown_required_header_root = (
            tmp_path / f"{mode}-cors-success-required-unknown"
        )
        unknown_required_header_root.mkdir()
        rejected_unknown_required_header = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_required_header_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_SUCCESS_REQUIRED_HEADER_FAULT": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_required_header.returncode != 0
        assert rejected_unknown_required_header.stdout == ""
        assert rejected_unknown_required_header.stderr == ""
        assert (
            unknown_required_header_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == later_advertised_fault_invocation
        )
        completed_unknown_required_header_modes.append(mode)
    assert tuple(completed_unknown_required_header_modes) == production_modes

    forbidden_header_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for (
            selector,
            forbidden_name,
            _response_header,
        ) in cors_success_forbidden_header_faults:
            forbidden_header_root = (
                tmp_path / f"{mode}-cors-success-forbidden-{selector}"
            )
            forbidden_header_root.mkdir()
            rejected_forbidden_header = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(forbidden_header_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_SUCCESS_FORBIDDEN_HEADER_KIND": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_forbidden_header.returncode != 0
            assert rejected_forbidden_header.stdout == ""
            assert rejected_forbidden_header.stderr.splitlines() == [
                f"{mode} advertised CORS preflight OPTIONS /api/settings for GET "
                f"returned forbidden {forbidden_name} response header"
            ]
            assert (
                forbidden_header_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == later_advertised_fault_invocation
            )
            forbidden_header_fault_coverage.add((mode, selector))
    assert forbidden_header_fault_coverage == {
        (mode, selector)
        for mode in production_modes
        for selector, _forbidden_name, _response_header in (
            cors_success_forbidden_header_faults
        )
    }

    completed_unknown_selector_modes: list[str] = []
    for mode in production_modes:
        unknown_forbidden_header_root = (
            tmp_path / f"{mode}-cors-success-forbidden-unknown"
        )
        unknown_forbidden_header_root.mkdir()
        rejected_unknown_forbidden_header = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_forbidden_header_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_SUCCESS_FORBIDDEN_HEADER_KIND": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_forbidden_header.returncode != 0
        assert rejected_unknown_forbidden_header.stdout == ""
        assert rejected_unknown_forbidden_header.stderr == ""
        assert (
            unknown_forbidden_header_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == later_advertised_fault_invocation
        )
        completed_unknown_selector_modes.append(mode)
    assert tuple(completed_unknown_selector_modes) == production_modes

    envelope_fault_coverage: set[tuple[str, str]] = set()
    for mode in production_modes:
        for selector, expected_diagnostic in cors_success_preflight_envelope_faults:
            envelope_fault_root = tmp_path / f"{mode}-cors-envelope-{selector}"
            envelope_fault_root.mkdir()
            rejected_envelope = subprocess.run(  # noqa: S603
                [
                    bash,
                    "-c",
                    program,
                    str(python_executable),
                    str(inventory),
                    str(envelope_fault_root),
                    mode,
                    fault_probe_scope("cors"),
                ],
                check=False,
                capture_output=True,
                cwd=REPOSITORY,
                env={
                    "CORS_SUCCESS_PREFLIGHT_ENVELOPE_KIND": selector,
                    "LC_ALL": "C",
                    "PATH": fixture_path,
                },
                text=True,
                timeout=20,
            )
            assert rejected_envelope.returncode != 0
            assert rejected_envelope.stdout == ""
            assert rejected_envelope.stderr.splitlines() == [
                f"{mode} advertised CORS preflight OPTIONS /api/settings for GET "
                f"{expected_diagnostic}"
            ]
            assert (
                envelope_fault_root.joinpath("request-invocations.tsv")
                .read_text()
                .splitlines()[-1]
                == later_advertised_fault_invocation
            )
            envelope_fault_coverage.add((mode, selector))
    assert envelope_fault_coverage == {
        (mode, selector)
        for mode in production_modes
        for selector, _expected_diagnostic in cors_success_preflight_envelope_faults
    }

    completed_unknown_envelope_modes: list[str] = []
    for mode in production_modes:
        unknown_envelope_root = tmp_path / f"{mode}-cors-envelope-unknown"
        unknown_envelope_root.mkdir()
        rejected_unknown_envelope = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(unknown_envelope_root),
                mode,
                fault_probe_scope("cors"),
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                "CORS_SUCCESS_PREFLIGHT_ENVELOPE_KIND": "unknown",
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected_unknown_envelope.returncode != 0
        assert rejected_unknown_envelope.stdout == ""
        assert rejected_unknown_envelope.stderr == ""
        assert (
            unknown_envelope_root.joinpath("request-invocations.tsv")
            .read_text()
            .splitlines()[-1]
            == later_advertised_fault_invocation
        )
        completed_unknown_envelope_modes.append(mode)
    assert tuple(completed_unknown_envelope_modes) == production_modes

    nonempty_head_root = tmp_path / "nonempty-head-result"
    nonempty_head_root.mkdir()
    rejected_head = subprocess.run(  # noqa: S603
        [
            bash,
            "-c",
            program,
            str(python_executable),
            str(inventory),
            str(nonempty_head_root),
            existing_fault_mode,
            fault_probe_scope("method"),
        ],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        env={
            "ADVERTISED_HEAD_BODY_KIND": "nonempty",
            "LC_ALL": "C",
            "PATH": fixture_path,
        },
        text=True,
        timeout=20,
    )
    assert rejected_head.returncode != 0
    assert rejected_head.stderr.splitlines() == [
        f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry "
        "returned response "
        "bytes after the HEAD headers"
    ]

    misleading_allow_root = tmp_path / "misleading-allow-result"
    misleading_allow_root.mkdir()
    rejected_misleading_allow = subprocess.run(  # noqa: S603
        [
            bash,
            "-c",
            program,
            str(python_executable),
            str(inventory),
            str(misleading_allow_root),
            existing_fault_mode,
            fault_probe_scope("method"),
        ],
        check=False,
        capture_output=True,
        cwd=REPOSITORY,
        env={
            "LC_ALL": "C",
            "MISLEADING_ALLOW": "1",
            "PATH": fixture_path,
        },
        text=True,
        timeout=20,
    )
    assert rejected_misleading_allow.returncode != 0
    assert "did not return exactly one Allow header" in rejected_misleading_allow.stderr

    for label, injected_environment, expected_diagnostic in (
        (
            "wrong-advertised-head-status",
            {"ADVERTISED_HEAD_STATUS": "200"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry returned curl "
            "status 0 and HTTP 200",
        ),
        (
            "wrong-advertised-head-content-type",
            {"ADVERTISED_HEAD_CONTENT_TYPE_KIND": "wrong"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Type header with value application/json",
        ),
        (
            "duplicate-conflicting-advertised-head-content-type",
            {"ADVERTISED_HEAD_CONTENT_TYPE_KIND": "duplicate_conflicting"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Type header with value application/json",
        ),
        (
            "missing-advertised-head-content-type",
            {"ADVERTISED_HEAD_CONTENT_TYPE_KIND": "missing"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Type header with value application/json",
        ),
        (
            "zero-advertised-head-content-length",
            {"ADVERTISED_HEAD_CONTENT_LENGTH_KIND": "zero"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Length header with a strict positive base-10 "
            "integer value",
        ),
        (
            "non-numeric-advertised-head-content-length",
            {"ADVERTISED_HEAD_CONTENT_LENGTH_KIND": "non_numeric"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Length header with a strict positive base-10 "
            "integer value",
        ),
        (
            "duplicate-conflicting-advertised-head-content-length",
            {"ADVERTISED_HEAD_CONTENT_LENGTH_KIND": "duplicate_conflicting"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Length header with a strict positive base-10 "
            "integer value",
        ),
        (
            "missing-advertised-head-content-length",
            {"ADVERTISED_HEAD_CONTENT_LENGTH_KIND": "missing"},
            f"{existing_fault_mode} unadvertised method HEAD /api/camera/retry did not return "
            "exactly one Content-Length header with a strict positive base-10 "
            "integer value",
        ),
        (
            "wrong-unknown-route-status",
            {"UNKNOWN_ROUTED_STATUS": "200"},
            "unknown API method GET returned curl status 0 and HTTP 200",
        ),
        (
            "wrong-unknown-non-head-code",
            {"UNKNOWN_NON_HEAD_CODE": "invalid_request"},
            f"{existing_fault_mode} unknown API method GET did not return the canonical "
            "resource_not_found rejection",
        ),
        (
            "wrong-unknown-non-head-message",
            {"UNKNOWN_NON_HEAD_MESSAGE": "fixture"},
            f"{existing_fault_mode} unknown API method GET did not return the canonical "
            "resource_not_found rejection",
        ),
        (
            "unknown-non-head-spa-body",
            {"UNKNOWN_NON_HEAD_BODY_KIND": "spa"},
            f"{existing_fault_mode} unknown API method GET did not return valid JSON",
        ),
        (
            "wrong-unknown-head-length",
            {"UNKNOWN_HEAD_LENGTH": "1"},
            "did not return exactly one Content-Length header",
        ),
        (
            "wrong-unknown-head-content-type",
            {"UNKNOWN_HEAD_CONTENT_TYPE": "text/plain"},
            f"{existing_fault_mode} unknown API method HEAD did not return exactly one "
            "Content-Type header with value application/json",
        ),
        (
            "wrong-unknown-head-wire-body",
            {"UNKNOWN_HEAD_BODY_KIND": "nonempty"},
            f"{existing_fault_mode} unknown API method HEAD returned response bytes after the "
            "HEAD headers",
        ),
        (
            "wrong-unknown-options-status",
            {"UNKNOWN_OPTIONS_STATUS": "404"},
            "unknown API OPTIONS boundary variant bare returned curl status 0 and HTTP 404",
        ),
        (
            "wrong-unknown-options-code",
            {"UNKNOWN_OPTIONS_CODE": "method_not_allowed"},
            "did not return the canonical request_forbidden rejection",
        ),
        (
            "wrong-cors-preflight-forbidden-message",
            {"CORS_PREFLIGHT_FORBIDDEN_MESSAGE": "fixture"},
            f"{existing_fault_mode} unadvertised CORS preflight OPTIONS /api/settings for POST "
            "did not return the canonical request_forbidden rejection",
        ),
        (
            "wrong-unknown-options-forbidden-message",
            {"UNKNOWN_OPTIONS_FORBIDDEN_MESSAGE": "fixture"},
            f"{existing_fault_mode} unknown API OPTIONS boundary variant bare "
            "did not return the canonical request_forbidden rejection",
        ),
        (
            "wrong-advertised-bare-options-status",
            {"ADVERTISED_BARE_OPTIONS_STATUS": "405"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry returned curl "
            "status 0 and HTTP 405",
        ),
        (
            "wrong-advertised-bare-options-code",
            {"ADVERTISED_BARE_OPTIONS_CODE": "method_not_allowed"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry did not return "
            "the canonical request_forbidden rejection",
        ),
        (
            "wrong-advertised-bare-options-message",
            {"ADVERTISED_BARE_OPTIONS_MESSAGE": "fixture"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry did not return "
            "the canonical request_forbidden rejection",
        ),
        (
            "non-json-advertised-bare-options-body",
            {"ADVERTISED_BARE_OPTIONS_BODY_KIND": "non_json"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry did not return "
            "valid JSON",
        ),
        (
            "multiple-json-documents-advertised-bare-options-body",
            {"ADVERTISED_BARE_OPTIONS_BODY_KIND": "multiple_documents"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry did not return "
            "exactly one JSON document with unique object keys",
        ),
        (
            "duplicate-json-keys-advertised-bare-options-body",
            {"ADVERTISED_BARE_OPTIONS_BODY_KIND": "duplicate_keys"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry did not return "
            "exactly one JSON document with unique object keys",
        ),
        (
            "wrong-advertised-bare-options-content-type",
            {"ADVERTISED_BARE_OPTIONS_CONTENT_TYPE": "text/plain"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry did not return "
            "exactly one Content-Type header with value application/json",
        ),
        (
            "advertised-bare-options-leaked-allow",
            {"ADVERTISED_BARE_OPTIONS_LEAKED_HEADER_KIND": "allow"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry returned "
            "forbidden Allow response header",
        ),
        (
            "advertised-bare-options-leaked-cors",
            {"ADVERTISED_BARE_OPTIONS_LEAKED_HEADER_KIND": "cors"},
            f"{existing_fault_mode} advertised bare OPTIONS /api/camera/retry returned "
            "forbidden Access-Control-Allow-Origin response header",
        ),
    ):
        selected_probe_scope = injected_fault_probe_scope(injected_environment)
        rejected_root = tmp_path / label
        rejected_root.mkdir()
        rejected = subprocess.run(  # noqa: S603
            [
                bash,
                "-c",
                program,
                str(python_executable),
                str(inventory),
                str(rejected_root),
                existing_fault_mode,
                selected_probe_scope,
            ],
            check=False,
            capture_output=True,
            cwd=REPOSITORY,
            env={
                **injected_environment,
                "LC_ALL": "C",
                "PATH": fixture_path,
            },
            text=True,
            timeout=20,
        )
        assert rejected.returncode != 0, label
        assert expected_diagnostic in rejected.stderr, label

    assert fault_probe_scope_counts == {
        "bare-options": 9,
        "cors": 103,
        "method": 10,
        "unknown": 10,
    }
    assert sum(fault_probe_scope_counts.values()) == 132

    results = json.loads((result_root / "openapi-method-results.json").read_text())
    assert len(results) == 135
    assert len({(result["path"], result["method"]) for result in results}) == 135
    assert {result["path"] for result in results} == {
        path for path, _method, _operation_id in EXPECTED_OPENAPI_OPERATIONS
    }
    assert {result["method"] for result in results} == {
        "get",
        "head",
        "post",
        "put",
        "patch",
        "delete",
        "options",
        "trace",
        "connect",
    }
    assert sum(result["classification"] == "advertised" for result in results) == 16
    assert sum(result["classification"] == "rejected" for result in results) == 104
    assert sum(result["classification"] == "cors_preflight" for result in results) == 15
    expected_allow_by_path: dict[str, str] = {}
    for path, method, _operation_id in EXPECTED_OPENAPI_OPERATIONS:
        methods = expected_allow_by_path.setdefault(path, "").split(", ")
        methods = [advertised for advertised in methods if advertised]
        methods.append(method.upper())
        expected_allow_by_path[path] = ", ".join(sorted(methods))
    assert all(
        result["allow"] == expected_allow_by_path[result["path"]]
        for result in results
        if result["classification"] == "rejected"
    )
    assert all(
        result["allow"] is None
        for result in results
        if result["classification"] != "rejected"
    )
    assert all(
        result["status"] == 405
        and result["response_class"] == "method_not_allowed_head"
        for result in results
        if result["method"] == "head"
    )
    assert all(
        result["status"] == 204
        and result["response_class"] == "cors_preflight"
        and result["preflight_for"] in {"get", "post", "patch"}
        for result in results
        if result["method"] == "options"
    )


def test_rust_routes_override_implicit_head_and_websocket_any() -> None:
    production_sources = load_production_routing_sources()
    assert_closed_production_routing(production_sources)

    websocket = (REPOSITORY / "rust/pokecon/src/server/websocket.rs").read_text()
    assert len(re.findall(r'\.route\(\s*"/ws"', websocket)) == 1
    assert "any(websocket_upgrade)" not in websocket
    for exact_websocket_proof in (
        "on(MethodFilter::GET, websocket_upgrade)",
        ".on(MethodFilter::HEAD, websocket_method_not_allowed)",
        ".fallback(websocket_method_not_allowed)",
        "Method::CONNECT",
        "websocket_route_accepts_only_get_from_the_standard_method_matrix",
    ):
        assert exact_websocket_proof in websocket

    exact_get_sources = {
        "devices": (2, "server/rest/devices.rs"),
        "settings": (1, "server/rest/settings.rs"),
        "state": (1, "server/rest/state.rs"),
    }
    for label, (expected_count, source_name) in exact_get_sources.items():
        source = (REPOSITORY / "rust/pokecon/src" / source_name).read_text()
        assert source.count("on(MethodFilter::GET") == expected_count, label
        assert (
            source.count(".on(MethodFilter::HEAD, method_not_allowed)")
            == expected_count
        ), label
        assert "routing::get" not in source, label

    rest = (REPOSITORY / "rust/pokecon/src/server/rest/mod.rs").read_text()
    security = (REPOSITORY / "rust/pokecon/src/server/security.rs").read_text()
    assert "normative_rest_routes_accept_exactly_the_advertised_method_matrix" in rest
    assert '"CONNECT"' in rest
    assert "assert_eq!(advertised_count, 15);" in rest
    assert "assert_eq!(rejected_count, 111);" in rest
    inner_unknown_matrix = section(
        rest,
        "async fn unknown_resources_and_methods_have_common_errors() {",
        "\n    }\n\n    async fn assert_public_unknown_non_options_matrix(",
    )
    public_unknown_non_options_matrix = section(
        rest,
        "async fn assert_public_unknown_non_options_matrix(",
        "\n    }\n\n    async fn assert_public_unknown_options_security_matrix(",
    )
    public_unknown_options_matrix = section(
        rest,
        "async fn assert_public_unknown_options_security_matrix(",
        "\n    }\n\n    #[tokio::test]\n    async fn api_catch_all_and_spa_fallback_compose_without_overlap() {",
    )
    public_unknown_composition = section(
        rest,
        "async fn api_catch_all_and_spa_fallback_compose_without_overlap() {",
        "\n    }\n\n    #[tokio::test]\n    async fn backend_failures_preserve_status_and_redacted_contract() {",
    )
    for inner_unknown_proof in (
        "for method_name in STANDARD_METHODS",
        'json_request(method, "/api/unknown", "{}")',
        "StatusCode::NOT_FOUND",
        "expected_not_found_length.to_string()",
        'expect("HEAD body")',
    ):
        assert inner_unknown_proof in inner_unknown_matrix
    assert inner_unknown_matrix.count("for method_name in STANDARD_METHODS") == 1
    for public_unknown_non_options_proof in (
        "for method_name in STANDARD_METHODS",
        '.filter(|method_name| *method_name != "OPTIONS")',
        '.uri("/api/unknown")',
        '.header(HOST, "localhost:8020")',
        '.header(ORIGIN, "http://localhost:8020")',
        '.header(CONTENT_TYPE, "application/json")',
        '.header("x-pokecon-request", "1")',
        "StatusCode::NOT_FOUND",
        "expected_not_found_length.to_string()",
        'expect("HEAD body")',
    ):
        assert public_unknown_non_options_proof in public_unknown_non_options_matrix
    assert (
        public_unknown_non_options_matrix.count("for method_name in STANDARD_METHODS")
        == 1
    )
    for public_unknown_options_proof in (
        '"bare", None, None',
        '"origin_only", Some("http://localhost:8020"), None',
        '"requested_method_only", None, Some("GET")',
        '"complete_unknown_preflight"',
        ".header(ACCESS_CONTROL_REQUEST_METHOD, requested_method)",
        "StatusCode::FORBIDDEN",
        "request_forbidden_error()",
    ):
        assert public_unknown_options_proof in public_unknown_options_matrix
    for public_composition_proof in (
        "public_router(",
        '.uri("/dashboard")',
        "StatusCode::OK",
        'to_bytes(spa.into_body(), 16).await.expect("body"), "spa"',
        ".method(Method::CONNECT)",
        '.uri("/api/state")',
        "StatusCode::METHOD_NOT_ALLOWED",
        'connect.headers()[ALLOW], "GET"',
        'Some(&json!("method_not_allowed"))',
        "assert_public_unknown_non_options_matrix(&app).await;",
        "assert_public_unknown_options_security_matrix(&app).await;",
    ):
        assert public_composition_proof in public_unknown_composition
    assert (
        public_unknown_composition.count(
            "assert_public_unknown_non_options_matrix(&app).await;"
        )
        == 1
    )
    assert (
        public_unknown_composition.count(
            "assert_public_unknown_options_security_matrix(&app).await;"
        )
        == 1
    )
    canonical_not_found_assertion = re.compile(
        r"assert_eq!\(\s*response_json\(response\)\.await,\s*"
        r'expected_not_found,\s*"\{method_name\}"\s*\);'
    )
    assert len(canonical_not_found_assertion.findall(inner_unknown_matrix)) == 1
    assert (
        len(canonical_not_found_assertion.findall(public_unknown_non_options_matrix))
        == 1
    )
    assert not canonical_not_found_assertion.search(public_unknown_options_matrix)
    assert not canonical_not_found_assertion.search(public_unknown_composition)
    canonical_forbidden_assertion = re.compile(
        r"assert_eq!\(\s*response_json\(response\)\.await,\s*"
        r'request_forbidden_error\(\),\s*"\{request_variant\}"\s*\);'
    )
    assert (
        len(canonical_forbidden_assertion.findall(public_unknown_options_matrix)) == 1
    )
    assert not canonical_forbidden_assertion.search(public_unknown_non_options_matrix)
    assert not canonical_forbidden_assertion.search(public_unknown_composition)
    assert public_unknown_options_matrix.count("Method::OPTIONS") == 1
    assert public_unknown_options_matrix.count("StatusCode::FORBIDDEN") == 1
    assert "preflights_match_the_openapi_surface" in security
    assert '"CONNECT"' in security
    assert security.count('"/api/') >= 14
    assert '"/ws"' in security
    assert 'const ALLOWED_METHODS: &str = "GET, PATCH, POST, OPTIONS";' in security


def test_production_routing_analysis_caches_are_content_keyed_and_immutable() -> None:
    canonical_source = (
        'fn route() { let literal = "ignored { delimiter } text"; serve(); }\n'
    )
    changed_source = canonical_source.replace("serve", "bypass", 1)
    literal_only_change = canonical_source.replace("ignored", "changed", 1)

    rust_lexical_mask.cache_clear()
    rust_delimiter_depths.cache_clear()
    assert rust_lexical_mask.cache_info().maxsize == 256
    assert rust_delimiter_depths.cache_info().maxsize == 256

    canonical_mask = rust_lexical_mask(canonical_source)
    assert canonical_mask == rust_lexical_mask.__wrapped__(canonical_source)
    assert rust_lexical_mask.cache_info().misses == 1
    assert rust_lexical_mask(canonical_source) is canonical_mask
    assert rust_lexical_mask.cache_info().hits == 1

    changed_mask = rust_lexical_mask(changed_source)
    assert changed_mask == rust_lexical_mask.__wrapped__(changed_source)
    assert changed_mask != canonical_mask
    literal_only_mask = rust_lexical_mask(literal_only_change)
    assert literal_only_mask == rust_lexical_mask.__wrapped__(literal_only_change)
    assert literal_only_mask == canonical_mask
    assert rust_lexical_mask.cache_info().misses == 3

    canonical_depths = rust_delimiter_depths(canonical_mask)
    assert canonical_depths == rust_delimiter_depths.__wrapped__(canonical_mask)
    assert all(isinstance(depths, tuple) for depths in canonical_depths)
    assert rust_delimiter_depths.cache_info().misses == 1
    assert rust_delimiter_depths(canonical_mask) is canonical_depths
    assert rust_delimiter_depths.cache_info().hits == 1

    changed_depths = rust_delimiter_depths(changed_mask)
    assert changed_depths == rust_delimiter_depths.__wrapped__(changed_mask)
    assert rust_delimiter_depths.cache_info().misses == 2
    assert rust_delimiter_depths(literal_only_mask) is canonical_depths
    assert rust_delimiter_depths.cache_info().hits == 2


def test_production_routing_mutation_shard_contract(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(PRODUCTION_ROUTING_MUTATION_SHARD_INDEX_ENV, raising=False)
    monkeypatch.delenv(PRODUCTION_ROUTING_MUTATION_SHARD_COUNT_ENV, raising=False)
    assert production_routing_mutation_shard() == (0, 1)

    for shard_count in range(1, PRODUCTION_ROUTING_MUTATION_MAX_SHARDS + 1):
        covered_indices = sorted(
            mutation_index
            for shard_index in range(shard_count)
            for mutation_index in range(shard_index, 384, shard_count)
        )
        assert covered_indices == list(range(384))

    for shard_index, shard_count in (("0", "1"), ("7", "8")):
        monkeypatch.setenv(PRODUCTION_ROUTING_MUTATION_SHARD_INDEX_ENV, shard_index)
        monkeypatch.setenv(PRODUCTION_ROUTING_MUTATION_SHARD_COUNT_ENV, shard_count)
        assert production_routing_mutation_shard() == (
            int(shard_index),
            int(shard_count),
        )

    invalid_shards = (
        (None, "1"),
        ("0", None),
        ("-1", "1"),
        ("+0", "1"),
        ("00", "1"),
        ("0", "0"),
        ("0", "9"),
        ("1", "1"),
    )
    for shard_index, shard_count in invalid_shards:
        if shard_index is None:
            monkeypatch.delenv(
                PRODUCTION_ROUTING_MUTATION_SHARD_INDEX_ENV, raising=False
            )
        else:
            monkeypatch.setenv(PRODUCTION_ROUTING_MUTATION_SHARD_INDEX_ENV, shard_index)
        if shard_count is None:
            monkeypatch.delenv(
                PRODUCTION_ROUTING_MUTATION_SHARD_COUNT_ENV, raising=False
            )
        else:
            monkeypatch.setenv(PRODUCTION_ROUTING_MUTATION_SHARD_COUNT_ENV, shard_count)
        with pytest.raises(AssertionError):
            production_routing_mutation_shard()


@pytest.mark.production_routing_mutation
def test_production_routing_audit_fails_closed_under_registration_mutations() -> None:
    sources = load_production_routing_sources()

    def refresh_canonical_flake_hash(flake: str) -> str:
        matches = tuple(
            re.finditer(
                r'canonicalFlakeHash = "(?P<hash>[0-9a-f]{64})";',
                flake,
            )
        )
        assert len(matches) == 1
        match = matches[0]
        normalized = (
            flake[: match.start("hash")]
            + "<canonical-flake-sha256>"
            + flake[match.end("hash") :]
        )
        digest = hashlib.sha256(normalized.encode()).hexdigest()
        return flake[: match.start("hash")] + digest + flake[match.end("hash") :]

    def replace_once(source_name: str, old: str, new: str) -> dict[str, str]:
        source = sources[source_name]
        assert old in source, (source_name, old)
        mutated = sources.copy()
        replacement = source.replace(old, new, 1)
        if source_name == FLAKE_SOURCE:
            replacement = refresh_canonical_flake_hash(replacement)
        mutated[source_name] = replacement
        return mutated

    def add_inventory_input(
        inventory_source: str,
        relative_path: str,
        content: str,
    ) -> dict[str, str]:
        mutated = sources.copy()
        raw_inventory = validated_json_value(
            json.loads(mutated[inventory_source]), inventory_source
        )
        assert isinstance(raw_inventory, list), inventory_source
        inventory: list[dict[str, JsonValue]] = [
            validated_json_object(item, inventory_source) for item in raw_inventory
        ]
        inventory.append({"path": relative_path, "kind": "regular"})

        def inventory_path(item: dict[str, JsonValue]) -> str:
            path = item["path"]
            assert isinstance(path, str), item
            return path

        inventory.sort(key=inventory_path)
        mutated[inventory_source] = json.dumps(inventory)
        mutated[f"@{relative_path}"] = content
        return mutated

    contracts_runtime_dependency = replace_once(
        "contracts/mod.rs",
        "\npub mod commands_typings;",
        "\nuse crate::runtime::ShutdownCoordinator;\n\npub mod commands_typings;",
    )
    runtime_server_dependency = replace_once(
        "runtime/mod.rs",
        "\nuse crate::platform::PlatformAdapter;",
        "\nuse crate::server::BoundServer;\nuse crate::platform::PlatformAdapter;",
    )
    settings_runtime_reverse_dependency = replace_once(
        "settings/mod.rs",
        "\npub mod hmac_key;",
        "\nuse crate::runtime::RuntimeContext;\n\npub mod hmac_key;",
    )
    server_hardware_owner = replace_once(
        "server/mod.rs",
        "\nuse std::io;",
        "\nuse crate::camera::CameraManager;\n\nuse std::io;",
    )
    worker_main_state_owner = replace_once(
        "worker/mod.rs",
        "\nuse serde::{Deserialize, Serialize};",
        "\nuse crate::device::InputArbiter;\n\nuse serde::{Deserialize, Serialize};",
    )

    constant_path = replace_once(
        "server/rest/state.rs",
        '"/api/state"',
        "STATE_PATH",
    )
    route_service = replace_once(
        "server/rest/state.rs",
        ".route(",
        ".route_service(",
    )
    alternate_constructor = replace_once(
        "server/rest/state.rs",
        "Router::new()",
        "ApiRouter::new()",
    )
    post_rebound_to_any = replace_once(
        "server/rest/commands.rs",
        "use axum::routing::post;",
        "use axum::routing::any as post;",
    )
    widened_method_filter_connect = replace_once(
        "server/rest/state.rs",
        "use axum::routing::{MethodFilter, on};",
        """use axum::routing::{MethodFilter as AxumMethodFilter, on};

struct MethodFilter;

impl MethodFilter {
    const GET: AxumMethodFilter = AxumMethodFilter::GET.or(AxumMethodFilter::CONNECT);
    const HEAD: AxumMethodFilter = AxumMethodFilter::HEAD;
}""",
    )
    rebound_on_import = replace_once(
        "server/rest/settings.rs",
        "use axum::routing::{MethodFilter, on};",
        "use axum::routing::{MethodFilter, any as on};",
    )
    rebound_router_import = replace_once(
        "server/rest/commands.rs",
        "use axum::Router;",
        "use crate::routing_facade::Router;",
    )
    rebound_any_import = replace_once(
        "server/rest/mod.rs",
        "use axum::routing::any;",
        "use crate::routing_facade::any;",
    )
    local_axum_facade = sources.copy()
    local_axum_facade["server/rest/state.rs"] += (
        "\nmod axum {\n    pub use ::axum::*;\n}\n"
    )
    foreign_merge = replace_once(
        "production.rs",
        ".merge(websocket.router())",
        ".merge(websocket.router()).merge(hidden_router())",
    )
    substituted_production_getter = replace_once(
        "production.rs",
        "self.router.clone()",
        "hidden_router()",
    )
    public_composition = replace_once(
        "lib.rs",
        "public_router(api, static_files, security)",
        "public_router(api.merge(hidden_router()), static_files, security)",
    )
    production_ui_composition = replace_once(
        "lib.rs",
        "ui_router(production.router(), static_files, listen_address, ui)",
        "ui_router(production.router().merge(hidden_router()), static_files, listen_address, ui)",
    )
    locally_shadowed_ui_router = replace_once(
        "lib.rs",
        "let app = ui_router(production.router(), static_files, listen_address, ui);",
        """let ui_router = |api, _static_files, _listen_address, _ui| api;
    let app = ui_router(production.router(), static_files, listen_address, ui);""",
    )
    locally_shadowed_secure_router = replace_once(
        "server/router.rs",
        "use crate::server::security::{RequestSecurity, secure_router};",
        """use crate::server::security::RequestSecurity;

fn secure_router(router: Router, _security: RequestSecurity) -> Router {
    router
}""",
    )
    websocket_extra_path = replace_once(
        "server/websocket.rs",
        ".with_state(self.state.clone())",
        '.route("/ws-hidden", on(MethodFilter::GET, websocket_upgrade))\n'
        "            .with_state(self.state.clone())",
    )
    nested_transport_macro = sources.copy()
    nested_transport_macro["server/rest/state.rs"] += """

macro_rules! define_transport {
    () => {
        mod axum {
            pub use ::axum::{Json, Router};
            pub mod routing {
                pub use ::axum::routing::on;
                pub struct MethodFilter;
                impl MethodFilter {
                    const GET: ::axum::routing::MethodFilter =
                        ::axum::routing::MethodFilter::GET
                            .or(::axum::routing::MethodFilter::CONNECT);
                    const HEAD: ::axum::routing::MethodFilter =
                        ::axum::routing::MethodFilter::HEAD;
                }
            }
        }
    };
}
define_transport!();
"""
    included_transport_item = sources.copy()
    included_transport_item["server/rest/state.rs"] += (
        '\ninclude!("alternate-transport.txt");\n'
    )
    included_transport_item["server/rest/alternate-transport.txt"] = "mod axum {}\n"
    aliased_item_macro = sources.copy()
    aliased_item_macro["server/rest/state.rs"] += (
        "\nuse std::include as inject_transport;\n"
        'inject_transport!("alternate-transport.txt");\n'
    )
    aliased_item_macro["server/rest/alternate-transport.txt"] = "mod axum {}\n"

    unknown_rest_source = sources.copy()
    unknown_rest_source["server/rest/hidden.rs"] = (
        'fn router() -> Router { Router::new().route("/api/hidden", post(hidden)) }'
    )
    foreign_route_location = sources.copy()
    foreign_route_location["server/router.rs"] += (
        '\nfn hidden() -> Router { Router::new().route("/api/hidden", post(hidden)) }\n'
    )
    ufcs_registration = sources.copy()
    ufcs_registration["server/rest/state.rs"] += (
        '\nfn hidden() -> Router { Router::route(Router::new(), "/api/hidden", post(hidden)) }\n'
    )
    angle_ufcs_registration = sources.copy()
    angle_ufcs_registration["server/rest/state.rs"] += (
        "\nfn hidden() -> Router { "
        "<Router>::route(hidden_router(), HIDDEN_PATH, hidden_handler) }\n"
    )
    generic_angle_ufcs_registration = sources.copy()
    generic_angle_ufcs_registration["server/rest/state.rs"] += (
        "\nfn hidden() -> Router { "
        "<Router<State>>::route(hidden_router(), HIDDEN_PATH, hidden_handler) }\n"
    )
    macro_registration = sources.copy()
    macro_registration["server/rest/state.rs"] += (
        "\nfn hidden() -> Router { api_routes!() }\n"
    )
    nested_registration = sources.copy()
    nested_registration["server/rest/state.rs"] += (
        '\nfn hidden() -> Router { Router::new().nest("/api", hidden_router()) }\n'
    )
    nested_service_registration = sources.copy()
    nested_service_registration["server/rest/state.rs"] += (
        '\nfn hidden() -> Router { Router::new().nest_service("/api", hidden_service()) }\n'
    )
    arbitrary_macro_return = replace_once(
        "server/rest/state.rs",
        """Router::new().route(
        "/api/state",
        on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
    )""",
        "hidden!()",
    )
    unused_known_chain = replace_once(
        "server/rest/state.rs",
        """Router::new().route(
        "/api/state",
        on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
    )""",
        """let _known = Router::new().route(
        "/api/state",
        on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
    );
    alternate_router()""",
    )
    conditional_hidden_catchall_success = replace_once(
        "server/rest/mod.rs",
        """async fn not_found() -> RestError {
    RestError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::ResourceNotFound,
        "API resource was not found",
    )
}""",
        """async fn not_found() -> RestError {
    if hidden_success() {
        return RestError::new(
            StatusCode::OK,
            ApiErrorCode::ResourceNotFound,
            "hidden success",
        );
    }
    RestError::new(
        StatusCode::NOT_FOUND,
        ApiErrorCode::ResourceNotFound,
        "API resource was not found",
    )
}""",
    )
    conditional_hidden_websocket_success = replace_once(
        "server/websocket.rs",
        """async fn websocket_method_not_allowed() -> Response {
    let mut response = http_error(
        StatusCode::METHOD_NOT_ALLOWED,
        ApiErrorCode::MethodNotAllowed,
        "HTTP method is not allowed for this resource",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static("GET"));
    response
}""",
        """async fn websocket_method_not_allowed() -> Response {
    if hidden_success() {
        return StatusCode::OK.into_response();
    }
    let mut response = http_error(
        StatusCode::METHOD_NOT_ALLOWED,
        ApiErrorCode::MethodNotAllowed,
        "HTTP method is not allowed for this resource",
    );
    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static("GET"));
    response
}""",
    )
    conditional_rest_error_constructor_success = replace_once(
        "server/rest/mod.rs",
        """    fn new(status: StatusCode, code: ApiErrorCode, message: impl Into<String>) -> Self {
        Self {
            status,""",
        """    fn new(status: StatusCode, code: ApiErrorCode, message: impl Into<String>) -> Self {
        let status = if hidden_success() {
            StatusCode::OK
        } else {
            status
        };
        Self {
            status,""",
    )
    rewritten_rest_error_constructor_status = replace_once(
        "server/rest/mod.rs",
        """        Self {
            status,
            error: ApiError {""",
        """        Self {
            status: StatusCode::OK,
            error: ApiError {""",
    )
    conditional_rest_error_response_success = replace_once(
        "server/rest/mod.rs",
        """    fn into_response(self) -> Response {
        (self.status, Json(ErrorEnvelope { error: self.error })).into_response()
    }""",
        """    fn into_response(self) -> Response {
        let status = if hidden_success() {
            StatusCode::OK
        } else {
            self.status
        };
        (status, Json(ErrorEnvelope { error: self.error })).into_response()
    }""",
    )
    rewritten_rest_error_response_status = replace_once(
        "server/rest/mod.rs",
        """        (self.status, Json(ErrorEnvelope { error: self.error })).into_response()""",
        """        (StatusCode::OK, Json(ErrorEnvelope { error: self.error })).into_response()""",
    )
    conditional_websocket_http_error_success = replace_once(
        "server/websocket.rs",
        """fn http_error(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    (
        status,""",
        """fn http_error(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    let status = if hidden_success() {
        StatusCode::OK
    } else {
        status
    };
    (
        status,""",
    )
    rewritten_websocket_http_error_status = replace_once(
        "server/websocket.rs",
        """fn http_error(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    (
        status,""",
        """fn http_error(status: StatusCode, code: ApiErrorCode, message: &str) -> Response {
    (
        StatusCode::OK,""",
    )
    missing_rest_allow = replace_once(
        "server/rest/mod.rs",
        """    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static(allow));
    response
}""",
        """    response
}""",
    )
    misleading_rest_allow = replace_once(
        "server/rest/mod.rs",
        '        "/api/settings" => "GET, PATCH",',
        '        "/api/settings" => "GET, HEAD, PATCH, OPTIONS",',
    )
    missing_websocket_allow = replace_once(
        "server/websocket.rs",
        """    response
        .headers_mut()
        .insert(ALLOW, HeaderValue::from_static("GET"));
    response
}""",
        """    response
}""",
    )
    misleading_websocket_allow = replace_once(
        "server/websocket.rs",
        '        .insert(ALLOW, HeaderValue::from_static("GET"));',
        '        .insert(ALLOW, HeaderValue::from_static("GET, HEAD, OPTIONS"));',
    )
    axum_derived_rest_allow = replace_once(
        "server/rest/mod.rs",
        "        .method_not_allowed_fallback(method_not_allowed)\n",
        "",
    )

    rebound_rest_status_import = sources.copy()
    rebound_rest_status_source = rebound_rest_status_import["server/rest/mod.rs"]
    canonical_rest_http_import = "use axum::http::{HeaderValue, StatusCode, Uri};"
    assert canonical_rest_http_import in rebound_rest_status_source
    rebound_rest_status_source = rebound_rest_status_source.replace(
        canonical_rest_http_import,
        "use axum::http::{HeaderValue, StatusCode as HttpStatus, Uri};",
        1,
    ).replace("StatusCode::", "HttpStatus::")
    rebound_rest_status_import["server/rest/mod.rs"] = rebound_rest_status_source
    hidden_allowed_preflight_path = replace_once(
        "server/security.rs",
        "        _ => false,",
        """        "/api/hidden" => requested_method == Method::GET,
        _ => false,""",
    )
    prefix_allowed_preflight_path = replace_once(
        "server/security.rs",
        "        _ => false,",
        '        _ => path.starts_with("/api/"),',
    )
    request_validator_preflight_bypass = replace_once(
        "server/security.rs",
        """        if method == Method::OPTIONS {
            let origin = origin.ok_or(SecurityError::Forbidden)?;
            validate_preflight(uri.path(), headers)?;""",
        """        if method == Method::OPTIONS {
            let origin = origin.ok_or(SecurityError::Forbidden)?;
            if uri.path() == "/api/hidden" {
                return Ok(SecurityDecision::Preflight { origin });
            }
            validate_preflight(uri.path(), headers)?;""",
    )
    required_header_alternate_name = replace_once(
        "server/security.rs",
        """fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<&'a HeaderValue, SecurityError> {
    optional_header(headers, name)?.ok_or(SecurityError::Forbidden)
}""",
        """fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<&'a HeaderValue, SecurityError> {
    if name == &ACCESS_CONTROL_REQUEST_METHOD {
        if let Some(alternate) = headers.get("x-pokecon-preflight-method") {
            return Ok(alternate);
        }
    }
    optional_header(headers, name)?.ok_or(SecurityError::Forbidden)
}""",
    )
    optional_header_alternate_name = replace_once(
        "server/security.rs",
        """fn optional_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<Option<&'a HeaderValue>, SecurityError> {
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(SecurityError::Forbidden);
    }
    Ok(value)
}""",
        """fn optional_header<'a>(
    headers: &'a HeaderMap,
    name: &HeaderName,
) -> Result<Option<&'a HeaderValue>, SecurityError> {
    let alternate = if name == &ACCESS_CONTROL_REQUEST_METHOD {
        headers.get("x-pokecon-preflight-method")
    } else {
        None
    };
    let mut values = headers.get_all(name).iter();
    let value = values.next();
    if values.next().is_some() {
        return Err(SecurityError::Forbidden);
    }
    Ok(alternate.or(value))
}""",
    )
    conditional_attacker_host = replace_once(
        "server/security.rs",
        """        if desktop_mode {
            allowed_origins.insert("tauri://localhost".to_owned());
        }
        Self {""",
        """        if desktop_mode {
            allowed_origins.insert("tauri://localhost".to_owned());
        }
        if address.port() == 8020 {
            allowed_hosts.insert("attacker.invalid".to_owned());
        }
        Self {""",
    )
    conditional_attacker_origin = replace_once(
        "server/security.rs",
        """        if desktop_mode {
            allowed_origins.insert("tauri://localhost".to_owned());
        }
        Self {""",
        """        if desktop_mode {
            allowed_origins.insert("tauri://localhost".to_owned());
        }
        if address.port() == 8020 {
            allowed_origins.insert("https://attacker.invalid".to_owned());
        }
        Self {""",
    )
    conditional_non_mutating_method = replace_once(
        "server/security.rs",
        """fn is_mutating(method: &Method) -> bool {
    method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
}""",
        """fn is_mutating(method: &Method) -> bool {
    if method == Method::POST
        && std::env::var_os("POKECON_ALLOW_SIMPLE_POST").is_some()
    {
        return false;
    }
    method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
}""",
    )
    widened_mutating_methods = replace_once(
        "server/security.rs",
        """fn is_mutating(method: &Method) -> bool {
    method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
}""",
        """fn is_mutating(method: &Method) -> bool {
    method == Method::GET
        || method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
}""",
    )
    narrowed_mutating_methods = replace_once(
        "server/security.rs",
        """fn is_mutating(method: &Method) -> bool {
    method == Method::PATCH
        || method == Method::POST
        || method == Method::PUT
        || method == Method::DELETE
}""",
        """fn is_mutating(method: &Method) -> bool {
    method == Method::PATCH || method == Method::POST || method == Method::PUT
}""",
    )
    rewritten_security_error_status = replace_once(
        "server/security.rs",
        """                StatusCode::FORBIDDEN,
                ApiErrorCode::RequestForbidden,""",
        """                StatusCode::NO_CONTENT,
                ApiErrorCode::RequestForbidden,""",
    )
    conditional_security_error_success = replace_once(
        "server/security.rs",
        """        };
        (
            status,
            Json(ErrorEnvelope {""",
        """        };
        let status = if std::env::var_os("POKECON_ALLOW_REJECTION").is_some() {
            StatusCode::OK
        } else {
            status
        };
        (
            status,
            Json(ErrorEnvelope {""",
    )
    changed_security_error_literal_spacing = replace_once(
        "server/security.rs",
        '                "request validation failed",',
        '                "request  validation failed",',
    )
    changed_request_marker = replace_once(
        "server/security.rs",
        """const REQUEST_MARKER: HeaderName = HeaderName::from_static("x-pokecon-request");""",
        """const REQUEST_MARKER: HeaderName =
    HeaderName::from_static("x-pokecon-preflight-method");""",
    )
    reexported_api_response_import = replace_once(
        "server/security.rs",
        "use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};",
        "pub use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};",
    )
    exposed_request_security_field = replace_once(
        "server/security.rs",
        "    allowed_hosts: BTreeSet<String>,",
        "    pub(crate) allowed_hosts: BTreeSet<String>,",
    )
    exposed_security_error = replace_once(
        "server/security.rs",
        "enum SecurityError {",
        "pub(crate) enum SecurityError {",
    )
    inherent_security_error_success = sources.copy()
    inherent_security_error_success["server/security.rs"] += """

impl SecurityError {
    fn into_response(&self) -> Response {
        StatusCode::OK.into_response()
    }
}
"""
    shadowed_format_macro = replace_once(
        "server/security.rs",
        "use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};",
        """use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};

macro_rules! format {
    ($($tokens:tt)*) => {
        "http://attacker.invalid".to_owned()
    };
}""",
    )
    attributed_shadowed_format_macro = replace_once(
        "server/security.rs",
        "use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};",
        """use crate::server::api::{ApiError, ApiErrorCode, ErrorEnvelope};

#[allow(unused_macros)] macro_rules! format {
    ($($tokens:tt)*) => {
        "http://attacker.invalid".to_owned()
    };
}""",
    )
    ancestor_shadowed_format_macro = replace_once(
        "server/api.rs",
        "use std::collections::BTreeMap;",
        """#![macro_use]

macro_rules! format {
    ("http://{authority}") => {
        "http://attacker.invalid".to_owned()
    };
    ("localhost:{}", $port:expr) => {{
        let _port = $port;
        "attacker.invalid:80".to_owned()
    }};
    ("http://{localhost}") => {
        "http://attacker.invalid".to_owned()
    };
}

use std::collections::BTreeMap;""",
    )
    ancestor_security = ancestor_shadowed_format_macro["server/security.rs"]
    assert ancestor_security.count("::std::format!") == 3
    ancestor_shadowed_format_macro["server/security.rs"] = ancestor_security.replace(
        "::std::format!", "format!"
    )
    shadowed_std_dependency = replace_once(
        POKECON_MANIFEST_SOURCE,
        "[dev-dependencies]",
        """std = { package = "pokecon-contracts", path = "../pokecon-contracts" }

[dev-dependencies]""",
    )
    included_request_security_callable = replace_once(
        "server/security.rs",
        """impl RequestSecurity {
    #[must_use]""",
        """impl RequestSecurity {
    include!("security-extra.txt");

    #[must_use]""",
    )
    included_request_security_callable["server/security-extra.txt"] = """
fn allow_attacker(&mut self) {
    self.allowed_hosts.insert("attacker.invalid".to_owned());
}
"""
    middleware_preflight_bypass = replace_once(
        "server/security.rs",
        """    match security.validate(request.method(), request.uri(), request.headers()) {""",
        """    if request.uri().path() == "/api/hidden" {
        return StatusCode::NO_CONTENT.into_response();
    }
    match security.validate(request.method(), request.uri(), request.headers()) {""",
    )
    direct_route_security_layer = replace_once(
        "server/security.rs",
        """    Router::new()
        .fallback_service(router)
        .layer(middleware::from_fn_with_state(security, enforce_security))""",
        """    router.layer(middleware::from_fn_with_state(security, enforce_security))""",
    )
    conflicting_cors_origin_header = replace_once(
        "server/security.rs",
        """    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    response
        .headers_mut()
        .append(VARY, HeaderValue::from_static("Origin"));""",
        """    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    response.headers_mut().append(
        ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
        .headers_mut()
        .append(VARY, HeaderValue::from_static("Origin"));""",
    )
    appended_cors_origin_header = replace_once(
        "server/security.rs",
        ".insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);",
        ".append(ACCESS_CONTROL_ALLOW_ORIGIN, origin);",
    )
    rebound_cors_origin_import = replace_once(
        "server/security.rs",
        "    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN,",
        "    ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,\n"
        "    ACCESS_CONTROL_EXPOSE_HEADERS as ACCESS_CONTROL_ALLOW_ORIGIN,",
    )
    changed_application_server_module = replace_once(
        "lib.rs",
        "mod server;",
        "mod alternate_server;",
    )
    restored_server_dependency = replace_once(
        POKECON_MANIFEST_SOURCE,
        "\n[build-dependencies]\n",
        '\npokecon-server = { path = "../pokecon-server" }\n\n[build-dependencies]\n',
    )
    changed_server_module = replace_once(
        "server/mod.rs",
        "pub mod rest;",
        "pub mod alternate_rest;",
    )
    redirected_server_module = replace_once(
        "server/mod.rs",
        "pub mod rest;",
        '#[path = "alternate/rest.rs"]\npub mod rest;',
    )
    redirected_application_server_module = replace_once(
        "lib.rs",
        "mod server;",
        '#[path = "server/alternate.rs"]\nmod server;',
    )
    rebound_rest_import = replace_once(
        "production.rs",
        "use crate::server::rest;",
        "use crate::alternate::rest as rest;",
    )
    rebound_websocket_import = replace_once(
        "production.rs",
        "use crate::server::websocket::{",
        "use crate::alternate::websocket::{",
    )
    rebound_public_router_import = replace_once(
        "lib.rs",
        "use crate::server::router::public_router;",
        "use crate::alternate::public_router as public_router;",
    )
    canonical_state_router = """pub(super) fn router() -> Router<RestState> {
    Router::new().route(
        "/api/state",
        on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
    )
}"""
    nested_rest_decoy = replace_once(
        "server/rest/state.rs",
        canonical_state_router,
        """mod decoy {
    use super::*;

    pub(super) fn router() -> Router<RestState> {
        Router::new().route(
            "/api/state",
            on(MethodFilter::GET, get_state).on(MethodFilter::HEAD, method_not_allowed),
        )
    }
}

pub(super) fn r#router() -> Router<RestState> {
    hidden!()
}""",
    )

    canonical_websocket_router = """    pub fn router(&self) -> Router {
        Router::new()
            .route(
                "/ws",
                on(MethodFilter::GET, websocket_upgrade)
                    .on(MethodFilter::HEAD, websocket_method_not_allowed)
                    .fallback(websocket_method_not_allowed),
            )
            .with_state(self.state.clone())
    }"""
    websocket_scope_source = sources["server/websocket.rs"]
    assert canonical_websocket_router in websocket_scope_source
    websocket_scope_source = websocket_scope_source.replace(
        canonical_websocket_router,
        """    pub fn r#router(&self) -> Router {
        hidden!()
    }""",
        1,
    )
    websocket_decoy_impl = f"""struct Decoy {{
    state: WebSocketState,
}}

impl Decoy {{
{canonical_websocket_router}
}}

impl WebSocketTransport {{"""
    assert "impl WebSocketTransport {" in websocket_scope_source
    websocket_scope_source = websocket_scope_source.replace(
        "impl WebSocketTransport {",
        websocket_decoy_impl,
        1,
    )
    websocket_owner_decoy = sources.copy()
    websocket_owner_decoy["server/websocket.rs"] = websocket_scope_source

    canonical_with_router = """    pub fn with_router(mut self, router: Router) -> Self {
        self.router = router;
        self
    }"""
    bound_server_scope_source = sources["server/mod.rs"]
    assert canonical_with_router in bound_server_scope_source
    bound_server_scope_source = bound_server_scope_source.replace(
        canonical_with_router,
        """    pub fn r#with_router(self, _router: Router) -> Self {
        hidden!()
    }""",
        1,
    )
    bound_server_decoy_impl = f"""struct Decoy {{
    router: Router,
}}

impl Decoy {{
{canonical_with_router}
}}

impl BoundServer {{"""
    assert "impl BoundServer {" in bound_server_scope_source
    bound_server_scope_source = bound_server_scope_source.replace(
        "impl BoundServer {",
        bound_server_decoy_impl,
        1,
    )
    bound_server_owner_decoy = sources.copy()
    bound_server_owner_decoy["server/mod.rs"] = bound_server_scope_source

    redirected_rest_submodule = replace_once(
        "server/rest/mod.rs",
        "mod state;",
        '#[path = "alternate/state.rs"]\nmod state;',
    )
    raw_production_module = replace_once(
        "lib.rs",
        "mod production;",
        '#[cfg(any())]\nmod production;\n#[path = "alternate/production.rs"]\nmod r#production;',
    )
    alternate_entrypoint_module = replace_once(
        "lib.rs",
        "mod entrypoint;",
        '#[path = "alternate/entrypoint.rs"]\nmod entrypoint;',
    )
    alternate_application_lib_target = sources.copy()
    alternate_application_lib_target[POKECON_MANIFEST_SOURCE] += (
        '\n[lib]\npath = "src/alternate.rs"\n'
    )
    alternate_primary_binary = replace_once(
        POKECON_MANIFEST_SOURCE,
        'path = "src/main.rs"',
        'path = "src/alternate.rs"',
    )
    alternate_main = replace_once(
        "main.rs",
        "pokecon::run_cli().await",
        "alternate::run_cli().await",
    )
    alternate_controlled_runner = replace_once(
        "entrypoint.rs",
        "    run_configured_controlled(\n        AppOptions {",
        "    alternate_controlled_runner(\n        AppOptions {",
    )
    pre_dynamic_final_guard_weakened = replace_once(
        "settings/pipeline.rs",
        "    setting.mutability == Mutability::StartupOnly "
        "&& setting.surfaces.dynamic.name.is_none()",
        "    setting.mutability == Mutability::StartupOnly && true",
    )
    pre_dynamic_cli_lookup_bypassed = replace_once(
        "settings/pipeline.rs",
        "        let Some(raw) = self.recipe.parsed_cli.assignments.get(id) else {",
        '        let Some(raw) = self.recipe.parsed_cli.assignments.get("__never__") else {',
    )
    resource_root_retarget_recipe_omitted = replace_once(
        "settings/pipeline.rs",
        "        self.recipe.request.resource_root = resource_root;",
        "        let _resource_root = resource_root;",
    )
    resource_root_retarget_default_kind_guard_omitted = replace_once(
        "settings/pipeline.rs",
        "            if !matches!(&setting.default, DefaultValue::ResourcePath { .. }) {",
        "            if false {",
    )
    resource_root_retarget_default_source_guard_omitted = replace_once(
        "settings/pipeline.rs",
        "            if resolved.source == SettingSource::Default {",
        "            if true {",
    )
    eager_packaged_resource_snapshot = replace_once(
        "entrypoint.rs",
        "    let request = PipelineRequest::current()?;\n"
        "    let before_dynamic = SettingsPipeline::new(request.clone()).load_before_dynamic()?;",
        "    let request = PipelineRequest::current()?;\n"
        "    let _eager_snapshot = packaged_resource_root(&request.resource_root)?;\n"
        "    let before_dynamic = SettingsPipeline::new(request.clone()).load_before_dynamic()?;",
    )
    packaged_resource_retarget_omitted = replace_once(
        "entrypoint.rs",
        "    let before_dynamic = "
        "before_dynamic.with_resource_root(request.resource_root.clone())?;",
        "    let before_dynamic = before_dynamic;",
    )
    packaged_resource_guard_dropped_before_backend = replace_once(
        "entrypoint.rs",
        "    let result = run_backend(\n",
        "    drop(resource_root_guard);\n    let result = run_backend(\n",
    )
    desktop_packaged_backend_bypassed = replace_once(
        "entrypoint.rs",
        """                let inner_task = runtime.spawn(async move {
                    run_packaged_backend(
                        request,
                        before_dynamic,""",
        """                let inner_task = runtime.spawn(async move {
                    run_backend(
                        request,
                        before_dynamic,""",
    )
    compositing_reexec_cli_overlay_omitted = replace_once(
        "entrypoint.rs",
        """        let disable_compositing =
            before_dynamic.pre_dynamic_final_boolean_with_cli("ui.desktop.disable_compositing")?;
        let already_reexecuted""",
        """        let disable_compositing = before_dynamic
            .settings
            .boolean("ui.desktop.disable_compositing")?;
        let already_reexecuted""",
    )
    desktop_compositing_cli_overlay_omitted = replace_once(
        "entrypoint.rs",
        """    let disable_compositing =
        before_dynamic.pre_dynamic_final_boolean_with_cli("ui.desktop.disable_compositing")?;
    let shell_config""",
        """    let disable_compositing = before_dynamic
        .settings
        .boolean("ui.desktop.disable_compositing")?;
    let shell_config""",
    )
    compositing_spawn_wait_restored = replace_once(
        "entrypoint.rs",
        "        .exec();",
        '        .status().expect("compositing child");',
    )
    compositing_pre_exec_state_retained = replace_once(
        "entrypoint.rs",
        "            drop((cli, before_dynamic, request));",
        "            let _retained_pre_exec_state = (&cli, &before_dynamic, &request);",
    )
    compositing_reexec_marker_omitted = replace_once(
        "entrypoint.rs",
        '.env(COMPOSITING_REEXEC_MARKER, "1")',
        '.env("PCME_UNUSED_REEXEC_MARKER", "1")',
    )
    compositing_arguments_omitted = replace_once(
        "entrypoint.rs",
        ".args(std::env::args_os().skip(1))",
        ".args(std::iter::empty::<std::ffi::OsString>())",
    )
    compositing_environment_omitted = replace_once(
        "entrypoint.rs",
        '.env("WEBKIT_DISABLE_COMPOSITING_MODE", "1")',
        '.env("PCME_UNUSED_COMPOSITING_MODE", "1")',
    )
    compositing_recursion_fence_omitted = replace_once(
        "entrypoint.rs",
        "&& !already_reexecuted",
        "&& true",
    )
    desktop_backend_supervisor_bypassed = replace_once(
        "entrypoint.rs",
        """                let supervisor_task = runtime.spawn(supervise_desktop_backend_startup(
                    inner_task,
                    task_shutdown,
                    readiness_guard,
                ));
                task_sender.send(supervisor_task).map_err(|_error| {""",
        """                let _supervisor_shutdown = (task_shutdown, readiness_guard);
                task_sender.send(inner_task).map_err(|_error| {""",
    )
    desktop_backend_returned_error_fatal_request_omitted = replace_once(
        "entrypoint.rs",
        """            if let Err(error) = result.as_ref() {
                let _fatal_claimed =
                    shutdown.request(ShutdownReason::FatalError(error.to_string()));
            }""",
        """            if let Err(error) = result.as_ref() {
                let _fatal_error = error.to_string();
            }""",
    )
    desktop_backend_join_error_fatal_request_omitted = replace_once(
        "entrypoint.rs",
        """            let error = MainError::BackendTask(error);
            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Err(error)""",
        """            let error = MainError::BackendTask(error);
            let _fatal_error = error.to_string();
            Err(error)""",
    )
    desktop_backend_inner_handle_double_join = replace_once(
        "entrypoint.rs",
        """async fn supervise_desktop_backend_task(
    inner_task: tokio::task::JoinHandle<Result<(), MainError>>,
    shutdown: ShutdownCoordinator,
) -> Result<(), MainError> {
    match inner_task.await {""",
        """async fn supervise_desktop_backend_task(
    mut inner_task: tokio::task::JoinHandle<Result<(), MainError>>,
    shutdown: ShutdownCoordinator,
) -> Result<(), MainError> {
    let _first_result = (&mut inner_task).await;
    match inner_task.await {""",
    )
    desktop_backend_readiness_guard_dropped_early = replace_once(
        "entrypoint.rs",
        """    let result = supervise_desktop_backend_task(inner_task, shutdown).await;
    drop(readiness_guard);
    result""",
        """    drop(readiness_guard);
    supervise_desktop_backend_task(inner_task, shutdown).await""",
    )
    desktop_backend_readiness_failure_marker_omitted = replace_once(
        "entrypoint.rs",
        """                let actual_address = ready_receiver.recv().map_err(|_error| {
                    startup_failure_marker.store(true, Ordering::Release);
                    DesktopError::BackendAddressUnavailable
                })?;""",
        """                let actual_address = ready_receiver.recv().map_err(|_error| {
                    let _startup_failure_marker = &startup_failure_marker;
                    DesktopError::BackendAddressUnavailable
                })?;""",
    )
    desktop_pre_readiness_shell_priority_restored = replace_once(
        "entrypoint.rs",
        """    if backend_stopped_before_readiness {
        return if let Err(error) = backend_result {""",
        """    if backend_stopped_before_readiness {
        shell_result?;
        return if let Err(error) = backend_result {""",
    )
    desktop_backend_task_published_after_readiness_wait = replace_once(
        "entrypoint.rs",
        """                task_sender.send(supervisor_task).map_err(|_error| {
                    DesktopError::BackendStartup("backend task receiver was dropped".to_owned())
                })?;
                let actual_address = ready_receiver.recv().map_err(|_error| {
                    startup_failure_marker.store(true, Ordering::Release);
                    DesktopError::BackendAddressUnavailable
                })?;""",
        """                let actual_address = ready_receiver.recv().map_err(|_error| {
                    startup_failure_marker.store(true, Ordering::Release);
                    DesktopError::BackendAddressUnavailable
                })?;
                task_sender.send(supervisor_task).map_err(|_error| {
                    DesktopError::BackendStartup("backend task receiver was dropped".to_owned())
                })?;""",
    )
    desktop_shell_error_priority_reversed = replace_once(
        "entrypoint.rs",
        "    shell_result?;\n    backend_result",
        "    backend_result?;\n    shell_result?;\n    Ok(())",
    )
    shutdown_only_application_wait = replace_once(
        "lib.rs",
        """            wait_for_shutdown_or_runtime_task(&shutdown, &mut server_task, &mut signal_task).await""",
        """            RuntimeTaskResult {
                early_task_error: None,
                server_task_consumed: false,
                signal_task_consumed: false,
            }""",
    )
    listener_readiness_barrier_bypassed = replace_once(
        "lib.rs",
        """    let (server_readiness_sender, server_readiness_receiver) = oneshot::channel();
    let mut server_task = tokio::spawn(serve_with_readiness_barrier(
        server.serve(server_shutdown.clone()),
        server_readiness_receiver,
    ));""",
        """    let (server_readiness_sender, _server_readiness_receiver) = oneshot::channel();
    let mut server_task = tokio::spawn(server.serve(server_shutdown.clone()));""",
    )
    listener_readiness_completion_deprioritized = replace_once(
        "lib.rs",
        """        result = &mut serve => return result,
        request = readiness => request,""",
        """        request = readiness => request,
        result = &mut serve => return result,""",
    )
    listener_readiness_publication_wait_omitted = replace_once(
        "lib.rs",
        "    let _published = request.published.await;",
        "    drop(request.published);",
    )
    listener_readiness_prepared_ack_omitted = replace_once(
        "lib.rs",
        "    if prepared_receiver.await.is_err() {",
        "    if false {",
    )
    listener_readiness_permit_dropped_before_publication = replace_once(
        "lib.rs",
        "            readiness_permit.publish();",
        "            drop(readiness_permit);",
    )
    omitted_listener_fatal_request = replace_once(
        "lib.rs",
        """    let error = early_server_task_error(result);
    shutdown.request(ShutdownReason::FatalError(error.to_string()));""",
        """    let error = early_server_task_error(result);
    let _fatal_error = error.to_string();""",
    )
    omitted_application_signal_fatal_request = replace_once(
        "lib.rs",
        """            let error = AppError::Task(error);
            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Some(EarlyRuntimeTaskError::Signal(error))""",
        """            let error = AppError::Task(error);
            let _fatal_error = error.to_string();
            Some(EarlyRuntimeTaskError::Signal(error))""",
    )
    application_rejected_fatal_claim_still_errors = replace_once(
        "lib.rs",
        """            let fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            fatal_claimed.then_some(EarlyRuntimeTaskError::Signal(error))""",
        """            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Some(EarlyRuntimeTaskError::Signal(error))""",
    )
    application_signal_handle_double_join = replace_once(
        "lib.rs",
        """    let signal_result = if task_result.signal_task_consumed {
        Ok(())
    } else {
        signal_task.await.map_err(AppError::Task)
    };""",
        """    let signal_result = signal_task.await.map_err(AppError::Task);""",
    )
    application_error_skips_remaining_signal_join = replace_once(
        "lib.rs",
        """    let signal_result = if task_result.signal_task_consumed {
        Ok(())
    } else {
        signal_task.await.map_err(AppError::Task)
    };
    if let Some(error) = task_result.early_task_error {
        return Err(match error {
            EarlyRuntimeTaskError::Server(error) | EarlyRuntimeTaskError::Signal(error) => error,
        });
    }""",
        """    if let Some(error) = task_result.early_task_error {
        return Err(match error {
            EarlyRuntimeTaskError::Server(error) | EarlyRuntimeTaskError::Signal(error) => error,
        });
    }
    let signal_result = signal_task.await.map_err(AppError::Task);""",
    )
    worker_without_signal_supervision = replace_once(
        "worker_binary/mod.rs",
        """        result = &mut *signal_task => {
            WorkerTaskObservation::Signal(completed_signal_task_result(shutdown, result))
        }
        result = &mut protocol => WorkerTaskObservation::Protocol(result),""",
        """        result = &mut protocol => WorkerTaskObservation::Protocol(result),""",
    )
    omitted_worker_signal_fatal_request = replace_once(
        "worker_binary/mod.rs",
        """            let error = WorkerError::SignalTask(error);
            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Some(error)""",
        """            let error = WorkerError::SignalTask(error);
            let _fatal_error = error.to_string();
            Some(error)""",
    )
    worker_rejected_fatal_claim_still_errors = replace_once(
        "worker_binary/mod.rs",
        """            let fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            fatal_claimed.then_some(error)""",
        """            let _fatal_claimed = shutdown.request(ShutdownReason::FatalError(error.to_string()));
            Some(error)""",
    )
    worker_signal_error_skips_protocol_completion = replace_once(
        "worker_binary/mod.rs",
        """        WorkerTaskObservation::Signal(result) => WorkerTaskResult {
            protocol_result: protocol.await,""",
        """        WorkerTaskObservation::Signal(result) => WorkerTaskResult {
            protocol_result: Ok(()),""",
    )
    worker_signal_handle_double_join = replace_once(
        "worker_binary/mod.rs",
        """    let signal_result = if signal_task_consumed {
        Ok(())
    } else {
        signal_task.await.map_err(WorkerError::SignalTask)
    };""",
        """    let signal_result = signal_task.await.map_err(WorkerError::SignalTask);""",
    )
    worker_protocol_error_skips_signal_join = replace_once(
        "worker_binary/mod.rs",
        """    let signal_result = if signal_task_consumed {
        Ok(())
    } else {
        signal_task.await.map_err(WorkerError::SignalTask)
    };
    if let Some(error) = early_signal_error {
        return Err(error);
    }
    protocol_result?;
    signal_result?;""",
        """    if let Some(error) = early_signal_error {
        return Err(error);
    }
    protocol_result?;
    let signal_result = signal_task.await.map_err(WorkerError::SignalTask);
    signal_result?;""",
    )
    cfg_disabled_rest_router = replace_once(
        "server/rest/state.rs",
        canonical_state_router,
        f"""#[cfg(any())]
{canonical_state_router}

pub(super) fn r#router() -> Router<RestState> {{
    hidden!()
}}""",
    )
    alternate_desktop_backend = replace_once(
        "entrypoint.rs",
        """                    run_packaged_backend(
                        request,
                        before_dynamic,
                        UiMode::Desktop,""",
        """                    alternate_packaged_backend(
                        request,
                        before_dynamic,
                        UiMode::Desktop,""",
    )
    changed_production_target_roots: list[ProductionRoutingMutation] = []
    for (
        target_root_source,
        target_root_path,
    ) in PRODUCTION_CARGO_TARGET_ROOT_SOURCES.items():
        changed_target_root = sources.copy()
        changed_target_root[target_root_source] += (
            "\n// attacker-controlled target root\n"
        )
        changed_production_target_roots.append(
            (
                f"changed production Cargo target root {target_root_path}",
                changed_target_root,
            )
        )
    symlinked_worker_target_root = sources.copy()
    target_root_inventory = json.loads(
        symlinked_worker_target_root[PRODUCTION_CARGO_TARGET_ROOT_INVENTORY_SOURCE]
    )
    for target_root_entry in target_root_inventory:
        if target_root_entry["path"] == "rust/pokecon/src/bin/worker.rs":
            target_root_entry["kind"] = "symlink"
    symlinked_worker_target_root[PRODUCTION_CARGO_TARGET_ROOT_INVENTORY_SOURCE] = (
        json.dumps(target_root_inventory)
    )
    custom_tauri_permission = add_inventory_input(
        TAURI_ACL_INPUT_INVENTORY_SOURCE,
        "rust/pokecon/permissions/custom/nested.toml",
        '[permission]\nidentifier = "allow-security-bypass"\n',
    )
    custom_tauri_capability = add_inventory_input(
        TAURI_ACL_INPUT_INVENTORY_SOURCE,
        "rust/pokecon/capabilities/desktop.json",
        '{"identifier":"desktop","permissions":["allow-security-bypass"]}\n',
    )
    custom_tauri_json5_capability = add_inventory_input(
        TAURI_ACL_INPUT_INVENTORY_SOURCE,
        "rust/pokecon/capabilities/desktop.json5",
        '{ identifier: "desktop", permissions: ["allow-security-bypass"] }\n',
    )
    nested_autogenerated_permission = add_inventory_input(
        TAURI_ACL_INPUT_INVENTORY_SOURCE,
        "rust/pokecon/permissions/nested/autogenerated/evil.toml",
        '[permission]\nidentifier = "allow-security-bypass"\n',
    )
    alternate_tauri_json5_config = add_inventory_input(
        TAURI_CONFIG_INVENTORY_SOURCE,
        "rust/pokecon/tauri.linux.conf.json5",
        '{ build: { beforeBundleCommand: "touch /tmp/attacker" } }\n',
    )
    alternate_tauri_toml_config = add_inventory_input(
        TAURI_CONFIG_INVENTORY_SOURCE,
        "rust/pokecon/Tauri.toml",
        '[build]\nbeforeBundleCommand = "touch /tmp/attacker"\n',
    )
    changed_flake_lock = replace_once(
        FLAKE_LOCK_SOURCE,
        '"rev": "17c9d6cdfc60c64f4ee8d306f9bc0b4ccb51481e"',
        '"rev": "0000000000000000000000000000000000000000"',
    )
    symlinked_flake_lock = sources.copy()
    symlinked_flake_lock[FLAKE_LOCK_INVENTORY_SOURCE] = json.dumps(
        {"path": "flake.lock", "kind": "symlink"}
    )
    changed_gitignore = replace_once(
        "@.gitignore",
        "/rust/pokecon/permissions/autogenerated/",
        "!/rust/pokecon/permissions/autogenerated/",
    )
    omitted_tauri_xcb_dependency = replace_once(
        "@rust/pokecon/tauri.conf.json",
        '          "libxcb1",\n',
        "",
    )
    changed_tauri_assets: list[ProductionRoutingMutation] = []
    for asset_source in (
        *PRODUCTION_BINARY_BUILD_INPUT_SOURCES,
        "@tauri/linux/70-pokecon-controller.rules",
        "@tauri/linux/reload-udev.sh",
    ):
        changed_asset = sources.copy()
        changed_asset[asset_source] += (
            "00"
            if asset_source in PRODUCTION_BINARY_BUILD_INPUT_SOURCES
            else "\n# attacker-controlled Tauri asset\n"
        )
        changed_tauri_assets.append(
            (f"changed Tauri asset {asset_source}", changed_asset)
        )
    symlinked_allowed_build_script = sources.copy()
    symlinked_build_script_inventory = json.loads(
        symlinked_allowed_build_script[BUILD_SCRIPT_INVENTORY_SOURCE]
    )
    for build_script_entry in symlinked_build_script_inventory:
        if build_script_entry["path"] == "rust/pokecon/build.rs":
            build_script_entry["kind"] = "symlink"
    symlinked_allowed_build_script[BUILD_SCRIPT_INVENTORY_SOURCE] = json.dumps(
        symlinked_build_script_inventory
    )
    changed_allowed_build_script = replace_once(
        "@pokecon/build.rs",
        "fn main() {",
        'fn main() { std::fs::write("../pokecon/src/server/security.rs", "").unwrap();',
    )
    target_specific_build_dependency = sources.copy()
    target_specific_build_dependency[WORKSPACE_MANIFEST_SOURCES["rust/pokecon"]] += """

[target.'cfg(unix)'.build-dependencies]
tauri-build = "2.5.4"
"""
    added_registry_dependency = replace_once(
        WORKSPACE_MANIFEST_SOURCES["rust/pokecon"],
        "[dependencies]\n",
        '[dependencies]\nring = "0.17.14"\n',
    )
    allowed_application_unsafe_code = replace_once(
        POKECON_MANIFEST_SOURCE,
        'unsafe_code = "deny"',
        'unsafe_code = "allow"',
    )
    changed_cargo_lock = replace_once(
        WORKSPACE_LOCK_SOURCE,
        "version = 4\n",
        "version = 3\n",
    )
    escaped_path_dependency = replace_once(
        POKECON_MANIFEST_SOURCE,
        "\n[build-dependencies]\n",
        '\nattacker = { path = "../../attacker" }\n\n[build-dependencies]\n',
    )
    aliased_pokecon_server_dependency = replace_once(
        POKECON_MANIFEST_SOURCE,
        "\n[build-dependencies]\n",
        '\npokecon-server = { package = "pokecon", path = "." }\n\n'
        "[build-dependencies]\n",
    )
    aliased_workspace_axum_dependency = replace_once(
        WORKSPACE_MANIFEST_SOURCE,
        'axum = "0.8.9"',
        'axum = { package = "pokecon", path = "rust/pokecon" }',
    )
    cargo_config_wrapper = sources.copy()
    cargo_config_wrapper[CARGO_CONFIG_INVENTORY_SOURCE] = json.dumps(
        [".cargo/config.toml"]
    )
    cargo_config_wrapper["@.cargo/config.toml"] = """
[build]
rustc-workspace-wrapper = "scripts/attacker-wrapper.sh"
rustflags = ["--cfg", "pokecon_security_bypass"]
"""
    legacy_cargo_config = sources.copy()
    legacy_cargo_config[CARGO_CONFIG_INVENTORY_SOURCE] = json.dumps([".cargo/config"])
    legacy_cargo_config["@.cargo/config"] = """
[build]
rustc-wrapper = "scripts/attacker-wrapper.sh"
"""
    nested_cargo_config = sources.copy()
    nested_cargo_config[CARGO_CONFIG_INVENTORY_SOURCE] = json.dumps(
        [{"path": "rust/pokecon/.cargo/config.toml", "kind": "regular"}]
    )
    nested_cargo_config["@rust/pokecon/.cargo/config.toml"] = """
[target.x86_64-unknown-linux-gnu]
runner = "scripts/attacker-runner.sh"
"""
    generated_cargo_config_wrapper = replace_once(
        FLAKE_SOURCE,
        "            [net]\n            offline = true\n",
        "            [build]\n"
        '            rustc-wrapper = "/tmp/attacker-rustc-wrapper"\n\n'
        "            [net]\n"
        "            offline = true\n",
    )
    generated_cargo_config_rustflags = replace_once(
        FLAKE_SOURCE,
        "            [net]\n            offline = true\n",
        "            [build]\n"
        '            rustflags = ["--cfg", "pokecon_security_bypass"]\n\n'
        "            [net]\n"
        "            offline = true\n",
    )
    generated_cargo_config_target_tools = replace_once(
        FLAKE_SOURCE,
        "            [net]\n            offline = true\n",
        "            [target.x86_64-unknown-linux-gnu]\n"
        '            rustc = "/tmp/attacker-rustc"\n'
        '            linker = "/tmp/attacker-linker"\n'
        '            runner = "/tmp/attacker-runner"\n\n'
        "            [net]\n"
        "            offline = true\n",
    )
    generated_cargo_config_alias = replace_once(
        FLAKE_SOURCE,
        "            [net]\n            offline = true\n",
        "            [alias]\n"
        '            build = ["run", "--manifest-path", "/tmp/attacker/Cargo.toml"]\n\n'
        "            [net]\n"
        "            offline = true\n",
    )
    redirected_pokecon_lib_overlay = replace_once(
        FLAKE_SOURCE,
        'path = "${source}/rust/pokecon/src/lib.rs"',
        'path = "src/alternate.rs"',
    )
    redirected_pokecon_main_overlay = replace_once(
        FLAKE_SOURCE,
        'path = \\"${source}/rust/pokecon/src/main.rs\\"',
        'path = \\"src/alternate-main.rs\\"',
    )
    redirected_pokecon_build_overlay = replace_once(
        FLAKE_SOURCE,
        'build = "${source}/rust/pokecon/build.rs"',
        'build = "build.rs"',
    )
    redirected_controlled_dependency = replace_once(
        FLAKE_SOURCE,
        "                      canonicalPokeconManifestText\n"
        "                  )\n"
        "              );",
        "                      (\n"
        '                        replaceManifestString "controlled dependency redirect"\n'
        "                          ''webrtc.workspace = true''\n"
        "                          ''webrtc = { package = \"pokecon\", path = \".\" }''\n"
        "                          canonicalPokeconManifestText\n"
        "                      )\n"
        "                  )\n"
        "              );",
    )
    accepted_symlinked_source = replace_once(
        FLAKE_SOURCE,
        '                  type == "regular"\n                  && (\n',
        '                  (type == "regular" || type == "symlink")\n'
        "                  && (\n",
    )
    changed_rust_toolchain = replace_once(
        RUST_TOOLCHAIN_SOURCE,
        'channel = "stable"',
        'channel = "nightly"',
    )
    symlinked_rust_toolchain = sources.copy()
    symlinked_rust_toolchain[RUST_TOOLCHAIN_INVENTORY_SOURCE] = json.dumps(
        {"path": "rust-toolchain.toml", "kind": "symlink"}
    )
    redirected_rust_toolchain_binding = replace_once(
        FLAKE_SOURCE,
        "            pkgsWithOverlays.rust-bin.fromRustupToolchainFile "
        '(inputs.self.outPath + "/rust-toolchain.toml");',
        "            pkgsWithOverlays.rust-bin.fromRustupToolchainFile "
        "/tmp/attacker-rust-toolchain.toml;",
    )
    reassigned_reproducible_rustc = replace_once(
        FLAKE_SOURCE,
        "            fi\n"
        "            : \"''${POKECON_RUST_REMAP_SOURCE:?POKECON_RUST_REMAP_SOURCE is required}\"",
        "            fi\n"
        '            rustc="/tmp/attacker-rustc"\n'
        "            : \"''${POKECON_RUST_REMAP_SOURCE:?POKECON_RUST_REMAP_SOURCE is required}\"",
    )
    reassigned_pinned_rustc = replace_once(
        FLAKE_SOURCE,
        '              "${rustToolchain}/bin/rustc")\n'
        '                exec "$compiler" "$@"',
        '              "${rustToolchain}/bin/rustc")\n'
        '                compiler="/tmp/attacker-rustc"\n'
        '                exec "$compiler" "$@"',
    )
    package_cargo_compiler_alias_after_sanitizer = replace_once(
        FLAKE_SOURCE,
        "            preBuild = ''\n"
        "              ${installControlledCargoManifests}\n"
        "              ${sanitizeCargoCompilerEnvironment}\n",
        "            preBuild = ''\n"
        "              ${installControlledCargoManifests}\n"
        "              ${sanitizeCargoCompilerEnvironment}\n"
        '              export CARGO_BUILD_RUSTC="/tmp/attacker-rustc"\n',
    )
    redirected_package_source = replace_once(
        FLAKE_SOURCE,
        "          pokeconPackage = rustPlatform.buildRustPackage {\n"
        '            pname = "pokecon";\n'
        "            version = workspaceVersion;\n"
        "            src = source;\n",
        "          pokeconPackage = rustPlatform.buildRustPackage {\n"
        '            pname = "pokecon";\n'
        "            version = workspaceVersion;\n"
        "            src = /tmp/attacker-source;\n",
    )
    custom_package_build_phase = replace_once(
        FLAKE_SOURCE,
        '            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";\n',
        "            buildPhase = ''\n"
        '              export CARGO_BUILD_RUSTC="/tmp/attacker-rustc"\n'
        "              cargoBuildHook\n"
        "            '';\n"
        '            POKECON_BUILD_UV_PATH = "${pkgs.uv}/bin/uv";\n',
    )
    recursive_package_release_tree_install = replace_once(
        FLAKE_SOURCE,
        '                "${pkgs.coreutils}/bin/install" -Dm755 -- \\\n'
        '                  "$packaged_source" "$out/bin/$packaged_binary"\n',
        '                "${pkgs.coreutils}/bin/cp" -R -- \\\n'
        '                  "$package_target/." "$out/bin/"\n',
    )
    omitted_package_preinstall_hook = replace_once(
        FLAKE_SOURCE,
        "            installPhase = ''\n"
        "              runHook preInstall\n"
        "              : \"''${cargoBuildType:?cargoBuildType is required}\"\n",
        "            installPhase = ''\n"
        "              : \"''${cargoBuildType:?cargoBuildType is required}\"\n",
    )
    extra_packaged_binary = replace_once(
        FLAKE_SOURCE,
        "              for packaged_binary in pokecon pokecon-worker; do\n",
        "              for packaged_binary in pokecon pokecon-worker attacker; do\n",
    )
    omitted_packaged_executable_guard = replace_once(
        FLAKE_SOURCE,
        '                if [ -L "$packaged_source" ] || [ ! -f "$packaged_source" ]'
        ' || [ ! -x "$packaged_source" ]; then\n'
        '                  echo "packaged executable is missing, redirected, or not executable:'
        ' $packaged_source" >&2\n'
        "                  exit 2\n"
        "                fi\n",
        "",
    )
    omitted_package_postinstall_hook = replace_once(
        FLAKE_SOURCE,
        "              unset packaged_binary packaged_source package_target\n"
        "              runHook postInstall\n"
        "            '';\n",
        "              unset packaged_binary packaged_source package_target\n"
        "            '';\n",
    )
    unlocked_package_build = replace_once(
        FLAKE_SOURCE,
        '            cargoBuildFlags = [\n              "--locked"\n',
        "            cargoBuildFlags = [\n",
    )
    tauri_cargo_compiler_alias_after_sanitizer = replace_once(
        FLAKE_SOURCE,
        "            ${sanitizeCargoCompilerEnvironment}\n"
        '            expected_cargo_target_dir="$gate_home/cargo-target"\n',
        "            ${sanitizeCargoCompilerEnvironment}\n"
        '            export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="/tmp/attacker-linker"\n'
        '            expected_cargo_target_dir="$gate_home/cargo-target"\n',
    )
    unguarded_tauri_cargo_before_boundary = replace_once(
        FLAKE_SOURCE,
        '                    export POKECON_RUST_REMAP_SOURCE="${controlledCargoSource}"\n',
        '                    "${rustToolchain}/bin/cargo" build \\\n'
        '                      --manifest-path "$workdir/Cargo.toml" \\\n'
        "                      --locked \\\n"
        "                      --release \\\n"
        "                      --package attacker\n"
        '                    export POKECON_RUST_REMAP_SOURCE="${controlledCargoSource}"\n',
    )
    omitted_controlled_workspace_manifest = replace_once(
        FLAKE_SOURCE,
        '            install_controlled_cargo_manifest "${controlledWorkspaceManifest}" Cargo.toml\n',
        "",
    )
    omitted_controlled_cargo_lock = replace_once(
        FLAKE_SOURCE,
        '            install_controlled_cargo_manifest "${controlledCargoLock}" Cargo.lock\n',
        "",
    )
    omitted_controlled_member_manifest = replace_once(
        FLAKE_SOURCE,
        "              ''install_controlled_cargo_manifest \"${\n"
        "                controlledWorkspaceMemberManifests.${memberPath}\n"
        '              }" "${memberPath}/Cargo.toml"\'\'\n'
        "            ) workspaceMemberPaths}\n"
        "            unset -f install_controlled_cargo_manifest",
        "              ''install_controlled_cargo_manifest \"${\n"
        "                controlledWorkspaceMemberManifests.${memberPath}\n"
        '              }" "${memberPath}/Cargo.toml"\'\'\n'
        '            ) (lib.filter (memberPath: memberPath != "rust/pokecon") workspaceMemberPaths)}\n'
        "            unset -f install_controlled_cargo_manifest",
    )
    bypassed_symlink_member_check = replace_once(
        FLAKE_SOURCE,
        '              if [ -L "$controlled_workspace_directory" ] \\\n'
        '                || [ ! -d "$controlled_workspace_directory" ] \\\n',
        '              if [ ! -L "$controlled_workspace_directory" ] \\\n'
        '                && [ ! -d "$controlled_workspace_directory" ] \\\n',
    )
    omitted_tauri_ancestor_config_guard = replace_once(
        FLAKE_SOURCE,
        '              cd "$cargo_source_root"\n'
        "              ${verifyControlledCargoManifests}\n"
        "              ${assertNoRepositoryCargoConfigs}\n"
        "              ${assertNoCargoConfigAncestors}\n",
        '              cd "$cargo_source_root"\n'
        "              ${verifyControlledCargoManifests}\n"
        "              ${assertNoRepositoryCargoConfigs}\n",
    )
    omitted_direct_cargo_ancestor_guard = replace_once(
        FLAKE_SOURCE,
        '                      cd "${cargoInvocationRoot}"\n'
        "                      ${assertNoCargoConfigAncestors}\n"
        "                      POKECON_RESOURCE_PROVENANCE=development \\\n"
        '                        "${rustToolchain}/bin/cargo" build',
        '                      cd "${cargoInvocationRoot}"\n'
        "                      POKECON_RESOURCE_PROVENANCE=development \\\n"
        '                        "${rustToolchain}/bin/cargo" build',
    )
    omitted_setup_workdir_ancestor_config_guard = replace_once(
        FLAKE_SOURCE,
        '            cd "$workdir"\n'
        "            ${assertNoCargoConfigAncestors}\n"
        "          '';\n\n"
        "          linuxDesktopPackages =",
        '            cd "$workdir"\n'
        "          '';\n\n"
        "          linuxDesktopPackages =",
    )
    changed_cargo_cache_directory_tag = replace_once(
        FLAKE_SOURCE,
        "            Signature: 8a477f597d28d172789f06886806bc55\n",
        "            Signature: 00000000000000000000000000000000\n",
    )
    omitted_cargo_cache_directory_tag_comparison = replace_once(
        FLAKE_SOURCE,
        '            if ! "${pkgs.diffutils}/bin/cmp" -s -- "${cargoCacheDirectoryTag}" "$cargo_cache_tag"; then\n',
        "            if false; then\n",
    )
    ignored_repository_config_scan_failure = replace_once(
        FLAKE_SOURCE,
        ') -print0 > "$repository_cargo_config_inventory"; then',
        ') -print0 > "$repository_cargo_config_inventory" && false; then',
    )
    legacy_cargo_home_config_not_cleared = replace_once(
        FLAKE_SOURCE,
        '            rm -rf -- "$CARGO_HOME/config" "$CARGO_HOME/config.toml"\n',
        '            rm -rf -- "$CARGO_HOME/config.toml"\n',
    )
    legacy_config_flake = legacy_cargo_home_config_not_cleared[FLAKE_SOURCE]
    legacy_config_check = (
        '            if [ -e "$CARGO_HOME/config" ] || [ -L "$CARGO_HOME/config" ] \\\n'
        '              || [ ! -L "$CARGO_HOME/config.toml" ] \\\n'
    )
    assert legacy_config_check in legacy_config_flake
    legacy_cargo_home_config_not_cleared[FLAKE_SOURCE] = legacy_config_flake.replace(
        legacy_config_check,
        '            if [ ! -L "$CARGO_HOME/config.toml" ] \\\n',
        1,
    )
    legacy_cargo_home_config_not_cleared[FLAKE_SOURCE] = refresh_canonical_flake_hash(
        legacy_cargo_home_config_not_cleared[FLAKE_SOURCE]
    )
    reassigned_tauri_cargo_home = replace_once(
        FLAKE_SOURCE,
        '                    cp -p "$normalized_application" "$application"\n'
        "                    ${prepareTauriCargoInvocation}\n"
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"',
        '                    cp -p "$normalized_application" "$application"\n'
        "                    ${prepareTauriCargoInvocation}\n"
        '                    export CARGO_HOME="/tmp/attacker-cargo-home"\n'
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"',
    )
    reassigned_tauri_cargo_executable = replace_once(
        FLAKE_SOURCE,
        '                    cp -p "$normalized_application" "$application"\n'
        "                    ${prepareTauriCargoInvocation}\n"
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"',
        '                    cp -p "$normalized_application" "$application"\n'
        "                    ${prepareTauriCargoInvocation}\n"
        '                    export CARGO="/tmp/attacker-cargo"\n'
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"',
    )
    redirected_tauri_cargo_path = replace_once(
        FLAKE_SOURCE,
        '                      export PATH="${rustToolchain}/bin:$PATH"\n',
        '                      export PATH="/tmp/attacker-bin:${rustToolchain}/bin:$PATH"\n',
    )
    omitted_final_tauri_boundary = replace_once(
        FLAKE_SOURCE,
        '                    cp -p "$normalized_application" "$application"\n'
        "                    ${prepareTauriCargoInvocation}\n"
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"',
        '                    cp -p "$normalized_application" "$application"\n'
        "                    (\n"
        '                      export PATH="${rustToolchain}/bin:$PATH"\n'
        '                      cd "$cargo_source_root/rust/pokecon"',
    )
    cargo_invocation_root_config = replace_once(
        FLAKE_SOURCE,
        "            mkdir -p \"$out\"\n          '';",
        '            mkdir -p "$out/.cargo"\n'
        '            cp /tmp/attacker-config "$out/.cargo/config.toml"\n'
        "          '';",
    )
    ignored_production_audit_failure = replace_once(
        FLAKE_SOURCE,
        '"${productionRoutingAuditTest}::'
        'test_rust_routes_override_implicit_head_and_websocket_any"',
        '"${productionRoutingAuditTest}::'
        'test_rust_routes_override_implicit_head_and_websocket_any" || true',
    )
    removed_json5_source_filter = replace_once(
        FLAKE_SOURCE,
        '                    || lib.hasSuffix ".json5" sourcePath\n',
        "",
    )
    weakened_direct_input_assertion = replace_once(
        FLAKE_SOURCE,
        "    assert\n"
        '      resolvedInputsAreCanonical || builtins.throw "resolved direct Nix inputs differ from flake.lock";\n',
        "    assert true;\n",
    )
    redirected_flake_parts_input = replace_once(
        FLAKE_SOURCE,
        "          value = flake-parts;\n",
        "          value = inputs.nixpkgs;\n",
    )
    omitted_canonical_flake_guard = replace_once(
        FLAKE_SOURCE,
        "    assert\n"
        '      builtins.hashString "sha256" normalizedCanonicalFlakeText == canonicalFlakeHash\n'
        '      || builtins.throw "flake.nix differs from its normalized canonical hash";\n',
        "",
    )
    mismatched_canonical_flake_hash = sources.copy()
    canonical_hash_match = re.search(
        r'canonicalFlakeHash = "(?P<hash>[0-9a-f]{64})";',
        sources[FLAKE_SOURCE],
    )
    assert canonical_hash_match is not None
    wrong_canonical_hash = (
        "f" * 64 if canonical_hash_match.group("hash") != "f" * 64 else "e" * 64
    )
    mismatched_canonical_flake_hash[FLAKE_SOURCE] = (
        sources[FLAKE_SOURCE][: canonical_hash_match.start("hash")]
        + wrong_canonical_hash
        + sources[FLAKE_SOURCE][canonical_hash_match.end("hash") :]
    )
    omitted_web_version = replace_once(
        FLAKE_SOURCE,
        "            POKECON_WEB_VERSION = workspaceVersion;\n",
        "",
    )
    redirected_portable_uv_execution_loader = replace_once(
        FLAKE_SOURCE,
        "portableUvExecutionLoader = "
        "linuxReleasePkgs.stdenv.cc.bintools.dynamicLinker;",
        "portableUvExecutionLoader = portableUvSystemInterpreter;",
    )
    changed_portable_uv_version_output = replace_once(
        FLAKE_SOURCE,
        '          portableUvVersionOutput = "uv 0.11.8 (x86_64-unknown-linux-gnu)";\n',
        '          portableUvVersionOutput = "uv 0.11.8";\n',
    )
    weakened_portable_uv_version_comparison = replace_once(
        FLAKE_SOURCE,
        '                if [ "$actual_portable_uv_version_output" != "${portableUvVersionOutput}" ]; then\n',
        '                if [ "$actual_portable_uv_version_output" != "uv ${portableUvVersion}" ]; then\n',
    )
    unchecked_portable_uv_version_probe = replace_once(
        FLAKE_SOURCE,
        '                if ! actual_portable_uv_version_output="$("$out/bin/uv" --version)"; then\n',
        '                actual_portable_uv_version_output="$("$out/bin/uv" --version)" || true\n'
        "                if false; then\n",
    )
    omitted_portable_uv_actual_version_diagnostic = replace_once(
        FLAKE_SOURCE,
        '                  echo "portable uv execution copy reports an unexpected version; expected: ${portableUvVersionOutput}; actual: $actual_portable_uv_version_output" >&2\n',
        '                  echo "portable uv execution copy reports an unexpected version" >&2\n',
    )
    omitted_portable_uv_probe_actual_diagnostic = replace_once(
        FLAKE_SOURCE,
        '                  echo "portable uv execution copy version probe failed; actual output: $actual_portable_uv_version_output" >&2\n',
        '                  echo "portable uv execution copy version probe failed" >&2\n',
    )
    weakened_portable_uv_needed_inventory = replace_once(
        FLAKE_SOURCE,
        '            "libgcc_s.so.1"\n',
        "",
    )
    omitted_portable_uv_raw_rpath_check = replace_once(
        FLAKE_SOURCE,
        '                if [ -n "$raw_uv_rpath" ]; then\n',
        "                if false; then\n",
    )
    unpatched_portable_uv_execution_interpreter = replace_once(
        FLAKE_SOURCE,
        '                  --set-interpreter "${portableUvExecutionLoader}" \\\n',
        '                  --set-interpreter "${portableUvSystemInterpreter}" \\\n',
    )
    raw_runtime_uv_execution = replace_once(
        FLAKE_SOURCE,
        '--uv "${portableUvExecutionBinary}"',
        '--uv "${portableUvBinary}"',
    )
    omitted_linux_release_gcc_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleaseCc.cc.lib\n",
        "",
    )
    host_linux_release_gcc_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleaseCc.cc.lib\n",
        "              pkgs.stdenv.cc.cc.lib\n",
    )
    host_linux_release_portaudio_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePortaudio\n",
        "              pkgs.portaudio\n",
    )
    omitted_linux_release_zlib_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.zlib\n",
        "",
    )
    host_linux_release_zlib_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.zlib\n",
        "              pkgs.zlib\n",
    )
    omitted_linux_release_xcb_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libxcb\n",
        "",
    )
    host_linux_release_xcb_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libxcb\n",
        "              pkgs.xorg.libxcb\n",
    )
    omitted_linux_release_libglvnd_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.libglvnd\n",
        "",
    )
    host_linux_release_libglvnd_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.libglvnd\n",
        "              pkgs.libglvnd\n",
    )
    omitted_linux_release_glib_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.glib.out\n",
        "",
    )
    host_linux_release_glib_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.glib.out\n",
        "              pkgs.glib.out\n",
    )
    omitted_linux_release_libsm_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libSM\n",
        "",
    )
    host_linux_release_libsm_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libSM\n",
        "              pkgs.xorg.libSM\n",
    )
    omitted_linux_release_libxext_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libXext\n",
        "",
    )
    host_linux_release_libxext_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libXext\n",
        "              pkgs.xorg.libXext\n",
    )
    omitted_linux_release_libxrender_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libXrender\n",
        "",
    )
    host_linux_release_libxrender_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleasePkgs.xorg.libXrender\n",
        "              pkgs.xorg.libXrender\n",
    )
    extra_linux_release_runtime_library = replace_once(
        FLAKE_SOURCE,
        "              linuxReleaseCc.cc.lib\n",
        "              linuxReleaseCc.cc.lib\n              linuxReleasePkgs.glibc\n",
    )
    raw_portaudio_fod_runtime_library_path = replace_once(
        FLAKE_SOURCE,
        '                    --runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"\n',
        '                    --runtime-library-path "${linuxReleasePortaudio}/lib"\n',
    )
    raw_portaudio_package_smoke_runtime_library_path = replace_once(
        FLAKE_SOURCE,
        '                  --runtime-library-path "${linuxReleaseRuntimeLibraries}/lib" \\\n',
        '                  --runtime-library-path "${linuxReleasePortaudio}/lib" \\\n',
    )
    split_fod_runtime_library_path = replace_once(
        FLAKE_SOURCE,
        '                    --runtime-library-path "${linuxReleaseRuntimeLibraries}/lib"\n',
        '                    --runtime-library-path "${linuxReleasePortaudio}/lib:${linuxReleaseCc.cc.lib}/lib:${linuxReleasePkgs.zlib}/lib:${linuxReleasePkgs.xorg.libxcb}/lib:${linuxReleasePkgs.libglvnd}/lib:${linuxReleasePkgs.glib.out}/lib:${linuxReleasePkgs.xorg.libSM}/lib:${linuxReleasePkgs.xorg.libXext}/lib:${linuxReleasePkgs.xorg.libXrender}/lib"\n',
    )
    inherited_fod_runtime_ld_library_path = replace_once(
        FLAKE_SOURCE,
        "                    UV_NO_CONFIG=1 \\\n",
        "                    UV_NO_CONFIG=1 \\\n"
        "                    LD_LIBRARY_PATH=\"''${LD_LIBRARY_PATH}\" \\\n",
    )
    inherited_package_smoke_ld_library_path = replace_once(
        FLAKE_SOURCE,
        "                unset LD_LIBRARY_PATH\n",
        "",
    )
    baked_runtime_library_path_into_wheel = replace_once(
        FLAKE_SOURCE,
        '                    LDFLAGS="-L${linuxReleasePortaudio}/lib" \\\n',
        '                    LDFLAGS="-Wl,-rpath,${linuxReleaseRuntimeLibraries}/lib -L${linuxReleasePortaudio}/lib" \\\n',
    )
    omitted_runtime_python_execution_loader = replace_once(
        FLAKE_SOURCE,
        '                    --execution-loader "${portableUvExecutionLoader}" \\\n',
        "",
    )
    omitted_runtime_python_execution_library_path = replace_once(
        FLAKE_SOURCE,
        '                    --execution-library-path "${portableUvExecutionLibraryPath}" \\\n',
        "",
    )
    unqualified_portable_python_install_request = replace_once(
        "@release/build_runtime.py",
        "    install_request = python_install_request(sys.platform)\n",
        "    install_request = PYTHON_VERSION\n",
    )
    unused_portable_python_install_selector_result = replace_once(
        "@release/build_runtime.py",
        "            install_request,\n",
        '            "cpython-3.14.3-linux-x86_64-gnu",\n',
    )
    unused_linux_portable_python_install_request = replace_once(
        "@release/build_runtime.py",
        "        return LINUX_PYTHON_INSTALL_REQUEST\n",
        '        return "cpython-3.14.3-linux-x86_64-gnu"\n',
    )
    unused_windows_portable_python_install_request = replace_once(
        "@release/build_runtime.py",
        "        return WINDOWS_PYTHON_INSTALL_REQUEST\n",
        '        return "cpython-3.14.3-windows-x86_64-none"\n',
    )
    wrong_portable_python_install_implementation = replace_once(
        "@release/build_runtime.py",
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"',
        'LINUX_PYTHON_INSTALL_REQUEST = "pypy-3.14.3-linux-x86_64-gnu"',
    )
    wrong_portable_python_install_version = replace_once(
        "@release/build_runtime.py",
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"',
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.2-linux-x86_64-gnu"',
    )
    wrong_portable_python_install_os = replace_once(
        "@release/build_runtime.py",
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"',
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-darwin-x86_64-gnu"',
    )
    wrong_portable_python_install_arch = replace_once(
        "@release/build_runtime.py",
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"',
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-aarch64-gnu"',
    )
    wrong_portable_python_install_libc = replace_once(
        "@release/build_runtime.py",
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-gnu"',
        'LINUX_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-musl"',
    )
    wrong_windows_portable_python_install_implementation = replace_once(
        "@release/build_runtime.py",
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"',
        'WINDOWS_PYTHON_INSTALL_REQUEST = "pypy-3.14.3-windows-x86_64-none"',
    )
    wrong_windows_portable_python_install_version = replace_once(
        "@release/build_runtime.py",
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"',
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.2-windows-x86_64-none"',
    )
    wrong_windows_portable_python_install_os = replace_once(
        "@release/build_runtime.py",
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"',
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-linux-x86_64-none"',
    )
    wrong_windows_portable_python_install_arch = replace_once(
        "@release/build_runtime.py",
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"',
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-aarch64-none"',
    )
    wrong_windows_portable_python_install_libc = replace_once(
        "@release/build_runtime.py",
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-none"',
        'WINDOWS_PYTHON_INSTALL_REQUEST = "cpython-3.14.3-windows-x86_64-gnu"',
    )
    widened_linux_portable_python_platform = replace_once(
        "@release/build_runtime.py",
        '    if platform_name == "linux":\n',
        '    if platform_name.startswith("linux"):\n',
    )
    redirected_linux_portable_python_install_request = replace_once(
        "@release/build_runtime.py",
        "        return LINUX_PYTHON_INSTALL_REQUEST\n",
        "        return WINDOWS_PYTHON_INSTALL_REQUEST\n",
    )
    redirected_windows_portable_python_install_request = replace_once(
        "@release/build_runtime.py",
        "        return WINDOWS_PYTHON_INSTALL_REQUEST\n",
        "        return LINUX_PYTHON_INSTALL_REQUEST\n",
    )
    wrong_linux_python_minor_redirect = replace_once(
        "@release/build_runtime.py",
        'LINUX_PYTHON_MINOR_REDIRECT = "cpython-3.14-linux-x86_64-gnu"',
        'LINUX_PYTHON_MINOR_REDIRECT = "cpython-3.13-linux-x86_64-gnu"',
    )
    wrong_windows_python_minor_redirect = replace_once(
        "@release/build_runtime.py",
        'WINDOWS_PYTHON_MINOR_REDIRECT = "cpython-3.14-windows-x86_64-none"',
        'WINDOWS_PYTHON_MINOR_REDIRECT = "cpython-3.13-windows-x86_64-none"',
    )
    redirected_linux_python_minor_redirect = replace_once(
        "@release/build_runtime.py",
        "        return LINUX_PYTHON_MINOR_REDIRECT\n",
        "        return WINDOWS_PYTHON_MINOR_REDIRECT\n",
    )
    redirected_windows_python_minor_redirect = replace_once(
        "@release/build_runtime.py",
        "        return WINDOWS_PYTHON_MINOR_REDIRECT\n",
        "        return LINUX_PYTHON_MINOR_REDIRECT\n",
    )
    accepted_unknown_portable_python_platform = replace_once(
        "@release/build_runtime.py",
        "    raise ValueError(message)\n\n\ndef python_minor_redirect_name",
        "    return LINUX_PYTHON_INSTALL_REQUEST\n\n\ndef python_minor_redirect_name",
    )
    accepted_unknown_python_minor_redirect_platform = replace_once(
        "@release/build_runtime.py",
        "    raise ValueError(message)\n\n\ndef python_executable",
        "    return LINUX_PYTHON_MINOR_REDIRECT\n\n\ndef python_executable",
    )
    changed_portable_python_runtime_version = replace_once(
        "@release/build_runtime.py",
        'PYTHON_VERSION = "3.14.3"',
        'PYTHON_VERSION = "3.14.2"',
    )
    qualified_request_used_for_python_version_validation = replace_once(
        "@release/build_runtime.py",
        "    if version != PYTHON_VERSION:\n",
        "    if version != python_install_request(sys.platform):\n",
    )
    accepted_missing_managed_python_inventory_entry = replace_once(
        "@release/build_runtime.py",
        "    if actual_names != expected_names:\n",
        "    if not actual_names.issubset(expected_names):\n",
    )
    accepted_extra_managed_python_inventory_entry = replace_once(
        "@release/build_runtime.py",
        "    if actual_names != expected_names:\n",
        "    if not expected_names.issubset(actual_names):\n",
    )
    accepted_redirected_managed_python_gitignore = replace_once(
        "@release/build_runtime.py",
        "not _is_real_regular_file(gitignore)",
        "not gitignore.is_file()",
    )
    accepted_redirected_managed_python_lock = replace_once(
        "@release/build_runtime.py",
        "not _is_real_regular_file(lock)",
        "not lock.is_file()",
    )
    changed_managed_python_gitignore_content = replace_once(
        "@release/build_runtime.py",
        'gitignore.read_bytes() != b"*"',
        'gitignore.read_bytes() not in {b"*", b"*\\n"}',
    )
    changed_managed_python_lock_content = replace_once(
        "@release/build_runtime.py",
        'lock.read_bytes() != b""',
        'lock.read_bytes() not in {b"", b"locked"}',
    )
    accepted_managed_python_temp_residue = replace_once(
        "@release/build_runtime.py",
        "    if not _is_real_directory(temporary) or tuple(temporary.iterdir()):\n",
        "    if not _is_real_directory(temporary):\n",
    )
    accepted_redirected_managed_python_temp = replace_once(
        "@release/build_runtime.py",
        "    if not _is_real_directory(temporary) or tuple(temporary.iterdir()):\n",
        "    if not temporary.is_dir() or tuple(temporary.iterdir()):\n",
    )
    accepted_redirected_managed_python_root = replace_once(
        "@release/build_runtime.py",
        "    if not _is_real_directory(install_root):\n",
        "    if not install_root.is_dir():\n",
    )
    accepted_redirected_managed_python_full_install = replace_once(
        "@release/build_runtime.py",
        "    if not _is_real_directory(full_install):\n",
        "    if not full_install.is_dir():\n",
    )
    accepted_missing_managed_python_minor_redirect = replace_once(
        "@release/build_runtime.py",
        "    if actual_names != expected_names:\n",
        "    if not actual_names.issubset(expected_names):\n",
    )
    missing_minor_source = accepted_missing_managed_python_minor_redirect[
        "@release/build_runtime.py"
    ]
    missing_minor_anchor = "    minor_redirect = entries[minor_redirect_name]\n"
    assert missing_minor_anchor in missing_minor_source
    accepted_missing_managed_python_minor_redirect["@release/build_runtime.py"] = (
        missing_minor_source.replace(
            missing_minor_anchor,
            "    minor_redirect = entries.get(minor_redirect_name)\n"
            "    if minor_redirect is None:\n"
            "        return installed_prefix\n",
            1,
        )
    )
    accepted_wrong_managed_python_minor_target = replace_once(
        "@release/build_runtime.py",
        "    if minor_redirect.resolve(strict=True) != installed_prefix:\n",
        "    if False:\n",
    )
    accepted_linux_managed_python_minor_junction = replace_once(
        "@release/build_runtime.py",
        "            minor_redirect.is_symlink() and not minor_redirect.is_junction()\n",
        "            minor_redirect.is_junction() and not minor_redirect.is_symlink()\n",
    )
    accepted_windows_managed_python_minor_symlink = replace_once(
        "@release/build_runtime.py",
        "            minor_redirect.is_junction() and not minor_redirect.is_symlink()\n",
        "            minor_redirect.is_symlink() and not minor_redirect.is_junction()\n",
    )
    first_managed_python_directory_fallback = replace_once(
        "@release/build_runtime.py",
        "    if actual_names != expected_names:\n",
        "    if not expected_names.issubset(actual_names):\n",
    )
    first_directory_source = first_managed_python_directory_fallback[
        "@release/build_runtime.py"
    ]
    first_directory_anchor = "    full_install = entries[install_request]\n"
    assert first_directory_anchor in first_directory_source
    first_managed_python_directory_fallback["@release/build_runtime.py"] = (
        first_directory_source.replace(
            first_directory_anchor,
            "    full_install = next(\n"
            "        entry for entry in entries.values()\n"
            "        if entry.name not in {'.gitignore', '.lock', '.temp', minor_redirect_name}\n"
            "    )\n",
            1,
        )
    )
    unnormalized_managed_python_inventory_stat_error = replace_once(
        "@release/build_runtime.py",
        "    except OSError:\n        return False\n",
        "    except FileNotFoundError:\n        return False\n",
    )
    patched_raw_portable_python_runtime = replace_once(
        "@release/build_runtime.py",
        '    execution_root = workspace / "python-execution"\n',
        "    execution_root = runtime_root\n",
    )
    executed_raw_portable_python = replace_once(
        "@release/build_runtime.py",
        "            execution_python, execution_root, raw_python_digest = (\n",
        "            _, _, raw_python_digest = (\n",
    )
    omitted_final_raw_python_digest_check = replace_once(
        "@release/build_runtime.py",
        "        if sha256_file(raw_python) != raw_python_digest:\n"
        '            message = "release build changed the raw portable Python executable"\n'
        "            raise ValueError(message)\n",
        "",
    )
    omitted_final_raw_python_elf_check = replace_once(
        "@release/build_runtime.py",
        "            if portable_python_elf_metadata(raw_python, patchelf) != (\n",
        "            if False and portable_python_elf_metadata(raw_python, patchelf) != (\n",
    )
    poisoned_runtime_pythonpath = replace_once(
        FLAKE_SOURCE,
        "                    PYTHONSAFEPATH=1 \\\n",
        "                    PYTHONSAFEPATH=1 \\\n"
        "                    PYTHONPATH=/tmp/attacker \\\n",
    )
    omitted_runtime_safepath = replace_once(
        FLAKE_SOURCE,
        "                    PYTHONSAFEPATH=1 \\\n",
        "",
    )
    poisoned_runtime_pip_config = replace_once(
        FLAKE_SOURCE,
        "                    PIP_CONFIG_FILE=/dev/null \\\n",
        "                    PIP_CONFIG_FILE=/tmp/attacker-pip.conf \\\n",
    )
    omitted_runtime_uv_no_config = replace_once(
        FLAKE_SOURCE,
        "                    UV_NO_CONFIG=1 \\\n",
        "",
    )
    omitted_runtime_uv_libc = replace_once(
        FLAKE_SOURCE,
        "                    UV_LIBC=gnu \\\n",
        "",
    )
    weakened_runtime_uv_libc = replace_once(
        FLAKE_SOURCE,
        "                    UV_LIBC=gnu \\\n",
        "                    UV_LIBC=\"''${UV_LIBC:-gnu}\" \\\n",
    )
    musl_runtime_uv_libc = replace_once(
        FLAKE_SOURCE,
        "                    UV_LIBC=gnu \\\n",
        "                    UV_LIBC=musl \\\n",
    )
    none_runtime_uv_libc = replace_once(
        FLAKE_SOURCE,
        "                    UV_LIBC=gnu \\\n",
        "                    UV_LIBC=none \\\n",
    )
    duplicated_runtime_uv_libc = replace_once(
        FLAKE_SOURCE,
        "                    UV_LIBC=gnu \\\n",
        "                    UV_LIBC=gnu \\\n                    UV_LIBC=gnu \\\n",
    )
    inherited_runtime_uv_libc = replace_once(
        FLAKE_SOURCE,
        "                    UV_LIBC=gnu \\\n",
        "                    UV_LIBC=\"''${UV_LIBC}\" \\\n",
    )
    omitted_runtime_pkg_config_libdir = replace_once(
        FLAKE_SOURCE,
        '                    PKG_CONFIG_LIBDIR="${linuxReleasePortaudio}/lib/pkgconfig" \\\n',
        "",
    )
    omitted_runtime_bytecode_check = replace_once(
        FLAKE_SOURCE,
        "fixed release runtime contains Python bytecode cache artifacts",
        "ignored fixed release runtime bytecode cache artifacts",
    )
    redirected_runtime_script = replace_once(
        FLAKE_SOURCE,
        '"${source}/scripts/release/build_runtime.py"',
        '"/tmp/attacker-build-runtime.py"',
    )
    redirected_runtime_project = replace_once(
        FLAKE_SOURCE,
        '--project "${controlledCargoSource}"',
        '--project "/tmp/attacker-project"',
    )
    redirected_runtime_output = replace_once(
        FLAKE_SOURCE,
        '--runtime-output "$out/python"',
        '--runtime-output "$TMPDIR/attacker-runtime"',
    )
    redirected_runtime_fixed_output_hash = sources.copy()
    runtime_hash_flake = sources[FLAKE_SOURCE]
    runtime_hash_start = runtime_hash_flake.index("linuxReleaseRuntime =")
    runtime_hash_end = runtime_hash_flake.index(
        "pokeconPackage = rustPlatform.buildRustPackage", runtime_hash_start
    )
    runtime_hash_match = re.search(
        r"outputHash = (?P<value>[^;]+);",
        runtime_hash_flake[runtime_hash_start:runtime_hash_end],
    )
    assert runtime_hash_match is not None
    runtime_hash_value_start = runtime_hash_start + runtime_hash_match.start("value")
    runtime_hash_value_end = runtime_hash_start + runtime_hash_match.end("value")
    redirected_runtime_fixed_output_hash[FLAKE_SOURCE] = refresh_canonical_flake_hash(
        runtime_hash_flake[:runtime_hash_value_start]
        + 'builtins.hashString "sha256" "attacker-runtime"'
        + runtime_hash_flake[runtime_hash_value_end:]
    )
    mutable_release_runtime = replace_once(
        FLAKE_SOURCE,
        '                    release_python="${linuxReleaseRuntime}/python"\n',
        '                    release_python="$workdir/python"\n'
        '                    "${pythonEnv}/bin/python" -I "${source}/scripts/release/build_runtime.py"\n',
    )
    omitted_tauri_target_reset = replace_once(
        FLAKE_SOURCE,
        "                    ${resetTauriCargoTarget}\n",
        "",
    )
    poisoned_tauri_target_after_reset = replace_once(
        FLAKE_SOURCE,
        "                    ${resetTauriCargoTarget}\n",
        "                    ${resetTauriCargoTarget}\n"
        '                    export CARGO_TARGET_DIR="/tmp/attacker-target"\n',
    )
    redirected_tauri_remap_source = replace_once(
        FLAKE_SOURCE,
        '                    export POKECON_RUST_REMAP_SOURCE="${controlledCargoSource}"\n',
        '                    export POKECON_RUST_REMAP_SOURCE="$workdir"\n',
    )
    redirected_tauri_manifest_path = replace_once(
        FLAKE_SOURCE,
        '--manifest-path "$cargo_source_root/Cargo.toml"',
        '--manifest-path "$workdir/Cargo.toml"',
    )
    redirected_controlled_source_input = replace_once(
        FLAKE_SOURCE,
        '            "${pkgs.coreutils}/bin/cp" -a -- "${source}/." "$out/"\n',
        '            "${pkgs.coreutils}/bin/cp" -a -- /tmp/attacker-source/. "$out/"\n',
    )
    omitted_controlled_source_diff = replace_once(
        FLAKE_SOURCE,
        "              --brief \\\n",
        "",
    )
    omitted_controlled_source_immutability = replace_once(
        FLAKE_SOURCE,
        '            "${pkgs.coreutils}/bin/chmod" -R a-w -- "$out"\n',
        "",
    )
    redirected_controlled_source_workdir = replace_once(
        FLAKE_SOURCE,
        '              cd "$cargo_source_root"\n',
        '              cd "$workdir"\n',
    )
    redirected_immutable_cargo_home = replace_once(
        FLAKE_SOURCE,
        '            export CARGO_HOME="${gateCargoHome}"\n',
        '            export CARGO_HOME="/tmp/attacker-cargo-home"\n',
    )
    audit_without_isolated_python = replace_once(
        FLAKE_SOURCE,
        '"${pythonEnv}/bin/python" -I -m pytest',
        '"${pythonEnv}/bin/python" -m pytest',
    )
    redirected_audit_pytest_config = replace_once(
        FLAKE_SOURCE,
        '-c "${auditPytestConfig}"',
        '-c "/tmp/attacker-pytest.ini"',
    )
    audit_with_repository_conftest = replace_once(
        FLAKE_SOURCE,
        "                  --noconftest \\\n",
        "",
    )
    audit_with_prepend_import_mode = replace_once(
        FLAKE_SOURCE,
        "                  --import-mode=importlib \\\n",
        "                  --import-mode=prepend \\\n",
    )
    mutation_runner_accepts_ambient_shard_count = replace_once(
        FLAKE_SOURCE,
        "                POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_COUNT \\\n",
        "",
    )
    mutation_runner_pins_every_shard_to_zero = replace_once(
        FLAKE_SOURCE,
        '                POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX="$mutation_shard_index" \\\n',
        '                POKECON_PRODUCTION_ROUTING_MUTATION_SHARD_INDEX="0" \\\n',
    )
    serial_mutation_runner = replace_once(
        FLAKE_SOURCE,
        '                  >"$mutation_log_directory/$mutation_shard_index.log" 2>&1 &\n',
        '                  >"$mutation_log_directory/$mutation_shard_index.log" 2>&1\n',
    )
    mutation_runner_skips_worker_wait = replace_once(
        FLAKE_SOURCE,
        '                if wait "$mutation_worker_pid"; then\n',
        "                if true; then\n",
    )
    default_test_includes_mutation_audit = replace_once(
        FLAKE_SOURCE,
        '                  -m "not production_routing_mutation"\n',
        "",
    )
    default_test_ignores_forwarded_arguments = replace_once(
        FLAKE_SOURCE,
        "                  \"''${pytest_arguments[@]}\" \\\n",
        "                  tests \\\n",
    )
    unhardened_mutation_audit_app = replace_once(
        FLAKE_SOURCE,
        "            test-production-routing-mutations = mkTask {\n"
        '              name = "test-production-routing-mutations";\n'
        "              text = ''\n"
        '                exec "${productionRoutingMutationAuditRunner}/bin/'
        'pokecon-production-routing-mutation-audit" "$@"\n'
        "              '';\n"
        "            };\n",
        "            test-production-routing-mutations =\n"
        '              mkApp "${productionRoutingMutationAuditRunner}/bin/'
        'pokecon-production-routing-mutation-audit";\n',
    )
    aggregate_omits_mutation_audit = replace_once(
        FLAKE_SOURCE,
        "                  --next \\\n"
        "                  production-routing-mutation-audit \\\n"
        '                  "${productionRoutingMutationAuditRunner}/bin/'
        'pokecon-production-routing-mutation-audit" \\\n'
        '                  --workers "$aggregate_mutation_workers"\n',
        "",
    )
    dedicated_check_omits_mutation_audit = replace_once(
        FLAKE_SOURCE,
        "              ''\n"
        '                "${productionRoutingMutationAuditRunner}/bin/'
        'pokecon-production-routing-mutation-audit"\n'
        '                mkdir -p "$out"\n',
        "              ''\n                mkdir -p \"$out\"\n",
    )
    omitted_directory_fd_lock_probe = replace_once(
        FLAKE_SOURCE,
        '                if flock -n -x "$artifact_directory_lock_contender_fd"; then\n'
        '                  echo "directory FD lock probe admitted a concurrent contender" >&2\n'
        "                  exit 2\n"
        "                fi\n",
        "",
    )
    redirected_audit_test_path = replace_once(
        FLAKE_SOURCE,
        '              relativeAuditTest = "/tests/quality/test_ui_package_check.py";\n',
        '              relativeAuditTest = "/tmp/attacker-audit.py";\n',
    )
    raw_tauri_bundle_arguments = replace_once(
        FLAKE_SOURCE,
        "bundle_args=(--bundles deb)",
        'bundle_args=("$@")',
    )
    widened_tauri_bundle_format = replace_once(
        FLAKE_SOURCE,
        '[ "$2" = deb ]',
        '[ "$2" = deb ] || [ "$2" = appimage ]',
    )
    staged_portable_uv_execution_copy = replace_once(
        FLAKE_SOURCE,
        '--uv "${portableUvBinary}"',
        '--uv "${portableUvExecutionBinary}"',
    )
    embedded_portable_uv_execution_copy = replace_once(
        FLAKE_SOURCE,
        'POKECON_BUILD_UV_PATH="${portableUvBinary}"',
        'POKECON_BUILD_UV_PATH="${portableUvExecutionBinary}"',
    )
    omitted_tauri_artifact_lock = replace_once(
        FLAKE_SOURCE,
        '                    "${pkgs.util-linux}/bin/flock" -x "$artifact_parent_fd"\n',
        "",
    )
    persistent_tauri_artifact_lock_file = replace_once(
        FLAKE_SOURCE,
        '                    exec {artifact_parent_fd}< "$artifact_parent"\n',
        '                    exec {artifact_parent_fd}>> "$artifact_parent/.tauri-build.lock"\n',
    )
    omitted_tauri_post_lock_parent_identity = replace_once(
        FLAKE_SOURCE,
        "                    if ! artifact_parent_identity_is_current; then\n"
        '                      echo "tauri-build artifact parent changed while waiting for its directory lock" >&2\n'
        "                      exit 2\n"
        "                    fi\n",
        "",
    )
    omitted_tauri_fail_closed_preflight = replace_once(
        FLAKE_SOURCE,
        "                    trap fail_closed_tauri_artifact_preflight EXIT\n",
        "",
    )
    omitted_setup_workdir_artifact_cleanup = replace_once(
        FLAKE_SOURCE,
        "              if declare -F pokecon_cleanup_task_artifacts >/dev/null; then\n"
        "                pokecon_cleanup_task_artifacts || gate_cleanup_status=$?\n"
        "              fi\n",
        "",
    )
    copied_stale_tauri_artifact_without_hiding = replace_once(
        FLAKE_SOURCE,
        '                      "${pkgs.coreutils}/bin/mv" -T -- \\\n'
        '                        "$artifact_dir" "$artifact_backup_dir"\n',
        '                      "${pkgs.coreutils}/bin/cp" -a -- \\\n'
        '                        "$artifact_dir" "$artifact_backup_dir"\n',
    )
    restored_stale_tauri_artifact_on_failure = replace_once(
        FLAKE_SOURCE,
        "                    pokecon_cleanup_task_artifacts() {\n"
        "                      local artifact_cleanup_status\n"
        "                      artifact_cleanup_status=0\n",
        "                    pokecon_cleanup_task_artifacts() {\n"
        "                      local artifact_cleanup_status\n"
        "                      artifact_cleanup_status=0\n"
        '                      if [ -d "$artifact_backup_dir" ] && [ ! -e "$artifact_dir" ]; then\n'
        '                        "${pkgs.coreutils}/bin/mv" -T -- \\\n'
        '                          "$artifact_backup_dir" "$artifact_dir"\n'
        "                      fi\n",
    )
    unanchored_tauri_artifact_cleanup = replace_once(
        FLAKE_SOURCE,
        '                      if ! "${pkgs.coreutils}/bin/rm" -rf -- "$artifact_cleanup_target"; then\n',
        '                      if ! "${pkgs.coreutils}/bin/rm" -rf -- "$artifact_parent/tauri"; then\n',
    )
    public_path_dependent_tauri_artifact_cleanup = replace_once(
        FLAKE_SOURCE,
        "                      if ! artifact_parent_anchor_identity_is_owned; then\n"
        '                        echo "refusing $artifact_cleanup_label cleanup after artifact parent descriptor identity changed" >&2\n',
        "                      if ! artifact_parent_identity_is_current; then\n"
        '                        echo "refusing $artifact_cleanup_label cleanup after artifact parent identity changed" >&2\n',
    )
    omitted_tauri_cleanup_inode_guard = replace_once(
        FLAKE_SOURCE,
        '                      if [ "$artifact_cleanup_actual_identity" != "$artifact_cleanup_expected_identity" ]; then\n'
        '                        echo "refusing $artifact_cleanup_label cleanup after its identity changed" >&2\n'
        "                        return 1\n"
        "                      fi\n",
        "",
    )
    random_tauri_publication_staging = replace_once(
        FLAKE_SOURCE,
        '                    artifact_publish_dir="$artifact_parent_anchor/.tauri-publish"\n',
        '                    artifact_publish_dir="$(mktemp -d --tmpdir="$artifact_parent_anchor" .tauri-publish.XXXXXXXX)"\n',
    )
    ignored_tauri_bundle_inventory_failure = replace_once(
        FLAKE_SOURCE,
        "                      -name '*.deb' -print0 > \"$bundle_deb_inventory\"; then\n",
        "                      -name '*.deb' -print0 > \"$bundle_deb_inventory\" && false; then\n",
    )
    ignored_tauri_publish_inventory_failure = replace_once(
        FLAKE_SOURCE,
        '                      -mindepth 1 -print0 > "$artifact_publish_inventory"; then\n',
        '                      -mindepth 1 -print0 > "$artifact_publish_inventory" && false; then\n',
    )
    ignored_tauri_published_inventory_failure = replace_once(
        FLAKE_SOURCE,
        '                      -mindepth 1 -print0 > "$artifact_published_inventory"; then\n',
        '                      -mindepth 1 -print0 > "$artifact_published_inventory" && false; then\n',
    )
    accepted_legacy_tauri_lock_residue = replace_once(
        FLAKE_SOURCE,
        '                    if [ -e "$artifact_legacy_lock" ] || [ -L "$artifact_legacy_lock" ]; then\n'
        '                      echo "tauri-build refuses legacy persistent lock residue: $artifact_parent/.tauri-build.lock" >&2\n'
        "                      exit 2\n"
        "                    fi\n",
        "",
    )
    accepted_multiple_tauri_debs = replace_once(
        FLAKE_SOURCE,
        "                    if [ \"''${#bundle_deb_entries[@]}\" -ne 1 ]; then\n",
        "                    if [ \"''${#bundle_deb_entries[@]}\" -lt 1 ]; then\n",
    )
    accepted_symlinked_tauri_deb = replace_once(
        FLAKE_SOURCE,
        '                    if [ -L "$package" ] || [ ! -f "$package" ]; then\n',
        '                    if [ ! -f "$package" ]; then\n',
    )
    weakened_tauri_publish_inventory = replace_once(
        FLAKE_SOURCE,
        "                    if [ \"''${#artifact_publish_entries[@]}\" -ne 1 ] \\\n",
        "                    if [ \"''${#artifact_publish_entries[@]}\" -lt 1 ] \\\n",
    )
    redirected_tauri_atomic_publication = replace_once(
        FLAKE_SOURCE,
        '                      "$artifact_publish_dir" "$artifact_dir"\n',
        '                      "$artifact_publish_dir" "$artifact_backup_dir"\n',
    )
    delayed_tauri_replacement_flag = replace_once(
        FLAKE_SOURCE,
        "                    artifact_committed_identity=$artifact_publish_identity\n"
        "                    artifact_replaced=1\n"
        '                    "${pkgs.coreutils}/bin/mv" -T -- \\\n'
        '                      "$artifact_publish_dir" "$artifact_dir"\n',
        '                    "${pkgs.coreutils}/bin/mv" -T -- \\\n'
        '                      "$artifact_publish_dir" "$artifact_dir"\n'
        "                    artifact_committed_identity=$artifact_publish_identity\n"
        "                    artifact_replaced=1\n",
    )
    weakened_tauri_published_inventory = replace_once(
        FLAKE_SOURCE,
        "                      || [ \"''${#artifact_published_entries[@]}\" -ne 1 ] \\\n",
        "                      || [ \"''${#artifact_published_entries[@]}\" -lt 1 ] \\\n",
    )
    omitted_tauri_success_residue_check = replace_once(
        FLAKE_SOURCE,
        "                    if ! artifact_parent_identity_is_current \\\n"
        '                      || [ -e "$artifact_backup_dir" ] || [ -L "$artifact_backup_dir" ] \\\n'
        '                      || [ -e "$artifact_publish_dir" ] || [ -L "$artifact_publish_dir" ] \\\n'
        '                      || [ -e "$artifact_legacy_lock" ] || [ -L "$artifact_legacy_lock" ]; then\n'
        '                      echo "tauri-build success left transient artifact state in dist" >&2\n'
        "                      exit 2\n"
        "                    fi\n",
        "",
    )
    omitted_tauri_exit_parent_identity_check = replace_once(
        FLAKE_SOURCE,
        '                      if [ "$gate_status" -eq 0 ] \\\n'
        "                        && ! artifact_parent_identity_is_current; then\n"
        '                        echo "tauri-build artifact parent changed before successful cleanup finalization" >&2\n'
        "                        gate_status=1\n"
        "                      fi\n",
        "",
    )
    cleared_tauri_ownership_before_exit_cleanup = replace_once(
        FLAKE_SOURCE,
        '                      echo "tauri-build success left transient artifact state in dist" >&2\n'
        "                      exit 2\n"
        "                    fi\n"
        "                  '';\n",
        '                      echo "tauri-build success left transient artifact state in dist" >&2\n'
        "                      exit 2\n"
        "                    fi\n"
        "                    artifact_replaced=0\n"
        "                    artifact_committed_identity=\n"
        "                  '';\n",
    )
    omitted_tauri_frontend_dist = replace_once(
        FLAKE_SOURCE,
        '                        "build": {"frontendDist": str(frontend_root)},\n',
        "",
    )
    redirected_tauri_web_stage = replace_once(
        FLAKE_SOURCE,
        '--web "${webPackage}"',
        '--web "$workdir/web/dist"',
    )
    omitted_tauri_platform_guard = replace_once(
        FLAKE_SOURCE,
        '              if system != "x86_64-linux" then\n',
        "              if false then\n",
    )
    omitted_tauri_build_staging_context = replace_once(
        "@pokecon/build.rs",
        "    env::set_current_dir(&context_manifest_directory)?;\n",
        "",
    )
    omitted_tauri_context_owner_write = replace_once(
        "@pokecon/build.rs",
        "    make_copied_file_owner_writable(destination, metadata.permissions())?;\n",
        "",
    )
    discarded_tauri_context_source_mode = replace_once(
        "@pokecon/build.rs",
        "        permissions.set_mode(permissions.mode() | 0o200);\n",
        "        permissions.set_mode(0o600);\n",
    )
    omitted_resource_provenance_rerun = replace_once(
        "@pokecon/build.rs",
        '    println!("cargo:rerun-if-env-changed={RESOURCE_PROVENANCE_ENVIRONMENT}");\n',
        "",
    )
    omitted_resource_provenance_rustc_env = replace_once(
        "@pokecon/build.rs",
        '    println!("cargo:rustc-env={RESOURCE_PROVENANCE_ENVIRONMENT}={provenance}");\n',
        "",
    )
    relaxed_resource_provenance_digest = replace_once(
        "@pokecon/build.rs",
        "byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)",
        "byte.is_ascii_hexdigit()",
    )
    weakened_resource_provenance_diagonal = replace_once(
        "entrypoint.rs",
        "(ResourceProvenance::Development, ResourceOrigin::Cargo)",
        "(ResourceProvenance::Development, _)",
    )
    widened_macos_packager_owned_icon = replace_once(
        "entrypoint.rs",
        '            Self::Macos => relative == "icon.icns",\n',
        '            Self::Macos => relative.ends_with(".icns"),\n',
    )
    leaked_macos_packager_ownership_to_linux = replace_once(
        "entrypoint.rs",
        '            Self::Macos => relative == "icon.icns",\n',
        '            Self::Linux | Self::Macos => relative == "icon.icns",\n',
    )
    omitted_source_resource_identity_binding = replace_once(
        "entrypoint.rs",
        "    require_packaged_resource_identity(\n"
        "        &expected.content_sha256,\n"
        "        required_content_sha256,\n"
        "        &manifest_path,\n"
        "    )?;\n",
        "",
    )
    omitted_snapshot_resource_identity_binding = replace_once(
        "entrypoint.rs",
        "    require_packaged_resource_identity(\n"
        "        &snapshot_expected.content_sha256,\n"
        "        required_content_sha256,\n"
        "        &manifest_path,\n"
        "    )?;\n",
        "",
    )
    mutations: tuple[ProductionRoutingMutation, ...] = (
        *changed_production_target_roots,
        ("contracts runtime dependency", contracts_runtime_dependency),
        ("runtime server dependency", runtime_server_dependency),
        (
            "settings runtime reverse dependency",
            settings_runtime_reverse_dependency,
        ),
        ("server hardware owner", server_hardware_owner),
        ("worker main state owner", worker_main_state_owner),
        ("symlinked production Cargo target root", symlinked_worker_target_root),
        ("custom nested Tauri permission", custom_tauri_permission),
        ("custom Tauri capability", custom_tauri_capability),
        ("custom Tauri JSON5 capability", custom_tauri_json5_capability),
        (
            "nested active autogenerated Tauri permission",
            nested_autogenerated_permission,
        ),
        ("alternate Tauri Linux JSON5 config", alternate_tauri_json5_config),
        ("alternate Tauri TOML config", alternate_tauri_toml_config),
        ("changed flake lock", changed_flake_lock),
        ("symlinked flake lock", symlinked_flake_lock),
        ("changed Git ignore rules", changed_gitignore),
        ("omitted Tauri XCB dependency", omitted_tauri_xcb_dependency),
        *changed_tauri_assets,
        ("removed JSON5 source filtering", removed_json5_source_filter),
        ("weakened direct input assertion", weakened_direct_input_assertion),
        ("redirected flake-parts input", redirected_flake_parts_input),
        ("omitted canonical flake guard", omitted_canonical_flake_guard),
        ("mismatched canonical flake hash", mismatched_canonical_flake_hash),
        ("omitted Web version input", omitted_web_version),
        (
            "redirected portable uv execution loader",
            redirected_portable_uv_execution_loader,
        ),
        ("changed portable uv version output", changed_portable_uv_version_output),
        (
            "weakened portable uv version comparison",
            weakened_portable_uv_version_comparison,
        ),
        (
            "unchecked portable uv version probe",
            unchecked_portable_uv_version_probe,
        ),
        (
            "omitted portable uv actual version diagnostic",
            omitted_portable_uv_actual_version_diagnostic,
        ),
        (
            "omitted portable uv probe actual diagnostic",
            omitted_portable_uv_probe_actual_diagnostic,
        ),
        (
            "weakened portable uv DT_NEEDED inventory",
            weakened_portable_uv_needed_inventory,
        ),
        (
            "omitted portable uv raw RPATH check",
            omitted_portable_uv_raw_rpath_check,
        ),
        (
            "unpatched portable uv execution interpreter",
            unpatched_portable_uv_execution_interpreter,
        ),
        ("raw runtime uv execution", raw_runtime_uv_execution),
        (
            "omitted Linux release GCC runtime library",
            omitted_linux_release_gcc_runtime_library,
        ),
        (
            "host Linux release GCC runtime library",
            host_linux_release_gcc_runtime_library,
        ),
        (
            "host Linux release PortAudio runtime library",
            host_linux_release_portaudio_runtime_library,
        ),
        (
            "omitted Linux release zlib runtime library",
            omitted_linux_release_zlib_runtime_library,
        ),
        (
            "host Linux release zlib runtime library",
            host_linux_release_zlib_runtime_library,
        ),
        (
            "omitted Linux release XCB runtime library",
            omitted_linux_release_xcb_runtime_library,
        ),
        (
            "host Linux release XCB runtime library",
            host_linux_release_xcb_runtime_library,
        ),
        (
            "omitted Linux release libglvnd runtime library",
            omitted_linux_release_libglvnd_runtime_library,
        ),
        (
            "host Linux release libglvnd runtime library",
            host_linux_release_libglvnd_runtime_library,
        ),
        (
            "omitted Linux release GLib runtime library",
            omitted_linux_release_glib_runtime_library,
        ),
        (
            "host Linux release GLib runtime library",
            host_linux_release_glib_runtime_library,
        ),
        (
            "omitted Linux release libSM runtime library",
            omitted_linux_release_libsm_runtime_library,
        ),
        (
            "host Linux release libSM runtime library",
            host_linux_release_libsm_runtime_library,
        ),
        (
            "omitted Linux release libXext runtime library",
            omitted_linux_release_libxext_runtime_library,
        ),
        (
            "host Linux release libXext runtime library",
            host_linux_release_libxext_runtime_library,
        ),
        (
            "omitted Linux release libXrender runtime library",
            omitted_linux_release_libxrender_runtime_library,
        ),
        (
            "host Linux release libXrender runtime library",
            host_linux_release_libxrender_runtime_library,
        ),
        (
            "extra Linux release runtime library",
            extra_linux_release_runtime_library,
        ),
        (
            "raw PortAudio FOD runtime library path",
            raw_portaudio_fod_runtime_library_path,
        ),
        (
            "raw PortAudio package-smoke runtime library path",
            raw_portaudio_package_smoke_runtime_library_path,
        ),
        ("split FOD runtime library path", split_fod_runtime_library_path),
        (
            "inherited FOD runtime LD_LIBRARY_PATH",
            inherited_fod_runtime_ld_library_path,
        ),
        (
            "inherited package-smoke LD_LIBRARY_PATH",
            inherited_package_smoke_ld_library_path,
        ),
        (
            "baked runtime library path into wheel",
            baked_runtime_library_path_into_wheel,
        ),
        (
            "omitted runtime Python execution loader",
            omitted_runtime_python_execution_loader,
        ),
        (
            "omitted runtime Python execution library path",
            omitted_runtime_python_execution_library_path,
        ),
        (
            "unqualified portable Python install request",
            unqualified_portable_python_install_request,
        ),
        (
            "unused portable Python install selector result",
            unused_portable_python_install_selector_result,
        ),
        (
            "unused Linux portable Python install request",
            unused_linux_portable_python_install_request,
        ),
        (
            "unused Windows portable Python install request",
            unused_windows_portable_python_install_request,
        ),
        (
            "wrong portable Python install implementation",
            wrong_portable_python_install_implementation,
        ),
        (
            "wrong portable Python install version",
            wrong_portable_python_install_version,
        ),
        (
            "wrong portable Python install OS",
            wrong_portable_python_install_os,
        ),
        (
            "wrong portable Python install architecture",
            wrong_portable_python_install_arch,
        ),
        (
            "wrong portable Python install libc",
            wrong_portable_python_install_libc,
        ),
        (
            "wrong Windows portable Python install implementation",
            wrong_windows_portable_python_install_implementation,
        ),
        (
            "wrong Windows portable Python install version",
            wrong_windows_portable_python_install_version,
        ),
        (
            "wrong Windows portable Python install OS",
            wrong_windows_portable_python_install_os,
        ),
        (
            "wrong Windows portable Python install architecture",
            wrong_windows_portable_python_install_arch,
        ),
        (
            "wrong Windows portable Python install libc",
            wrong_windows_portable_python_install_libc,
        ),
        (
            "widened Linux portable Python platform",
            widened_linux_portable_python_platform,
        ),
        (
            "redirected Linux portable Python install request",
            redirected_linux_portable_python_install_request,
        ),
        (
            "redirected Windows portable Python install request",
            redirected_windows_portable_python_install_request,
        ),
        ("wrong Linux Python minor redirect", wrong_linux_python_minor_redirect),
        ("wrong Windows Python minor redirect", wrong_windows_python_minor_redirect),
        (
            "redirected Linux Python minor redirect",
            redirected_linux_python_minor_redirect,
        ),
        (
            "redirected Windows Python minor redirect",
            redirected_windows_python_minor_redirect,
        ),
        (
            "accepted unknown portable Python platform",
            accepted_unknown_portable_python_platform,
        ),
        (
            "accepted unknown Python minor redirect platform",
            accepted_unknown_python_minor_redirect_platform,
        ),
        (
            "changed portable Python runtime version",
            changed_portable_python_runtime_version,
        ),
        (
            "qualified request used for Python version validation",
            qualified_request_used_for_python_version_validation,
        ),
        (
            "accepted missing managed Python inventory entry",
            accepted_missing_managed_python_inventory_entry,
        ),
        (
            "accepted extra managed Python inventory entry",
            accepted_extra_managed_python_inventory_entry,
        ),
        (
            "accepted redirected managed Python gitignore",
            accepted_redirected_managed_python_gitignore,
        ),
        (
            "accepted redirected managed Python lock",
            accepted_redirected_managed_python_lock,
        ),
        (
            "changed managed Python gitignore content",
            changed_managed_python_gitignore_content,
        ),
        (
            "changed managed Python lock content",
            changed_managed_python_lock_content,
        ),
        (
            "accepted managed Python temp residue",
            accepted_managed_python_temp_residue,
        ),
        (
            "accepted redirected managed Python temp",
            accepted_redirected_managed_python_temp,
        ),
        (
            "accepted redirected managed Python root",
            accepted_redirected_managed_python_root,
        ),
        (
            "accepted redirected managed Python full install",
            accepted_redirected_managed_python_full_install,
        ),
        (
            "accepted missing managed Python minor redirect",
            accepted_missing_managed_python_minor_redirect,
        ),
        (
            "accepted wrong managed Python minor target",
            accepted_wrong_managed_python_minor_target,
        ),
        (
            "accepted Linux managed Python minor junction",
            accepted_linux_managed_python_minor_junction,
        ),
        (
            "accepted Windows managed Python minor symlink",
            accepted_windows_managed_python_minor_symlink,
        ),
        (
            "first managed Python directory fallback",
            first_managed_python_directory_fallback,
        ),
        (
            "unnormalized managed Python inventory stat error",
            unnormalized_managed_python_inventory_stat_error,
        ),
        ("patched raw portable Python runtime", patched_raw_portable_python_runtime),
        ("executed raw portable Python", executed_raw_portable_python),
        (
            "omitted final raw Python digest check",
            omitted_final_raw_python_digest_check,
        ),
        (
            "omitted final raw Python ELF check",
            omitted_final_raw_python_elf_check,
        ),
        ("poisoned runtime PYTHONPATH", poisoned_runtime_pythonpath),
        ("omitted runtime safe-path mode", omitted_runtime_safepath),
        ("poisoned runtime pip config", poisoned_runtime_pip_config),
        ("omitted runtime uv no-config", omitted_runtime_uv_no_config),
        ("omitted runtime uv libc", omitted_runtime_uv_libc),
        ("weakened runtime uv libc", weakened_runtime_uv_libc),
        ("musl runtime uv libc", musl_runtime_uv_libc),
        ("none runtime uv libc", none_runtime_uv_libc),
        ("duplicated runtime uv libc", duplicated_runtime_uv_libc),
        ("inherited runtime uv libc", inherited_runtime_uv_libc),
        ("omitted runtime pkg-config libdir", omitted_runtime_pkg_config_libdir),
        ("omitted runtime bytecode check", omitted_runtime_bytecode_check),
        ("redirected runtime builder", redirected_runtime_script),
        ("redirected runtime project", redirected_runtime_project),
        ("redirected runtime output", redirected_runtime_output),
        (
            "redirected runtime fixed-output hash",
            redirected_runtime_fixed_output_hash,
        ),
        ("mutable release runtime", mutable_release_runtime),
        ("omitted Tauri target reset", omitted_tauri_target_reset),
        ("poisoned Tauri target after reset", poisoned_tauri_target_after_reset),
        ("redirected Tauri remap source", redirected_tauri_remap_source),
        ("redirected Tauri manifest path", redirected_tauri_manifest_path),
        ("redirected controlled source input", redirected_controlled_source_input),
        ("omitted controlled source diff", omitted_controlled_source_diff),
        (
            "omitted controlled source immutability",
            omitted_controlled_source_immutability,
        ),
        ("redirected controlled source workdir", redirected_controlled_source_workdir),
        ("redirected immutable Cargo home", redirected_immutable_cargo_home),
        ("audit without isolated Python", audit_without_isolated_python),
        ("redirected audit pytest config", redirected_audit_pytest_config),
        ("audit with repository conftest", audit_with_repository_conftest),
        ("audit with prepend import mode", audit_with_prepend_import_mode),
        (
            "mutation runner accepts ambient shard count",
            mutation_runner_accepts_ambient_shard_count,
        ),
        (
            "mutation runner pins every shard to zero",
            mutation_runner_pins_every_shard_to_zero,
        ),
        ("serial mutation runner", serial_mutation_runner),
        ("mutation runner skips worker wait", mutation_runner_skips_worker_wait),
        (
            "default test includes mutation audit",
            default_test_includes_mutation_audit,
        ),
        (
            "default test ignores forwarded arguments",
            default_test_ignores_forwarded_arguments,
        ),
        ("unhardened mutation audit app", unhardened_mutation_audit_app),
        ("aggregate omits mutation audit", aggregate_omits_mutation_audit),
        ("dedicated check omits mutation audit", dedicated_check_omits_mutation_audit),
        ("omitted directory FD lock probe", omitted_directory_fd_lock_probe),
        ("redirected audit test path", redirected_audit_test_path),
        ("raw Tauri bundle arguments", raw_tauri_bundle_arguments),
        ("widened Tauri bundle format", widened_tauri_bundle_format),
        ("staged portable uv execution copy", staged_portable_uv_execution_copy),
        (
            "embedded portable uv execution copy",
            embedded_portable_uv_execution_copy,
        ),
        ("omitted Tauri artifact lock", omitted_tauri_artifact_lock),
        (
            "persistent Tauri artifact lock file",
            persistent_tauri_artifact_lock_file,
        ),
        (
            "omitted Tauri post-lock parent identity",
            omitted_tauri_post_lock_parent_identity,
        ),
        (
            "omitted Tauri fail-closed preflight trap",
            omitted_tauri_fail_closed_preflight,
        ),
        (
            "omitted setup-workdir artifact cleanup composition",
            omitted_setup_workdir_artifact_cleanup,
        ),
        (
            "copied stale Tauri artifact without hiding",
            copied_stale_tauri_artifact_without_hiding,
        ),
        (
            "restored stale Tauri artifact on failure",
            restored_stale_tauri_artifact_on_failure,
        ),
        ("unanchored Tauri artifact cleanup", unanchored_tauri_artifact_cleanup),
        (
            "public-path-dependent Tauri artifact cleanup",
            public_path_dependent_tauri_artifact_cleanup,
        ),
        ("omitted Tauri cleanup inode guard", omitted_tauri_cleanup_inode_guard),
        (
            "random Tauri publication staging",
            random_tauri_publication_staging,
        ),
        (
            "ignored Tauri bundle inventory failure",
            ignored_tauri_bundle_inventory_failure,
        ),
        (
            "ignored Tauri publish inventory failure",
            ignored_tauri_publish_inventory_failure,
        ),
        (
            "ignored Tauri published inventory failure",
            ignored_tauri_published_inventory_failure,
        ),
        ("accepted legacy Tauri lock residue", accepted_legacy_tauri_lock_residue),
        ("accepted multiple Tauri debs", accepted_multiple_tauri_debs),
        ("accepted symlinked Tauri deb", accepted_symlinked_tauri_deb),
        (
            "weakened Tauri publication inventory",
            weakened_tauri_publish_inventory,
        ),
        (
            "redirected Tauri atomic publication",
            redirected_tauri_atomic_publication,
        ),
        ("delayed Tauri replacement flag", delayed_tauri_replacement_flag),
        (
            "weakened Tauri published inventory",
            weakened_tauri_published_inventory,
        ),
        (
            "omitted Tauri success residue check",
            omitted_tauri_success_residue_check,
        ),
        (
            "omitted Tauri EXIT parent identity check",
            omitted_tauri_exit_parent_identity_check,
        ),
        (
            "cleared Tauri ownership before EXIT cleanup",
            cleared_tauri_ownership_before_exit_cleanup,
        ),
        ("omitted Tauri frontend distribution", omitted_tauri_frontend_dist),
        ("redirected Tauri Web staging", redirected_tauri_web_stage),
        ("omitted Tauri platform guard", omitted_tauri_platform_guard),
        (
            "omitted Tauri build-script staging context",
            omitted_tauri_build_staging_context,
        ),
        (
            "omitted Tauri context owner-write permission",
            omitted_tauri_context_owner_write,
        ),
        (
            "discarded Tauri context source mode",
            discarded_tauri_context_source_mode,
        ),
        ("omitted resource provenance rerun", omitted_resource_provenance_rerun),
        (
            "omitted resource provenance rustc env",
            omitted_resource_provenance_rustc_env,
        ),
        ("relaxed resource provenance digest", relaxed_resource_provenance_digest),
        (
            "weakened resource provenance diagonal",
            weakened_resource_provenance_diagonal,
        ),
        (
            "widened macOS packager-owned icon",
            widened_macos_packager_owned_icon,
        ),
        (
            "leaked macOS packager ownership to Linux",
            leaked_macos_packager_ownership_to_linux,
        ),
        (
            "omitted source resource identity binding",
            omitted_source_resource_identity_binding,
        ),
        (
            "omitted snapshot resource identity binding",
            omitted_snapshot_resource_identity_binding,
        ),
        ("symlinked allowed build.rs", symlinked_allowed_build_script),
        ("changed allowed build.rs", changed_allowed_build_script),
        ("target-specific build dependency", target_specific_build_dependency),
        ("added registry dependency", added_registry_dependency),
        ("allowed application unsafe code", allowed_application_unsafe_code),
        ("changed Cargo lock", changed_cargo_lock),
        ("escaped Cargo path dependency", escaped_path_dependency),
        ("aliased pokecon-server path dependency", aliased_pokecon_server_dependency),
        ("aliased workspace axum dependency", aliased_workspace_axum_dependency),
        ("Cargo config workspace wrapper and rustflags", cargo_config_wrapper),
        ("legacy Cargo config wrapper", legacy_cargo_config),
        ("nested Cargo config runner", nested_cargo_config),
        ("generated Cargo config wrapper", generated_cargo_config_wrapper),
        ("generated Cargo config rustflags", generated_cargo_config_rustflags),
        (
            "generated Cargo config target tools",
            generated_cargo_config_target_tools,
        ),
        ("generated Cargo config alias", generated_cargo_config_alias),
        ("redirected controlled pokecon lib overlay", redirected_pokecon_lib_overlay),
        (
            "redirected controlled pokecon main overlay",
            redirected_pokecon_main_overlay,
        ),
        (
            "redirected controlled pokecon build overlay",
            redirected_pokecon_build_overlay,
        ),
        ("redirected controlled dependency", redirected_controlled_dependency),
        ("accepted symlinked source", accepted_symlinked_source),
        ("changed Rust toolchain", changed_rust_toolchain),
        ("symlinked Rust toolchain", symlinked_rust_toolchain),
        ("redirected Rust toolchain binding", redirected_rust_toolchain_binding),
        ("reassigned reproducible rustc", reassigned_reproducible_rustc),
        ("reassigned pinned rustc", reassigned_pinned_rustc),
        (
            "package Cargo compiler alias after sanitizer",
            package_cargo_compiler_alias_after_sanitizer,
        ),
        ("redirected package source", redirected_package_source),
        ("custom package build phase", custom_package_build_phase),
        (
            "recursive package release-tree install",
            recursive_package_release_tree_install,
        ),
        ("omitted package pre-install hook", omitted_package_preinstall_hook),
        ("extra packaged binary", extra_packaged_binary),
        (
            "omitted packaged executable guard",
            omitted_packaged_executable_guard,
        ),
        ("omitted package post-install hook", omitted_package_postinstall_hook),
        ("unlocked package build", unlocked_package_build),
        (
            "tauri Cargo compiler alias after sanitizer",
            tauri_cargo_compiler_alias_after_sanitizer,
        ),
        (
            "unguarded Tauri Cargo before boundary",
            unguarded_tauri_cargo_before_boundary,
        ),
        (
            "omitted controlled workspace manifest",
            omitted_controlled_workspace_manifest,
        ),
        ("omitted controlled Cargo lock", omitted_controlled_cargo_lock),
        ("omitted controlled member manifest", omitted_controlled_member_manifest),
        ("bypassed symlink member check", bypassed_symlink_member_check),
        (
            "omitted Tauri ancestor Cargo config guard",
            omitted_tauri_ancestor_config_guard,
        ),
        (
            "omitted direct Cargo ancestor config guard",
            omitted_direct_cargo_ancestor_guard,
        ),
        (
            "omitted copied-worktree ancestor Cargo config guard",
            omitted_setup_workdir_ancestor_config_guard,
        ),
        (
            "changed Cargo cache directory tag",
            changed_cargo_cache_directory_tag,
        ),
        (
            "omitted Cargo cache directory tag comparison",
            omitted_cargo_cache_directory_tag_comparison,
        ),
        (
            "ignored repository Cargo config scan failure",
            ignored_repository_config_scan_failure,
        ),
        (
            "legacy Cargo-home config not cleared",
            legacy_cargo_home_config_not_cleared,
        ),
        ("reassigned Tauri Cargo home", reassigned_tauri_cargo_home),
        ("reassigned Tauri Cargo executable", reassigned_tauri_cargo_executable),
        ("redirected Tauri Cargo PATH", redirected_tauri_cargo_path),
        ("omitted final Tauri boundary", omitted_final_tauri_boundary),
        ("Cargo invocation-root config", cargo_invocation_root_config),
        ("ignored production audit failure", ignored_production_audit_failure),
        ("constant route path", constant_path),
        ("route_service", route_service),
        ("alternate Router constructor", alternate_constructor),
        ("post rebound to any", post_rebound_to_any),
        ("MethodFilter widened to CONNECT", widened_method_filter_connect),
        ("rebound on import", rebound_on_import),
        ("rebound Router import", rebound_router_import),
        ("rebound any import", rebound_any_import),
        ("local axum facade", local_axum_facade),
        ("foreign production merge", foreign_merge),
        ("substituted production router getter", substituted_production_getter),
        ("foreign public composition", public_composition),
        ("foreign production UI composition", production_ui_composition),
        ("locally shadowed UI router", locally_shadowed_ui_router),
        ("locally shadowed secure_router", locally_shadowed_secure_router),
        ("extra WebSocket path", websocket_extra_path),
        ("nested item-producing macro", nested_transport_macro),
        ("included transport item", included_transport_item),
        ("aliased top-level item macro", aliased_item_macro),
        ("unknown REST source", unknown_rest_source),
        ("route outside the allowlist", foreign_route_location),
        ("Router::route UFCS", ufcs_registration),
        ("angle-bracket Router UFCS", angle_ufcs_registration),
        ("generic angle-bracket Router UFCS", generic_angle_ufcs_registration),
        ("route-producing macro", macro_registration),
        ("nested router", nested_registration),
        ("nested service", nested_service_registration),
        ("arbitrary hidden macro router return", arbitrary_macro_return),
        ("unused known router chain with alternate return", unused_known_chain),
        (
            "conditional hidden REST catch-all success",
            conditional_hidden_catchall_success,
        ),
        (
            "conditional hidden WebSocket catch-all success",
            conditional_hidden_websocket_success,
        ),
        (
            "conditional RestError constructor success",
            conditional_rest_error_constructor_success,
        ),
        (
            "rewritten RestError constructor status",
            rewritten_rest_error_constructor_status,
        ),
        (
            "conditional RestError IntoResponse success",
            conditional_rest_error_response_success,
        ),
        (
            "rewritten RestError IntoResponse status",
            rewritten_rest_error_response_status,
        ),
        (
            "conditional WebSocket http_error success",
            conditional_websocket_http_error_success,
        ),
        (
            "rewritten WebSocket http_error status",
            rewritten_websocket_http_error_status,
        ),
        ("missing REST Allow header", missing_rest_allow),
        ("misleading REST Allow header", misleading_rest_allow),
        ("missing WebSocket Allow header", missing_websocket_allow),
        ("misleading WebSocket Allow header", misleading_websocket_allow),
        ("Axum-derived REST Allow header", axum_derived_rest_allow),
        ("rebound REST status import", rebound_rest_status_import),
        ("hidden allowed preflight path", hidden_allowed_preflight_path),
        ("prefix-allowed preflight path", prefix_allowed_preflight_path),
        ("request-validator preflight bypass", request_validator_preflight_bypass),
        ("required header alternate-name fallback", required_header_alternate_name),
        ("optional header alternate-name merge", optional_header_alternate_name),
        ("conditional attacker host", conditional_attacker_host),
        ("conditional attacker origin", conditional_attacker_origin),
        ("conditional non-mutating method", conditional_non_mutating_method),
        ("widened mutating methods", widened_mutating_methods),
        ("narrowed mutating methods", narrowed_mutating_methods),
        ("rewritten SecurityError status", rewritten_security_error_status),
        ("conditional SecurityError success", conditional_security_error_success),
        (
            "changed SecurityError literal spacing",
            changed_security_error_literal_spacing,
        ),
        ("changed request marker", changed_request_marker),
        ("re-exported API response import", reexported_api_response_import),
        ("exposed RequestSecurity field", exposed_request_security_field),
        ("exposed SecurityError", exposed_security_error),
        ("inherent SecurityError success", inherent_security_error_success),
        ("shadowed format macro", shadowed_format_macro),
        (
            "attribute-prefixed shadowed format macro",
            attributed_shadowed_format_macro,
        ),
        ("ancestor-shadowed format macro", ancestor_shadowed_format_macro),
        ("shadowed std dependency", shadowed_std_dependency),
        (
            "included RequestSecurity callable",
            included_request_security_callable,
        ),
        ("middleware preflight bypass", middleware_preflight_bypass),
        ("direct route security layer", direct_route_security_layer),
        ("conflicting CORS origin header", conflicting_cors_origin_header),
        ("appended CORS origin header", appended_cors_origin_header),
        ("rebound CORS origin import", rebound_cors_origin_import),
        ("changed application server module", changed_application_server_module),
        ("restored pokecon-server dependency", restored_server_dependency),
        ("changed canonical server module declaration", changed_server_module),
        ("redirected canonical server module", redirected_server_module),
        (
            "redirected application server module",
            redirected_application_server_module,
        ),
        ("rebound production REST import", rebound_rest_import),
        ("rebound production WebSocket import", rebound_websocket_import),
        ("rebound public router import", rebound_public_router_import),
        ("nested canonical REST decoy with reachable raw router", nested_rest_decoy),
        (
            "canonical WebSocket router on a decoy impl",
            websocket_owner_decoy,
        ),
        ("canonical BoundServer method on a decoy impl", bound_server_owner_decoy),
        ("redirected REST submodule", redirected_rest_submodule),
        ("cfg-disabled production module with raw replacement", raw_production_module),
        ("alternate entrypoint module", alternate_entrypoint_module),
        ("alternate application library target", alternate_application_lib_target),
        ("alternate primary binary target", alternate_primary_binary),
        ("alternate binary main", alternate_main),
        ("alternate controlled runner", alternate_controlled_runner),
        (
            "pre-dynamic final setting guard weakened",
            pre_dynamic_final_guard_weakened,
        ),
        ("pre-dynamic CLI lookup bypassed", pre_dynamic_cli_lookup_bypassed),
        (
            "resource-root retarget recipe update omitted",
            resource_root_retarget_recipe_omitted,
        ),
        (
            "resource-root retarget default-kind guard omitted",
            resource_root_retarget_default_kind_guard_omitted,
        ),
        (
            "resource-root retarget explicit-source guard omitted",
            resource_root_retarget_default_source_guard_omitted,
        ),
        ("eager packaged resource snapshot", eager_packaged_resource_snapshot),
        ("packaged resource retarget omitted", packaged_resource_retarget_omitted),
        (
            "packaged resource guard dropped before backend",
            packaged_resource_guard_dropped_before_backend,
        ),
        ("desktop packaged backend bypassed", desktop_packaged_backend_bypassed),
        (
            "compositing reexec CLI overlay omitted",
            compositing_reexec_cli_overlay_omitted,
        ),
        (
            "desktop compositing CLI overlay omitted",
            desktop_compositing_cli_overlay_omitted,
        ),
        (
            "compositing spawn-and-wait wrapper restored",
            compositing_spawn_wait_restored,
        ),
        ("compositing pre-exec state retained", compositing_pre_exec_state_retained),
        ("compositing reexec marker omitted", compositing_reexec_marker_omitted),
        ("compositing arguments omitted", compositing_arguments_omitted),
        ("compositing environment omitted", compositing_environment_omitted),
        ("compositing recursion fence omitted", compositing_recursion_fence_omitted),
        (
            "desktop backend supervisor bypassed",
            desktop_backend_supervisor_bypassed,
        ),
        (
            "desktop backend returned error omits fatal request",
            desktop_backend_returned_error_fatal_request_omitted,
        ),
        (
            "desktop backend join error omits fatal request",
            desktop_backend_join_error_fatal_request_omitted,
        ),
        (
            "desktop backend inner handle double join",
            desktop_backend_inner_handle_double_join,
        ),
        (
            "desktop backend readiness guard dropped before classification",
            desktop_backend_readiness_guard_dropped_early,
        ),
        (
            "desktop backend readiness failure marker omitted",
            desktop_backend_readiness_failure_marker_omitted,
        ),
        (
            "desktop pre-readiness shell priority restored",
            desktop_pre_readiness_shell_priority_restored,
        ),
        (
            "desktop backend task published after readiness wait",
            desktop_backend_task_published_after_readiness_wait,
        ),
        (
            "desktop shell error priority reversed",
            desktop_shell_error_priority_reversed,
        ),
        ("shutdown-only application wait", shutdown_only_application_wait),
        (
            "listener readiness barrier bypassed",
            listener_readiness_barrier_bypassed,
        ),
        (
            "listener completion loses readiness arbitration priority",
            listener_readiness_completion_deprioritized,
        ),
        (
            "listener readiness publication wait omitted",
            listener_readiness_publication_wait_omitted,
        ),
        (
            "listener readiness prepared acknowledgement omitted",
            listener_readiness_prepared_ack_omitted,
        ),
        (
            "listener readiness permit dropped before publication",
            listener_readiness_permit_dropped_before_publication,
        ),
        ("omitted listener fatal request", omitted_listener_fatal_request),
        (
            "omitted application signal fatal request",
            omitted_application_signal_fatal_request,
        ),
        (
            "application rejected fatal claim still reports SignalStopped",
            application_rejected_fatal_claim_still_errors,
        ),
        (
            "application signal handle double join",
            application_signal_handle_double_join,
        ),
        (
            "application error skips remaining signal join",
            application_error_skips_remaining_signal_join,
        ),
        ("worker without signal supervision", worker_without_signal_supervision),
        (
            "omitted worker signal fatal request",
            omitted_worker_signal_fatal_request,
        ),
        (
            "worker rejected fatal claim still reports SignalStopped",
            worker_rejected_fatal_claim_still_errors,
        ),
        (
            "worker signal error skips protocol completion",
            worker_signal_error_skips_protocol_completion,
        ),
        ("worker signal handle double join", worker_signal_handle_double_join),
        (
            "worker protocol error skips signal join",
            worker_protocol_error_skips_signal_join,
        ),
        (
            "cfg-disabled canonical REST router with raw replacement",
            cfg_disabled_rest_router,
        ),
        ("alternate desktop backend", alternate_desktop_backend),
    )
    assert len(mutations) == 384
    mutation_labels = tuple(label for label, _mutated_sources in mutations)
    assert len(set(mutation_labels)) == len(mutation_labels)
    mutation_deltas = tuple(
        tuple(
            (
                source_name,
                sources.get(source_name),
                mutated_sources.get(source_name),
            )
            for source_name in sorted(sources.keys() | mutated_sources.keys())
            if sources.get(source_name) != mutated_sources.get(source_name)
        )
        for _label, mutated_sources in mutations
    )
    assert all(mutation_deltas)
    assert len(set(mutation_deltas)) == len(mutation_deltas)

    shard_index, shard_count = production_routing_mutation_shard()
    shard_mutations = mutations[shard_index::shard_count]
    assert shard_mutations
    for label, mutated_sources in shard_mutations:
        try:
            assert_closed_production_routing(mutated_sources)
        except AssertionError:
            continue
        diagnostic = f"production routing audit accepted {label}"
        raise AssertionError(diagnostic)

    if shard_index != 0:
        return

    ignored_generated_acl = sources.copy()
    ignored_generated_acl[
        "@rust/pokecon/permissions/autogenerated/local-build-output.toml"
    ] = '[permission]\nidentifier = "ignored-local-generated-output"\n'
    assert_closed_production_routing(ignored_generated_acl)

    test_only_route = (
        "fn production() {}\n#[cfg(test)]\nmod tests {\n"
        "    fn test_only() { Router::new().route(TEST_PATH, handler); }\n"
        "}\n"
    )
    production_prefix = "fn production() {}\n"
    test_only_production = production_rust_source(test_only_route)
    assert test_only_production[: len(production_prefix)] == production_prefix
    assert len(test_only_production) == len(test_only_route)
    assert tuple(
        index
        for index, character in enumerate(test_only_production)
        if character in "\r\n"
    ) == tuple(
        index for index, character in enumerate(test_only_route) if character in "\r\n"
    )
    assert not test_only_production[len(production_prefix) :].strip()

    production_after_test_tail = (
        REPOSITORY / "rust/pokecon/src/server/rest/mod.rs"
    ).read_text() + "\npub fn hidden_router() -> Router { hidden!() }\n"
    try:
        production_rust_source(production_after_test_tail)
    except AssertionError:
        pass
    else:
        diagnostic = "production composition after cfg(test) tail was accepted"
        raise AssertionError(diagnostic)


def test_gate_covers_immutable_http_native_window_and_diagnostics() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    webkit_observation = section(
        gate,
        "observe_webkit() {",
        "\n}\n\nvalidate_private_process_group_closure() {",
    )
    desktop_surface = section(
        gate,
        "validate_desktop_surface() {",
        "\n}\n\ndiagnostic_sequence() {",
    )

    for required in (
        "/api/state",
        "/api/settings",
        "/ui/",
        "grep -qiE '404|Not Found' \"$packaged_index\"",
        "packaged Web entrypoint contains SvelteKit error page indicators",
        "main Svelte asset bytes differ",
        "resource_not_found",
        "settings.normalized.json",
        "xdotool search --onlyvisible --name '^PokeCon Controller$'",
        "_NET_WM_PID",
        "Map State: IsViewable",
        "WebKitWebProcess",
        "collect_descendants",
        "validate_private_process_group_closure",
        "ps -e -o pid= -o pgid=",
        'readlink -f -- "/proc/$candidate_pid/exe"',
        '"$store_directory"/*/*',
        "desktop_process_group_members",
        "validate_desktop_dbus",
        "DBUS_SESSION_BUS_ADDRESS=",
        "unix:path=$gate_root/runtime/bus",
        "stat -c '%a' -- \"$gate_root/runtime\"",
        '[ ! -S "$desktop_dbus_socket" ]',
        "desktop_dbus_address",
        "sleep 2",
        "libayatana-appindicator3.so.1",
        "POKECON-RUNTIME-0001",
        "POKECON-RUNTIME-0002",
        "POKECON-RUNTIME-0003",
        "POKECON-RUNTIME-0004",
        "Signal(Terminate)",
        "/usr/libexec",
        "xdg-desktop-portal",
        "Activating service name=.*org\\.freedesktop\\.portal",
        "Successfully activated service .*org\\.freedesktop\\.portal",
    ):
        assert required in gate
    for runtime_environment_proof in (
        "while IFS= read -r -d '' environment_entry; do",
        "local compositing_environment_count=0",
        "WEBKIT_DISABLE_COMPOSITING_MODE=*)",
        '"$environment_entry" != WEBKIT_DISABLE_COMPOSITING_MODE=1',
        "WEBKIT_DISABLE_DMABUF_RENDERER=*)",
        '"$environment_entry" != WEBKIT_DISABLE_DMABUF_RENDERER=1',
        "LIBGL_DRIVERS_PATH=*)",
        '"$environment_entry" != "LIBGL_DRIVERS_PATH=$mesa_dri_directory"',
        "LIBGL_ALWAYS_SOFTWARE=*)",
        '"$environment_entry" != LIBGL_ALWAYS_SOFTWARE=1',
        "__EGL_VENDOR_LIBRARY_FILENAMES=*)",
        '"$environment_entry" != "__EGL_VENDOR_LIBRARY_FILENAMES=$mesa_egl_vendor_manifest"',
        'done <"/proc/$selected/environ"',
        '"$compositing_environment_count" -ne 1',
        '"$dmabuf_environment_count" -ne 1',
        '"$libgl_drivers_environment_count" -ne 1',
        '"$libgl_software_environment_count" -ne 1',
        '"$egl_vendor_environment_count" -ne 1',
        'done <"/proc/$selected/maps"',
        '"$canonical_mapped_path" = "$mesa_egl_vendor_library"',
        '"$canonical_mesa_renderer"/* | "$store_directory"/*-libglvnd-*/lib/libEGL.so.1*',
        '"$canonical_mesa_swrast_driver" | "$canonical_mesa_renderer"/lib/libgallium*.so*',
        '"$found_mesa_vendor_library" != true',
        '[ -z "$observed_mesa_software_driver" ]',
        'desktop_mesa_software_driver="$observed_mesa_software_driver"',
        '[ -z "$observed_egl_dispatcher" ]',
        '"$store_directory"/*-libglvnd-*/lib/libEGL.so.1*',
        "/usr/* | /lib/* | /lib64/* | /run/opengl-driver/*",
        "libEGL*.so* | libGL*.so* | libOpenGL*.so* | libgbm*.so* | *_dri.so* | *gallium*.so*",
    ):
        assert runtime_environment_proof in webkit_observation

    for renderer_validation in (
        'if [ "$#" -ne 12 ]',
        'readonly mesa_renderer_input="$8"',
        'readonly proc_socket_evidence_input="$9"',
        'readonly pidfd_signal_input="${10}"',
        'readonly ewmh_close_relay_input="${11}"',
        'readonly openapi_input="${12}"',
        'local mesa_renderer="${11}"',
        'local identity_file="${12}"',
        '"$root/runtime" "$canonical_mesa_renderer"',
        '"$(dirname -- "$canonical_mesa_renderer")" != "$store_directory"',
        'mesa_egl_vendor_manifest="$canonical_mesa_renderer/share/glvnd/egl_vendor.d/50_mesa.json"',
        '[ ! -f "$mesa_egl_vendor_manifest" ] || [ -L "$mesa_egl_vendor_manifest" ]',
        ".ICD.library_path",
        'mesa_dri_directory="$canonical_mesa_renderer/lib/dri"',
        'mesa_swrast_entry="$mesa_dri_directory/swrast_dri.so"',
        "software_renderer: {",
        "output: $mesa_renderer_output",
        "egl_vendor_manifest: $mesa_egl_vendor_manifest",
        "egl_vendor_library: $mesa_egl_vendor_library",
        "swrast_entry: $mesa_swrast_entry",
        "swrast_target: $mesa_swrast_target",
        "mapped_software_driver: $mesa_mapped_software_driver",
        "egl_dispatcher: $egl_dispatcher",
    ):
        assert renderer_validation in gate
    assert gate.count('"$root/runtime" "$canonical_mesa_renderer"') == 3
    for bounded_readiness in (
        "readiness_deadline=$((SECONDS + 60))",
        'while [ "$SECONDS" -lt "$readiness_deadline" ]; do',
        'kill -0 "$active_pid"',
        "sleep 0.1",
    ):
        assert bounded_readiness in desktop_surface
    assert "for _attempt in {1..600}" not in desktop_surface
    assert webkit_observation.index(
        '"$canonical_mesa_renderer"/* | "$store_directory"/*-libglvnd-*/lib/libEGL.so.1*'
    ) < webkit_observation.index('readlink -f -- "$mapped_path"')
    assert webkit_observation.count("local environment_entry") == 1


def test_gate_proves_keep_backend_close_and_same_session_reopen() -> None:
    gate = (REPOSITORY / "scripts/integration/ui_package_check.sh").read_text()
    helper = (REPOSITORY / "scripts/integration/proc_socket_evidence.py").read_text()
    owned_socket = section(
        helper,
        "class OwnedSocket:",
        "\n\n@dataclass(frozen=True, order=True, slots=True)\nclass ConnectionTriple:",
    )
    initial = section(
        gate,
        "capture_initial_socket_baseline() {",
        "\n}\n\nobserve_window() {",
    )
    close = section(
        gate,
        "close_initial_window_and_wait() {",
        "\n}\n\ncapture_post_close_socket_baseline() {",
    )
    post_close = section(
        gate,
        "capture_post_close_socket_baseline() {",
        "\n}\n\nterminate_secondary_if_live() {",
    )
    secondary = section(
        gate,
        "launch_same_session_secondary() {",
        "\n}\n\nwait_for_reopened_surface_and_socket() {",
    )
    reopened = section(
        gate,
        "wait_for_reopened_surface_and_socket() {",
        "\n}\n\nvalidate_desktop_surface() {",
    )
    desktop_surface = section(
        gate,
        "validate_desktop_surface() {",
        "\n}\n\ndiagnostic_sequence() {",
    )
    final_stop = section(
        gate,
        "stop_mode_normally() {",
        "\n}\n\nrun_mode() {",
    )

    for required in (
        '.data.values["ui.desktop.close_behavior"]',
        '"$configured_close_behavior" != keep_backend',
        '[ "$observed_width" -ne 1440 ]',
        '[ "$observed_height" -ne 900 ]',
        '"$project_python" -I -S "$canonical_proc_socket_evidence" snapshot',
        '"$project_python" -I -S "$canonical_proc_socket_evidence" compare',
        '"$project_python" -I -S "$canonical_ewmh_close_relay"',
        '"/proc/$active_pid/maps" | sort -u',
        'env DISPLAY="$desktop_display" XAUTHORITY="$desktop_xauthority"',
        '.evidence.received.message_type == "_NET_CLOSE_WINDOW"',
        ".evidence.received.data == [0, 0, 0, 0, 0]",
        '.evidence.forwarded.message_type == "WM_PROTOCOLS"',
        '.evidence.forwarded.protocol == "WM_DELETE_WINDOW"',
        ".evidence.forwarded.send_count == 1",
        ".evidence.forwarded.xsync_succeeded == true",
        'desktop_close_relay_evidence="$(jq -c',
        "socket_snapshot_status=$?",
        "socket_compare_status=$?",
        '.evidence == null or (.evidence | type) == "object"',
        '"$current_mode_root"/socket.*.json',
        '"$current_mode_root"/socket.*.stderr',
        '"$current_mode_root/secondary.log"',
        '"$current_mode_root/secondary.pid"',
        '"$current_mode_root/secondary.status"',
        '"$current_mode_root/secondary.display"',
        '"$current_mode_root/secondary.xauthority"',
        '"$current_mode_root"/resource-snapshots.*',
        '"$current_mode_root/close-relay.ready.json"',
        '"$current_mode_root/close-relay.json"',
        '"$current_mode_root/close-relay.stderr"',
        ".evidence.listener.inode",
        "keep_backend_close: {",
        "relay: $desktop_close_relay_evidence",
        "excluded_post_close_fingerprints:",
        '--argjson desktop_initial_window "$desktop_initial_window"',
        '--argjson desktop_reopened_window "$desktop_reopened_window"',
        '(.desktop.initial.window.id | type) == "number"',
        '(.desktop.reopened.window.id | type) == "number"',
        ".desktop.initial.window.id != .desktop.reopened.window.id",
        ".desktop.keep_backend_close.relay.target_window",
        "== .desktop.initial.window.id",
        "ui-package-check window identity output schema is invalid",
        "resource_snapshot_inventory_unchanged: true",
        "resource_snapshots_after_shutdown: 0",
        "((.web | keys) == [",
        '"resource_snapshots_after_shutdown", "shutdown_status"',
        ".web.resource_snapshots_after_shutdown == 0",
        ".desktop.resource_snapshots_after_shutdown == 0",
        "((.desktop.secondary | keys) == [",
        '"resource_snapshot_inventory_unchanged", "same_dbus_session"',
        ".desktop.secondary.resource_snapshot_inventory_unchanged == true",
    ):
        assert required in gate
    assert '--arg desktop_initial_window "$desktop_initial_window"' not in gate
    assert '--arg desktop_reopened_window "$desktop_reopened_window"' not in gate
    assert ".evidence.listener.socket" not in gate
    assert "document = self.socket.to_json()" in owned_socket
    assert 'document["fd"] = self.fd' in owned_socket
    assert '"evidence": error.partial_evidence' in helper

    assert (
        initial.index("initial.first")
        < initial.index("sleep 2")
        < initial.index("initial.second")
        < initial.index("compare_socket_snapshots")
    )
    assert 'socket_snapshot_status" -eq 75' in initial
    assert 'socket_compare_status" -eq 75' in initial
    assert "record_primary_socket_identity" in initial

    assert "xdotool windowquit" in close
    assert "jq -s -e" in close
    assert "xdotool windowclose" not in gate
    assert "xdotool windowkill" not in gate
    assert 'xwininfo -id "$desktop_initial_window"' in close
    assert close.index("start_close_request_relay") < close.index("xdotool windowquit")
    assert "POKECON-RUNTIME-0003" in gate
    assert (
        post_close.index("post-close.first")
        < post_close.index("sleep 2")
        < post_close.index("post-close.second")
    )
    assert post_close.count("connection-unavailable") >= 4
    assert "verify_socket_continuity" in post_close
    assert post_close.index('first_code="$(') < post_close.index(
        'verify_socket_continuity "$first"'
    )
    assert post_close.index('second_code="$(') < post_close.index(
        'verify_socket_continuity "$second"'
    )
    assert post_close.count("validate_primary_continuity") >= 3
    assert 'desktop_disappeared_fingerprints="$(' in post_close

    for required in (
        "timeout --signal=TERM --kill-after=2s 20s",
        '"$bash_binary" "$script_path" __launch_product',
        'desktop "$root" "$application" "$desktop_port" "$child_path"',
        'DISPLAY="$desktop_display"',
        'XAUTHORITY="$desktop_xauthority"',
        'DBUS_SESSION_BUS_ADDRESS="$desktop_dbus_address"',
        '"$root/runtime" "$canonical_mesa_renderer"',
        "desktop_secondary_helper_status=$?",
        'desktop_secondary_status="$(head -n 1 -- "$status_file")"',
        'consume_published_secondary_identity "$identity_file" "$pid_file"',
    ):
        assert required in secondary
    assert "dbus-run-session" not in secondary
    assert "xvfb-run" not in secondary

    for cleanup_proof in (
        'local snapshots_after_shutdown="$root/resource-snapshots.after-shutdown"',
        'capture_resource_snapshot_inventory "$root" "$snapshots_after_shutdown"',
        '[ -s "$snapshots_after_shutdown" ]',
        "$mode retained private resource snapshots after clean shutdown",
    ):
        assert cleanup_proof in final_stop
    shutdown_snapshot_capture = (
        'capture_resource_snapshot_inventory "$root" "$snapshots_after_shutdown"'
    )
    assert (
        final_stop.index('wait "$active_supervisor"')
        < final_stop.index(
            'if [ "$application_status" != 0 ] || [ "$supervisor_status" -ne 0 ]'
        )
        < final_stop.index(shutdown_snapshot_capture)
        < final_stop.index('if [ -s "$snapshots_after_shutdown" ]')
        < final_stop.index('validate_diagnostics "$mode" "$root"')
    )

    assert (
        reopened.index('"$observed_window" = "$desktop_initial_window"')
        < reopened.index("reopened.first")
        < reopened.index("sleep 2")
        < reopened.index("reopened.second")
        < reopened.index("compare_socket_snapshots")
    )
    assert '"$desktop_post_close_fingerprints"' in reopened
    assert 'observe_webkit "$desktop_webkit_store"' in reopened
    assert "validate_primary_continuity" in reopened

    assert (
        desktop_surface.index("capture_initial_socket_baseline")
        < desktop_surface.index("close_initial_window_and_wait")
        < desktop_surface.index("capture_post_close_socket_baseline")
        < desktop_surface.index("launch_same_session_secondary")
        < desktop_surface.index("wait_for_reopened_surface_and_socket")
        < desktop_surface.rindex("validate_private_process_group_closure")
        < desktop_surface.rindex("validate_desktop_dbus")
    )
    assert "signal_process_if_identity_matches" in final_stop
    assert 'TERM "$mode application"' in final_stop
    assert 'kill -TERM "$active_pid"' not in final_stop
    assert "ps -o stat=" not in final_stop
    assert "validate_diagnostics" in final_stop


def test_flake_gate_inputs_exclude_desktop_application_libraries() -> None:
    flake = (REPOSITORY / "flake.nix").read_text()
    package = section(
        flake,
        "pokeconPackage = rustPlatform.buildRustPackage {",
        "\n          gateCargoLock =",
    )
    session_bus = section(
        flake,
        "uiPackageSessionBusConfig = pkgs.writeTextFile {",
        "\n          uiPackageCheck = mkTask {",
    )
    gate = section(flake, "uiPackageCheck = mkTask {", "\n        in\n")
    runtime_inputs = section(gate, "runtimeInputs = [", "\n            ];")

    for required in (
        'destination = "/share/dbus-1/session.conf";',
        "<type>session</type>",
        "<keep_umask/>",
        "<listen>unix:runtime=yes</listen>",
        "<auth>EXTERNAL</auth>",
        '<policy context="default">',
        '<allow send_destination="*" eavesdrop="true"/>',
        '<allow eavesdrop="true"/>',
        '<allow own="*"/>',
    ):
        assert required in session_bus
    for forbidden in (
        "standard_session_servicedirs",
        "<include",
        "<includedir",
        "/etc",
    ):
        assert forbidden not in session_bus

    for required in (
        'name = "ui-package-check";',
        "pkgs.python314",
        "pkgs.curl",
        "pkgs.dbus",
        "pkgs.findutils",
        '"${uiPackageSessionBusConfig}/share/dbus-1/session.conf"',
        '"${uiPackageSoftwareRenderer}"',
        '"${source}/scripts/integration/proc_socket_evidence.py"',
        '"${source}/scripts/integration/pidfd_signal.py"',
        '"${source}/scripts/integration/ewmh_close_relay.py"',
        "pkgs.jq",
        "pkgs.xdotool",
        "pkgs.xprop",
        "pkgs.xwininfo",
        "pkgs.xvfb-run",
        "self'.packages.pokecon",
    ):
        assert required in gate
    for forbidden in (
        "pythonEnv",
        "rustTaskInputs",
        "linuxDesktopPackages",
        "linuxApplicationRuntimePackages",
        "pkgs.gtk3",
        "pkgs.webkitgtk",
        "pkgs.libayatana",
        "pkgs.mesa.drivers",
    ):
        assert forbidden not in gate
    assert runtime_inputs.count("pkgs.findutils") == 1
    assert "pkgs.mesa" not in runtime_inputs
    assert package.count('POKECON_RESOURCE_PROVENANCE = "nix-exact";') == 1
    assert package.count('ln -s ../web "$out/bin/web"') == 1
    assert flake.count("uiPackageSoftwareRenderer = pkgs.mesa;") == 1
    assert gate.count('"${uiPackageSoftwareRenderer}"') == 1
    assert gate.count('"${source}/scripts/integration/proc_socket_evidence.py"') == 1
    assert gate.count('"${source}/scripts/integration/pidfd_signal.py"') == 1
    assert gate.count('"${source}/scripts/integration/ewmh_close_relay.py"') == 1
    assert (
        gate.index('"${uiPackageSoftwareRenderer}"')
        < gate.index('"${source}/scripts/integration/proc_socket_evidence.py"')
        < gate.index('"${source}/scripts/integration/pidfd_signal.py"')
        < gate.index('"${source}/scripts/integration/ewmh_close_relay.py"')
    )
    assert "ui-package-check = uiPackageCheck;" in flake
    assert "patchelf --add-rpath" in flake
    assert "patchelf --print-rpath" in flake


def test_desktop_shell_uses_only_the_published_listener_address() -> None:
    entrypoint = (REPOSITORY / "rust/pokecon/src/entrypoint.rs").read_text()
    desktop = (REPOSITORY / "rust/pokecon/src/desktop/mod.rs").read_text()
    shell_config = section(
        desktop,
        "pub struct DesktopShellConfig {",
        "\n}\n\n/// Failure while validating or running the native desktop shell.",
    )
    setup = section(
        desktop,
        ".setup(move |app| {",
        "\n            .on_window_event",
    )
    builder = section(
        desktop,
        "        let builder = tauri::Builder::default()",
        "\n\n        let app = builder.build(context)?;",
    )

    assert "app_url" not in shell_config
    assert "configured_address" not in entrypoint
    assert "backend listener did not match canonical startup settings" not in entrypoint
    assert "Ok(actual_address)" in entrypoint
    assert desktop.count("FnOnce() -> Result<SocketAddr, DesktopError>") == 2
    assert "FnOnce(PathBuf)" not in desktop
    assert builder.count("tauri_plugin_single_instance::init(") == 1
    assert builder.count(".setup(move |app| {") == 1
    assert builder.index("tauri_plugin_single_instance::init(") < builder.index(
        ".setup(move |app| {"
    )
    assert setup.count("on_primary_instance()?") == 1
    assert re.search(r"\.\s*resource_dir\s*(?:::\s*<[^>]*>\s*)?\(", desktop) is None
    assert "tauri::utils::Env::default()" not in entrypoint
    assert "APPDIR" not in entrypoint
    assert "APPIMAGE" not in entrypoint
    assert "tauri::utils::platform::resource_dir" not in entrypoint
    assert re.search(r"\bresource_dir\s*(?:::\s*<[^>]*>\s*)?\(", entrypoint) is None
    for required in (
        "backend_address: OnceLock<SocketAddr>",
        "BackendAddressUnavailable",
        "BackendAddressAlreadyPublished",
        "backend_url_preserves_ipv4_and_ipv6_listener_authority",
        "returns its actual listener before",
    ):
        assert required in desktop
    assert (
        setup.index("on_primary_instance")
        < setup.index("backend_app_url")
        < setup.index("add_capability")
        < setup.index(".backend_address")
        < setup.index("open_main_window")
        < setup.index("tauri::async_runtime::spawn")
        < setup.index("shell_ready.store(true, Ordering::Release)")
        < setup.index("Ok(())")
    )


def test_single_instance_primary_election_dependency_is_review_pinned() -> None:
    cargo_lock = tomllib.loads((REPOSITORY / "Cargo.lock").read_text())
    packages = [
        package
        for package in cargo_lock["package"]
        if package["name"] == "tauri-plugin-single-instance"
    ]

    # In 2.4.3 every supported desktop platform exits a notified secondary
    # during plugin setup, before Tauri invokes the application setup callback.
    # Updating this pin requires re-reviewing that cross-platform ordering.
    assert len(packages) == 1
    assert packages[0]["version"] == "2.4.3"
    assert packages[0]["source"] == (
        "registry+https://github.com/rust-lang/crates.io-index"
    )
    assert packages[0]["checksum"] == (
        "b3214becf9ef5783c0ae99a3bb25adf5353a7a16ebf53e74b909e29205735c6c"
    )


def test_rust_ci_parallelizes_source_and_package_gates_without_rebuilding() -> None:
    workflow = (REPOSITORY / ".github/workflows/rust-ci.yml").read_text()

    assert workflow.count("- 'web/**'") == 2
    assert workflow.count("- 'scripts/integration/**'") == 2
    assert workflow.count("- 'tests/fixtures/cli-help/**'") == 2
    assert workflow.count("branches: [main, master, refactor/rust-core]") == 2
    source_job = section(workflow, "  build:\n", "  package:\n")
    package_job = section(workflow, "  package:\n", "  build-windows:\n")
    rust_core_step = (
        "      - name: Clippy, build, test, and compatibility\n"
        "        if: steps.rust-check.outputs.applicable == 'true'\n"
        "        run: nix run .#rust-ci-core\n"
    )
    worker_step = (
        "      - name: Verify packaged worker roles\n"
        "        if: steps.rust-check.outputs.applicable == 'true'\n"
        "        run: nix run .#worker-package-check\n"
    )
    ui_step_name = "      - name: Verify immutable Web and Tauri modes\n"
    cli_step_name = "      - name: Verify packaged CLI help\n"
    assert workflow.count(rust_core_step) == 1
    assert source_job.count(rust_core_step) == 1
    assert package_job.count(worker_step) == 1
    assert "needs:" not in package_job
    for superseded_command in (
        "nix run .#clippy",
        "nix run .#build-rust",
        "nix run .#cargo-test",
        "nix run .#compatibility",
    ):
        assert superseded_command not in workflow
    assert workflow.count(ui_step_name) == 1
    assert workflow.count(cli_step_name) == 1
    assert section(package_job, ui_step_name, cli_step_name) == (
        "        run: nix run .#ui-package-check\n"
    )
    assert section(workflow, cli_step_name, "\n  build-windows:\n") == (
        "        run: nix run .#cli-help-check\n"
    )
    assert package_job.index(worker_step) < package_job.index(ui_step_name)
    assert package_job.index(ui_step_name) < package_job.index(cli_step_name)


def test_ci_executes_each_existing_logical_check_once() -> None:
    workflow_root = REPOSITORY / ".github/workflows"
    retired_workflows = (
        "basedpyright.yml",
        "nix-source-filter-check.yml",
        "ruff.yml",
        "spa-404-check.yml",
    )
    assert all(not (workflow_root / name).exists() for name in retired_workflows)

    workflows = "\n".join(
        path.read_text() for path in sorted(workflow_root.glob("*.yml"))
    )
    lint = (workflow_root / "lint.yml").read_text()
    remote_flake = (workflow_root / "remote-flake.yml").read_text()
    flake = (REPOSITORY / "flake.nix").read_text()
    contract_check = section(
        flake,
        "            contract-check = mkTask {\n",
        "            generate-contracts = mkTask {\n",
    )

    assert workflows.count("nix fmt -- --ci") == 1
    assert lint.count("nix run .#ruff-check") == 1
    assert lint.count("nix run .#ruff-format-check") == 1
    assert workflows.count("nix run .#clippy") == 0
    assert workflows.count("nix run .#rust-ci-core") == 1
    assert contract_check.count('bun --bun "${basedpyrightCli}"') == 1
    assert contract_check.count("python -m scripts.quality.source_filter") == 1
    source_materialization = "nix derivation show .#pokecon > /dev/null"
    assert remote_flake.count("nix flake check --no-build") == 1
    assert remote_flake.count(source_materialization) == 1
    assert remote_flake.index(source_materialization) < remote_flake.index(
        "nix flake check --no-build"
    )
    assert remote_flake.count("Run default app help from remote") == 1
    for redundant_flake_probe in (
        "Run default app help locally",
        "Run check app help locally",
        "Run check app help from remote",
        "nix run .#check -- --help",
    ):
        assert redundant_flake_probe not in remote_flake


def test_retired_desktop_product_feature_is_absent_from_release_surfaces() -> None:
    release_surfaces = [
        REPOSITORY / "flake.nix",
        REPOSITORY / "scripts/release/gate.py",
        *(REPOSITORY / ".github/workflows").glob("*.yml"),
    ]
    assert (
        sorted(
            path.relative_to(REPOSITORY).as_posix()
            for path in release_surfaces
            if RETIRED_DESKTOP_PRODUCT_FEATURE in path.read_text(encoding="utf-8")
        )
        == []
    )


def test_pytest_ci_runs_common_and_mutation_gates_as_parallel_jobs() -> None:
    workflow = (REPOSITORY / ".github/workflows/pytest.yml").read_text()
    common_job = section(
        workflow,
        "  test-linux:\n",
        "  production-routing-mutation-audit-linux:\n",
    )
    mutation_job = workflow[
        workflow.index("  production-routing-mutation-audit-linux:\n") :
    ]

    source_guard_step = (
        "      - name: Evaluate pytest source guard\n"
        "        id: pytest-check\n"
        '        run: nix run .#source-guard -- pytest --github-output "$GITHUB_OUTPUT"\n'
    )
    common_test_step = (
        "      - name: Run pytest\n"
        "        if: steps.pytest-check.outputs.applicable == 'true'\n"
        "        run: nix run .#test\n"
    )
    mutation_test_step = (
        "      - name: Run production-routing mutation audit\n"
        "        if: steps.pytest-check.outputs.applicable == 'true'\n"
        "        run: nix run .#test-production-routing-mutations\n"
    )
    assert common_job.count(common_test_step) == 1
    assert mutation_job.count(mutation_test_step) == 1
    for job in (common_job, mutation_job):
        assert job.count("runs-on: ubuntu-latest") == 1
        assert job.count(source_guard_step) == 1
        assert job.count("if:") == 1
        assert "continue-on-error:" not in job
        assert "|| true" not in job
        assert "needs:" not in job


def test_windows_rust_ci_binds_development_resource_provenance_to_final_check() -> None:
    workflow = (REPOSITORY / ".github/workflows/rust-ci.yml").read_text()
    step_name = "      - name: Check all Windows targets and features\n"
    run_command = (
        "        run: cargo check --locked --workspace --all-targets --all-features\n"
    )
    expected_final_step = (
        step_name
        + "        env:\n"
        + "          POKECON_RESOURCE_PROVENANCE: development\n"
        + run_command
    )

    assert workflow.count(step_name) == 1
    assert workflow.count("name: Check workspace (Windows)") == 1
    assert "name: Build workspace (Windows)" not in workflow
    assert workflow.endswith(expected_final_step)
    assert workflow.count("POKECON_RESOURCE_PROVENANCE") == 1
    assert workflow.count("POKECON_RESOURCE_PROVENANCE: development") == 1
    assert workflow.count(run_command) == 1
