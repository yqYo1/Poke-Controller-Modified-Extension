//! Command start/stop reservations (upstream `services/run_schedule.py`).
//!
//! The service is UI-independent: the runtime calls [`RunSchedule::tick`] a few
//! times per second, and the tick starts or stops commands through the injected
//! [`ScheduleIo`]. Reservations live in memory only, are never persisted, and
//! the PC sleep settings are never touched; the UI shows [`SLEEP_WARNING`]
//! before the first reservation. User-facing strings follow the upstream
//! module, matching the run-list module conventions.

use chrono::{Duration, NaiveDateTime};

use crate::command_service::CommandIdentity;

/// Upstream `TIME_FORMAT`.
pub const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";
/// Upstream `MINUTES_MAX` (7 days).
pub const MINUTES_MAX: f64 = 60.0 * 24.0 * 7.0;
/// Upstream `START_LATE_LIMIT_SECONDS`: a start later than this many seconds
/// past its reserved time is skipped (the PC was probably asleep).
pub const START_LATE_LIMIT_SECONDS: f64 = 60.0;
/// Upstream `SLEEP_WARNING` shown by the UI before reserving.
pub const SLEEP_WARNING: &str = "予約した時刻に PC がスリープしていると、予約は正常に動作しません。\n\
(開始は行われず、停止は、スリープから戻った後になります)\n\
このアプリは、PC のスリープの設定を変えません。\n\
予約の時刻まで PC がスリープしないように、Windows の電源の設定を確かめてください。";

/// One reserved start target: the stable command identity (run lists included)
/// plus the display name used in messages.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScheduleTarget {
    pub identity: CommandIdentity,
    pub name: String,
}

/// Side effects [`RunSchedule::tick`] performs on the running application.
pub trait ScheduleIo {
    /// Whether a command is currently running (or paused).
    fn is_running(&self) -> impl std::future::Future<Output = bool>;
    /// Starts one command; returns whether the start was accepted.
    fn start(&mut self, target: &ScheduleTarget) -> impl std::future::Future<Output = bool>;
    /// Stops the running command.
    fn stop(&mut self) -> impl std::future::Future<Output = ()>;
}

/// Holds the start and stop reservations and acts when the time comes.
#[derive(Debug, Default)]
pub struct RunSchedule {
    start_at: Option<NaiveDateTime>,
    start_target: Option<ScheduleTarget>,
    stop_at: Option<NaiveDateTime>,
    stop_after_minutes: Option<f64>,
    /// Whether the command was seen running after the stop reservation; when
    /// it finishes first, the reservation is dropped.
    stop_armed: bool,
    messages: Vec<String>,
}

/// Keeps the most recent messages; the UI only needs the tail, and the log
/// mirror carries the rest.
const MESSAGES_MAX: usize = 64;

impl RunSchedule {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reserves a start; a previous start reservation is replaced.
    pub fn reserve_start(&mut self, when: NaiveDateTime, target: ScheduleTarget) {
        let name = target.name.clone();
        self.start_at = Some(when);
        self.start_target = Some(target);
        self.notify(format!(
            "開始を予約しました: {} に「{}」",
            when.format(TIME_FORMAT),
            name
        ));
    }

    /// Cancels the start reservation; a "stop after start" reservation is
    /// cancelled together with it.
    pub fn cancel_start(&mut self) {
        if self.start_at.is_none() {
            return;
        }
        self.start_at = None;
        self.start_target = None;
        self.notify("開始の予約を取り消しました。".to_owned());
        if self.stop_after_minutes.is_some() {
            self.stop_after_minutes = None;
            self.notify("「開始から」の停止の予約も、取り消しました。".to_owned());
        }
    }

    /// Reserves a stop at a fixed time; a previous stop reservation is
    /// replaced.
    pub fn reserve_stop_at(&mut self, when: NaiveDateTime, is_running: bool) {
        self.stop_at = Some(when);
        self.stop_after_minutes = None;
        self.stop_armed = is_running;
        self.notify(format!("停止を予約しました: {}", when.format(TIME_FORMAT)));
    }

    /// Reserves a stop `minutes` after the start. While a command is running,
    /// the count starts now; with a reserved start it counts from that start.
    /// Returns the user-facing reason when no reservation is possible.
    ///
    /// # Errors
    ///
    /// Rejects the reservation when no command is running and no start is
    /// reserved.
    pub fn reserve_stop_after(
        &mut self,
        minutes: f64,
        is_running: bool,
        now: NaiveDateTime,
    ) -> Result<(), String> {
        if is_running {
            let stop_at = minutes_from(now, minutes);
            self.stop_at = Some(stop_at);
            self.stop_after_minutes = None;
            self.stop_armed = true;
            self.notify(format!(
                "停止を予約しました: 今から {minutes} 分後({})",
                stop_at.format(TIME_FORMAT)
            ));
        } else if self.start_at.is_some() {
            self.stop_at = None;
            self.stop_after_minutes = Some(minutes);
            self.stop_armed = false;
            self.notify(format!(
                "停止を予約しました: 予約した開始から {minutes} 分後"
            ));
        } else {
            return Err(
                "コマンドを実行中か、開始を予約しているときだけ、「開始から」の停止を予約できます。"
                    .to_owned(),
            );
        }
        Ok(())
    }

    /// Cancels the stop reservation.
    pub fn cancel_stop(&mut self) {
        if self.stop_at.is_none() && self.stop_after_minutes.is_none() {
            return;
        }
        self.stop_at = None;
        self.stop_after_minutes = None;
        self.stop_armed = false;
        self.notify("停止の予約を取り消しました。".to_owned());
    }

    /// Clears every reservation without notifying (application shutdown).
    pub fn cancel_all(&mut self) {
        self.start_at = None;
        self.start_target = None;
        self.stop_at = None;
        self.stop_after_minutes = None;
        self.stop_armed = false;
    }

    /// Whether any reservation remains.
    #[must_use]
    pub fn pending(&self) -> bool {
        self.start_at.is_some() || self.stop_at.is_some() || self.stop_after_minutes.is_some()
    }

    /// The reserved start time, if any.
    #[must_use]
    pub fn start_at(&self) -> Option<NaiveDateTime> {
        self.start_at
    }

    /// The reserved stop time, if any.
    #[must_use]
    pub fn stop_at(&self) -> Option<NaiveDateTime> {
        self.stop_at
    }

    /// The reserved "stop after start" minutes, if any.
    #[must_use]
    pub fn stop_after_minutes(&self) -> Option<f64> {
        self.stop_after_minutes
    }

    /// The most recent message, used by the UI status line.
    #[must_use]
    pub fn last_message(&self) -> &str {
        self.messages.last().map_or("", String::as_str)
    }

    /// All retained messages (tests and diagnostics).
    #[cfg(test)]
    #[must_use]
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Acts when the reserved times have come; the runtime calls this a few
    /// times per second with the current local time.
    pub async fn tick<Io>(&mut self, now: NaiveDateTime, io: &mut Io)
    where
        Io: ScheduleIo,
    {
        if let Some(start_at) = self.start_at
            && now >= start_at
        {
            let target = self.start_target.take();
            self.start_at = None;
            if let Some(target) = target {
                let late = seconds_between(start_at, now);
                let started = if late > START_LATE_LIMIT_SECONDS {
                    self.notify(format!(
                            "予約の時刻を {} 過ぎていたので、「{}」は開始しませんでした(PC がスリープしていた可能性があります)。",
                            format_remaining(late),
                            target.name
                        ));
                    false
                } else if io.is_running().await {
                    self.notify(format!(
                            "予約の時刻になりましたが、ほかのコマンドを実行中のため、「{}」は開始しませんでした。",
                            target.name
                        ));
                    false
                } else {
                    self.notify(format!(
                        "予約の時刻になりました。「{}」を開始します。",
                        target.name
                    ));
                    let accepted = io.start(&target).await;
                    if !accepted {
                        self.notify(format!("「{}」を開始できませんでした。", target.name));
                    }
                    accepted
                };
                if let Some(minutes) = self.stop_after_minutes {
                    if started {
                        let stop_at = now + minutes_duration(minutes);
                        self.stop_at = Some(stop_at);
                        self.stop_armed = true;
                        self.notify(format!("停止の予約: {}", stop_at.format(TIME_FORMAT)));
                    } else {
                        self.notify(
                            "開始しなかったので、「開始から」の停止の予約を取り消しました。"
                                .to_owned(),
                        );
                    }
                    self.stop_after_minutes = None;
                }
            }
        }
        if let Some(stop_at) = self.stop_at {
            let running = io.is_running().await;
            if running {
                self.stop_armed = true;
            }
            if now >= stop_at {
                self.stop_at = None;
                self.stop_armed = false;
                if running {
                    self.notify("停止の予約の時刻になりました。コマンドを止めます。".to_owned());
                    io.stop().await;
                } else {
                    self.notify(
                        "停止の予約の時刻になりましたが、実行中のコマンドはありませんでした。"
                            .to_owned(),
                    );
                }
            } else if self.stop_armed && !running {
                // The command that the stop was reserved for already finished.
                self.stop_at = None;
                self.stop_armed = false;
                self.notify("コマンドが終わったので、停止の予約を取り消しました。".to_owned());
            }
        }
    }

    /// The current reservation display (start, stop); "予約はありません" when
    /// nothing is reserved.
    #[must_use]
    pub fn describe(&self, now: NaiveDateTime) -> (String, String) {
        let start = if let (Some(at), Some(target)) = (self.start_at, self.start_target.as_ref()) {
            format!(
                "{} に「{}」を開始(あと {})",
                at.format(TIME_FORMAT),
                target.name,
                format_remaining(seconds_between(now, at))
            )
        } else {
            "予約はありません".to_owned()
        };
        let stop = if let Some(at) = self.stop_at {
            format!(
                "{} に停止(あと {})",
                at.format(TIME_FORMAT),
                format_remaining(seconds_between(now, at))
            )
        } else if let Some(minutes) = self.stop_after_minutes {
            format!("予約した開始から {minutes} 分後に停止")
        } else {
            "予約はありません".to_owned()
        };
        (start, stop)
    }

    fn notify(&mut self, text: String) {
        tracing::info!(message = %text, "run schedule");
        if self.messages.len() == MESSAGES_MAX {
            self.messages.remove(0);
        }
        self.messages.push(text);
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "reservation minutes are bounded by MINUTES_MAX"
)]
fn minutes_duration(minutes: f64) -> Duration {
    Duration::milliseconds((minutes * 60_000.0).round() as i64)
}

#[allow(
    clippy::cast_precision_loss,
    reason = "sub-second resolution of a short interval"
)]
fn seconds_between(from: NaiveDateTime, to: NaiveDateTime) -> f64 {
    (to - from).num_milliseconds() as f64 / 1000.0
}

/// "時:分" or "時:分:秒" input to the next occurrence of that time (times at or
/// before now move to tomorrow).
///
/// # Errors
///
/// Rejects unreadable or out-of-range text with the upstream message.
pub fn parse_clock(text: &str, now: NaiveDateTime) -> Result<NaiveDateTime, String> {
    fn range_error() -> String {
        "時刻は 0:00 〜 23:59 で入力してください。".to_owned()
    }

    let normalized = text.trim().replace('：', ":");
    let parts: Vec<&str> = normalized.split(':').collect();
    let unreadable = (parts.len() != 2 && parts.len() != 3)
        || parts
            .iter()
            .any(|part| part.trim().is_empty() || !part.trim().chars().all(|c| c.is_ascii_digit()));
    if unreadable {
        return Err("時刻は 23:30 のように、時:分 で入力してください。".to_owned());
    }
    let hour = parts[0].trim().parse::<u32>().map_err(|_| range_error())?;
    let minute = parts[1].trim().parse::<u32>().map_err(|_| range_error())?;
    let second = if parts.len() == 3 {
        parts[2].trim().parse::<u32>().map_err(|_| range_error())?
    } else {
        0
    };
    if hour > 23 || minute > 59 || second > 59 {
        return Err("時刻は 0:00 〜 23:59 で入力してください。".to_owned());
    }
    let when = now
        .date()
        .and_hms_opt(hour, minute, second)
        .expect("range checked");
    if when <= now {
        Ok(when + Duration::days(1))
    } else {
        Ok(when)
    }
}

/// Validates a minute count (greater than 0, at most [`MINUTES_MAX`]).
///
/// # Errors
///
/// Rejects out-of-range values with the upstream message.
pub fn check_minutes(minutes: f64) -> Result<(), String> {
    if minutes > 0.0 && minutes <= MINUTES_MAX {
        Ok(())
    } else {
        Err(format!(
            "分は、0 より大きく {MINUTES_MAX} までにしてください。"
        ))
    }
}

/// "◯分" input to a minute count.
///
/// # Errors
///
/// Rejects unreadable or out-of-range text with the upstream message.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "upstream parity for text input; the web client sends numbers"
    )
)]
pub fn parse_minutes(text: &str) -> Result<f64, String> {
    let minutes: f64 = text
        .trim()
        .parse()
        .map_err(|_| "分は、数で入力してください(例: 30、1.5)。".to_owned())?;
    check_minutes(minutes)?;
    Ok(minutes)
}

/// `now` plus a validated minute count.
#[must_use]
pub fn minutes_from(now: NaiveDateTime, minutes: f64) -> NaiveDateTime {
    now + minutes_duration(minutes)
}

/// Remaining-time display (e.g. "1 時間 02 分 03 秒"), rounded up.
#[must_use]
#[allow(
    clippy::cast_possible_truncation,
    reason = "whole seconds of a runtime duration"
)]
pub fn format_remaining(seconds: f64) -> String {
    let total = ((seconds + 0.999) as i64).max(0);
    let (hours, rest) = (total / 3600, total % 3600);
    let (minutes, secs) = (rest / 60, rest % 60);
    if hours > 0 {
        format!("{hours} 時間 {minutes:02} 分 {secs:02} 秒")
    } else if minutes > 0 {
        format!("{minutes} 分 {secs:02} 秒")
    } else {
        format!("{secs} 秒")
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn start_time() -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 10, 11)
            .expect("date")
            .and_hms_opt(22, 0, 0)
            .expect("time")
    }

    fn target(name: &str) -> ScheduleTarget {
        ScheduleTarget {
            identity: CommandIdentity {
                module_path: format!("Commands.PythonCommands.samples.{name}"),
                class_name: name.to_owned(),
            },
            name: name.to_owned(),
        }
    }

    #[derive(Default)]
    struct TestIo {
        running: bool,
        can_start: bool,
        started: Vec<ScheduleTarget>,
        stopped: u32,
    }

    impl ScheduleIo for TestIo {
        async fn is_running(&self) -> bool {
            self.running
        }

        async fn start(&mut self, target: &ScheduleTarget) -> bool {
            self.started.push(target.clone());
            if self.can_start {
                self.running = true;
            }
            self.can_start
        }

        async fn stop(&mut self) {
            self.stopped += 1;
            self.running = false;
        }
    }

    struct Harness {
        now: NaiveDateTime,
        io: TestIo,
        schedule: RunSchedule,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                now: start_time(),
                io: TestIo {
                    can_start: true,
                    ..TestIo::default()
                },
                schedule: RunSchedule::new(),
            }
        }

        async fn advance(&mut self, delta: Duration) {
            self.now += delta;
            self.schedule.tick(self.now, &mut self.io).await;
        }

        fn minutes(value: i64) -> Duration {
            Duration::minutes(value)
        }
    }

    #[tokio::test]
    async fn command_starts_at_the_time() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(30), target("A連打"));
        assert!(h.schedule.pending());
        h.advance(Duration::minutes(29) + Duration::seconds(59))
            .await;
        assert_eq!(h.io.started.len(), 0);
        h.advance(Duration::seconds(1)).await;
        assert_eq!(h.io.started.len(), 1);
        assert_eq!(h.io.started[0].name, "A連打");
        assert!(!h.schedule.pending());
        h.advance(Duration::minutes(10)).await;
        assert_eq!(h.io.started.len(), 1);
    }

    #[tokio::test]
    async fn start_is_skipped_while_another_command_is_running() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(1), target("A連打"));
        h.io.running = true;
        h.advance(Harness::minutes(1)).await;
        assert_eq!(h.io.started.len(), 0);
        assert!(!h.schedule.pending());
        assert!(
            h.schedule
                .messages()
                .iter()
                .any(|message| message.contains("実行中のため"))
        );
    }

    #[tokio::test]
    async fn failure_to_start_is_reported() {
        let mut h = Harness::new();
        h.io.can_start = false;
        h.schedule.reserve_start(
            start_time() + Duration::minutes(1),
            target("消したコマンド"),
        );
        h.advance(Harness::minutes(1)).await;
        assert!(
            h.schedule
                .messages()
                .iter()
                .any(|message| message.contains("開始できませんでした"))
        );
    }

    #[tokio::test]
    async fn cancel_and_replace() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(1), target("a"));
        h.schedule
            .reserve_start(start_time() + Duration::minutes(5), target("b"));
        h.advance(Duration::minutes(2)).await;
        assert_eq!(h.io.started.len(), 0);
        h.schedule.cancel_start();
        h.advance(Duration::minutes(10)).await;
        assert_eq!(h.io.started.len(), 0);
        assert!(!h.schedule.pending());
    }

    #[tokio::test]
    async fn stop_at_a_time() {
        let mut h = Harness::new();
        h.io.running = true;
        h.schedule
            .reserve_stop_at(start_time() + Duration::minutes(10), true);
        h.advance(Harness::minutes(9)).await;
        assert_eq!(h.io.stopped, 0);
        h.advance(Harness::minutes(1)).await;
        assert_eq!(h.io.stopped, 1);
        assert!(!h.schedule.pending());
    }

    #[tokio::test]
    async fn stop_after_minutes_counts_from_now_while_running() {
        let mut h = Harness::new();
        h.io.running = true;
        assert_eq!(h.schedule.reserve_stop_after(5.0, true, h.now), Ok(()));
        assert_eq!(
            h.schedule.stop_at().map(|at| at - start_time()),
            Some(Duration::minutes(5))
        );
        h.advance(Harness::minutes(5)).await;
        assert_eq!(h.io.stopped, 1);
    }

    #[tokio::test]
    async fn stop_after_minutes_counts_from_the_reserved_start() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(30), target("a"));
        assert_eq!(h.schedule.reserve_stop_after(20.0, false, h.now), Ok(()));
        assert_eq!(h.schedule.stop_at(), None);
        h.advance(Duration::minutes(30)).await;
        assert_eq!(h.io.started.len(), 1);
        assert_eq!(
            h.schedule.stop_at().map(|at| at - start_time()),
            Some(Duration::minutes(50))
        );
        h.advance(Duration::minutes(19)).await;
        assert_eq!(h.io.stopped, 0);
        h.advance(Duration::minutes(1)).await;
        assert_eq!(h.io.stopped, 1);
        assert!(!h.schedule.pending());
    }

    #[tokio::test]
    async fn stop_after_needs_a_running_command_or_a_reserved_start() {
        let mut h = Harness::new();
        let error = h
            .schedule
            .reserve_stop_after(5.0, false, h.now)
            .expect_err("rejected");
        assert!(error.contains("実行中か"));
        assert!(!h.schedule.pending());
    }

    #[tokio::test]
    async fn stop_after_is_cancelled_when_the_start_fails_or_is_cancelled() {
        let mut h = Harness::new();
        h.io.can_start = false;
        h.schedule
            .reserve_start(start_time() + Duration::minutes(1), target("a"));
        assert_eq!(h.schedule.reserve_stop_after(5.0, false, h.now), Ok(()));
        h.advance(Harness::minutes(1)).await;
        assert!(!h.schedule.pending());
        h.schedule
            .reserve_start(start_time() + Duration::minutes(10), target("a"));
        assert_eq!(h.schedule.reserve_stop_after(5.0, false, h.now), Ok(()));
        h.schedule.cancel_start();
        assert!(!h.schedule.pending());
    }

    #[tokio::test]
    async fn stop_is_dropped_when_the_command_ends_first() {
        let mut h = Harness::new();
        h.io.running = true;
        h.schedule
            .reserve_stop_at(start_time() + Duration::minutes(10), true);
        h.advance(Harness::minutes(1)).await;
        h.io.running = false;
        h.advance(Harness::minutes(1)).await;
        assert!(!h.schedule.pending());
        assert!(
            h.schedule
                .messages()
                .iter()
                .any(|message| message.contains("コマンドが終わったので"))
        );
        h.advance(Duration::minutes(20)).await;
        assert_eq!(h.io.stopped, 0);
    }

    #[tokio::test]
    async fn stop_reserved_while_idle_waits_for_a_command() {
        let mut h = Harness::new();
        h.schedule
            .reserve_stop_at(start_time() + Duration::minutes(10), false);
        h.advance(Harness::minutes(1)).await;
        assert!(h.schedule.pending());
        h.io.running = true;
        h.advance(Harness::minutes(9)).await;
        assert_eq!(h.io.stopped, 1);
    }

    #[tokio::test]
    async fn stop_time_with_nothing_running() {
        let mut h = Harness::new();
        h.schedule
            .reserve_stop_at(start_time() + Duration::minutes(1), false);
        h.advance(Harness::minutes(1)).await;
        assert_eq!(h.io.stopped, 0);
        assert!(!h.schedule.pending());
        assert!(
            h.schedule
                .messages()
                .iter()
                .any(|message| message.contains("実行中のコマンドはありませんでした"))
        );
    }

    #[tokio::test]
    async fn cancel_stop() {
        let mut h = Harness::new();
        h.io.running = true;
        h.schedule
            .reserve_stop_at(start_time() + Duration::minutes(1), true);
        h.schedule.cancel_stop();
        h.advance(Harness::minutes(5)).await;
        assert_eq!(h.io.stopped, 0);
    }

    #[test]
    fn the_sleep_setting_of_the_pc_is_not_touched() {
        // Scan only the production section: the assertions below contain the
        // forbidden literals themselves.
        let source = include_str!("run_schedule.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("production section");
        assert!(!production.contains("SetThreadExecutionState"));
        assert!(!production.contains("ctypes"));
        assert!(SLEEP_WARNING.contains("スリープ"));
        assert!(SLEEP_WARNING.contains("正常に動作しません"));
    }

    #[tokio::test]
    async fn start_is_skipped_when_the_time_passed_long_ago() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(30), target("A連打"));
        assert_eq!(h.schedule.reserve_stop_after(20.0, false, h.now), Ok(()));
        h.advance(Duration::hours(2) + Duration::minutes(30)).await;
        assert_eq!(h.io.started.len(), 0);
        assert!(!h.schedule.pending());
        assert!(
            h.schedule
                .messages()
                .iter()
                .any(|message| message.contains("過ぎていたので") && message.contains("スリープ"))
        );
        assert!(
            h.schedule
                .messages()
                .iter()
                .any(|message| message.contains("「開始から」の停止の予約を取り消しました"))
        );
    }

    #[tokio::test]
    async fn start_a_little_late_still_starts() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(30), target("A連打"));
        h.advance(Duration::minutes(30) + Duration::seconds(60))
            .await;
        assert_eq!(h.io.started.len(), 1);
    }

    #[tokio::test]
    async fn stop_still_works_after_the_time_passed() {
        let mut h = Harness::new();
        h.io.running = true;
        h.schedule
            .reserve_stop_at(start_time() + Duration::minutes(10), true);
        h.advance(Duration::hours(3)).await;
        assert_eq!(h.io.stopped, 1);
    }

    #[tokio::test]
    async fn cancel_all() {
        let mut h = Harness::new();
        h.schedule
            .reserve_start(start_time() + Duration::minutes(1), target("a"));
        assert_eq!(h.schedule.reserve_stop_after(5.0, false, h.now), Ok(()));
        h.schedule.cancel_all();
        assert!(!h.schedule.pending());
        assert!(
            h.schedule
                .messages()
                .last()
                .expect("message")
                .starts_with("停止を予約しました")
        );
        h.advance(Duration::minutes(10)).await;
        assert_eq!(h.io.started.len(), 0);
    }

    #[test]
    fn clock_means_the_next_time_it_comes() {
        assert_eq!(
            parse_clock("23:30", start_time()).expect("clock"),
            NaiveDate::from_ymd_opt(2026, 10, 11)
                .expect("date")
                .and_hms_opt(23, 30, 0)
                .expect("time")
        );
        assert_eq!(
            parse_clock(" 6:05 ", start_time()).expect("clock"),
            NaiveDate::from_ymd_opt(2026, 10, 12)
                .expect("date")
                .and_hms_opt(6, 5, 0)
                .expect("time")
        );
        assert_eq!(
            parse_clock("22:00", start_time()).expect("clock"),
            NaiveDate::from_ymd_opt(2026, 10, 12)
                .expect("date")
                .and_hms_opt(22, 0, 0)
                .expect("time")
        );
        assert_eq!(
            parse_clock("22:00:30", start_time()).expect("clock"),
            NaiveDate::from_ymd_opt(2026, 10, 11)
                .expect("date")
                .and_hms_opt(22, 0, 30)
                .expect("time")
        );
        assert_eq!(
            parse_clock("23：15", start_time()).expect("clock"),
            NaiveDate::from_ymd_opt(2026, 10, 11)
                .expect("date")
                .and_hms_opt(23, 15, 0)
                .expect("time")
        );
    }

    #[test]
    fn bad_clocks() {
        for text in ["", "2330", "24:00", "12:60", "a:b", "1:2:3:4", "-1:00"] {
            assert!(parse_clock(text, start_time()).is_err(), "{text}");
        }
    }

    #[test]
    fn minutes() {
        assert_eq!(parse_minutes(" 1.5 "), Ok(1.5));
        for text in ["", "abc", "0", "-3", "nan", "100000"] {
            assert!(parse_minutes(text).is_err(), "{text}");
        }
    }

    #[test]
    fn remaining_time() {
        assert_eq!(format_remaining(3723.0), "1 時間 02 分 03 秒");
        assert_eq!(format_remaining(65.2), "1 分 06 秒");
        assert_eq!(format_remaining(0.4), "1 秒");
        assert_eq!(format_remaining(-5.0), "0 秒");
    }

    #[tokio::test]
    async fn describe() {
        let mut h = Harness::new();
        assert_eq!(
            h.schedule.describe(h.now),
            ("予約はありません".to_owned(), "予約はありません".to_owned())
        );
        h.schedule
            .reserve_start(start_time() + Duration::minutes(90), target("A連打"));
        assert_eq!(h.schedule.reserve_stop_after(20.0, false, h.now), Ok(()));
        let (start, stop) = h.schedule.describe(h.now);
        assert_eq!(
            start,
            "2026-10-11 23:30:00 に「A連打」を開始(あと 1 時間 30 分 00 秒)"
        );
        assert_eq!(stop, "予約した開始から 20 分後に停止");
        h.io.running = true;
        h.schedule
            .reserve_stop_at(start_time() + Duration::seconds(30), true);
        assert_eq!(
            h.schedule.describe(h.now).1,
            "2026-10-11 22:00:30 に停止(あと 30 秒)"
        );
    }
}
