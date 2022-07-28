pub mod error;
pub mod raft;
pub mod node;
pub mod status;
pub mod read_only;
pub mod log_unstable;

/// The default logger we fall back to when passed `None` in external facing constructors.
///
/// Currently, this is a `log` adaptor behind a `Once` to ensure there is no clobbering.
#[cfg(any(test, feature = "default-logger"))]
pub fn default_logger() -> slog::Logger {
    use slog::{o, Drain};
    use std::sync::{Mutex, Once};

    static LOGGER_INITIALIZED: Once = Once::new();
    static mut LOGGER: Option<slog::Logger> = None;

    let logger = unsafe {
        LOGGER_INITIALIZED.call_once(|| {
            let decorator = slog_term::TermDecorator::new().build();
            let drain = slog_term::CompactFormat::new(decorator).build();
            let drain = slog_envlogger::new(drain);
            LOGGER = Some(slog::Logger::root(Mutex::new(drain).fuse(), o!()));
        });
        LOGGER.as_ref().unwrap()
    };
    if let Some(case) = std::thread::current()
        .name()
        .and_then(|v| v.split(':').last())
    {
        logger.new(o!("case" => case.to_string()))
    } else {
        logger.new(o!())
    }
}