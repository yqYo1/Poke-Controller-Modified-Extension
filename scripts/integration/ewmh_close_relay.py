from __future__ import annotations

import argparse
import ctypes
import json
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, ClassVar, Protocol, Self, cast, final

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence

CLIENT_MESSAGE = 33
CODE_ATOM_UNAVAILABLE = "atom-unavailable"
CODE_CLOSE_REQUEST_INVALID = "close-request-invalid"
CODE_CLOSE_REQUEST_TIMEOUT = "close-request-timeout"
CODE_CONFIGURATION_INVALID = "configuration-invalid"
CODE_DELETE_FORWARD_FAILED = "delete-forward-failed"
CODE_DELETE_PROTOCOL_CHANGED = "delete-protocol-changed"
CODE_DELETE_PROTOCOL_UNAVAILABLE = "delete-protocol-unavailable"
CODE_DISPLAY_OPEN_FAILED = "display-open-failed"
CODE_LIBRARY_INVALID = "library-invalid"
CODE_X11_ASYNC_ERROR = "x11-asynchronous-error"
CURRENT_TIME = 0
EXIT_ERROR = 2
EXIT_OK = 0
NO_EVENT_MASK = 0
SCHEMA_VERSION = 1
SUBSTRUCTURE_NOTIFY_MASK = 1 << 19
X_FALSE = 0

type JsonObject = dict[str, object]


class RelayFailure(RuntimeError):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(message.replace("\n", " ")[:512])
        self.code: str = code

    @classmethod
    def create(cls, code: str, message: str) -> Self:
        return cls(code, message)


@dataclass(frozen=True, slots=True)
class RelayConfig:
    display: str
    lib_x11: Path
    ready_file: Path
    target_window: int
    timeout_seconds: float
    xauthority: Path


@dataclass(frozen=True, slots=True)
class ClientMessage:
    data: tuple[int, int, int, int, int]
    format: int
    message_type: int
    send_event: bool
    type: int
    window: int


class X11Api(Protocol):
    def close(self) -> None: ...

    def get_wm_protocols(self, window: int) -> tuple[int, ...]: ...

    def intern_atom(self, name: str) -> int: ...

    def next_event(self) -> ClientMessage: ...

    def pending(self) -> int: ...

    def root_window(self) -> int: ...

    def select_input(self, window: int, event_mask: int) -> None: ...

    def send_client_message(self, window: int, event: ClientMessage) -> bool: ...

    def sync_checked(self, phase: str) -> None: ...


class _ClientData(ctypes.Union):
    _fields_: ClassVar[list[tuple[str, object]]] = [
        ("bytes", ctypes.c_char * 20),
        ("shorts", ctypes.c_short * 10),
        ("longs", ctypes.c_long * 5),
    ]


class _ClientMessageEvent(ctypes.Structure):
    _fields_: ClassVar[list[tuple[str, object]]] = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
        ("message_type", ctypes.c_ulong),
        ("format", ctypes.c_int),
        ("data", _ClientData),
    ]


class _XEvent(ctypes.Union):
    _fields_: ClassVar[list[tuple[str, object]]] = [
        ("type", ctypes.c_int),
        ("client", _ClientMessageEvent),
        ("padding", ctypes.c_long * 24),
    ]


class _XErrorEvent(ctypes.Structure):
    _fields_: ClassVar[list[tuple[str, object]]] = [
        ("type", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("resource_id", ctypes.c_ulong),
        ("serial", ctypes.c_ulong),
        ("error_code", ctypes.c_ubyte),
        ("request_code", ctypes.c_ubyte),
        ("minor_code", ctypes.c_ubyte),
    ]


_XErrorHandler = ctypes.CFUNCTYPE(
    ctypes.c_int,
    ctypes.c_void_p,
    ctypes.c_void_p,
)


@final
class CtypesX11:
    def __init__(self, library_path: Path, display_name: str) -> None:
        self._errors: list[tuple[int, int, int, int, int]] = []
        self._library = ctypes.CDLL(str(library_path), mode=os.RTLD_LOCAL | os.RTLD_NOW)
        self._configure_signatures()
        self._error_handler = _XErrorHandler(self._capture_error)
        self._library.XSetErrorHandler(self._error_handler)
        display = self._library.XOpenDisplay(display_name.encode())
        if display is None:
            message = f"cannot open private X11 display {display_name!r}"
            raise RelayFailure.create(CODE_DISPLAY_OPEN_FAILED, message)
        self._display = cast("int", display)

    def _configure_signatures(self) -> None:
        library = self._library
        library.XOpenDisplay.argtypes = [ctypes.c_char_p]
        library.XOpenDisplay.restype = ctypes.c_void_p
        library.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        library.XDefaultRootWindow.restype = ctypes.c_ulong
        library.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
        library.XInternAtom.restype = ctypes.c_ulong
        library.XSelectInput.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_long]
        library.XSelectInput.restype = ctypes.c_int
        library.XPending.argtypes = [ctypes.c_void_p]
        library.XPending.restype = ctypes.c_int
        library.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.POINTER(_XEvent)]
        library.XNextEvent.restype = ctypes.c_int
        library.XGetWMProtocols.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.POINTER(ctypes.POINTER(ctypes.c_ulong)),
            ctypes.POINTER(ctypes.c_int),
        ]
        library.XGetWMProtocols.restype = ctypes.c_int
        library.XFree.argtypes = [ctypes.c_void_p]
        library.XFree.restype = ctypes.c_int
        library.XSendEvent.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ulong,
            ctypes.c_int,
            ctypes.c_long,
            ctypes.POINTER(_XEvent),
        ]
        library.XSendEvent.restype = ctypes.c_int
        library.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        library.XSync.restype = ctypes.c_int
        library.XCloseDisplay.argtypes = [ctypes.c_void_p]
        library.XCloseDisplay.restype = ctypes.c_int
        library.XSetErrorHandler.argtypes = [_XErrorHandler]
        library.XSetErrorHandler.restype = ctypes.c_void_p

    def _capture_error(
        self,
        _display: int | None,
        event_address: int | None,
    ) -> int:
        if event_address is None:
            return 0
        event = _XErrorEvent.from_address(event_address)
        self._errors.append(
            (
                int(event.error_code),
                int(event.request_code),
                int(event.minor_code),
                int(event.resource_id),
                int(event.serial),
            )
        )
        return 0

    def close(self) -> None:
        self._library.XCloseDisplay(self._display)

    def get_wm_protocols(self, window: int) -> tuple[int, ...]:
        protocols_pointer = ctypes.POINTER(ctypes.c_ulong)()
        count = ctypes.c_int()
        status = self._library.XGetWMProtocols(
            self._display,
            window,
            ctypes.byref(protocols_pointer),
            ctypes.byref(count),
        )
        if status == 0:
            return ()
        try:
            return tuple(int(protocols_pointer[index]) for index in range(count.value))
        finally:
            if protocols_pointer:
                self._library.XFree(protocols_pointer)

    def intern_atom(self, name: str) -> int:
        atom = int(self._library.XInternAtom(self._display, name.encode(), X_FALSE))
        if atom == 0:
            message = f"X11 did not intern required atom {name}"
            raise RelayFailure.create(CODE_ATOM_UNAVAILABLE, message)
        return atom

    def next_event(self) -> ClientMessage:
        event = _XEvent()
        self._library.XNextEvent(self._display, ctypes.byref(event))
        client = event.client
        return ClientMessage(
            data=cast(
                "tuple[int, int, int, int, int]",
                tuple(int(client.data.longs[index]) for index in range(5)),
            ),
            format=int(client.format),
            message_type=int(client.message_type),
            send_event=bool(client.send_event),
            type=int(event.type),
            window=int(client.window),
        )

    def pending(self) -> int:
        return int(self._library.XPending(self._display))

    def root_window(self) -> int:
        return int(self._library.XDefaultRootWindow(self._display))

    def select_input(self, window: int, event_mask: int) -> None:
        self._library.XSelectInput(self._display, window, event_mask)

    def send_client_message(self, window: int, event: ClientMessage) -> bool:
        native_event = _XEvent()
        native_event.client.type = event.type
        native_event.client.send_event = int(event.send_event)
        native_event.client.display = self._display
        native_event.client.window = event.window
        native_event.client.message_type = event.message_type
        native_event.client.format = event.format
        for index, value in enumerate(event.data):
            native_event.client.data.longs[index] = value
        return bool(
            self._library.XSendEvent(
                self._display,
                window,
                X_FALSE,
                NO_EVENT_MASK,
                ctypes.byref(native_event),
            )
        )

    def sync_checked(self, phase: str) -> None:
        self._library.XSync(self._display, X_FALSE)
        if not self._errors:
            return
        error_code, request_code, minor_code, resource_id, serial = self._errors[0]
        self._errors.clear()
        message = (
            f"X11 error during {phase}: error={error_code} request={request_code} "
            f"minor={minor_code} resource={resource_id} serial={serial}"
        )
        raise RelayFailure.create(CODE_X11_ASYNC_ERROR, message)


def _write_json(path: Path, document: JsonObject) -> None:
    encoded = json.dumps(
        document,
        ensure_ascii=True,
        separators=(",", ":"),
        sort_keys=True,
    )
    pending = path.with_name(f"{path.name}.pending")
    try:
        with pending.open("x", encoding="utf-8") as handle:
            handle.write(encoded)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(pending, path)
    finally:
        pending.unlink(missing_ok=True)


def _success_document(evidence: JsonObject) -> JsonObject:
    return {
        "diagnostics": [],
        "evidence": evidence,
        "schema_version": SCHEMA_VERSION,
        "status": "ok",
    }


def _failure_document(error: RelayFailure) -> JsonObject:
    return {
        "diagnostics": [{"code": error.code, "message": str(error)[:512]}],
        "evidence": None,
        "schema_version": SCHEMA_VERSION,
        "status": "error",
    }


def _write_stdout(document: JsonObject) -> None:
    sys.stdout.write(
        json.dumps(
            document,
            ensure_ascii=True,
            separators=(",", ":"),
            sort_keys=True,
        )
    )
    sys.stdout.write("\n")


def _protocol_evidence(protocols: tuple[int, ...], delete_atom: int) -> JsonObject:
    return {
        "advertised_atom_ids": list(protocols),
        "wm_delete_window_advertised": delete_atom in protocols,
    }


def relay_close_request(
    config: RelayConfig,
    api: X11Api,
    *,
    monotonic: Callable[[], float] = time.monotonic,
    sleeper: Callable[[float], None] = time.sleep,
) -> JsonObject:
    root = api.root_window()
    close_atom = api.intern_atom("_NET_CLOSE_WINDOW")
    protocols_atom = api.intern_atom("WM_PROTOCOLS")
    delete_atom = api.intern_atom("WM_DELETE_WINDOW")
    initial_protocols = api.get_wm_protocols(config.target_window)
    if delete_atom not in initial_protocols:
        message = (
            "target window did not advertise WM_DELETE_WINDOW before relay readiness"
        )
        raise RelayFailure.create(CODE_DELETE_PROTOCOL_UNAVAILABLE, message)
    api.select_input(root, SUBSTRUCTURE_NOTIFY_MASK)
    api.sync_checked("subscription")
    ready_document: JsonObject = {
        "display": config.display,
        "event_mask": SUBSTRUCTURE_NOTIFY_MASK,
        "protocols": _protocol_evidence(initial_protocols, delete_atom),
        "root_window": root,
        "schema_version": SCHEMA_VERSION,
        "status": "ready",
        "target_window": config.target_window,
        "xauthority": str(config.xauthority),
    }
    _write_json(config.ready_file, ready_document)

    deadline = monotonic() + config.timeout_seconds
    request: ClientMessage | None = None
    while request is None:
        remaining = deadline - monotonic()
        if remaining <= 0:
            message = "no exact _NET_CLOSE_WINDOW request arrived before the deadline"
            raise RelayFailure.create(CODE_CLOSE_REQUEST_TIMEOUT, message)
        if api.pending() == 0:
            sleeper(min(0.01, remaining))
            continue
        candidate = api.next_event()
        if candidate.type != CLIENT_MESSAGE or candidate.message_type != close_atom:
            continue
        if (
            not candidate.send_event
            or candidate.window != config.target_window
            or candidate.format != 32
            or candidate.data != (0, 0, 0, 0, 0)
        ):
            message = "_NET_CLOSE_WINDOW request did not match pinned xdotool semantics"
            raise RelayFailure.create(CODE_CLOSE_REQUEST_INVALID, message)
        request = candidate

    forward_protocols = api.get_wm_protocols(config.target_window)
    if delete_atom not in forward_protocols:
        message = "target window stopped advertising WM_DELETE_WINDOW before forwarding"
        raise RelayFailure.create(CODE_DELETE_PROTOCOL_CHANGED, message)
    forwarded = ClientMessage(
        data=(delete_atom, request.data[0], 0, 0, 0),
        format=32,
        message_type=protocols_atom,
        send_event=True,
        type=CLIENT_MESSAGE,
        window=config.target_window,
    )
    if not api.send_client_message(config.target_window, forwarded):
        message = "XSendEvent could not forward WM_PROTOCOLS/WM_DELETE_WINDOW"
        raise RelayFailure.create(CODE_DELETE_FORWARD_FAILED, message)
    api.sync_checked("forward")
    return {
        "display": config.display,
        "event_mask": SUBSTRUCTURE_NOTIFY_MASK,
        "forwarded": {
            "data": list(forwarded.data),
            "format": forwarded.format,
            "message_type": "WM_PROTOCOLS",
            "protocol": "WM_DELETE_WINDOW",
            "send_count": 1,
            "target_window": forwarded.window,
            "xsync_succeeded": True,
        },
        "lib_x11": str(config.lib_x11),
        "protocols_before_forward": _protocol_evidence(
            forward_protocols,
            delete_atom,
        ),
        "protocols_before_ready": _protocol_evidence(initial_protocols, delete_atom),
        "received": {
            "data": list(request.data),
            "format": request.format,
            "message_type": "_NET_CLOSE_WINDOW",
            "received_on_root": root,
            "send_event": request.send_event,
            "target_window": request.window,
            "type": "ClientMessage",
        },
        "target_window": config.target_window,
        "xauthority": str(config.xauthority),
    }


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    parser.add_argument("--display", required=True)
    parser.add_argument("--lib-x11", required=True, type=Path)
    parser.add_argument("--ready-file", required=True, type=Path)
    parser.add_argument("--target-window", required=True, type=int)
    parser.add_argument("--timeout-seconds", default=8.0, type=float)
    return parser


def _config_from_namespace(namespace: argparse.Namespace) -> RelayConfig:
    display = cast("str", namespace.display)
    library_input = cast("Path", namespace.lib_x11)
    ready_file = cast("Path", namespace.ready_file)
    target_window = cast("int", namespace.target_window)
    timeout_seconds = cast("float", namespace.timeout_seconds)
    environment_display = os.environ.get("DISPLAY")
    xauthority_text = os.environ.get("XAUTHORITY", "")
    if not display or not display.startswith(":"):
        message = "display must name a local X server"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    if target_window <= 0:
        message = "target window must be positive"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    if not 0.1 <= timeout_seconds <= 30.0:
        message = "relay timeout must be between 0.1 and 30 seconds"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    if not library_input.is_absolute() or not ready_file.is_absolute():
        message = "library and readiness paths must be absolute"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    if environment_display != display or not xauthority_text:
        message = (
            "DISPLAY and XAUTHORITY must identify the requested private X11 session"
        )
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    xauthority_input = Path(xauthority_text)
    try:
        xauthority = xauthority_input.resolve(strict=True)
    except OSError as error:
        message = f"cannot resolve Xauthority file: {error}"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message) from error
    if (
        xauthority != xauthority_input
        or not xauthority.is_file()
        or xauthority.is_symlink()
    ):
        message = "Xauthority must be a canonical regular file"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    try:
        library = library_input.resolve(strict=True)
    except OSError as error:
        message = f"cannot resolve X11 library: {error}"
        raise RelayFailure.create(CODE_LIBRARY_INVALID, message) from error
    if library != library_input or not library.is_file():
        message = "X11 library must be a canonical regular file"
        raise RelayFailure.create(CODE_LIBRARY_INVALID, message)
    if ready_file.exists() or ready_file.is_symlink() or not ready_file.parent.is_dir():
        message = "readiness path must be absent beneath an existing directory"
        raise RelayFailure.create(CODE_CONFIGURATION_INVALID, message)
    return RelayConfig(
        display=display,
        lib_x11=library,
        ready_file=ready_file,
        target_window=target_window,
        timeout_seconds=timeout_seconds,
        xauthority=xauthority,
    )


def main(argv: Sequence[str] | None = None) -> int:
    api: CtypesX11 | None = None
    try:
        namespace = _build_parser().parse_args(argv)
        config = _config_from_namespace(namespace)
        api = CtypesX11(config.lib_x11, config.display)
        evidence = relay_close_request(config, api)
    except RelayFailure as error:
        _write_stdout(_failure_document(error))
        return EXIT_ERROR
    except (OSError, ValueError) as error:
        failure = RelayFailure("relay-internal-error", str(error))
        _write_stdout(_failure_document(failure))
        return EXIT_ERROR
    finally:
        if api is not None:
            api.close()
    _write_stdout(_success_document(evidence))
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
