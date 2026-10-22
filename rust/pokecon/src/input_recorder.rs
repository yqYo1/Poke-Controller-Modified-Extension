//! Controller input recording and record-to-script generation (PLAN `Record→スクリプト生成`).
//!
//! Samples are captured from the effective controller output (the same state
//! written to the serial device), saved as a small CSV, and converted into a
//! runnable `PythonCommand` script that replays taps as `press`, sustained
//! holds as `hold`/`holdEnd`, and idle gaps as `wait`.

use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use crate::device::controller::{Button, ControllerState, Hat};

/// Maximum retained samples per recording (about 30 minutes at 60 Hz).
pub const MAX_SAMPLES: usize = 100_000;
/// A button interval at most this long becomes `press` instead of hold/end.
pub const TAP_MAX_SECONDS: f64 = 0.5;
/// Shortest emitted wait; smaller gaps are folded into the next action.
pub const MIN_WAIT_SECONDS: f64 = 0.05;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedSample {
    pub offset_ms: u64,
    /// Button names in canonical order (see [`BUTTON_NAMES`]).
    pub buttons: Vec<String>,
    pub hat: &'static str,
    pub left: (u8, u8),
    pub right: (u8, u8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordSnapshot {
    pub recording: bool,
    pub samples: usize,
    pub elapsed_ms: u64,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RecordError {
    #[error("記録はすでに開始されています")]
    AlreadyRecording,
    #[error("記録は開始されていません")]
    NotRecording,
    #[error("記録件数の上限に達しました")]
    LimitReached,
    #[error("記録データを読み書きできません: {0}")]
    Io(String),
    #[allow(
        dead_code,
        reason = "CSV replay parsing is exercised by unit tests and reserved for file-based generation"
    )]
    #[error("記録CSVの形式が不正です: {0}")]
    InvalidCsv(String),
    #[error("スクリプト名が不正です")]
    InvalidName,
}

/// Button names in bitmask order.
pub const BUTTON_NAMES: [&str; 14] = [
    "A", "B", "X", "Y", "L", "R", "ZL", "ZR", "LClick", "RClick", "Plus", "Minus", "Home",
    "Capture",
];

fn button_mask(state: &ControllerState) -> u16 {
    let mut mask = 0u16;
    for (index, button) in [
        Button::A,
        Button::B,
        Button::X,
        Button::Y,
        Button::L,
        Button::R,
        Button::Zl,
        Button::Zr,
        Button::Lclick,
        Button::Rclick,
        Button::Plus,
        Button::Minus,
        Button::Home,
        Button::Capture,
    ]
    .into_iter()
    .enumerate()
    {
        if state.buttons.get(button) {
            mask |= 1 << index;
        }
    }
    mask
}

fn button_names(mask: u16) -> Vec<String> {
    BUTTON_NAMES
        .iter()
        .enumerate()
        .filter(|(index, _name)| mask & (1 << index) != 0)
        .map(|(_index, name)| (*name).to_owned())
        .collect()
}

fn hat_name(hat: Hat) -> &'static str {
    match hat {
        Hat::Up => "Up",
        Hat::Down => "Down",
        Hat::Left => "Left",
        Hat::Right => "Right",
        Hat::UpRight => "UpRight",
        Hat::UpLeft => "UpLeft",
        Hat::DownRight => "DownRight",
        Hat::DownLeft => "DownLeft",
        Hat::Neutral => "Neutral",
    }
}

#[allow(dead_code, reason = "used by CSV replay parsing in tests")]
fn hat_from_name(name: &str) -> Option<Hat> {
    Some(match name {
        "Up" => Hat::Up,
        "Down" => Hat::Down,
        "Left" => Hat::Left,
        "Right" => Hat::Right,
        "UpRight" => Hat::UpRight,
        "UpLeft" => Hat::UpLeft,
        "DownRight" => Hat::DownRight,
        "DownLeft" => Hat::DownLeft,
        "Neutral" => Hat::Neutral,
        _ => return None,
    })
}

fn parse_mask(names: &str) -> u16 {
    let mut mask = 0u16;
    for name in names.split('|') {
        if let Some(index) = BUTTON_NAMES.iter().position(|candidate| *candidate == name) {
            mask |= 1 << index;
        }
    }
    mask
}

/// One in-progress recording.
#[derive(Debug)]
pub struct InputRecorder {
    started: Option<Instant>,
    last_offset_ms: u64,
    last_mask: Option<u16>,
    samples: Vec<RecordedSample>,
    limit: usize,
}

impl Default for InputRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl InputRecorder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: None,
            last_offset_ms: 0,
            last_mask: None,
            samples: Vec::new(),
            limit: MAX_SAMPLES,
        }
    }

    #[must_use]
    #[allow(dead_code, reason = "used by the recorder limit unit test")]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            limit,
            ..Self::new()
        }
    }

    pub fn start(&mut self) -> Result<(), RecordError> {
        if self.started.is_some() {
            return Err(RecordError::AlreadyRecording);
        }
        self.started = Some(Instant::now());
        self.last_offset_ms = 0;
        self.last_mask = None;
        self.samples.clear();
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), RecordError> {
        if self.started.take().is_none() {
            return Err(RecordError::NotRecording);
        }
        Ok(())
    }

    #[must_use]
    pub fn is_recording(&self) -> bool {
        self.started.is_some()
    }

    #[must_use]
    pub fn snapshot(&self) -> RecordSnapshot {
        RecordSnapshot {
            recording: self.is_recording(),
            samples: self.samples.len(),
            elapsed_ms: self.started.map_or(0, |start| {
                u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
            }),
            limit: self.limit,
        }
    }

    /// Records one effective controller state, deduplicating identical samples.
    pub fn record(&mut self, state: &ControllerState) -> Result<(), RecordError> {
        if self.started.is_none() {
            return Err(RecordError::NotRecording);
        }
        let mask = button_mask(state);
        if self.last_mask == Some(mask) {
            self.last_offset_ms = self.offset_now();
            return Ok(());
        }
        if self.samples.len() >= self.limit {
            return Err(RecordError::LimitReached);
        }
        self.last_mask = Some(mask);
        self.last_offset_ms = self.offset_now();
        self.samples.push(RecordedSample {
            offset_ms: self.last_offset_ms,
            buttons: button_names(mask),
            hat: hat_name(state.hat),
            left: (state.left_stick.x, state.left_stick.y),
            right: (state.right_stick.x, state.right_stick.y),
        });
        Ok(())
    }

    fn offset_now(&self) -> u64 {
        self.started.map_or(0, |start| {
            u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
        })
    }

    #[must_use]
    pub fn samples(&self) -> &[RecordedSample] {
        &self.samples
    }

    pub fn clear(&mut self) {
        self.samples.clear();
        self.last_mask = None;
        self.last_offset_ms = 0;
    }

    /// Writes the current samples to `path` (CSV with a header).
    pub fn save_csv(&self, path: &Path) -> Result<(), RecordError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| RecordError::Io(error.to_string()))?;
        }
        let mut text = String::from("time_ms,buttons,hat,left_x,left_y,right_x,right_y\n");
        for sample in &self.samples {
            let _ignored = writeln!(
                text,
                "{},{},{},{},{},{},{}",
                sample.offset_ms,
                sample.buttons.join("|"),
                sample.hat,
                sample.left.0,
                sample.left.1,
                sample.right.0,
                sample.right.1
            );
        }
        let mut file = atomic_write(path).map_err(|error| RecordError::Io(error.to_string()))?;
        file.write_all(text.as_bytes())
            .map_err(|error| RecordError::Io(error.to_string()))?;
        file.commit()
            .map_err(|error| RecordError::Io(error.to_string()))?;
        Ok(())
    }
}

fn atomic_write(path: &Path) -> std::io::Result<atomic_write_file::AtomicWriteFile> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "path has no parent")
    })?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    let options = {
        use atomic_write_file::unix::OpenOptionsExt as AtomicOpenOptionsExt;
        use std::os::unix::fs::OpenOptionsExt as StdOpenOptionsExt;

        let mut options = atomic_write_file::OpenOptions::new();
        options.mode(0o600);
        options.preserve_mode(path.exists());
        options
    };
    #[cfg(not(unix))]
    let options = atomic_write_file::OpenOptions::new();
    options.open(path)
}

/// Reads a recorded CSV back into samples.
#[allow(dead_code, reason = "reserved for file-based record generation")]
pub fn parse_csv(text: &str) -> Result<Vec<RecordedSample>, RecordError> {
    let mut samples = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            if line.trim() != "time_ms,buttons,hat,left_x,left_y,right_x,right_y" {
                return Err(RecordError::InvalidCsv("header".to_owned()));
            }
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != 7 {
            return Err(RecordError::InvalidCsv(format!("line {}", index + 1)));
        }
        let offset_ms = fields[0]
            .parse::<u64>()
            .map_err(|_error| RecordError::InvalidCsv(format!("time at line {}", index + 1)))?;
        let hat = hat_from_name(fields[2])
            .ok_or_else(|| RecordError::InvalidCsv(format!("hat at line {}", index + 1)))?;
        let parse_stick = |value: &str| -> Result<u8, RecordError> {
            value
                .parse::<u8>()
                .map_err(|_error| RecordError::InvalidCsv(format!("stick at line {}", index + 1)))
        };
        samples.push(RecordedSample {
            offset_ms,
            buttons: button_names(parse_mask(fields[1])),
            hat: hat_name(hat),
            left: (parse_stick(fields[3])?, parse_stick(fields[4])?),
            right: (parse_stick(fields[5])?, parse_stick(fields[6])?),
        });
    }
    Ok(samples)
}

/// Builds a `PythonCommand` script that replays the recorded button timeline.
pub fn generate_script(
    class_name: &str,
    command_name: &str,
    samples: &[RecordedSample],
) -> Result<String, RecordError> {
    if !is_valid_identifier(class_name) {
        return Err(RecordError::InvalidName);
    }
    let mut lines = vec![
        "from Commands.PythonCommandBase import PythonCommand".to_owned(),
        String::new(),
        String::new(),
        format!("class {class_name}(PythonCommand):"),
        format!("    NAME = {command_name:?}"),
        "    TAGS = [\"Record\"]".to_owned(),
        String::new(),
        "    def do(self):".to_owned(),
    ];
    let actions = replay_actions(samples);
    if actions.is_empty() {
        lines.push("        pass".to_owned());
    } else {
        for action in actions {
            lines.push(format!("        {action}"));
        }
    }
    lines.push(String::new());
    Ok(lines.join("\n"))
}

fn is_valid_identifier(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[allow(
    clippy::cast_precision_loss,
    reason = "recorded millisecond offsets comfortably fit an f64 mantissa"
)]
fn replay_actions(samples: &[RecordedSample]) -> Vec<String> {
    let mut actions: Vec<String> = Vec::new();
    if samples.len() < 2 {
        return actions;
    }
    let mut index = 0usize;
    while index + 1 < samples.len() {
        let start = samples[index].offset_ms;
        let end = samples[index + 1].offset_ms;
        let duration = (end.saturating_sub(start)) as f64 / 1000.0;
        let mask = parse_mask(&samples[index].buttons.join("|"));
        let next_mask = parse_mask(&samples[index + 1].buttons.join("|"));
        if mask == 0 {
            push_wait(&mut actions, duration);
            index += 1;
            continue;
        }
        let total = (end.saturating_sub(start)) as f64 / 1000.0;
        let names = button_names(mask)
            .into_iter()
            .map(|name| format!("Button.{name}"))
            .collect::<Vec<_>>();
        if next_mask == 0 && total <= TAP_MAX_SECONDS {
            actions.push(format!(
                "self.press([{}], {:.3})",
                names.join(", "),
                total.max(0.01)
            ));
        } else {
            actions.push(format!("self.hold([{}])", names.join(", ")));
            push_wait(&mut actions, total);
            actions.push(format!("self.holdEnd([{}])", names.join(", ")));
        }
        index += 1;
    }
    actions
}

fn push_wait(actions: &mut Vec<String>, duration: f64) {
    if duration < MIN_WAIT_SECONDS {
        return;
    }
    actions.push(format!("self.wait({duration:.3})"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_at(offset_ms: u64, buttons: &[&str]) -> RecordedSample {
        RecordedSample {
            offset_ms,
            buttons: buttons.iter().map(|name| (*name).to_owned()).collect(),
            hat: "Neutral",
            left: (128, 128),
            right: (128, 128),
        }
    }

    #[test]
    fn replays_a_tap_as_press() {
        let samples = [
            sample_at(0, &["A"]),
            sample_at(120, &[]),
            sample_at(300, &[]),
        ];
        let script = generate_script("Recorded", "recorded", &samples).unwrap();
        assert!(script.contains("self.press([Button.A], 0.120)"), "{script}");
    }

    #[test]
    fn replays_a_long_hold_and_waits() {
        let samples = [
            sample_at(0, &["A", "B"]),
            sample_at(1500, &[]),
            sample_at(1800, &[]),
        ];
        let script = generate_script("Recorded", "recorded", &samples).unwrap();
        assert!(
            script.contains("self.hold([Button.A, Button.B])"),
            "{script}"
        );
        assert!(script.contains("self.wait(1.500)"), "{script}");
        assert!(
            script.contains("self.holdEnd([Button.A, Button.B])"),
            "{script}"
        );
        assert!(script.contains("self.wait(0.300)"), "{script}");
    }

    #[test]
    fn csv_round_trips_and_rejects_bad_headers() {
        let samples = [sample_at(0, &["A", "ZL"]), sample_at(50, &[])];
        let mut text = String::from("time_ms,buttons,hat,left_x,left_y,right_x,right_y\n");
        for sample in &samples {
            let _ignored = writeln!(
                text,
                "{},{},{},{},{},{},{}",
                sample.offset_ms,
                sample.buttons.join("|"),
                sample.hat,
                sample.left.0,
                sample.left.1,
                sample.right.0,
                sample.right.1
            );
        }
        assert_eq!(parse_csv(&text).unwrap(), samples);
        assert!(matches!(
            parse_csv("bad,header\n"),
            Err(RecordError::InvalidCsv(_))
        ));
    }

    #[test]
    fn rejects_invalid_class_names() {
        assert!(generate_script("1Bad", "name", &[]).is_err());
        assert!(generate_script("Bad Name", "name", &[]).is_err());
    }

    #[test]
    fn recorder_requires_start_and_deduplicates() {
        let mut recorder = InputRecorder::new();
        assert_eq!(
            recorder.record(&ControllerState::NEUTRAL),
            Err(RecordError::NotRecording)
        );
        recorder.start().unwrap();
        assert_eq!(recorder.start(), Err(RecordError::AlreadyRecording));
        recorder.record(&ControllerState::NEUTRAL).unwrap();
        recorder.record(&ControllerState::NEUTRAL).unwrap();
        assert_eq!(recorder.samples().len(), 1);
        recorder.stop().unwrap();
        assert!(!recorder.is_recording());
    }

    #[test]
    fn recorder_enforces_the_sample_limit() {
        let mut recorder = InputRecorder::with_limit(1);
        recorder.start().unwrap();
        recorder.record(&ControllerState::NEUTRAL).unwrap();
        let mut pressed = ControllerState::NEUTRAL;
        pressed.buttons.a = true;
        assert_eq!(recorder.record(&pressed), Err(RecordError::LimitReached));
    }
}
