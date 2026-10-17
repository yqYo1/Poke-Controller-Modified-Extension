from __future__ import annotations

import json
from dataclasses import dataclass, field, replace
from pathlib import Path

import pytest

from scripts.integration import ewmh_close_relay as relay


@dataclass(slots=True)
class FakeX11:
    events: list[relay.ClientMessage]
    protocols: tuple[int, ...] = (303,)
    calls: list[tuple[object, ...]] = field(default_factory=list)
    forwarded: list[relay.ClientMessage] = field(default_factory=list)

    def close(self) -> None:
        self.calls.append(("close",))

    def get_wm_protocols(self, window: int) -> tuple[int, ...]:
        self.calls.append(("get_wm_protocols", window))
        return self.protocols

    def intern_atom(self, name: str) -> int:
        atoms = {
            "_NET_CLOSE_WINDOW": 301,
            "WM_PROTOCOLS": 302,
            "WM_DELETE_WINDOW": 303,
        }
        self.calls.append(("intern_atom", name))
        return atoms[name]

    def next_event(self) -> relay.ClientMessage:
        self.calls.append(("next_event",))
        return self.events.pop(0)

    def pending(self) -> int:
        self.calls.append(("pending",))
        return len(self.events)

    def root_window(self) -> int:
        self.calls.append(("root_window",))
        return 42

    def select_input(self, window: int, event_mask: int) -> None:
        self.calls.append(("select_input", window, event_mask))

    def send_client_message(
        self,
        window: int,
        event: relay.ClientMessage,
    ) -> bool:
        self.calls.append(("send_client_message", window))
        self.forwarded.append(event)
        return True

    def sync_checked(self, phase: str) -> None:
        self.calls.append(("sync_checked", phase))


def close_request(
    *,
    data: tuple[int, int, int, int, int] = (0, 0, 0, 0, 0),
    target: int = 1001,
) -> relay.ClientMessage:
    return relay.ClientMessage(
        data=data,
        format=32,
        message_type=301,
        send_event=True,
        type=relay.CLIENT_MESSAGE,
        window=target,
    )


def config(tmp_path: Path, *, timeout_seconds: float = 1.0) -> relay.RelayConfig:
    return relay.RelayConfig(
        display=":91",
        lib_x11=Path("/nix/store/example-libx11/lib/libX11.so.6.4.0"),
        ready_file=tmp_path / "relay.ready.json",
        target_window=1001,
        timeout_seconds=timeout_seconds,
        xauthority=tmp_path / "Xauthority",
    )


def test_notify_subscription_handshake_and_exact_forward(tmp_path: Path) -> None:
    api = FakeX11(events=[close_request()])

    evidence = relay.relay_close_request(config(tmp_path), api)

    ready = json.loads((tmp_path / "relay.ready.json").read_text())
    assert ready == {
        "display": ":91",
        "event_mask": relay.SUBSTRUCTURE_NOTIFY_MASK,
        "protocols": {
            "advertised_atom_ids": [303],
            "wm_delete_window_advertised": True,
        },
        "root_window": 42,
        "schema_version": relay.SCHEMA_VERSION,
        "status": "ready",
        "target_window": 1001,
        "xauthority": str(tmp_path / "Xauthority"),
    }
    assert api.calls == [
        ("root_window",),
        ("intern_atom", "_NET_CLOSE_WINDOW"),
        ("intern_atom", "WM_PROTOCOLS"),
        ("intern_atom", "WM_DELETE_WINDOW"),
        ("get_wm_protocols", 1001),
        ("select_input", 42, 1 << 19),
        ("sync_checked", "subscription"),
        ("pending",),
        ("next_event",),
        ("get_wm_protocols", 1001),
        ("send_client_message", 1001),
        ("sync_checked", "forward"),
    ]
    assert api.forwarded == [
        relay.ClientMessage(
            data=(303, relay.CURRENT_TIME, 0, 0, 0),
            format=32,
            message_type=302,
            send_event=True,
            type=relay.CLIENT_MESSAGE,
            window=1001,
        )
    ]
    assert evidence["event_mask"] == 1 << 19
    assert evidence["received"] == {
        "data": [0, 0, 0, 0, 0],
        "format": 32,
        "message_type": "_NET_CLOSE_WINDOW",
        "received_on_root": 42,
        "send_event": True,
        "target_window": 1001,
        "type": "ClientMessage",
    }
    assert evidence["forwarded"] == {
        "data": [303, 0, 0, 0, 0],
        "format": 32,
        "message_type": "WM_PROTOCOLS",
        "protocol": "WM_DELETE_WINDOW",
        "send_count": 1,
        "target_window": 1001,
        "xsync_succeeded": True,
    }


@pytest.mark.parametrize(
    "candidate",
    [
        close_request(target=1002),
        close_request(data=(1, 0, 0, 0, 0)),
        relay.ClientMessage(
            data=(0, 0, 0, 0, 0),
            format=32,
            message_type=301,
            send_event=False,
            type=relay.CLIENT_MESSAGE,
            window=1001,
        ),
    ],
)
def test_noncanonical_close_request_is_rejected(
    tmp_path: Path,
    candidate: relay.ClientMessage,
) -> None:
    api = FakeX11(events=[candidate])

    with pytest.raises(relay.RelayFailure, match="pinned xdotool semantics") as raised:
        relay.relay_close_request(config(tmp_path), api)

    assert raised.value.code == "close-request-invalid"
    assert api.forwarded == []


def test_delete_protocol_is_required_before_ready_and_forward(tmp_path: Path) -> None:
    missing_initial = FakeX11(events=[close_request()], protocols=())
    with pytest.raises(relay.RelayFailure) as initial:
        relay.relay_close_request(config(tmp_path), missing_initial)
    assert initial.value.code == "delete-protocol-unavailable"
    assert not (tmp_path / "relay.ready.json").exists()

    @dataclass(slots=True)
    class ChangingProtocols(FakeX11):
        protocol_reads: int = 0

        def get_wm_protocols(self, window: int) -> tuple[int, ...]:
            self.protocol_reads += 1
            self.calls.append(("get_wm_protocols", window))
            return (303,) if self.protocol_reads == 1 else ()

    changed = ChangingProtocols(events=[close_request()])
    with pytest.raises(relay.RelayFailure) as forward:
        relay.relay_close_request(
            replace(
                config(tmp_path),
                ready_file=tmp_path / "relay-second.ready.json",
            ),
            changed,
        )
    assert forward.value.code == "delete-protocol-changed"
    assert changed.forwarded == []


def test_wait_is_bounded_when_no_request_arrives(tmp_path: Path) -> None:
    api = FakeX11(events=[])
    now = 0.0

    def monotonic() -> float:
        return now

    def sleeper(duration: float) -> None:
        nonlocal now
        now += duration

    with pytest.raises(relay.RelayFailure) as raised:
        relay.relay_close_request(
            config(tmp_path, timeout_seconds=0.02),
            api,
            monotonic=monotonic,
            sleeper=sleeper,
        )

    assert raised.value.code == "close-request-timeout"
    assert now == pytest.approx(0.02)
    assert api.forwarded == []


def test_relay_boundary_has_no_direct_window_destruction() -> None:
    repository = Path(__file__).resolve().parents[2]
    helper = (repository / "scripts/integration/ewmh_close_relay.py").read_text()
    gate = (repository / "scripts/integration/ui_package_check.sh").read_text()
    close_start = gate.index('start_close_request_relay "$root"')
    close_request = gate.index(
        'xdotool windowquit "$desktop_initial_window"',
        close_start,
    )

    for forbidden in (
        "XDestroyWindow",
        "XKillClient",
        "SubstructureRedirectMask",
    ):
        assert forbidden not in helper
    assert "xdotool windowclose" not in gate
    assert "xdotool windowkill" not in gate
    assert gate.count('xdotool windowquit "$desktop_initial_window"') == 1
    assert close_start < close_request
    assert 'env DISPLAY="$desktop_display" XAUTHORITY="$desktop_xauthority"' in gate
