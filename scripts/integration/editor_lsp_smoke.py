"""Exercise the flake-provided language servers over real LSP sessions."""

from __future__ import annotations

import argparse
import json
import os
import queue
import subprocess
import threading
import time
from collections import deque
from dataclasses import dataclass
from pathlib import Path
from typing import cast, final

type JsonScalar = None | bool | int | float | str
type JsonValue = JsonScalar | list[JsonValue] | dict[str, JsonValue]
type JsonObject = dict[str, JsonValue]
type IncomingItem = JsonObject | BaseException | None


class LspSmokeError(RuntimeError):
    """Report a failed or incomplete language-server analysis."""


@dataclass(frozen=True)
class Configuration:
    """Resolved command-line contract for the smoke harness."""

    fixture_root: Path
    rust_analyzer: Path
    basedpyright: Path
    typescript_language_server: Path
    svelte_language_server: Path
    python: Path
    timeout_seconds: float


@dataclass(frozen=True)
class ExpectedDiagnostic:
    """Stable properties of the deliberate fixture error."""

    code_fragment: str
    message_fragment: str
    line: int


@dataclass(frozen=True)
class ServerCase:
    """One language server, workspace, document, and expected result."""

    name: str
    command: tuple[str, ...]
    workspace: Path
    document: Path
    language_id: str
    expected_diagnostic: ExpectedDiagnostic
    settings: JsonObject
    initialization_options: JsonObject


def _json_object(value: object, label: str) -> JsonObject:
    if not isinstance(value, dict):
        msg = f"{label} must be a JSON object"
        raise LspSmokeError(msg)
    raw_object = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in raw_object):
        msg = f"{label} keys must be strings"
        raise LspSmokeError(msg)
    return cast("JsonObject", raw_object)


@final
class LspSession:
    """Minimal JSON-RPC client with enough server-request support for LSP."""

    def __init__(self, case: ServerCase) -> None:
        self._case = case
        self._incoming: queue.Queue[IncomingItem] = queue.Queue()
        self._stderr_lines: deque[str] = deque(maxlen=80)
        self._stderr_lock = threading.Lock()
        self._responses: dict[int, JsonObject] = {}
        self._diagnostics: dict[str, list[JsonObject]] = {}
        self._next_request_id = 1
        self._process: subprocess.Popen[bytes] = subprocess.Popen(  # noqa: S603
            case.command,
            cwd=case.workspace,
            env=os.environ.copy(),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        if (
            self._process.stdin is None
            or self._process.stdout is None
            or self._process.stderr is None
        ):
            msg = f"{case.name}: failed to open language-server pipes"
            raise LspSmokeError(msg)
        self._reader = threading.Thread(
            target=self._read_messages,
            name=f"{case.name}-lsp-reader",
            daemon=True,
        )
        self._stderr_reader = threading.Thread(
            target=self._read_stderr,
            name=f"{case.name}-stderr-reader",
            daemon=True,
        )
        self._reader.start()
        self._stderr_reader.start()

    def _read_exactly(self, length: int) -> bytes:
        stdout = self._process.stdout
        if stdout is None:
            msg = f"{self._case.name}: stdout pipe disappeared"
            raise LspSmokeError(msg)
        chunks: list[bytes] = []
        remaining = length
        while remaining:
            chunk = stdout.read(remaining)
            if not chunk:
                msg = f"{self._case.name}: language server closed a partial LSP body"
                raise LspSmokeError(msg)
            chunks.append(chunk)
            remaining -= len(chunk)
        return b"".join(chunks)

    def _read_messages(self) -> None:
        stdout = self._process.stdout
        if stdout is None:
            msg = f"{self._case.name}: stdout pipe disappeared"
            self._incoming.put(LspSmokeError(msg))
            return
        try:
            while True:
                headers: dict[str, str] = {}
                while True:
                    line = stdout.readline()
                    if not line:
                        self._incoming.put(None)
                        return
                    if line in {b"\n", b"\r\n"}:
                        break
                    decoded = line.decode("ascii", errors="strict").strip()
                    name, separator, value = decoded.partition(":")
                    if not separator:
                        msg = f"{self._case.name}: malformed LSP header: {decoded!r}"
                        raise LspSmokeError(msg)
                    headers[name.lower()] = value.strip()
                raw_length = headers.get("content-length")
                if raw_length is None:
                    msg = f"{self._case.name}: LSP message omitted Content-Length"
                    raise LspSmokeError(msg)
                body = self._read_exactly(int(raw_length))
                parsed: object = json.loads(body.decode("utf-8"))
                self._incoming.put(_json_object(parsed, "LSP message"))
        except BaseException as error:  # propagated to the controlling thread
            self._incoming.put(error)

    def _read_stderr(self) -> None:
        stderr = self._process.stderr
        if stderr is None:
            return
        for raw_line in iter(stderr.readline, b""):
            line = raw_line.decode("utf-8", errors="replace").rstrip()
            with self._stderr_lock:
                self._stderr_lines.append(line)

    def _stderr_tail(self) -> str:
        with self._stderr_lock:
            lines = tuple(self._stderr_lines)
        if not lines:
            return "<no server stderr>"
        return "\n".join(lines)

    def _send(self, payload: JsonObject) -> None:
        stdin = self._process.stdin
        if stdin is None:
            msg = f"{self._case.name}: stdin pipe disappeared"
            raise LspSmokeError(msg)
        encoded = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        frame = f"Content-Length: {len(encoded)}\r\n\r\n".encode("ascii") + encoded
        try:
            stdin.write(frame)
            stdin.flush()
        except BrokenPipeError as error:
            msg = (
                f"{self._case.name}: language server closed stdin\n"
                f"{self._stderr_tail()}"
            )
            raise LspSmokeError(msg) from error

    def notify(self, method: str, params: JsonValue) -> None:
        """Send a JSON-RPC notification."""

        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def request(self, method: str, params: JsonValue, timeout: float) -> JsonValue:
        """Send a request and await its response while serving peer requests."""

        request_id = self._next_request_id
        self._next_request_id += 1
        self._send(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": method,
                "params": params,
            }
        )
        return self._await_response(request_id, timeout)

    def _configuration_value(self, section: str) -> JsonValue:
        value: JsonValue = self._case.settings
        if not section:
            return value
        for component in section.split("."):
            if not isinstance(value, dict) or component not in value:
                return None
            value = value[component]
        return value

    def _server_request_result(self, method: str, params: JsonValue) -> JsonValue:
        if method == "workspace/configuration":
            if not isinstance(params, dict):
                return []
            items = params.get("items")
            if not isinstance(items, list):
                return []
            results: list[JsonValue] = []
            for raw_item in items:
                if isinstance(raw_item, dict):
                    section = raw_item.get("section")
                    if isinstance(section, str):
                        results.append(self._configuration_value(section))
                        continue
                results.append(None)
            return results
        if method == "workspace/workspaceFolders":
            return [
                {
                    "uri": self._case.workspace.as_uri(),
                    "name": self._case.workspace.name,
                }
            ]
        if method == "workspace/applyEdit":
            return {"applied": False}
        return None

    def _record_diagnostics(self, params: JsonValue) -> None:
        if not isinstance(params, dict):
            return
        uri = params.get("uri")
        raw_diagnostics = params.get("diagnostics")
        if not isinstance(uri, str) or not isinstance(raw_diagnostics, list):
            return
        diagnostics = [
            cast("JsonObject", diagnostic)
            for diagnostic in raw_diagnostics
            if isinstance(diagnostic, dict)
        ]
        self._diagnostics[uri] = diagnostics

    def _handle_message(self, message: JsonObject) -> None:
        method = message.get("method")
        if isinstance(method, str):
            params = message.get("params")
            if method == "textDocument/publishDiagnostics":
                self._record_diagnostics(params)
            raw_id = message.get("id")
            if isinstance(raw_id, int):
                self._send(
                    {
                        "jsonrpc": "2.0",
                        "id": raw_id,
                        "result": self._server_request_result(method, params),
                    }
                )
            return
        raw_id = message.get("id")
        if isinstance(raw_id, int):
            self._responses[raw_id] = message

    def _pump(self, timeout: float) -> None:
        try:
            item = self._incoming.get(timeout=timeout)
        except queue.Empty as error:
            msg = (
                f"{self._case.name}: timed out waiting for an LSP message\n"
                f"{self._stderr_tail()}"
            )
            raise LspSmokeError(msg) from error
        if item is None:
            code = self._process.poll()
            msg = (
                f"{self._case.name}: language server exited early with status {code}\n"
                f"{self._stderr_tail()}"
            )
            raise LspSmokeError(msg)
        if isinstance(item, BaseException):
            msg = (
                f"{self._case.name}: failed to read LSP output: {item}\n"
                f"{self._stderr_tail()}"
            )
            raise LspSmokeError(msg) from item
        self._handle_message(item)

    def _await_response(self, request_id: int, timeout: float) -> JsonValue:
        deadline = time.monotonic() + timeout
        while request_id not in self._responses:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                msg = (
                    f"{self._case.name}: timed out waiting for request {request_id}\n"
                    f"{self._stderr_tail()}"
                )
                raise LspSmokeError(msg)
            self._pump(remaining)
        response = self._responses.pop(request_id)
        error = response.get("error")
        if error is not None:
            rendered = json.dumps(error, sort_keys=True)
            msg = f"{self._case.name}: LSP request failed: {rendered}"
            raise LspSmokeError(msg)
        return response.get("result")

    @staticmethod
    def _matching_diagnostic(
        diagnostics: list[JsonObject], expected: ExpectedDiagnostic
    ) -> JsonObject | None:
        for diagnostic in diagnostics:
            code = diagnostic.get("code")
            message = diagnostic.get("message")
            severity = diagnostic.get("severity")
            raw_range = diagnostic.get("range")
            if not isinstance(message, str) or not isinstance(raw_range, dict):
                continue
            start = raw_range.get("start")
            if not isinstance(start, dict):
                continue
            line = start.get("line")
            code_text = str(code)
            if (
                expected.code_fragment in code_text
                and expected.message_fragment.lower() in message.lower()
                and line == expected.line
                and severity == 1
            ):
                return diagnostic
        return None

    def wait_for_expected_diagnostic(
        self, uri: str, expected: ExpectedDiagnostic, timeout: float
    ) -> JsonObject:
        """Require the precise intentional type error from this document."""

        deadline = time.monotonic() + timeout
        while True:
            matching = self._matching_diagnostic(
                self._diagnostics.get(uri, []), expected
            )
            if matching is not None:
                return matching
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                observed = json.dumps(self._diagnostics.get(uri, []), sort_keys=True)
                msg = (
                    f"{self._case.name}: expected diagnostic "
                    f"{expected.code_fragment!r}/{expected.message_fragment!r} "
                    f"on line {expected.line}, observed {observed}\n"
                    f"{self._stderr_tail()}"
                )
                raise LspSmokeError(msg)
            self._pump(remaining)

    def shutdown(self, timeout: float) -> None:
        """Perform the LSP shutdown/exit sequence and require a clean process."""

        if self._process.poll() is not None:
            msg = (
                f"{self._case.name}: server exited before shutdown\n"
                f"{self._stderr_tail()}"
            )
            raise LspSmokeError(msg)
        self.request("shutdown", None, timeout)
        self.notify("exit", None)
        try:
            return_code = self._process.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            self.abort()
            msg = f"{self._case.name}: server did not exit after shutdown"
            raise LspSmokeError(msg) from error
        if return_code != 0:
            msg = (
                f"{self._case.name}: server exited with status {return_code}\n"
                f"{self._stderr_tail()}"
            )
            raise LspSmokeError(msg)

    def abort(self) -> None:
        """Stop a failed server without leaving a child process behind."""

        if self._process.poll() is not None:
            return
        self._process.terminate()
        try:
            self._process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait(timeout=3)


def _write_fixture(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def create_fixtures(root: Path) -> dict[str, Path]:
    """Create isolated projects whose single type errors have stable diagnostics."""

    if {path.name for path in root.iterdir()} != {"node_modules"}:
        msg = f"fixture root must contain only the Nix-provided node_modules: {root}"
        raise LspSmokeError(msg)
    rust_root = root / "rust"
    python_root = root / "python"
    frontend_root = root / "frontend"
    _write_fixture(
        rust_root / "Cargo.toml",
        """[package]
name = "pokecon-editor-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]
""",
    )
    _write_fixture(
        rust_root / "src/lib.rs",
        """pub fn typed_value() -> u32 {
    "intentional editor smoke"
}
""",
    )
    _write_fixture(
        python_root / "pyrightconfig.json",
        json.dumps(
            {
                "include": ["."],
                "pythonVersion": "3.14",
                "typeCheckingMode": "strict",
            },
            indent=2,
        )
        + "\n",
    )
    _write_fixture(
        python_root / "editor_smoke.py",
        """def typed_value() -> int:
    return "intentional editor smoke"
""",
    )
    _write_fixture(
        frontend_root / "package.json",
        """{
  "name": "pokecon-editor-smoke",
  "private": true,
  "type": "module"
}
""",
    )
    _write_fixture(
        frontend_root / "tsconfig.json",
        """{
  "compilerOptions": {
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "noEmit": true,
    "strict": true,
    "target": "ES2022"
  }
}
""",
    )
    _write_fixture(
        frontend_root / "editor_smoke.ts",
        'export const typedValue: number = "intentional editor smoke";\n',
    )
    _write_fixture(
        frontend_root / "EditorSmoke.svelte",
        """<script lang="ts">
  const typedValue: number = "intentional editor smoke";
</script>

<p>{typedValue}</p>
""",
    )
    return {
        "rust_root": rust_root,
        "rust_document": rust_root / "src/lib.rs",
        "python_root": python_root,
        "python_document": python_root / "editor_smoke.py",
        "frontend_root": frontend_root,
        "typescript_document": frontend_root / "editor_smoke.ts",
        "svelte_document": frontend_root / "EditorSmoke.svelte",
    }


def _initialize_params(case: ServerCase) -> JsonObject:
    return {
        "processId": None,
        "clientInfo": {"name": "pokecon-editor-smoke", "version": "1"},
        "locale": "C",
        "rootPath": str(case.workspace),
        "rootUri": case.workspace.as_uri(),
        "workspaceFolders": [
            {"uri": case.workspace.as_uri(), "name": case.workspace.name}
        ],
        "capabilities": {
            "general": {"positionEncodings": ["utf-16"]},
            "window": {"workDoneProgress": True},
            "workspace": {
                "configuration": True,
                "workspaceFolders": True,
            },
            "textDocument": {
                "documentSymbol": {"hierarchicalDocumentSymbolSupport": True},
                "publishDiagnostics": {
                    "codeDescriptionSupport": True,
                    "dataSupport": True,
                    "relatedInformation": True,
                    "versionSupport": True,
                },
                "synchronization": {
                    "didSave": True,
                    "dynamicRegistration": False,
                    "willSave": False,
                    "willSaveWaitUntil": False,
                },
            },
        },
        "initializationOptions": case.initialization_options,
        "trace": "off",
    }


def run_case(case: ServerCase, timeout: float) -> None:
    """Initialize, analyze a document, verify its error, and shut down."""

    session = LspSession(case)
    completed = False
    try:
        initialize_result = session.request(
            "initialize", _initialize_params(case), timeout
        )
        if not isinstance(initialize_result, dict) or not isinstance(
            initialize_result.get("capabilities"), dict
        ):
            msg = f"{case.name}: initialize returned no server capabilities"
            raise LspSmokeError(msg)
        session.notify("initialized", {})
        session.notify("workspace/didChangeConfiguration", {"settings": case.settings})
        document_uri = case.document.as_uri()
        session.notify(
            "textDocument/didOpen",
            {
                "textDocument": {
                    "uri": document_uri,
                    "languageId": case.language_id,
                    "version": 1,
                    "text": case.document.read_text(encoding="utf-8"),
                }
            },
        )
        symbols = session.request(
            "textDocument/documentSymbol",
            {"textDocument": {"uri": document_uri}},
            timeout,
        )
        if not isinstance(symbols, list) or not symbols:
            msg = f"{case.name}: documentSymbol returned no analyzed symbols"
            raise LspSmokeError(msg)
        diagnostic = session.wait_for_expected_diagnostic(
            document_uri, case.expected_diagnostic, timeout
        )
        code = diagnostic.get("code")
        message = diagnostic.get("message")
        session.shutdown(timeout)
        print(
            f"editor-smoke: {case.name}: initialize/didOpen/documentSymbol/"
            f"diagnostic/shutdown passed ({code}: {message})"
        )
        completed = True
    finally:
        if not completed:
            session.abort()


def _resolved_executable(raw_path: str, name: str) -> Path:
    path = Path(raw_path)
    if not path.is_absolute() or not path.is_file() or not os.access(path, os.X_OK):
        msg = f"{name} must be an absolute executable path: {path}"
        raise LspSmokeError(msg)
    return path.resolve(strict=True)


def parse_args() -> Configuration:
    """Parse and validate the Nix app's fixed tool paths."""

    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture-root", required=True)
    parser.add_argument("--rust-analyzer", required=True)
    parser.add_argument("--basedpyright", required=True)
    parser.add_argument("--typescript-language-server", required=True)
    parser.add_argument("--svelte-language-server", required=True)
    parser.add_argument("--python", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=120.0)
    namespace = parser.parse_args()
    fixture_root = Path(cast("str", namespace.fixture_root))
    if (
        not fixture_root.is_absolute()
        or not fixture_root.is_dir()
        or fixture_root.is_symlink()
        or not (fixture_root / "node_modules").is_dir()
    ):
        parser.error(
            "--fixture-root must be an absolute real directory with node_modules"
        )
    timeout_seconds = cast("float", namespace.timeout_seconds)
    if timeout_seconds < 10 or timeout_seconds > 600:
        parser.error("--timeout-seconds must be between 10 and 600")
    return Configuration(
        fixture_root=fixture_root,
        rust_analyzer=_resolved_executable(
            cast("str", namespace.rust_analyzer), "rust-analyzer"
        ),
        basedpyright=_resolved_executable(
            cast("str", namespace.basedpyright), "basedpyright"
        ),
        typescript_language_server=_resolved_executable(
            cast("str", namespace.typescript_language_server),
            "typescript-language-server",
        ),
        svelte_language_server=_resolved_executable(
            cast("str", namespace.svelte_language_server),
            "svelte-language-server",
        ),
        python=_resolved_executable(cast("str", namespace.python), "python"),
        timeout_seconds=timeout_seconds,
    )


def main() -> None:
    """Run all four language stacks in isolated, generated projects."""

    configuration = parse_args()
    fixtures = create_fixtures(configuration.fixture_root)
    common_settings: JsonObject = {}
    cases = (
        ServerCase(
            name="rust-analyzer",
            command=(str(configuration.rust_analyzer),),
            workspace=fixtures["rust_root"],
            document=fixtures["rust_document"],
            language_id="rust",
            expected_diagnostic=ExpectedDiagnostic(
                code_fragment="E0308",
                message_fragment="mismatched types",
                line=1,
            ),
            settings={"rust-analyzer": {"check": {"command": "check"}}},
            initialization_options={"check": {"command": "check"}},
        ),
        ServerCase(
            name="basedpyright",
            command=(str(configuration.basedpyright), "--stdio"),
            workspace=fixtures["python_root"],
            document=fixtures["python_document"],
            language_id="python",
            expected_diagnostic=ExpectedDiagnostic(
                code_fragment="reportReturnType",
                message_fragment="not assignable to return type",
                line=1,
            ),
            settings={
                "python": {
                    "pythonPath": str(configuration.python),
                    "analysis": {"typeCheckingMode": "strict"},
                }
            },
            initialization_options={"pythonPath": str(configuration.python)},
        ),
        ServerCase(
            name="typescript-language-server",
            command=(str(configuration.typescript_language_server), "--stdio"),
            workspace=fixtures["frontend_root"],
            document=fixtures["typescript_document"],
            language_id="typescript",
            expected_diagnostic=ExpectedDiagnostic(
                code_fragment="2322",
                message_fragment="not assignable to type 'number'",
                line=0,
            ),
            settings=common_settings,
            initialization_options={},
        ),
        ServerCase(
            name="svelteserver",
            command=(str(configuration.svelte_language_server), "--stdio"),
            workspace=fixtures["frontend_root"],
            document=fixtures["svelte_document"],
            language_id="svelte",
            expected_diagnostic=ExpectedDiagnostic(
                code_fragment="2322",
                message_fragment="not assignable to type 'number'",
                line=1,
            ),
            settings=common_settings,
            initialization_options={},
        ),
    )
    for case in cases:
        run_case(case, configuration.timeout_seconds)
    print("editor-smoke: all language-server analyses passed")


if __name__ == "__main__":
    main()
