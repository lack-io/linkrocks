#[macro_export]
macro_rules! fatal {
    ($logger:expr, $msg:expr) => {{
        let owned_kv = ($logger).list();
        let s = crate::util::format_kv_list(&owned_kv);
        if s.is_empty() {
            panic!("{}", $msg)
        } else {
            panic!("{}, {}", $msg, s)
        }
    }};
    ($logger:expr, $fmt:expr, $($arg:tt)+) => {{
        fatal!($logger, format_args!($fmt, $($arg)+))
    }};
}

pub mod errors;
mod log_unstable;
pub mod bootstrap;
pub mod node;
pub mod raw_node;
pub mod quorum;
pub mod raft;
pub mod raft_log;
mod read_only;
pub mod status;
pub mod config;
pub mod storage;
pub mod tracker;
pub mod confchange;
pub mod util;

pub use crate::raft::{INVALID_ID, INVALID_INDEX};

pub mod prelude {
    pub use raftpb::prelude::*;
}

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

type DefaultHashBuilder = std::hash::BuildHasherDefault<fxhash::FxHasher>;
type HashMap<K, V> = std::collections::HashMap<K, V, DefaultHashBuilder>;
type HashSet<K> = std::collections::HashSet<K, DefaultHashBuilder>;
