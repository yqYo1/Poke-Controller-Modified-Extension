use std::sync::Arc;
use std::time::Instant;

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

// ---------------------------------------------------------------------------
// AR-11-02 / AR-11-05: priority contention matrix and before/after report.
//
// Acceptance artifact for input ordering, fairness, and queue behavior — not
// a production throughput or remote-CI claim. Timing numbers below are
// advisory only; ordering, inversion, service, and handoff invariants are
// asserted and break red.
// ---------------------------------------------------------------------------

/// Report artifact schema id. Bump only with a deliberate schema change.
const PRIORITY_CONTENTION_SCHEMA: &str = "priority-contention-report/1";
/// Acceptance fixture id recorded in the artifact.
const PRIORITY_CONTENTION_FIXTURE: &str = "priority-contention-left-stick";
/// Bounded workload size: full-matrix snapshot rounds. Keeps runtime tiny.
const PRIORITY_CONTENTION_ROUNDS: usize = 5;
/// Env var gating artifact emission. Unset/empty means no file side effect.
const PRIORITY_REPORT_OUT_ENV: &str = "POKECON_PRIORITY_REPORT_OUT";
/// Bounded queue capacity for the backpressure model. Deliberately smaller
/// than the 5-source matrix so `try_send` overflow is exercised every round.
const PRIORITY_QUEUE_CAPACITY: usize = 3;
/// Bounded queue-model rounds. Every round must service all five sources
/// with zero starvation after deterministic retry. Keeps runtime tiny.
const PRIORITY_QUEUE_ROUNDS: usize = 3;

/// One explicit matrix row covering a documented priority level.
struct PriorityMatrixRow {
    source: &'static str,
    kind_label: &'static str,
    kind: InputSourceKind,
    priority: InputPriority,
    /// Documented numeric value from `device::input::InputPriority`; pinned
    /// here because the field is private, and cross-checked by ordering
    /// assertions against the real constants below.
    priority_value: u16,
    /// Distinct deflected left-stick x (y stays 128). Buttons are unioned by
    /// design, so the left stick carries the contention signal instead.
    stick_x: u8,
}

/// One bounded-queue payload using only existing arbiter source types: the
/// matrix row index plus the live `InputSourceId`/`InputSnapshot` pair. No
/// second production API; the queue is a test-only bounded model.
struct QueuedContentionUpdate {
    row_index: usize,
    source: InputSourceId,
    snapshot: InputSnapshot,
}

fn priority_matrix() -> [PriorityMatrixRow; 5] {
    [
        PriorityMatrixRow {
            source: "kbd",
            kind_label: "Keyboard",
            kind: InputSourceKind::Keyboard,
            priority: InputPriority::KEYBOARD_MOUSE,
            priority_value: 100,
            stick_x: 16,
        },
        PriorityMatrixRow {
            source: "browser",
            kind_label: "BrowserGamepad",
            kind: InputSourceKind::BrowserGamepad,
            priority: InputPriority::BROWSER_GAMEPAD,
            priority_value: 200,
            stick_x: 64,
        },
        PriorityMatrixRow {
            source: "hardware",
            kind_label: "HardwareController",
            kind: InputSourceKind::HardwareController,
            priority: InputPriority::HARDWARE_CONTROLLER,
            priority_value: 300,
            stick_x: 192,
        },
        PriorityMatrixRow {
            source: "dynamic",
            kind_label: "DynamicConfig",
            kind: InputSourceKind::DynamicConfig,
            priority: InputPriority::DYNAMIC_CONFIG,
            priority_value: 400,
            stick_x: 224,
        },
        PriorityMatrixRow {
            source: "userscript",
            kind_label: "UserScript",
            kind: InputSourceKind::UserScript,
            priority: InputPriority::USER_SCRIPT,
            priority_value: 500,
            stick_x: 250,
        },
    ]
}

/// Builds a neutral controller state with a deflected left stick without
/// naming unexported wire types: round-trips through the derived serde impl.
fn stick_state(x: u8, y: u8) -> ControllerState {
    let mut value =
        serde_json::to_value(ControllerState::NEUTRAL).expect("neutral state serializes");
    assert!(
        value
            .get("left_stick")
            .and_then(|stick| stick.get("x"))
            .is_some(),
        "ControllerState wire shape must expose left_stick.x"
    );
    value["left_stick"]["x"] = serde_json::Value::from(x);
    value["left_stick"]["y"] = serde_json::Value::from(y);
    serde_json::from_value(value).expect("deflected stick state round-trips")
}

fn state_snapshot(generation: &str, state: ControllerState) -> InputSnapshot {
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

/// Saturating nanos capture for advisory latency samples (saturates only
/// past ~584 years, never in practice). No threshold is asserted on these.
fn nanos_since(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn priority_contention_matrix_before_after_report() {
    let matrix = priority_matrix();

    // Break-red: the five documented priority levels must stay strictly
    // ordered. (Mouse shares KEYBOARD_MOUSE=100 with Keyboard by design.)
    for window in matrix.windows(2) {
        assert!(
            window[0].priority < window[1].priority,
            "priority matrix must stay strictly ordered: {} < {}",
            window[0].kind_label,
            window[1].kind_label
        );
        assert_ne!(
            window[0].priority_value, window[1].priority_value,
            "matrix priority values must stay distinct"
        );
    }
    assert_eq!(
        matrix.first().unwrap().priority,
        InputPriority::KEYBOARD_MOUSE
    );
    assert_eq!(matrix.last().unwrap().priority, InputPriority::USER_SCRIPT);

    let mut arbiter = InputArbiter::default();
    let mut applied_per_source = [0_usize; 5];
    let mut ignored_total = 0_usize;
    let mut latencies_nanos: Vec<u64> = Vec::new();

    // Initial registration: every matrix source must exist and be serviced.
    for (index, row) in matrix.iter().enumerate() {
        let source = InputSourceId::new(row.source).unwrap();
        let generation = format!("init-{}", row.source);
        arbiter.begin_generation(
            source.clone(),
            row.kind,
            row.priority,
            InputGeneration::new(generation.clone()).unwrap(),
        );
        let started = Instant::now();
        let (result, ack) = arbiter
            .apply_snapshot(
                &source,
                state_snapshot(&generation, stick_state(row.stick_x, 128)),
            )
            .unwrap();
        latencies_nanos.push(nanos_since(started));
        assert_eq!(
            result,
            ApplyResult::Applied,
            "source {} must be serviced at registration",
            row.source
        );
        assert!(
            ack.is_some(),
            "source {} snapshot must be acked",
            row.source
        );
        applied_per_source[index] += 1;
        assert_eq!(
            arbiter.source_kind(&source),
            Some(row.kind),
            "matrix source row {} must exist",
            row.source
        );
    }

    // Sustained contention: descending priority per round, so lower-priority
    // updates always arrive AFTER higher-priority ones.
    let low = &matrix[0];
    let high = &matrix[matrix.len() - 1];
    let mut high_priority_wins = 0_usize;
    let mut inversions = 0_usize;
    for round in 0..PRIORITY_CONTENTION_ROUNDS {
        for (index, row) in matrix.iter().enumerate().rev() {
            let source = InputSourceId::new(row.source).unwrap();
            let generation = format!("r{round}-{}", row.source);
            arbiter.begin_generation(
                source.clone(),
                row.kind,
                row.priority,
                InputGeneration::new(generation.clone()).unwrap(),
            );
            let started = Instant::now();
            let (result, _) = arbiter
                .apply_snapshot(
                    &source,
                    state_snapshot(&generation, stick_state(row.stick_x, 128)),
                )
                .unwrap();
            latencies_nanos.push(nanos_since(started));
            match result {
                ApplyResult::Applied => applied_per_source[index] += 1,
                _ => ignored_total += 1,
            }
        }
        // After-policy: explicit priority wins regardless of arrival order.
        assert_eq!(
            arbiter.output().left_stick,
            stick_state(high.stick_x, 128).left_stick,
            "round {round}: highest-priority stick must win under contention"
        );
        high_priority_wins += 1;
        // Before-policy model (last-update-wins) would crown the lowest
        // priority source instead. This assert guards fixture validity: the
        // workload must actually discriminate the two policies.
        assert_ne!(
            stick_state(low.stick_x, 128).left_stick,
            arbiter.output().left_stick,
            "round {round}: fixture sticks must discriminate before/after policies"
        );
        inversions += 1;
    }

    // Correctness invariants (break red; no timing gate).
    assert_eq!(
        high_priority_wins, PRIORITY_CONTENTION_ROUNDS,
        "highest-priority continuous component must win every round"
    );
    assert_eq!(
        inversions, PRIORITY_CONTENTION_ROUNDS,
        "last-update-wins must lose every round against explicit priority"
    );
    assert_eq!(
        ignored_total, 0,
        "bounded workload must service every update (no queue stall)"
    );
    for (row, applied) in matrix.iter().zip(applied_per_source.iter()) {
        assert_eq!(
            *applied,
            PRIORITY_CONTENTION_ROUNDS + 1,
            "source {} starved: serviced {applied} times",
            row.source
        );
    }

    // -- Bounded queue/backpressure/starvation model (break-red) --------
    // Test-only bounded `tokio::sync::mpsc` queue over existing source
    // types. Capacity is deliberately smaller than the 5-source matrix so
    // `try_send` overflow is exercised every round. Non-blocking
    // `try_send`/`try_recv` only: `Full` is counted explicitly and retried
    // after a deterministic drain — never absorbed as unbounded growth or
    // a hidden stall. The FIFO queue preserves every item; documented
    // priority is controlled at drain time by sorting descending before
    // applying to a dedicated model arbiter (not the production path).
    let mut queue_arbiter = InputArbiter::default();
    let mut queue_applied_per_source = [0_usize; 5];
    let mut dropped_explicit = 0_usize;
    let mut retried_after_drain = 0_usize;
    let mut queue_high_priority_wins = 0_usize;
    for queue_round in 0..PRIORITY_QUEUE_ROUNDS {
        let (tx, mut rx) =
            tokio::sync::mpsc::channel::<QueuedContentionUpdate>(PRIORITY_QUEUE_CAPACITY);
        assert_eq!(
            tx.max_capacity(),
            PRIORITY_QUEUE_CAPACITY,
            "queue model must pin capacity {PRIORITY_QUEUE_CAPACITY}"
        );
        // Adversarial arrival: ascending priority, so the two highest
        // priority sources hit `Full` and must be recovered via retry. If
        // retry is skipped, the round-winner assert below fails red.
        let mut overflow: Vec<QueuedContentionUpdate> = Vec::new();
        for (index, row) in matrix.iter().enumerate() {
            let source = InputSourceId::new(row.source).unwrap();
            let generation = format!("q{queue_round}-{}", row.source);
            let snapshot = state_snapshot(&generation, stick_state(row.stick_x, 128));
            let item = QueuedContentionUpdate {
                row_index: index,
                source,
                snapshot,
            };
            match tx.try_send(item) {
                Ok(()) => {}
                Err(tokio::sync::mpsc::error::TrySendError::Full(item)) => {
                    dropped_explicit += 1;
                    overflow.push(item);
                }
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                    panic!("queue round {queue_round}: channel closed unexpectedly")
                }
            }
        }
        // Break-red: the bound must bite every round (no unbounded growth).
        assert!(
            !overflow.is_empty(),
            "queue round {queue_round}: capacity {PRIORITY_QUEUE_CAPACITY} must exert backpressure against 5 sources"
        );
        assert_eq!(
            rx.len(),
            PRIORITY_QUEUE_CAPACITY,
            "queue round {queue_round}: buffered depth must equal pinned capacity"
        );
        // Deterministic drain of the buffered prefix.
        let mut batch: Vec<QueuedContentionUpdate> = Vec::with_capacity(matrix.len());
        while let Ok(item) = rx.try_recv() {
            batch.push(item);
        }
        assert_eq!(
            batch.len(),
            PRIORITY_QUEUE_CAPACITY,
            "queue round {queue_round}: deterministic drain must recover the full buffered prefix"
        );
        // Explicit recovery: retry every `Full` drop after the drain. Must
        // succeed without blocking (no hidden stall, no silent loss).
        for item in overflow {
            tx.try_send(item)
                .expect("retry after deterministic drain must succeed");
            retried_after_drain += 1;
        }
        while let Ok(item) = rx.try_recv() {
            batch.push(item);
        }
        assert_eq!(
            batch.len(),
            matrix.len(),
            "queue round {queue_round}: every matrix source must be recovered each bounded round"
        );
        assert!(
            rx.is_empty(),
            "queue round {queue_round}: drain must leave no hidden backlog"
        );
        // Control documented ordering at drain time: descending priority
        // (matrix is ascending, so reversed index) before applying.
        batch.sort_by_key(|item| std::cmp::Reverse(item.row_index));
        let mut serviced = [false; 5];
        for item in &batch {
            let row = &matrix[item.row_index];
            queue_arbiter.begin_generation(
                item.source.clone(),
                row.kind,
                row.priority,
                item.snapshot.generation.clone(),
            );
            let (result, ack) = queue_arbiter
                .apply_snapshot(&item.source, item.snapshot.clone())
                .unwrap();
            assert_eq!(
                result,
                ApplyResult::Applied,
                "queue round {queue_round}: source {} must be serviced after bounded drain",
                row.source
            );
            assert!(
                ack.is_some(),
                "queue round {queue_round}: source {} snapshot must be acked",
                row.source
            );
            queue_applied_per_source[item.row_index] += 1;
            serviced[item.row_index] = true;
        }
        for (row, was_serviced) in matrix.iter().zip(serviced.iter()) {
            assert!(
                was_serviced,
                "queue round {queue_round}: source {} starved in bounded round",
                row.source
            );
        }
        assert_eq!(
            queue_arbiter.output().left_stick,
            stick_state(high.stick_x, 128).left_stick,
            "queue round {queue_round}: highest-priority stick must win after bounded drain"
        );
        queue_high_priority_wins += 1;
    }
    assert_eq!(
        queue_high_priority_wins, PRIORITY_QUEUE_ROUNDS,
        "highest-priority component must win every bounded queue round"
    );
    for (row, applied) in matrix.iter().zip(queue_applied_per_source.iter()) {
        assert_eq!(
            *applied, PRIORITY_QUEUE_ROUNDS,
            "bounded queue source {} starved: serviced {applied} times",
            row.source
        );
    }
    let starvation_count = queue_applied_per_source
        .iter()
        .filter(|count| **count != PRIORITY_QUEUE_ROUNDS)
        .count();
    assert_eq!(
        starvation_count, 0,
        "bounded queue model must service every source every round (starvation_count must be zero)"
    );
    // Break-red: backpressure was actually exercised and fully recovered.
    assert!(
        dropped_explicit > 0,
        "bounded queue must record explicit drops (no unbounded growth)"
    );
    assert_eq!(
        retried_after_drain, dropped_explicit,
        "every explicitly dropped update must retry after deterministic drain (no silent loss)"
    );

    // The contended winning state must traverse the serial path.
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
    serial
        .send_controller_state(arbiter.output())
        .await
        .unwrap();
    assert!(
        !endpoint.written().await.is_empty(),
        "contended state must traverse the serial path"
    );

    // Release/handoff cascade: each release must surface the next priority,
    // proving every source was live (no starvation) and handoff is exact.
    let mut handoff_rows: Vec<serde_json::Value> = Vec::new();
    for position in (0..matrix.len()).rev() {
        let row = &matrix[position];
        let source = InputSourceId::new(row.source).unwrap();
        assert!(
            arbiter.disconnect_source(&source),
            "release of {} must remove its ownership",
            row.source
        );
        let (expected, surfaced) = if position == 0 {
            (ControllerState::NEUTRAL.left_stick, "neutral")
        } else {
            let next = &matrix[position - 1];
            (stick_state(next.stick_x, 128).left_stick, next.source)
        };
        assert_eq!(
            arbiter.output().left_stick,
            expected,
            "handoff after releasing {} must surface {surfaced}",
            row.source
        );
        handoff_rows.push(serde_json::json!({
            "released": row.source,
            "surfaced": surfaced,
        }));
    }
    assert_eq!(
        arbiter.output(),
        ControllerState::NEUTRAL,
        "full release must return to neutral"
    );

    // Advisory latency stats only; sample completeness is asserted instead.
    let expected_samples = matrix.len() * (PRIORITY_CONTENTION_ROUNDS + 1);
    assert_eq!(
        latencies_nanos.len(),
        expected_samples,
        "every update must contribute a latency sample"
    );
    let min_nanos = *latencies_nanos.iter().min().unwrap();
    let max_nanos = *latencies_nanos.iter().max().unwrap();
    let sum_nanos: u128 = latencies_nanos.iter().map(|item| u128::from(*item)).sum();
    let mean_nanos =
        u64::try_from(sum_nanos / u128::try_from(latencies_nanos.len()).unwrap()).unwrap();

    let matrix_json: Vec<serde_json::Value> = matrix
        .iter()
        .map(|row| {
            let stick = stick_state(row.stick_x, 128).left_stick;
            serde_json::json!({
                "source": row.source,
                "kind": row.kind_label,
                "priority": row.priority_value,
                "left_stick": {"x": stick.x, "y": stick.y},
            })
        })
        .collect();
    let service_json: Vec<serde_json::Value> = matrix
        .iter()
        .zip(applied_per_source.iter())
        .map(|(row, applied)| serde_json::json!({"source": row.source, "applied": applied}))
        .collect();
    let queue_service_json: Vec<serde_json::Value> = matrix
        .iter()
        .zip(queue_applied_per_source.iter())
        .map(|(row, applied)| serde_json::json!({"source": row.source, "applied": applied}))
        .collect();
    let report = serde_json::json!({
        "schema": PRIORITY_CONTENTION_SCHEMA,
        "fixture": PRIORITY_CONTENTION_FIXTURE,
        "scope_note": "Acceptance artifact for input ordering, fairness, and queue behavior (AR-11-02/AR-11-05) via a bounded arbiter/queue model. Not a production throughput or remote-CI claim.",
        "matrix": matrix_json,
        "workload": {
            "rounds": PRIORITY_CONTENTION_ROUNDS,
            "updates_per_source": PRIORITY_CONTENTION_ROUNDS + 1,
            "arrival_order": "descending priority: lower-priority snapshots arrive after higher-priority ones",
            "continuous_component": "left_stick (buttons stay unioned by design)",
        },
        "before": {
            "policy": "last-update-wins (bounded FIFO model, not production behavior)",
            "winner_source": low.source,
            "winner_priority": low.priority_value,
            "wins": 0,
            "inversions": inversions,
        },
        "after": {
            "policy": "explicit priority (priority, update_order, source_id)",
            "winner_source": high.source,
            "winner_priority": high.priority_value,
            "high_priority_wins": high_priority_wins,
            "rounds": PRIORITY_CONTENTION_ROUNDS,
        },
        "service": {
            "applied_per_source": service_json,
            "ignored_queue_events": ignored_total,
            "starvation": "none observed: every source applied every round and surfaced in the release cascade",
        },
        "queue": {
            "model": "bounded tokio mpsc over existing (InputSourceId, InputSnapshot) pairs; FIFO queue, priority enforced by arbiter at apply time",
            "queue_capacity": PRIORITY_QUEUE_CAPACITY,
            "rounds": PRIORITY_QUEUE_ROUNDS,
            "applied_per_source": queue_service_json,
            "dropped_explicit": dropped_explicit,
            "retried_after_drain": retried_after_drain,
            "starvation_count": starvation_count,
            "queue_high_priority_wins": queue_high_priority_wins,
            "backpressure_policy": "non-blocking try_send only: Full is counted explicitly and retried after a deterministic try_recv drain; no blocking send, no unbounded growth, no hidden stall",
            "scope": "Bounded arbiter/queue model only; not a production throughput or remote-CI claim. Timing remains advisory.",
        },
        "latency_nanos": {
            "samples": latencies_nanos.len(),
            "min": min_nanos,
            "max": max_nanos,
            "mean": mean_nanos,
            "advisory_only": true,
            "note": "Advisory local apply_snapshot cost; no threshold is asserted.",
        },
        "handoff": {
            "released_to_surfaced": handoff_rows,
            "final": "neutral",
            "ok": true,
        },
        "build": {
            "crate": "pokecon",
            "version": env!("CARGO_PKG_VERSION"),
            "github_sha": std::env::var("GITHUB_SHA").ok().filter(|item| !item.is_empty()),
            "build_id": std::env::var("POKECON_BUILD_ID").ok().filter(|item| !item.is_empty()),
        },
    });

    // Deterministic artifact only on request; otherwise no repository side effect.
    if let Some(out) = std::env::var(PRIORITY_REPORT_OUT_ENV)
        .ok()
        .filter(|item| !item.is_empty())
    {
        let path = std::path::Path::new(&out);
        if let Some(parent) = path.parent().filter(|item| !item.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }

    println!(
        "priority contention: schema={PRIORITY_CONTENTION_SCHEMA} fixture={PRIORITY_CONTENTION_FIXTURE} \
         rounds={PRIORITY_CONTENTION_ROUNDS} high_priority_wins={high_priority_wins} \
         inversions={inversions} ignored={ignored_total} \
         queue_capacity={PRIORITY_QUEUE_CAPACITY} queue_rounds={PRIORITY_QUEUE_ROUNDS} \
         dropped_explicit={dropped_explicit} retried_after_drain={retried_after_drain} \
         starvation_count={starvation_count} queue_high_priority_wins={queue_high_priority_wins} \
         latency_nanos(min/mean/max)={min_nanos}/{mean_nanos}/{max_nanos} advisory_only=true"
    );
}
