from __future__ import annotations

import argparse
import hashlib
import ipaddress
import json
import os
import re
import socket
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Literal, cast

if TYPE_CHECKING:
    from collections.abc import Callable, Iterable, Mapping, Sequence

EXIT_OK = 0
EXIT_ERROR = 2
EXIT_RETRYABLE = 75
SCHEMA_VERSION = 1
TCP_ESTABLISHED = "01"
TCP_LISTEN = "0A"
DEAD_PROCESS_STATES = frozenset({"X", "Z", "x"})
_SOCKET_LINK = re.compile(r"socket:\[(?P<inode>[0-9]+)\]")

type JsonObject = dict[str, object]
type TcpFamily = Literal["tcp4", "tcp6"]


class EvidenceFailure(RuntimeError):
    def __init__(
        self,
        code: str,
        message: str,
        *,
        partial_evidence: JsonObject | None = None,
        retryable: bool,
    ) -> None:
        super().__init__(message)
        self.code: str = code
        self.partial_evidence: JsonObject | None = partial_evidence
        self.retryable: bool = retryable


class RetryableEvidence(EvidenceFailure):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(code, message, retryable=True)


class StableEvidenceError(EvidenceFailure):
    def __init__(self, code: str, message: str) -> None:
        super().__init__(code, message, retryable=False)


@dataclass(frozen=True, order=True, slots=True)
class Endpoint:
    address: str
    port: int

    def to_json(self) -> JsonObject:
        return {"address": self.address, "port": self.port}


@dataclass(frozen=True, order=True, slots=True)
class TcpSocket:
    family: TcpFamily
    state: str
    local: Endpoint
    remote: Endpoint
    inode: int

    def to_json(self) -> JsonObject:
        return {
            "family": self.family,
            "inode": self.inode,
            "local": self.local.to_json(),
            "remote": self.remote.to_json(),
            "state": self.state,
        }


@dataclass(frozen=True, order=True, slots=True)
class ProcessIdentity:
    executable: str
    pgid: int
    pid: int
    ppid: int
    start_ticks: int
    state: str

    def to_json(self) -> JsonObject:
        return {
            "executable": self.executable,
            "pgid": self.pgid,
            "pid": self.pid,
            "ppid": self.ppid,
            "start_ticks": self.start_ticks,
            "state": self.state,
        }


@dataclass(frozen=True, order=True, slots=True)
class OwnedSocket:
    fd: int
    socket: TcpSocket

    def to_json(self) -> JsonObject:
        document = self.socket.to_json()
        document["fd"] = self.fd
        return document


@dataclass(frozen=True, order=True, slots=True)
class ConnectionTriple:
    client: OwnedSocket
    network_process: ProcessIdentity
    server: OwnedSocket
    listener: OwnedSocket
    fingerprint: str

    def to_json(self) -> JsonObject:
        return {
            "client": self.client.to_json(),
            "fingerprint": self.fingerprint,
            "listener": self.listener.to_json(),
            "network_process": self.network_process.to_json(),
            "server": self.server.to_json(),
        }


@dataclass(frozen=True, slots=True)
class SnapshotConfig:
    address: str
    backend_pgid: int
    backend_pid: int
    port: int
    proc_root: Path = Path("/proc")
    webkit_store: Path = Path("/")


@dataclass(frozen=True, slots=True)
class SnapshotEvidence:
    backend: ProcessIdentity
    candidate_webkit_processes: tuple[ProcessIdentity, ...]
    connections: tuple[ConnectionTriple, ...]
    listener: OwnedSocket

    def to_json(self) -> JsonObject:
        return {
            "backend": self.backend.to_json(),
            "candidate_webkit_processes": [
                process.to_json() for process in self.candidate_webkit_processes
            ],
            "connections": [connection.to_json() for connection in self.connections],
            "fingerprints": [connection.fingerprint for connection in self.connections],
            "listener": self.listener.to_json(),
        }


def _failure(
    message: str,
    *,
    code: str,
    partial_evidence: JsonObject | None = None,
    retryable: bool,
) -> EvidenceFailure:
    bounded = message.replace("\n", " ")[:512]
    error: EvidenceFailure
    if retryable:
        error = RetryableEvidence(code, bounded)
    else:
        error = StableEvidenceError(code, bounded)
    error.partial_evidence = partial_evidence
    return error


def _read_text(path: Path, label: str) -> str:
    try:
        return path.read_text()
    except FileNotFoundError as error:
        message = f"{label} disappeared: {path}"
        raise _failure(message, code="proc-entry-churn", retryable=True) from error
    except PermissionError as error:
        message = f"permission denied reading {label}: {path}"
        raise _failure(
            message, code="proc-permission-denied", retryable=False
        ) from error
    except OSError as error:
        message = f"cannot read {label}: {path}: {error}"
        raise _failure(message, code="proc-read-failed", retryable=False) from error


def _read_link(path: Path, label: str) -> str:
    try:
        return os.readlink(path)
    except FileNotFoundError as error:
        message = f"{label} disappeared: {path}"
        raise _failure(message, code="proc-entry-churn", retryable=True) from error
    except PermissionError as error:
        message = f"permission denied reading {label}: {path}"
        raise _failure(
            message, code="proc-permission-denied", retryable=False
        ) from error
    except OSError as error:
        message = f"cannot read {label}: {path}: {error}"
        raise _failure(message, code="proc-read-failed", retryable=False) from error


def _parse_decimal(value: str, label: str) -> int:
    try:
        parsed = int(value, 10)
    except ValueError as error:
        message = f"{label} is not decimal: {value!r}"
        raise _failure(message, code="proc-format-invalid", retryable=False) from error
    if parsed < 0:
        message = f"{label} is negative: {parsed}"
        raise _failure(message, code="proc-format-invalid", retryable=False)
    return parsed


def parse_process_stat(raw: str, expected_pid: int) -> tuple[str, int, int, int]:
    opening = raw.find("(")
    closing = raw.rfind(")")
    if opening < 1 or closing <= opening or closing + 2 > len(raw):
        message = f"pid {expected_pid} has malformed stat delimiters"
        raise _failure(message, code="proc-stat-invalid", retryable=False)
    parsed_pid = _parse_decimal(raw[:opening].strip(), "stat pid")
    fields = raw[closing + 2 :].split()
    if parsed_pid != expected_pid or len(fields) < 20:
        message = f"pid {expected_pid} has malformed stat fields"
        raise _failure(message, code="proc-stat-invalid", retryable=False)
    state = fields[0]
    if len(state) != 1:
        message = f"pid {expected_pid} has invalid process state"
        raise _failure(message, code="proc-stat-invalid", retryable=False)
    return (
        state,
        _parse_decimal(fields[1], "stat ppid"),
        _parse_decimal(fields[2], "stat pgid"),
        _parse_decimal(fields[19], "stat start ticks"),
    )


def read_process_identity(proc_root: Path, pid: int) -> ProcessIdentity:
    process_root = proc_root / str(pid)
    state, ppid, pgid, start_ticks = parse_process_stat(
        _read_text(process_root / "stat", f"pid {pid} stat"),
        pid,
    )
    executable = _read_link(process_root / "exe", f"pid {pid} executable")
    if not executable.startswith("/"):
        message = f"pid {pid} executable link is not absolute: {executable}"
        raise _failure(message, code="process-executable-invalid", retryable=False)
    return ProcessIdentity(
        executable=executable,
        pgid=pgid,
        pid=pid,
        ppid=ppid,
        start_ticks=start_ticks,
        state=state,
    )


def _decode_kernel_address(encoded: str, family: TcpFamily) -> str:
    expected_length = 8 if family == "tcp4" else 32
    if len(encoded) != expected_length:
        message = (
            f"{family} address has length {len(encoded)}, expected {expected_length}"
        )
        raise _failure(message, code="tcp-address-invalid", retryable=False)
    try:
        if family == "tcp4":
            packed = bytes.fromhex(encoded)[::-1]
            address = ipaddress.ip_address(packed)
        else:
            packed = b"".join(
                bytes.fromhex(encoded[offset : offset + 8])[::-1]
                for offset in range(0, 32, 8)
            )
            address = ipaddress.ip_address(socket.inet_ntop(socket.AF_INET6, packed))
    except (ValueError, OSError) as error:
        message = f"cannot decode {family} address: {encoded}"
        raise _failure(message, code="tcp-address-invalid", retryable=False) from error
    if isinstance(address, ipaddress.IPv6Address) and address.ipv4_mapped is not None:
        return str(address.ipv4_mapped)
    return str(address)


def parse_kernel_endpoint(encoded: str, family: TcpFamily) -> Endpoint:
    address_text, separator, port_text = encoded.partition(":")
    if separator != ":" or not port_text:
        message = f"malformed {family} endpoint: {encoded!r}"
        raise _failure(message, code="tcp-endpoint-invalid", retryable=False)
    try:
        port = int(port_text, 16)
    except ValueError as error:
        message = f"malformed {family} port: {port_text!r}"
        raise _failure(message, code="tcp-endpoint-invalid", retryable=False) from error
    if not 0 <= port <= 65535:
        message = f"{family} port is out of range: {port}"
        raise _failure(message, code="tcp-endpoint-invalid", retryable=False)
    return Endpoint(_decode_kernel_address(address_text, family), port)


def parse_tcp_table(raw: str, family: TcpFamily) -> tuple[TcpSocket, ...]:
    rows: list[TcpSocket] = []
    lines = raw.splitlines()
    if not lines or "local_address" not in lines[0]:
        message = f"{family} table header is malformed"
        raise _failure(message, code="tcp-table-invalid", retryable=False)
    for line_number, line in enumerate(lines[1:], start=2):
        if not line.strip():
            continue
        fields = line.split()
        if len(fields) < 10 or not fields[0].endswith(":"):
            message = f"{family} row {line_number} is malformed"
            raise _failure(message, code="tcp-table-invalid", retryable=False)
        state = fields[3].upper()
        if not re.fullmatch(r"[0-9A-F]{2}", state):
            message = f"{family} row {line_number} has invalid state"
            raise _failure(message, code="tcp-table-invalid", retryable=False)
        rows.append(
            TcpSocket(
                family=family,
                inode=_parse_decimal(fields[9], f"{family} inode"),
                local=parse_kernel_endpoint(fields[1], family),
                remote=parse_kernel_endpoint(fields[2], family),
                state=state,
            )
        )
    return tuple(rows)


def read_namespace_sockets(proc_root: Path, backend_pid: int) -> tuple[TcpSocket, ...]:
    network_root = proc_root / str(backend_pid) / "net"
    sockets = [
        *parse_tcp_table(_read_text(network_root / "tcp", "tcp namespace"), "tcp4"),
        *parse_tcp_table(_read_text(network_root / "tcp6", "tcp6 namespace"), "tcp6"),
    ]
    sockets.sort()
    return tuple(sockets)


def read_socket_fds(proc_root: Path, pid: int) -> dict[int, tuple[int, ...]]:
    fd_root = proc_root / str(pid) / "fd"
    try:
        entries = sorted(fd_root.iterdir(), key=lambda entry: entry.name)
    except FileNotFoundError as error:
        message = f"pid {pid} fd directory disappeared"
        raise _failure(message, code="proc-entry-churn", retryable=True) from error
    except PermissionError as error:
        message = f"permission denied reading pid {pid} fd directory"
        raise _failure(
            message, code="proc-permission-denied", retryable=False
        ) from error
    except OSError as error:
        message = f"cannot list pid {pid} fd directory: {error}"
        raise _failure(message, code="proc-read-failed", retryable=False) from error
    owned: dict[int, list[int]] = {}
    for entry in entries:
        if not entry.name.isdecimal():
            continue
        try:
            target = os.readlink(entry)
        except FileNotFoundError:
            continue
        except PermissionError as error:
            message = f"permission denied reading pid {pid} fd {entry.name}"
            raise _failure(
                message, code="proc-permission-denied", retryable=False
            ) from error
        except OSError as error:
            message = f"cannot read pid {pid} fd {entry.name}: {error}"
            raise _failure(message, code="proc-read-failed", retryable=False) from error
        match = _SOCKET_LINK.fullmatch(target)
        if match is None:
            continue
        inode = _parse_decimal(match.group("inode"), "socket inode")
        owned.setdefault(inode, []).append(int(entry.name))
    return {inode: tuple(sorted(fds)) for inode, fds in sorted(owned.items())}


def _read_process_table(
    proc_root: Path,
    backend: ProcessIdentity,
    relevant_socket_inodes: frozenset[int],
) -> dict[int, ProcessIdentity]:
    identities = {backend.pid: backend}
    try:
        entries = sorted(proc_root.iterdir(), key=lambda entry: entry.name)
    except OSError as error:
        message = f"cannot enumerate proc root {proc_root}: {error}"
        raise _failure(message, code="proc-read-failed", retryable=False) from error
    for entry in entries:
        if not entry.name.isdecimal() or int(entry.name) == backend.pid:
            continue
        pid = int(entry.name)
        try:
            identity = read_process_identity(proc_root, pid)
        except RetryableEvidence:
            continue
        except StableEvidenceError as error:
            try:
                executable = _read_link(entry / "exe", f"pid {pid} executable")
            except EvidenceFailure:
                continue
            exact_webkit = Path(executable).name == "WebKitNetworkProcess"
            try:
                owned = read_socket_fds(proc_root, pid)
            except EvidenceFailure:
                if exact_webkit:
                    raise
                continue
            if exact_webkit or relevant_socket_inodes.intersection(owned):
                raise error
            continue
        identities[pid] = identity
    return identities


def _is_descendant(
    pid: int,
    backend_pid: int,
    identities: Mapping[int, ProcessIdentity],
) -> bool:
    seen: set[int] = set()
    current = pid
    while current not in seen:
        seen.add(current)
        identity = identities.get(current)
        if identity is None:
            return False
        if identity.ppid == backend_pid:
            return True
        if identity.ppid <= 1:
            return False
        current = identity.ppid
    return False


def _inside_store(executable: str, store: Path) -> bool:
    executable_path = Path(executable)
    return executable_path != store and executable_path.is_relative_to(store)


def _partial_evidence(
    backend: ProcessIdentity,
    endpoint: Endpoint,
    sockets: Iterable[TcpSocket],
    backend_fds: Mapping[int, tuple[int, ...]],
    processes: Iterable[ProcessIdentity] = (),
    process_fds: Mapping[int, Mapping[int, tuple[int, ...]]] | None = None,
) -> JsonObject:
    matching_rows = sorted(
        row for row in sockets if endpoint in (row.local, row.remote)
    )[:32]
    relevant_processes = sorted(processes)[:32]
    owned_inodes: Mapping[int, Mapping[int, tuple[int, ...]]] = (
        {} if process_fds is None else process_fds
    )
    return {
        "backend": backend.to_json(),
        "backend_owned_inodes": list(sorted(backend_fds)[:64]),
        "candidate_owned_inodes": [
            {
                "inodes": list(sorted(owned_inodes.get(process.pid, {}))[:64]),
                "pid": process.pid,
            }
            for process in relevant_processes
        ],
        "candidate_processes": [process.to_json() for process in relevant_processes],
        "endpoint": endpoint.to_json(),
        "matching_port_rows": [row.to_json() for row in matching_rows],
    }


def _fingerprint(
    listener: TcpSocket,
    client: TcpSocket,
    server: TcpSocket,
    process: ProcessIdentity,
) -> str:
    identity = {
        "client": client.to_json(),
        "listener_inode": listener.inode,
        "network_pid": process.pid,
        "network_start_ticks": process.start_ticks,
        "server": server.to_json(),
    }
    canonical = json.dumps(identity, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode()).hexdigest()


def _verify_identity(
    proc_root: Path,
    expected: ProcessIdentity,
) -> None:
    observed = read_process_identity(proc_root, expected.pid)
    if observed != expected:
        message = f"pid {expected.pid} identity changed during snapshot"
        raise _failure(message, code="process-identity-churn", retryable=True)


def _verify_fd(proc_root: Path, pid: int, fd: int, inode: int) -> None:
    target = _read_link(proc_root / str(pid) / "fd" / str(fd), f"pid {pid} fd {fd}")
    if target != f"socket:[{inode}]":
        message = f"pid {pid} fd {fd} changed ownership during snapshot"
        raise _failure(message, code="socket-fd-churn", retryable=True)


def capture_snapshot(
    config: SnapshotConfig,
    *,
    before_revalidation: Callable[[], None] | None = None,
) -> SnapshotEvidence:
    try:
        normalized_address = str(ipaddress.ip_address(config.address))
    except ValueError as error:
        message = f"backend address is invalid: {config.address!r}"
        raise _failure(
            message, code="configuration-invalid", retryable=False
        ) from error
    if config.backend_pid <= 1 or config.backend_pgid <= 1:
        message = "backend pid and pgid must be greater than one"
        raise _failure(message, code="configuration-invalid", retryable=False)
    if not 1 <= config.port <= 65535:
        message = f"backend port is out of range: {config.port}"
        raise _failure(message, code="configuration-invalid", retryable=False)
    if not config.webkit_store.is_absolute():
        message = f"WebKit store output is not absolute: {config.webkit_store}"
        raise _failure(message, code="configuration-invalid", retryable=False)

    backend = read_process_identity(config.proc_root, config.backend_pid)
    if backend.state in DEAD_PROCESS_STATES or backend.pgid != config.backend_pgid:
        message = f"backend pid {backend.pid} is not live in pgid {config.backend_pgid}"
        raise _failure(message, code="backend-identity-invalid", retryable=False)
    sockets = read_namespace_sockets(config.proc_root, backend.pid)
    backend_fds = read_socket_fds(config.proc_root, backend.pid)
    endpoint = Endpoint(normalized_address, config.port)
    partial = _partial_evidence(backend, endpoint, sockets, backend_fds)
    listener_rows = tuple(
        row for row in sockets if row.state == TCP_LISTEN and row.local == endpoint
    )
    if not listener_rows:
        message = f"listener {normalized_address}:{config.port} is not present"
        raise _failure(
            message,
            code="listener-unavailable",
            partial_evidence=partial,
            retryable=True,
        )
    if len(listener_rows) != 1:
        message = (
            f"listener {normalized_address}:{config.port} has {len(listener_rows)} rows"
        )
        raise _failure(
            message,
            code="listener-not-unique",
            partial_evidence=partial,
            retryable=False,
        )
    listener_row = listener_rows[0]
    listener_fds = backend_fds.get(listener_row.inode, ())
    if not listener_fds:
        message = f"backend pid {backend.pid} does not own listener inode {listener_row.inode}"
        raise _failure(
            message,
            code="listener-unowned",
            partial_evidence=partial,
            retryable=False,
        )
    listener = OwnedSocket(listener_fds[0], listener_row)

    established_clients = tuple(
        row
        for row in sockets
        if row.state == TCP_ESTABLISHED
        and row.family == listener_row.family
        and row.remote == endpoint
    )
    identities = _read_process_table(
        config.proc_root,
        backend,
        frozenset(row.inode for row in established_clients),
    )
    relevant_processes = tuple(
        identity
        for identity in identities.values()
        if identity.pid != backend.pid
        and (
            identity.pgid == backend.pgid
            or Path(identity.executable).name == "WebKitNetworkProcess"
            or _is_descendant(identity.pid, backend.pid, identities)
        )
    )
    relevant_fds: dict[int, dict[int, tuple[int, ...]]] = {}
    all_process_fds: dict[int, dict[int, tuple[int, ...]]] = {backend.pid: backend_fds}
    for identity in sorted(identities.values()):
        if identity.pid == backend.pid:
            continue
        try:
            owned = read_socket_fds(config.proc_root, identity.pid)
        except RetryableEvidence:
            continue
        except StableEvidenceError:
            if identity in relevant_processes:
                raise
            continue
        all_process_fds[identity.pid] = owned
        if identity in relevant_processes:
            relevant_fds[identity.pid] = owned
    partial = _partial_evidence(
        backend,
        endpoint,
        sockets,
        backend_fds,
        relevant_processes,
        relevant_fds,
    )
    if not established_clients:
        message = "no established client row to the backend listener is available"
        raise _failure(
            message,
            code="connection-unavailable",
            partial_evidence=partial,
            retryable=True,
        )

    owned_clients: list[
        tuple[ProcessIdentity, dict[int, tuple[int, ...]], TcpSocket]
    ] = []
    invalid_owners: list[tuple[str, str]] = []
    blocking_webkit_failures: list[tuple[str, str]] = []
    transient_ownership = False
    for client_row in established_clients:
        owners = [
            (identities[pid], owned)
            for pid, owned in sorted(all_process_fds.items())
            if client_row.inode in owned
        ]
        if not owners:
            transient_ownership = True
            continue
        if len(owners) != 1:
            invalid_owners.append(
                (
                    "client-owner-not-unique",
                    f"client socket inode {client_row.inode} has {len(owners)} owners",
                )
            )
            continue
        identity, owned = owners[0]
        executable_name = Path(identity.executable).name
        owner_relevant = (
            identity.pgid == backend.pgid
            or executable_name == "WebKitNetworkProcess"
            or _is_descendant(identity.pid, backend.pid, identities)
        )
        invalid: tuple[str, str] | None = None
        if executable_name != "WebKitNetworkProcess":
            if not owner_relevant:
                continue
            invalid = (
                "client-owner-executable-invalid",
                f"client socket inode {client_row.inode} owner is {executable_name!r}",
            )
        elif identity.state in DEAD_PROCESS_STATES:
            invalid = (
                "network-process-dead",
                f"WebKit network process pid {identity.pid} is not live",
            )
        elif identity.pgid != backend.pgid:
            invalid = (
                "network-process-pgid-invalid",
                f"WebKit network process pid {identity.pid} has wrong pgid",
            )
        elif not _inside_store(identity.executable, config.webkit_store):
            invalid = (
                "network-process-store-invalid",
                f"WebKit network process pid {identity.pid} has wrong store output",
            )
        elif not _is_descendant(identity.pid, backend.pid, identities):
            invalid = (
                "network-process-ancestry-invalid",
                f"WebKit network process pid {identity.pid} is not a backend descendant",
            )
        if invalid is not None:
            invalid_owners.append(invalid)
            if executable_name == "WebKitNetworkProcess":
                blocking_webkit_failures.append(invalid)
            continue
        owned_clients.append((identity, owned, client_row))

    triples: list[ConnectionTriple] = []
    selected_fds: set[tuple[int, int, int]] = {
        (backend.pid, listener.fd, listener.socket.inode)
    }
    missing_reverse = False
    for candidate, candidate_fds, client_row in owned_clients:
        matching_reverse = tuple(
            row
            for row in sockets
            if row.state == TCP_ESTABLISHED
            and row.family == client_row.family
            and row.local == endpoint
            and row.remote == client_row.local
        )
        reverse_rows = tuple(
            row for row in matching_reverse if row.inode in backend_fds
        )
        if not reverse_rows:
            missing_reverse = True
            continue
        if len(reverse_rows) != 1:
            invalid_owners.append(
                (
                    "reverse-peer-not-unique",
                    f"client socket inode {client_row.inode} has multiple reverse peers",
                )
            )
            continue
        server_row = reverse_rows[0]
        client = OwnedSocket(candidate_fds[client_row.inode][0], client_row)
        server = OwnedSocket(backend_fds[server_row.inode][0], server_row)
        triples.append(
            ConnectionTriple(
                client=client,
                fingerprint=_fingerprint(
                    listener_row,
                    client_row,
                    server_row,
                    candidate,
                ),
                listener=listener,
                network_process=candidate,
                server=server,
            )
        )
        selected_fds.add((candidate.pid, client.fd, client.socket.inode))
        selected_fds.add((backend.pid, server.fd, server.socket.inode))

    if blocking_webkit_failures:
        code, message = sorted(blocking_webkit_failures)[0]
        raise _failure(
            message,
            code=code,
            partial_evidence=partial,
            retryable=False,
        )
    if not triples:
        if invalid_owners:
            code, message = sorted(invalid_owners)[0]
            raise _failure(
                message,
                code=code,
                partial_evidence=partial,
                retryable=False,
            )
        code = (
            "reverse-peer-unavailable" if missing_reverse else "connection-unavailable"
        )
        message = (
            "no complete WebKit client/backend accepted connection triple is available"
        )
        if transient_ownership:
            code = "client-owner-unavailable"
        raise _failure(
            message,
            code=code,
            partial_evidence=partial,
            retryable=True,
        )

    candidates = tuple(sorted({triple.network_process for triple in triples}))

    if before_revalidation is not None:
        before_revalidation()
    _verify_identity(config.proc_root, backend)
    for candidate in candidates:
        _verify_identity(config.proc_root, candidate)
    for pid, fd, inode in sorted(selected_fds):
        _verify_fd(config.proc_root, pid, fd, inode)

    unique_triples = {triple.fingerprint: triple for triple in triples}
    return SnapshotEvidence(
        backend=backend,
        candidate_webkit_processes=candidates,
        connections=tuple(unique_triples[key] for key in sorted(unique_triples)),
        listener=listener,
    )


def stable_fingerprints(
    first: Iterable[str],
    second: Iterable[str],
    excluded: Iterable[str] = (),
) -> tuple[str, ...]:
    return tuple(sorted((set(first) & set(second)) - set(excluded)))


def _success_document(evidence: JsonObject) -> JsonObject:
    return {
        "diagnostics": [],
        "evidence": evidence,
        "schema_version": SCHEMA_VERSION,
        "status": "ok",
    }


def _failure_document(error: EvidenceFailure) -> JsonObject:
    return {
        "diagnostics": [
            {
                "code": error.code,
                "message": str(error)[:512],
                "retryable": error.retryable,
            }
        ],
        "evidence": error.partial_evidence,
        "schema_version": SCHEMA_VERSION,
        "status": "retryable" if error.retryable else "error",
    }


def _write_document(document: JsonObject) -> None:
    sys.stdout.write(
        json.dumps(document, ensure_ascii=True, separators=(",", ":"), sort_keys=True)
    )
    sys.stdout.write("\n")


def _load_json_object(path: Path) -> JsonObject:
    try:
        value = cast("object", json.loads(path.read_text()))
    except (OSError, json.JSONDecodeError) as error:
        message = f"cannot load evidence JSON {path}: {error}"
        raise _failure(
            message, code="evidence-json-invalid", retryable=False
        ) from error
    if not isinstance(value, dict):
        message = f"evidence JSON is not an object: {path}"
        raise _failure(message, code="evidence-json-invalid", retryable=False)
    untyped = cast("dict[object, object]", value)
    if not all(isinstance(key, str) for key in untyped):
        message = f"evidence JSON has a non-string key: {path}"
        raise _failure(message, code="evidence-json-invalid", retryable=False)
    return cast("JsonObject", value)


def _fingerprints_from_document(document: Mapping[str, object]) -> tuple[str, ...]:
    evidence = document.get("evidence")
    if not isinstance(evidence, dict):
        message = "evidence JSON does not contain an evidence object"
        raise _failure(
            message,
            code="evidence-json-invalid",
            retryable=False,
        )
    typed_evidence = cast("dict[str, object]", evidence)
    fingerprints = typed_evidence.get("fingerprints")
    if not isinstance(fingerprints, list):
        message = "evidence JSON does not contain string fingerprints"
        raise _failure(
            message,
            code="evidence-json-invalid",
            retryable=False,
        )
    fingerprint_values = cast("list[object]", fingerprints)
    if not all(isinstance(value, str) for value in fingerprint_values):
        message = "evidence JSON does not contain string fingerprints"
        raise _failure(
            message,
            code="evidence-json-invalid",
            retryable=False,
        )
    return tuple(cast("list[str]", fingerprint_values))


def _build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    snapshot = subparsers.add_parser("snapshot")
    snapshot.add_argument("--proc-root", type=Path, default=Path("/proc"))
    snapshot.add_argument("--backend-pid", type=int, required=True)
    snapshot.add_argument("--backend-pgid", type=int, required=True)
    snapshot.add_argument("--address", required=True)
    snapshot.add_argument("--port", type=int, required=True)
    snapshot.add_argument("--webkit-store", type=Path, required=True)
    compare = subparsers.add_parser("compare")
    compare.add_argument("--first", type=Path, required=True)
    compare.add_argument("--second", type=Path, required=True)
    compare.add_argument("--exclude", action="append", default=[])
    return parser


def _run_snapshot(namespace: argparse.Namespace) -> JsonObject:
    config = SnapshotConfig(
        address=cast("str", namespace.address),
        backend_pgid=cast("int", namespace.backend_pgid),
        backend_pid=cast("int", namespace.backend_pid),
        port=cast("int", namespace.port),
        proc_root=cast("Path", namespace.proc_root),
        webkit_store=cast("Path", namespace.webkit_store),
    )
    return _success_document(capture_snapshot(config).to_json())


def _run_compare(namespace: argparse.Namespace) -> JsonObject:
    first = _load_json_object(cast("Path", namespace.first))
    second = _load_json_object(cast("Path", namespace.second))
    excluded = cast("list[str]", namespace.exclude)
    fingerprints = stable_fingerprints(
        _fingerprints_from_document(first),
        _fingerprints_from_document(second),
        excluded,
    )
    if not fingerprints:
        message = "no unexcluded connection fingerprint is present in both snapshots"
        raise _failure(
            message,
            code="stable-connection-unavailable",
            retryable=True,
        )
    return _success_document({"fingerprints": list(fingerprints)})


def main(argv: Sequence[str] | None = None) -> int:
    namespace = _build_parser().parse_args(argv)
    try:
        if cast("str", namespace.command) == "snapshot":
            document = _run_snapshot(namespace)
        else:
            document = _run_compare(namespace)
    except EvidenceFailure as error:
        _write_document(_failure_document(error))
        return EXIT_RETRYABLE if error.retryable else EXIT_ERROR
    _write_document(document)
    return EXIT_OK


if __name__ == "__main__":
    raise SystemExit(main())
