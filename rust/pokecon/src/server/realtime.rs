//! Deterministic WebRTC-primary and WebSocket-fallback route arbitration.
//! Bounded control priority: WebRTC attempts and fallback are serialized and
//! never block the main command/camera/serial/script paths; recovery probes
//! are coalesced and bounded.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use thiserror::Error;

/// The transport currently carrying browser video and realtime data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeRoute {
    /// The initial WebRTC attempt has not yet completed or timed out.
    Connecting,
    /// WebRTC video and the independent `DataChannel` input/log paths are ready.
    WebRtc,
    /// Motion JPEG and JSON WebSocket messages remain active.
    WebSocketFallback,
}

/// Why one WebRTC peer attempt was started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebRtcAttemptKind {
    Initial,
    AutomaticRecovery,
    ManualRecovery,
}

/// Side effects emitted by [`RealtimeTransportController`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeAction {
    StartWebRtcAttempt {
        attempt: u64,
        kind: WebRtcAttemptKind,
    },
    StopWebRtcAttempt {
        attempt: u64,
    },
    BeginInputHandoff {
        route: RealtimeRoute,
        attempt: Option<u64>,
    },
    ActivateWebRtc {
        attempt: u64,
    },
    EnableMotionJpeg,
}

/// Fixed failure deadlines plus runtime-adjustable recovery policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RealtimeTransportConfig {
    pub connect_timeout: Duration,
    pub inactivity_timeout: Duration,
    pub auto_recover: bool,
    pub recovery_probe_interval: Duration,
}

impl RealtimeTransportConfig {
    /// Validates all monotonic timer durations.
    ///
    /// # Errors
    ///
    /// Rejects zero duration timers.
    pub const fn new(
        connect_timeout: Duration,
        inactivity_timeout: Duration,
        auto_recover: bool,
        recovery_probe_interval: Duration,
    ) -> Result<Self, RealtimeConfigError> {
        if connect_timeout.is_zero()
            || inactivity_timeout.is_zero()
            || recovery_probe_interval.is_zero()
        {
            return Err(RealtimeConfigError::ZeroDuration);
        }
        Ok(Self {
            connect_timeout,
            inactivity_timeout,
            auto_recover,
            recovery_probe_interval,
        })
    }
}

impl Default for RealtimeTransportConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            inactivity_timeout: Duration::from_secs(3),
            auto_recover: true,
            recovery_probe_interval: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RealtimeConfigError {
    #[error("realtime transport durations must be greater than zero")]
    ZeroDuration,
}

#[derive(Clone, Copy, Debug)]
struct Attempt {
    id: u64,
    kind: WebRtcAttemptKind,
    deadline: Instant,
    readiness: PeerReadiness,
    promotion: PromotionState,
}

#[derive(Clone, Copy, Debug, Default)]
struct PeerReadiness {
    video_ready: bool,
    data_ready: bool,
}

impl PeerReadiness {
    const fn complete(self) -> bool {
        self.video_ready && self.data_ready
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromotionState {
    Eligible,
    Requested,
    Suppressed,
}

/// Pure state machine for one browser connection.
///
/// It deliberately emits input-handoff actions before activating a recovered
/// peer. The caller must acknowledge a sequence-zero snapshot through
/// [`Self::primary_input_applied`] before Motion JPEG can be stopped.
#[derive(Debug)]
pub struct RealtimeTransportController {
    config: RealtimeTransportConfig,
    route: RealtimeRoute,
    attempt: Option<Attempt>,
    primary_attempt: Option<u64>,
    last_primary_activity: Option<Instant>,
    next_recovery_probe: Option<Instant>,
    next_attempt_id: u64,
    actions: VecDeque<RealtimeAction>,
}

impl RealtimeTransportController {
    /// Starts the initial, five-second-by-default WebRTC attempt.
    #[must_use]
    pub fn new(config: RealtimeTransportConfig, now: Instant) -> Self {
        let initial = Attempt {
            id: 1,
            kind: WebRtcAttemptKind::Initial,
            deadline: deadline(now, config.connect_timeout),
            readiness: PeerReadiness::default(),
            promotion: PromotionState::Eligible,
        };
        Self {
            config,
            route: RealtimeRoute::Connecting,
            attempt: Some(initial),
            primary_attempt: None,
            last_primary_activity: None,
            next_recovery_probe: None,
            next_attempt_id: 2,
            actions: VecDeque::from([RealtimeAction::StartWebRtcAttempt {
                attempt: initial.id,
                kind: initial.kind,
            }]),
        }
    }

    #[must_use]
    pub const fn route(&self) -> RealtimeRoute {
        self.route
    }

    #[must_use]
    pub fn active_attempt(&self) -> Option<(u64, WebRtcAttemptKind)> {
        self.attempt.map(|attempt| (attempt.id, attempt.kind))
    }

    #[must_use]
    pub const fn next_recovery_probe(&self) -> Option<Instant> {
        self.next_recovery_probe
    }

    /// Returns the earliest monotonic instant at which [`Self::tick`] can
    /// produce a transition.
    #[must_use]
    pub fn next_wakeup(&self) -> Option<Instant> {
        let attempt_deadline = self.attempt.map(|attempt| attempt.deadline);
        let inactivity_deadline = (self.route == RealtimeRoute::WebRtc)
            .then(|| {
                self.last_primary_activity
                    .map(|activity| deadline(activity, self.config.inactivity_timeout))
            })
            .flatten();
        [
            attempt_deadline,
            inactivity_deadline,
            self.next_recovery_probe,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Drains all ordered side effects produced by previous transitions.
    pub fn take_actions(&mut self) -> Vec<RealtimeAction> {
        self.actions.drain(..).collect()
    }

    /// Advances timeout and recovery timers using a caller-owned monotonic
    /// instant.
    pub fn tick(&mut self, now: Instant) {
        if self.route == RealtimeRoute::WebRtc
            && self
                .last_primary_activity
                .is_some_and(|activity| deadline(activity, self.config.inactivity_timeout) <= now)
        {
            self.enter_fallback(now, true);
        }

        if self.attempt.is_some_and(|attempt| attempt.deadline <= now) {
            self.finish_attempt_unsuccessfully(now);
        }

        if self.route == RealtimeRoute::WebSocketFallback
            && self.attempt.is_none()
            && self.config.auto_recover
            && self.next_recovery_probe.is_some_and(|due| due <= now)
        {
            self.start_attempt(WebRtcAttemptKind::AutomaticRecovery, now);
        }
    }

    /// Records independent readiness of the video track and `DataChannels`.
    /// Promotion begins only after both are ready.
    pub fn peer_readiness(
        &mut self,
        attempt_id: u64,
        video_ready: bool,
        data_ready: bool,
        now: Instant,
    ) {
        if self
            .attempt
            .is_some_and(|attempt| attempt.id == attempt_id && attempt.deadline <= now)
        {
            self.finish_attempt_unsuccessfully(now);
            return;
        }
        let Some(attempt) = self
            .attempt
            .as_mut()
            .filter(|attempt| attempt.id == attempt_id)
        else {
            return;
        };
        attempt.readiness.video_ready |= video_ready;
        attempt.readiness.data_ready |= data_ready;
        if !attempt.readiness.complete() || attempt.promotion == PromotionState::Requested {
            return;
        }
        if attempt.promotion == PromotionState::Suppressed {
            self.finish_attempt_unsuccessfully(now);
            return;
        }
        attempt.promotion = PromotionState::Requested;
        self.actions.push_back(RealtimeAction::BeginInputHandoff {
            route: RealtimeRoute::WebRtc,
            attempt: Some(attempt_id),
        });
    }

    /// Atomically activates a peer only after the new route's full input
    /// snapshot has been accepted.
    pub fn primary_input_applied(&mut self, attempt_id: u64, now: Instant) {
        if self
            .attempt
            .is_some_and(|attempt| attempt.id == attempt_id && attempt.deadline <= now)
        {
            self.finish_attempt_unsuccessfully(now);
            return;
        }
        let Some(attempt) = self.attempt.filter(|attempt| {
            attempt.id == attempt_id
                && attempt.readiness.complete()
                && attempt.promotion == PromotionState::Requested
        }) else {
            return;
        };
        self.attempt = None;
        self.route = RealtimeRoute::WebRtc;
        self.primary_attempt = Some(attempt.id);
        self.last_primary_activity = Some(now);
        self.next_recovery_probe = None;
        self.actions.push_back(RealtimeAction::ActivateWebRtc {
            attempt: attempt.id,
        });
    }

    /// Records successful video or `DataChannel` traffic for the active peer.
    pub fn primary_activity(&mut self, attempt_id: u64, now: Instant) {
        if self.route != RealtimeRoute::WebRtc || self.primary_attempt != Some(attempt_id) {
            return;
        }
        if self
            .last_primary_activity
            .is_some_and(|activity| deadline(activity, self.config.inactivity_timeout) <= now)
        {
            self.enter_fallback(now, true);
            return;
        }
        self.last_primary_activity = Some(now);
    }

    /// Handles a peer close or transport error without accepting stale peer
    /// identifiers.
    pub fn peer_failed(&mut self, attempt_id: u64, now: Instant) {
        if self.attempt.is_some_and(|attempt| attempt.id == attempt_id) {
            self.finish_attempt_unsuccessfully(now);
        } else if self.route == RealtimeRoute::WebRtc && self.primary_attempt == Some(attempt_id) {
            self.enter_fallback(now, true);
        }
    }

    /// Starts a user-requested attempt even when automatic recovery is off.
    /// Concurrent probes are deliberately coalesced.
    pub fn manual_reconnect(&mut self, now: Instant) {
        if self.route == RealtimeRoute::WebSocketFallback && self.attempt.is_none() {
            self.start_attempt(WebRtcAttemptKind::ManualRecovery, now);
        }
    }

    /// Applies runtime recovery settings. Disabling automatic recovery marks
    /// an in-flight automatic probe as permanently ineligible for promotion,
    /// while allowing it to finish safely.
    ///
    /// # Errors
    ///
    /// Rejects a zero recovery interval without modifying current settings.
    pub fn update_recovery(
        &mut self,
        auto_recover: bool,
        recovery_probe_interval: Duration,
        now: Instant,
    ) -> Result<(), RealtimeConfigError> {
        if recovery_probe_interval.is_zero() {
            return Err(RealtimeConfigError::ZeroDuration);
        }
        self.config.auto_recover = auto_recover;
        self.config.recovery_probe_interval = recovery_probe_interval;
        let cancel_requested_promotion = if !auto_recover
            && let Some(attempt) = self
                .attempt
                .as_mut()
                .filter(|attempt| attempt.kind == WebRtcAttemptKind::AutomaticRecovery)
        {
            let requested = attempt.promotion == PromotionState::Requested;
            attempt.promotion = PromotionState::Suppressed;
            requested.then_some(attempt.id)
        } else {
            None
        };
        if let Some(attempt) = cancel_requested_promotion {
            self.attempt = None;
            self.actions
                .push_back(RealtimeAction::StopWebRtcAttempt { attempt });
            self.actions.push_back(RealtimeAction::BeginInputHandoff {
                route: RealtimeRoute::WebSocketFallback,
                attempt: None,
            });
        }
        self.next_recovery_probe = (self.route == RealtimeRoute::WebSocketFallback
            && self.attempt.is_none()
            && auto_recover)
            .then(|| deadline(now, recovery_probe_interval));
        Ok(())
    }

    fn start_attempt(&mut self, kind: WebRtcAttemptKind, now: Instant) {
        let id = self.next_attempt_id;
        let Some(next) = id.checked_add(1) else {
            self.next_recovery_probe = None;
            return;
        };
        self.next_attempt_id = next;
        self.attempt = Some(Attempt {
            id,
            kind,
            deadline: deadline(now, self.config.connect_timeout),
            readiness: PeerReadiness::default(),
            promotion: PromotionState::Eligible,
        });
        self.next_recovery_probe = None;
        self.actions
            .push_back(RealtimeAction::StartWebRtcAttempt { attempt: id, kind });
    }

    fn finish_attempt_unsuccessfully(&mut self, now: Instant) {
        let Some(attempt) = self.attempt.take() else {
            return;
        };
        let input_handoff_started = attempt.promotion == PromotionState::Requested;
        self.actions.push_back(RealtimeAction::StopWebRtcAttempt {
            attempt: attempt.id,
        });
        if attempt.kind == WebRtcAttemptKind::Initial {
            self.enter_fallback(now, input_handoff_started);
        } else {
            if input_handoff_started {
                self.actions.push_back(RealtimeAction::BeginInputHandoff {
                    route: RealtimeRoute::WebSocketFallback,
                    attempt: None,
                });
            }
            self.schedule_recovery(now);
        }
    }

    fn enter_fallback(&mut self, now: Instant, handoff_input: bool) {
        if let Some(primary) = self.primary_attempt.take() {
            self.actions
                .push_back(RealtimeAction::StopWebRtcAttempt { attempt: primary });
        }
        self.route = RealtimeRoute::WebSocketFallback;
        self.last_primary_activity = None;
        self.actions.push_back(RealtimeAction::EnableMotionJpeg);
        if handoff_input {
            self.actions.push_back(RealtimeAction::BeginInputHandoff {
                route: RealtimeRoute::WebSocketFallback,
                attempt: None,
            });
        }
        self.schedule_recovery(now);
    }

    fn schedule_recovery(&mut self, now: Instant) {
        self.next_recovery_probe = self
            .config
            .auto_recover
            .then(|| deadline(now, self.config.recovery_probe_interval));
    }
}

fn deadline(now: Instant, duration: Duration) -> Instant {
    now.checked_add(duration).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(controller: &mut RealtimeTransportController) -> Vec<RealtimeAction> {
        controller.take_actions()
    }

    #[test]
    fn exact_initial_deadline_enables_fallback_and_schedules_recovery() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::StartWebRtcAttempt {
                attempt: 1,
                kind: WebRtcAttemptKind::Initial,
            }]
        );

        controller.tick(now + Duration::from_millis(4_999));
        assert!(actions(&mut controller).is_empty());
        controller.tick(now + Duration::from_secs(5));
        assert_eq!(controller.route(), RealtimeRoute::WebSocketFallback);
        assert_eq!(
            actions(&mut controller),
            [
                RealtimeAction::StopWebRtcAttempt { attempt: 1 },
                RealtimeAction::EnableMotionJpeg,
            ]
        );
        assert_eq!(
            controller.next_recovery_probe(),
            Some(now + Duration::from_secs(35))
        );
    }

    #[test]
    fn readiness_arriving_at_the_deadline_cannot_revive_an_expired_attempt() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);

        controller.peer_readiness(1, true, true, now + Duration::from_secs(5));
        assert_eq!(controller.route(), RealtimeRoute::WebSocketFallback);
        assert_eq!(
            actions(&mut controller),
            [
                RealtimeAction::StopWebRtcAttempt { attempt: 1 },
                RealtimeAction::EnableMotionJpeg,
            ]
        );
    }

    #[test]
    fn both_peer_paths_and_snapshot_ack_are_required_for_atomic_promotion() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);

        controller.peer_readiness(1, true, false, now + Duration::from_secs(1));
        assert!(actions(&mut controller).is_empty());
        controller.peer_readiness(1, false, true, now + Duration::from_secs(2));
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::BeginInputHandoff {
                route: RealtimeRoute::WebRtc,
                attempt: Some(1),
            }]
        );
        assert_eq!(controller.route(), RealtimeRoute::Connecting);

        controller.primary_input_applied(99, now + Duration::from_secs(2));
        assert!(actions(&mut controller).is_empty());
        controller.primary_input_applied(1, now + Duration::from_secs(2));
        assert_eq!(controller.route(), RealtimeRoute::WebRtc);
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::ActivateWebRtc { attempt: 1 }]
        );
    }

    #[test]
    fn failed_initial_peer_restores_websocket_input_after_handoff_started() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);
        controller.peer_readiness(1, true, true, now + Duration::from_secs(1));
        let _handoff = actions(&mut controller);

        controller.peer_failed(1, now + Duration::from_secs(2));

        assert_eq!(controller.route(), RealtimeRoute::WebSocketFallback);
        assert_eq!(
            actions(&mut controller),
            [
                RealtimeAction::StopWebRtcAttempt { attempt: 1 },
                RealtimeAction::EnableMotionJpeg,
                RealtimeAction::BeginInputHandoff {
                    route: RealtimeRoute::WebSocketFallback,
                    attempt: None,
                },
            ]
        );
    }

    #[test]
    fn three_seconds_without_primary_activity_releases_route_to_fallback() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);
        controller.peer_readiness(1, true, true, now);
        let _handoff = actions(&mut controller);
        controller.primary_input_applied(1, now);
        let _activation = actions(&mut controller);

        controller.primary_activity(1, now + Duration::from_secs(1));
        controller.tick(now + Duration::from_millis(3_999));
        assert!(actions(&mut controller).is_empty());
        controller.tick(now + Duration::from_secs(4));
        assert_eq!(controller.route(), RealtimeRoute::WebSocketFallback);
        assert_eq!(
            actions(&mut controller),
            [
                RealtimeAction::StopWebRtcAttempt { attempt: 1 },
                RealtimeAction::EnableMotionJpeg,
                RealtimeAction::BeginInputHandoff {
                    route: RealtimeRoute::WebSocketFallback,
                    attempt: None,
                },
            ]
        );
    }

    #[test]
    fn recovery_probes_are_serial_and_repeat_after_failure() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);
        controller.tick(now + Duration::from_secs(5));
        let _fallback = actions(&mut controller);

        controller.tick(now + Duration::from_secs(35));
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::StartWebRtcAttempt {
                attempt: 2,
                kind: WebRtcAttemptKind::AutomaticRecovery,
            }]
        );
        controller.tick(now + Duration::from_secs(36));
        assert!(actions(&mut controller).is_empty());
        controller.peer_failed(2, now + Duration::from_secs(36));
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::StopWebRtcAttempt { attempt: 2 }]
        );
        assert_eq!(
            controller.next_recovery_probe(),
            Some(now + Duration::from_secs(66))
        );
    }

    #[test]
    fn failed_recovery_peer_restores_websocket_input_after_handoff_started() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);
        controller.tick(now + Duration::from_secs(5));
        let _fallback = actions(&mut controller);
        controller.tick(now + Duration::from_secs(35));
        let _probe = actions(&mut controller);
        controller.peer_readiness(2, true, true, now + Duration::from_secs(36));
        let _handoff = actions(&mut controller);

        controller.peer_failed(2, now + Duration::from_secs(37));

        assert_eq!(controller.route(), RealtimeRoute::WebSocketFallback);
        assert_eq!(
            actions(&mut controller),
            [
                RealtimeAction::StopWebRtcAttempt { attempt: 2 },
                RealtimeAction::BeginInputHandoff {
                    route: RealtimeRoute::WebSocketFallback,
                    attempt: None,
                },
            ]
        );
    }

    #[test]
    fn next_wakeup_tracks_attempt_activity_and_recovery_deadlines() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        assert_eq!(controller.next_wakeup(), Some(now + Duration::from_secs(5)));
        let _initial = actions(&mut controller);

        controller.peer_readiness(1, true, true, now + Duration::from_secs(1));
        let _handoff = actions(&mut controller);
        controller.primary_input_applied(1, now + Duration::from_secs(2));
        assert_eq!(controller.next_wakeup(), Some(now + Duration::from_secs(5)));
        let _activation = actions(&mut controller);

        controller.primary_activity(1, now + Duration::from_secs(3));
        assert_eq!(controller.next_wakeup(), Some(now + Duration::from_secs(6)));
        controller.tick(now + Duration::from_secs(6));
        assert_eq!(
            controller.next_wakeup(),
            Some(now + Duration::from_secs(36))
        );
    }

    #[test]
    fn disabling_auto_recovery_suppresses_in_flight_probe_promotion() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);
        controller.tick(now + Duration::from_secs(5));
        let _fallback = actions(&mut controller);
        controller.tick(now + Duration::from_secs(35));
        let _probe = actions(&mut controller);

        controller
            .update_recovery(
                false,
                Duration::from_secs(10),
                now + Duration::from_secs(36),
            )
            .expect("valid recovery settings");
        assert_eq!(controller.next_recovery_probe(), None);
        controller.peer_readiness(2, true, true, now + Duration::from_secs(37));
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::StopWebRtcAttempt { attempt: 2 }]
        );
        assert_eq!(controller.route(), RealtimeRoute::WebSocketFallback);

        controller.manual_reconnect(now + Duration::from_secs(38));
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::StartWebRtcAttempt {
                attempt: 3,
                kind: WebRtcAttemptKind::ManualRecovery,
            }]
        );
        controller.peer_readiness(3, true, true, now + Duration::from_secs(39));
        assert_eq!(
            actions(&mut controller),
            [RealtimeAction::BeginInputHandoff {
                route: RealtimeRoute::WebRtc,
                attempt: Some(3),
            }]
        );
    }

    #[test]
    fn interval_change_reschedules_from_application_time() {
        let now = Instant::now();
        let mut controller =
            RealtimeTransportController::new(RealtimeTransportConfig::default(), now);
        let _initial = actions(&mut controller);
        controller.tick(now + Duration::from_secs(5));
        let _fallback = actions(&mut controller);

        controller
            .update_recovery(true, Duration::from_secs(7), now + Duration::from_secs(10))
            .expect("valid interval");
        assert_eq!(
            controller.next_recovery_probe(),
            Some(now + Duration::from_secs(17))
        );
        assert_eq!(
            controller.update_recovery(true, Duration::ZERO, now),
            Err(RealtimeConfigError::ZeroDuration)
        );
        assert_eq!(
            controller.next_recovery_probe(),
            Some(now + Duration::from_secs(17))
        );
    }
}
