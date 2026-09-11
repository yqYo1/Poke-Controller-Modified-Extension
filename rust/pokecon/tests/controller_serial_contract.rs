use std::sync::Arc;

use pokecon::integration_test_support::device::controller::{Button, ControllerState};
use pokecon::integration_test_support::device::input::{
    ApplyResult, InputArbiter, InputEvent, InputGeneration, InputPriority, InputSequence,
    InputSnapshot, InputSourceId, InputSourceKind, MouseButtons, PressState,
};
use pokecon::integration_test_support::device::serial::{
    ControllerFormat, SerialConfig, SerialManager, VirtualOpenPlan, VirtualSerialBackend,
    VirtualSerialEndpoint,
};

fn snapshot(generation: &str, pressed: Button) -> InputSnapshot {
    let mut state = ControllerState::NEUTRAL;
    state.buttons.set(pressed, true);
    InputSnapshot {
        generation: InputGeneration::new(generation).unwrap(),
        sequence: InputSequence::zero(),
        keyboard_keys: Vec::new(),
        mouse_buttons: MouseButtons::default(),
        buttons: state.buttons,
        hat: state.hat,
        left_stick: state.left_stick,
        right_stick: state.right_stick,
        touch: state.touch,
    }
}

#[tokio::test]
async fn worker_and_websocket_disconnects_reach_neutral_loopback_state() {
    let backend = VirtualSerialBackend::default();
    let endpoint = VirtualSerialEndpoint::new();
    backend
        .push_plan(VirtualOpenPlan::Success(endpoint.clone()))
        .await;
    let serial = SerialManager::new(Arc::new(backend));
    serial
        .apply_config(SerialConfig::new("loopback", 9600, ControllerFormat::Default).unwrap())
        .await
        .unwrap();

    let worker = InputSourceId::new("worker").unwrap();
    let websocket = InputSourceId::new("websocket").unwrap();
    let mut arbiter = InputArbiter::default();
    for (source, generation, kind, priority, button) in [
        (
            worker.clone(),
            "worker-1",
            InputSourceKind::UserScript,
            InputPriority::USER_SCRIPT,
            Button::A,
        ),
        (
            websocket.clone(),
            "websocket-1",
            InputSourceKind::BrowserGamepad,
            InputPriority::BROWSER_GAMEPAD,
            Button::B,
        ),
    ] {
        arbiter.begin_generation(
            source.clone(),
            kind,
            priority,
            InputGeneration::new(generation).unwrap(),
        );
        arbiter
            .apply_snapshot(&source, snapshot(generation, button))
            .unwrap();
    }
    serial
        .send_controller_state(arbiter.output())
        .await
        .unwrap();
    assert!(arbiter.output().buttons.a && arbiter.output().buttons.b);

    arbiter.disconnect_source(&worker);
    serial
        .send_controller_state(arbiter.output())
        .await
        .unwrap();
    assert!(!arbiter.output().buttons.a && arbiter.output().buttons.b);

    arbiter.disconnect_source(&websocket);
    serial
        .send_controller_state(arbiter.output())
        .await
        .unwrap();
    assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    let written = endpoint.written().await;
    assert!(
        std::str::from_utf8(&written)
            .unwrap()
            .ends_with("0x000000 8\r\n")
    );
}

#[tokio::test]
async fn old_generation_cannot_reassert_after_route_switch() {
    let source = InputSourceId::new("websocket").unwrap();
    let old = InputGeneration::new("old").unwrap();
    let mut arbiter = InputArbiter::default();
    arbiter.begin_generation(
        source.clone(),
        InputSourceKind::BrowserGamepad,
        InputPriority::BROWSER_GAMEPAD,
        old.clone(),
    );
    arbiter
        .apply_snapshot(&source, snapshot("old", Button::A))
        .unwrap();
    arbiter.begin_generation(
        source.clone(),
        InputSourceKind::BrowserGamepad,
        InputPriority::BROWSER_GAMEPAD,
        InputGeneration::new("new").unwrap(),
    );
    assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
    assert_eq!(
        arbiter
            .apply_event(
                &source,
                &old,
                InputSequence::new("1").unwrap(),
                InputEvent::ControllerButton {
                    button: Button::A,
                    state: PressState::Pressed,
                },
            )
            .unwrap(),
        ApplyResult::IgnoredOldGeneration
    );
    assert_eq!(arbiter.output(), ControllerState::NEUTRAL);
}
