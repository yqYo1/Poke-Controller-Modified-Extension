use thiserror::Error;
use tracing_subscriber::{
    EnvFilter,
    layer::SubscriberExt,
    util::{SubscriberInitExt, TryInitError},
};

/// Builds the run's log file name: `log_<profile>_<timestamp>.log`.
///
/// Mirrors the upstream `PokeConLogger.log_file_name`: characters that are
/// unsafe in file names are replaced with `_`, the sanitized profile is
/// stripped, and an empty result becomes `_`.
#[must_use]
pub fn log_file_name(profile: &str, timestamp: &str) -> String {
    let safe: String = profile
        .chars()
        .map(|character| {
            if matches!(
                character,
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    let safe = safe.trim();
    let safe = if safe.is_empty() { "_" } else { safe };
    format!("log_{safe}_{timestamp}.log")
}

/// Timestamp format used in log file names (upstream `%Y%m%d-%H%M%S`).
pub const LOG_FILE_TIMESTAMP_FORMAT: &str = "%Y%m%d-%H%M%S";

/// Diagnostic emitted when a managed process begins startup.
pub const APP_STARTING: &str = "POKECON-RUNTIME-0001";
/// Diagnostic emitted after a managed process has stopped cleanly.
pub const APP_STOPPED: &str = "POKECON-RUNTIME-0002";
/// Diagnostic emitted when the common shutdown coordinator accepts a request.
pub const SHUTDOWN_REQUESTED: &str = "POKECON-RUNTIME-0003";
/// Diagnostic emitted if an operating-system signal handler cannot be installed.
pub const SIGNAL_HANDLER_FAILED: &str = "POKECON-RUNTIME-0004";

/// Error returned when the global tracing subscriber cannot be installed.
#[derive(Debug, Error)]
pub enum TracingInitError {
    /// The subscriber was already installed elsewhere.
    #[error("failed to install the global tracing subscriber: {0}")]
    Subscriber(#[source] TryInitError),
    /// The run's log file could not be created.
    #[error("failed to prepare the log file: {0}")]
    File(#[source] std::io::Error),
}

/// Installs structured JSON logging with an environment-filter override.
///
/// When `log_file` is provided, the same JSON stream is also written to that
/// file (the run's `log_<profile>_<timestamp>.log`); the parent directory is
/// created when missing, matching the upstream `./log` folder behavior.
///
/// # Errors
///
/// Returns an error when another global tracing subscriber was already
/// installed or the log file cannot be created.
pub fn init_tracing(
    default_filter: &str,
    log_file: Option<&std::path::Path>,
) -> Result<(), TracingInitError> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    let file_writer = match log_file {
        Some(path) => Some(std::sync::Arc::new(create_log_file(path)?)),
        None => None,
    };
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json());
    match file_writer {
        Some(writer) => subscriber
            .with(tracing_subscriber::fmt::layer().json().with_writer(writer))
            .try_init()
            .map_err(TracingInitError::Subscriber),
        None => subscriber.try_init().map_err(TracingInitError::Subscriber),
    }
}

fn create_log_file(path: &std::path::Path) -> Result<std::fs::File, TracingInitError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent).map_err(|error| {
            TracingInitError::File(std::io::Error::new(
                error.kind(),
                format!(
                    "failed to create log directory {}: {error}",
                    parent.display()
                ),
            ))
        })?;
    }
    std::fs::File::create(path).map_err(TracingInitError::File)
}

/// Installs structured JSON logging on stderr.
///
/// Worker stdout is reserved for the framed IPC protocol, so managed worker
/// processes must use this initializer instead of [`init_tracing`].
///
/// # Errors
///
/// Returns an error when another global tracing subscriber was already installed.
pub fn init_tracing_to_stderr(default_filter: &str) -> Result<(), TracingInitError> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(std::io::stderr),
        )
        .try_init()
        .map_err(TracingInitError::Subscriber)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_log_directory_is_created_with_the_log_file() {
        let directory = tempfile::TempDir::new().expect("temporary directory must exist");
        let path = directory.path().join("log").join("log_default.log");
        create_log_file(&path).expect("log file must be created");
        assert!(path.is_file());
    }

    #[test]
    fn log_file_name_follows_the_upstream_shape() {
        assert_eq!(
            log_file_name("default", "20261015-153000"),
            "log_default_20261015-153000.log"
        );
    }

    #[test]
    fn unsafe_characters_are_replaced_with_underscores() {
        assert_eq!(
            log_file_name(r"a/b\c:d*e?f", "20261015-153000"),
            "log_a_b_c_d_e_f_20261015-153000.log"
        );
        assert_eq!(
            log_file_name("x<y>z|q\"r", "20261015-153000"),
            "log_x_y_z_q_r_20261015-153000.log"
        );
    }

    #[test]
    fn control_characters_and_whitespace_fall_back_to_underscore() {
        assert_eq!(
            log_file_name("\u{7}abc\n", "20261015-153000"),
            "log__abc__20261015-153000.log"
        );
        assert_eq!(
            log_file_name("   ", "20261015-153000"),
            "log___20261015-153000.log"
        );
    }
}
