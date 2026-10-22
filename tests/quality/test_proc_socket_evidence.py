from __future__ import annotations

import ipaddress
import json
import os
from dataclasses import dataclass
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from pathlib import Path

from scripts.integration.proc_socket_evidence import (
    EXIT_ERROR,
    EXIT_OK,
    EXIT_RETRYABLE,
    RetryableEvidence,
    SnapshotConfig,
    StableEvidenceError,
    capture_snapshot,
    main,
    parse_kernel_endpoint,
    stable_fingerprints,
)

TCP_HEADER = (
    "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when "
    "retrnsmt   uid  timeout inode\n"
)


def kernel_address(address: str, family: str) -> str:
    parsed = ipaddress.ip_address(address)
    if family == "tcp4":
        assert isinstance(parsed, ipaddress.IPv4Address)
        return parsed.packed[::-1].hex().upper()
    assert isinstance(parsed, ipaddress.IPv6Address)
    return "".join(
        parsed.packed[offset : offset + 4][::-1].hex().upper()
        for offset in range(0, 16, 4)
    )


def tcp_row(
    slot: int,
    family: str,
    local_address: str,
    local_port: int,
    remote_address: str,
    remote_port: int,
    state: str,
    inode: int,
) -> str:
    local = kernel_address(local_address, family)
    remote = kernel_address(remote_address, family)
    return (
        f" {slot:3d}: {local}:{local_port:04X} {remote}:{remote_port:04X} "
        f"{state} 00000000:00000000 00:00000000 00000000 1000 0 {inode}\n"
    )


def process_stat(
    pid: int,
    name: str,
    *,
    ppid: int,
    pgid: int,
    start_ticks: int,
    state: str = "S",
) -> str:
    fields = [state, str(ppid), str(pgid), *("0" for _index in range(16))]
    fields.append(str(start_ticks))
    return f"{pid} ({name}) {' '.join(fields)}\n"


@dataclass(slots=True)
class FakeProc:
    backend_executable: Path
    backend_pgid: int
    backend_pid: int
    client_inode: int
    client_pid: int
    listener_inode: int
    root: Path
    server_inode: int
    webkit_store: Path

    def write_process(
        self,
        pid: int,
        executable: Path,
        *,
        name: str,
        ppid: int,
        pgid: int,
        start_ticks: int,
        state: str = "S",
    ) -> None:
        process = self.root / str(pid)
        (process / "fd").mkdir(parents=True, exist_ok=True)
        executable.parent.mkdir(parents=True, exist_ok=True)
        executable.touch(exist_ok=True)
        (process / "stat").write_text(
            process_stat(
                pid,
                name,
                ppid=ppid,
                pgid=pgid,
                start_ticks=start_ticks,
                state=state,
            )
        )
        link = process / "exe"
        link.unlink(missing_ok=True)
        link.symlink_to(executable)

    def write_fd(self, pid: int, fd: int, inode: int) -> None:
        link = self.root / str(pid) / "fd" / str(fd)
        link.unlink(missing_ok=True)
        link.symlink_to(f"socket:[{inode}]")

    def write_tables(
        self,
        tcp4_rows: list[str],
        tcp6_rows: list[str],
    ) -> None:
        network = self.root / str(self.backend_pid) / "net"
        network.mkdir(parents=True, exist_ok=True)
        (network / "tcp").write_text(TCP_HEADER + "".join(tcp4_rows))
        (network / "tcp6").write_text(TCP_HEADER + "".join(tcp6_rows))


def make_fake_proc(
    tmp_path: Path,
    *,
    family: str = "tcp4",
    mapped_ipv6: bool = False,
) -> tuple[FakeProc, SnapshotConfig]:
    root = tmp_path / "proc"
    root.mkdir(parents=True)
    store = tmp_path / "nix/store/hash-webkitgtk"
    backend_executable = tmp_path / "nix/store/hash-pokecon/bin/pokecon"
    network_executable = store / "libexec/WebKitNetworkProcess"
    fake = FakeProc(
        backend_executable=backend_executable,
        backend_pgid=700,
        backend_pid=100,
        client_inode=2001,
        client_pid=200,
        listener_inode=1000,
        root=root,
        server_inode=1001,
        webkit_store=store,
    )
    fake.write_process(
        fake.backend_pid,
        backend_executable,
        name="pokecon",
        ppid=50,
        pgid=fake.backend_pgid,
        start_ticks=10000,
    )
    fake.write_process(
        fake.client_pid,
        network_executable,
        name="WebKitNetworkProcess",
        ppid=fake.backend_pid,
        pgid=fake.backend_pgid,
        start_ticks=20000,
    )
    fake.write_fd(fake.backend_pid, 3, fake.listener_inode)
    fake.write_fd(fake.backend_pid, 4, fake.server_inode)
    fake.write_fd(fake.client_pid, 5, fake.client_inode)

    if family == "tcp6":
        backend_address = "::ffff:127.0.0.1" if mapped_ipv6 else "::1"
        client_address = "::ffff:127.0.0.1" if mapped_ipv6 else "::1"
        zero_address = "::"
    else:
        backend_address = "127.0.0.1"
        client_address = "127.0.0.1"
        zero_address = "0.0.0.0"  # noqa: S104 - encoded as a proc-table wildcard peer
    rows = [
        tcp_row(
            0,
            family,
            backend_address,
            8020,
            zero_address,
            0,
            "0A",
            fake.listener_inode,
        ),
        tcp_row(
            1,
            family,
            backend_address,
            8020,
            client_address,
            51000,
            "01",
            fake.server_inode,
        ),
        tcp_row(
            2,
            family,
            client_address,
            51000,
            backend_address,
            8020,
            "01",
            fake.client_inode,
        ),
    ]
    fake.write_tables(
        rows if family == "tcp4" else [], rows if family == "tcp6" else []
    )
    expected_address = "127.0.0.1" if mapped_ipv6 else backend_address
    return fake, SnapshotConfig(
        address=expected_address,
        backend_pgid=fake.backend_pgid,
        backend_pid=fake.backend_pid,
        port=8020,
        proc_root=fake.root,
        webkit_store=fake.webkit_store,
    )


def test_tcp4_snapshot_joins_listener_client_and_reverse_fd_ownership(
    tmp_path: Path,
) -> None:
    fake, config = make_fake_proc(tmp_path)

    evidence = capture_snapshot(config)

    assert evidence.backend.pid == fake.backend_pid
    assert evidence.listener.fd == 3
    assert evidence.listener.socket.inode == fake.listener_inode
    assert [process.pid for process in evidence.candidate_webkit_processes] == [
        fake.client_pid
    ]
    assert len(evidence.connections) == 1
    connection = evidence.connections[0]
    assert connection.client.fd == 5
    assert connection.client.socket.inode == fake.client_inode
    assert connection.server.fd == 4
    assert connection.server.socket.inode == fake.server_inode
    assert connection.client.socket.remote == connection.listener.socket.local
    assert connection.server.socket.remote == connection.client.socket.local
    assert len(connection.fingerprint) == 64
    assert capture_snapshot(config).to_json() == evidence.to_json()


def test_tcp6_and_ipv4_mapped_addresses_normalize_kernel_word_order(
    tmp_path: Path,
) -> None:
    _fake, mapped_config = make_fake_proc(
        tmp_path / "mapped", family="tcp6", mapped_ipv6=True
    )
    mapped = capture_snapshot(mapped_config)
    assert mapped.listener.socket.local.address == "127.0.0.1"
    assert mapped.listener.socket.family == "tcp6"

    encoded_loopback = kernel_address("::1", "tcp6")
    assert parse_kernel_endpoint(f"{encoded_loopback}:1F54", "tcp6").address == "::1"
    assert parse_kernel_endpoint("0100007F:1F54", "tcp4").address == "127.0.0.1"


def test_unowned_listener_namespace_row_is_a_stable_error(tmp_path: Path) -> None:
    fake, config = make_fake_proc(tmp_path)
    (fake.root / str(fake.backend_pid) / "fd" / "3").unlink()

    with pytest.raises(StableEvidenceError) as captured:
        capture_snapshot(config)

    assert captured.value.code == "listener-unowned"


@pytest.mark.parametrize(
    ("mutation", "expected_code"),
    [
        ("wrong_name", "client-owner-executable-invalid"),
        ("wrong_store", "network-process-store-invalid"),
        ("wrong_pgid", "network-process-pgid-invalid"),
        ("not_descendant", "network-process-ancestry-invalid"),
        ("dead", "network-process-dead"),
    ],
)
def test_invalid_network_process_identity_is_rejected(
    tmp_path: Path,
    mutation: str,
    expected_code: str,
) -> None:
    fake, config = make_fake_proc(tmp_path)
    executable = fake.webkit_store / "libexec/WebKitNetworkProcess"
    name = "WebKitNetworkProcess"
    ppid = fake.backend_pid
    pgid = fake.backend_pgid
    state = "S"
    if mutation == "wrong_name":
        executable = fake.webkit_store / "libexec/NotWebKitNetworkProcess"
        name = "NotWebKitNetworkProcess"
    elif mutation == "wrong_store":
        executable = tmp_path / "nix/store/other-webkit/libexec/WebKitNetworkProcess"
    elif mutation == "wrong_pgid":
        pgid += 1
    elif mutation == "not_descendant":
        ppid = 1
    else:
        state = "X"
    fake.write_process(
        fake.client_pid,
        executable,
        name=name,
        ppid=ppid,
        pgid=pgid,
        start_ticks=20000,
        state=state,
    )

    with pytest.raises(StableEvidenceError) as captured:
        capture_snapshot(config)

    assert captured.value.code == expected_code
    assert captured.value.partial_evidence is not None


def test_unowned_client_and_missing_reverse_peer_are_rejected(tmp_path: Path) -> None:
    fake, config = make_fake_proc(tmp_path)
    (fake.root / str(fake.client_pid) / "fd" / "5").unlink()
    with pytest.raises(RetryableEvidence) as unowned:
        capture_snapshot(config)
    assert unowned.value.code == "client-owner-unavailable"
    assert unowned.value.partial_evidence is not None

    fake.write_fd(fake.client_pid, 5, fake.client_inode)
    network = fake.root / str(fake.backend_pid) / "net" / "tcp"
    rows = network.read_text().splitlines(keepends=True)
    network.write_text("".join([rows[0], rows[1], rows[3]]))
    with pytest.raises(RetryableEvidence) as missing_reverse:
        capture_snapshot(config)
    assert missing_reverse.value.code == "reverse-peer-unavailable"


def test_reverse_peer_must_use_the_same_tcp_family(tmp_path: Path) -> None:
    fake, config = make_fake_proc(tmp_path)
    tcp4 = (
        (fake.root / str(fake.backend_pid) / "net" / "tcp")
        .read_text()
        .splitlines(keepends=True)
    )
    cross_family_server = tcp_row(
        0,
        "tcp6",
        "::ffff:127.0.0.1",
        8020,
        "::ffff:127.0.0.1",
        51000,
        "01",
        fake.server_inode,
    )
    fake.write_tables([tcp4[1], tcp4[3]], [cross_family_server])

    with pytest.raises(RetryableEvidence) as captured:
        capture_snapshot(config)

    assert captured.value.code == "reverse-peer-unavailable"


def test_unrelated_owned_client_can_coexist_with_valid_webkit_triple(
    tmp_path: Path,
) -> None:
    fake, config = make_fake_proc(tmp_path)
    unrelated_pid = 300
    unrelated_client_inode = 3001
    unrelated_server_inode = 3002
    fake.write_process(
        unrelated_pid,
        tmp_path / "nix/store/hash-curl/bin/curl",
        name="curl",
        ppid=1,
        pgid=800,
        start_ticks=30000,
    )
    fake.write_fd(unrelated_pid, 6, unrelated_client_inode)
    fake.write_fd(fake.backend_pid, 7, unrelated_server_inode)
    network = fake.root / str(fake.backend_pid) / "net" / "tcp"
    network.write_text(
        network.read_text()
        + tcp_row(
            3,
            "tcp4",
            "127.0.0.1",
            8020,
            "127.0.0.1",
            52000,
            "01",
            unrelated_server_inode,
        )
        + tcp_row(
            4,
            "tcp4",
            "127.0.0.1",
            52000,
            "127.0.0.1",
            8020,
            "01",
            unrelated_client_inode,
        )
    )

    evidence = capture_snapshot(config)

    assert len(evidence.connections) == 1
    assert evidence.connections[0].network_process.pid == fake.client_pid
    assert [process.pid for process in evidence.candidate_webkit_processes] == [
        fake.client_pid
    ]


def test_curl_only_endpoint_client_is_retryable_not_identity_invalid(
    tmp_path: Path,
) -> None:
    fake, config = make_fake_proc(tmp_path)
    fake.write_process(
        fake.client_pid,
        tmp_path / "nix/store/hash-curl/bin/curl",
        name="curl",
        ppid=1,
        pgid=800,
        start_ticks=20000,
    )

    with pytest.raises(RetryableEvidence) as captured:
        capture_snapshot(config)

    assert captured.value.code == "connection-unavailable"


def test_transient_webkit_client_and_curl_client_remain_retryable(
    tmp_path: Path,
) -> None:
    fake, config = make_fake_proc(tmp_path)
    (fake.root / str(fake.client_pid) / "fd" / "5").unlink()
    curl_pid = 300
    curl_client_inode = 3001
    curl_server_inode = 3002
    fake.write_process(
        curl_pid,
        tmp_path / "nix/store/hash-curl/bin/curl",
        name="curl",
        ppid=1,
        pgid=800,
        start_ticks=30000,
    )
    fake.write_fd(curl_pid, 6, curl_client_inode)
    fake.write_fd(fake.backend_pid, 7, curl_server_inode)
    network = fake.root / str(fake.backend_pid) / "net" / "tcp"
    network.write_text(
        network.read_text()
        + tcp_row(
            3,
            "tcp4",
            "127.0.0.1",
            8020,
            "127.0.0.1",
            52000,
            "01",
            curl_server_inode,
        )
        + tcp_row(
            4,
            "tcp4",
            "127.0.0.1",
            52000,
            "127.0.0.1",
            8020,
            "01",
            curl_client_inode,
        )
    )

    with pytest.raises(RetryableEvidence) as captured:
        capture_snapshot(config)

    assert captured.value.code == "client-owner-unavailable"


def test_invalid_webkit_shaped_owner_blocks_otherwise_valid_snapshot(
    tmp_path: Path,
) -> None:
    fake, config = make_fake_proc(tmp_path)
    invalid_pid = 201
    invalid_client_inode = 2011
    invalid_server_inode = 1011
    fake.write_process(
        invalid_pid,
        tmp_path / "nix/store/wrong-webkit/libexec/WebKitNetworkProcess",
        name="WebKitNetworkProcess",
        ppid=fake.backend_pid,
        pgid=fake.backend_pgid,
        start_ticks=20100,
    )
    fake.write_fd(invalid_pid, 8, invalid_client_inode)
    fake.write_fd(fake.backend_pid, 9, invalid_server_inode)
    network = fake.root / str(fake.backend_pid) / "net" / "tcp"
    network.write_text(
        network.read_text()
        + tcp_row(
            3,
            "tcp4",
            "127.0.0.1",
            8020,
            "127.0.0.1",
            53000,
            "01",
            invalid_server_inode,
        )
        + tcp_row(
            4,
            "tcp4",
            "127.0.0.1",
            53000,
            "127.0.0.1",
            8020,
            "01",
            invalid_client_inode,
        )
    )

    with pytest.raises(StableEvidenceError) as captured:
        capture_snapshot(config)

    assert captured.value.code == "network-process-store-invalid"


def test_fingerprint_intersection_and_exclusion_are_deterministic() -> None:
    assert stable_fingerprints(["b", "a", "b"], ["c", "b", "a"]) == ("a", "b")
    assert stable_fingerprints(["b", "a"], ["a", "b"], ["a"]) == ("b",)
    assert stable_fingerprints(["a"], ["a"], ["a"]) == ()


@pytest.mark.parametrize("churn", ["start_ticks", "fd"])
def test_selected_process_identity_and_fd_are_revalidated(
    tmp_path: Path,
    churn: str,
) -> None:
    fake, config = make_fake_proc(tmp_path)

    def mutate() -> None:
        if churn == "start_ticks":
            fake.write_process(
                fake.client_pid,
                fake.webkit_store / "libexec/WebKitNetworkProcess",
                name="WebKitNetworkProcess",
                ppid=fake.backend_pid,
                pgid=fake.backend_pgid,
                start_ticks=20001,
            )
        else:
            fake.write_fd(fake.client_pid, 5, 9999)

    with pytest.raises(RetryableEvidence) as captured:
        capture_snapshot(config, before_revalidation=mutate)

    expected_code = (
        "process-identity-churn" if churn == "start_ticks" else "socket-fd-churn"
    )
    assert captured.value.code == expected_code


def test_cli_emits_deterministic_status_documents_and_compare_results(
    tmp_path: Path,
    capsys: pytest.CaptureFixture[str],
) -> None:
    fake, config = make_fake_proc(tmp_path)
    arguments = [
        "snapshot",
        "--proc-root",
        os.fspath(config.proc_root),
        "--backend-pid",
        str(config.backend_pid),
        "--backend-pgid",
        str(config.backend_pgid),
        "--address",
        config.address,
        "--port",
        str(config.port),
        "--webkit-store",
        os.fspath(config.webkit_store),
    ]
    assert main(arguments) == EXIT_OK
    first_output = capsys.readouterr().out
    first = json.loads(first_output)
    assert first["status"] == "ok"
    assert (
        first_output == json.dumps(first, sort_keys=True, separators=(",", ":")) + "\n"
    )

    first_path = tmp_path / "first.json"
    second_path = tmp_path / "second.json"
    first_path.write_text(first_output)
    second_path.write_text(first_output)
    assert (
        main(
            [
                "compare",
                "--first",
                os.fspath(first_path),
                "--second",
                os.fspath(second_path),
            ]
        )
        == EXIT_OK
    )
    compared = json.loads(capsys.readouterr().out)
    assert compared["status"] == "ok"
    assert compared["evidence"]["fingerprints"] == first["evidence"]["fingerprints"]

    fingerprint = first["evidence"]["fingerprints"][0]
    assert (
        main(
            [
                "compare",
                "--first",
                os.fspath(first_path),
                "--second",
                os.fspath(second_path),
                "--exclude",
                fingerprint,
            ]
        )
        == EXIT_RETRYABLE
    )
    retry = json.loads(capsys.readouterr().out)
    assert retry["status"] == "retryable"

    (fake.root / str(fake.backend_pid) / "fd" / "3").unlink()
    assert main(arguments) == EXIT_ERROR
    error = json.loads(capsys.readouterr().out)
    assert error["status"] == "error"
    assert error["evidence"]["backend_owned_inodes"] == [fake.server_inode]


def test_retryable_cli_diagnostic_is_bounded(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    missing_root = tmp_path / "missing-proc"
    status = main(
        [
            "snapshot",
            "--proc-root",
            os.fspath(missing_root),
            "--backend-pid",
            "100",
            "--backend-pgid",
            "700",
            "--address",
            "127.0.0.1",
            "--port",
            "8020",
            "--webkit-store",
            "/nix/store/hash-webkit",
        ]
    )

    assert status == EXIT_RETRYABLE
    document = json.loads(capsys.readouterr().out)
    assert document["status"] == "retryable"
    assert len(document["diagnostics"][0]["message"]) <= 512
