//! Capturing every log record this crate emits while a journey drives it.
//!
//! The redaction journey's claim is about *every* record, not about the ones a
//! journey thought to look at, so this installs itself as the process's logger
//! at the most verbose level there is and keeps whatever arrives.

use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Metadata, Record};

/// Everything the process has logged since the logger was installed.
static CAPTURED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The logger itself, installed at most once per process.
static LOGGER: Capturing = Capturing;

/// Whether the logger has been installed.
static INSTALLED: OnceLock<()> = OnceLock::new();

/// A logger that keeps every record it is given.
struct Capturing;

impl log::Log for Capturing {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        let rendered = format!(
            "{} {} {}",
            level_name(record.level()),
            record.target(),
            record.args()
        );
        CAPTURED
            .lock()
            .expect("the capture is not poisoned")
            .push(rendered);
    }

    fn flush(&self) {}
}

/// The word one level is recorded under.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    }
}

/// Install the capturing logger, once.
pub fn capture_everything() {
    INSTALLED.get_or_init(|| {
        log::set_logger(&LOGGER).expect("no other logger is installed");
        log::set_max_level(LevelFilter::Trace);
    });
}

/// Every record captured so far, taking them.
///
/// # Panics
///
/// Panics when the capture is poisoned, which is a panic inside the logger.
pub fn taken() -> Vec<String> {
    std::mem::take(&mut *CAPTURED.lock().expect("the capture is not poisoned"))
}
