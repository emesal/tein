//! `(tein time)` — sandbox-safe r7rs time procedures.
//!
//! provides:
//! - `current-second` — wall-clock POSIX time as inexact seconds since epoch
//! - `current-jiffy` — monotonic nanosecond counter (exact integer)
//! - `jiffies-per-second` — constant 10⁹

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tein_macros::tein_module;

/// monotonic epoch for jiffies — set on first `current-jiffy` call,
/// constant for the rest of the program run (per r7rs).
static JIFFY_EPOCH: OnceLock<Instant> = OnceLock::new();

#[tein_module("time")]
pub(crate) mod time_impl {
    /// nanoseconds per second — the jiffy resolution constant.
    #[allow(dead_code)]
    #[tein_const]
    pub const JIFFIES_PER_SECOND: i64 = 1_000_000_000;

    /// return current wall-clock time as inexact seconds since the POSIX epoch.
    ///
    /// r7rs specifies TAI but explicitly allows UTC with a constant offset.
    /// we return UTC-based time, matching common implementation practice.
    #[tein_fn(name = "current-second")]
    pub fn current_second() -> f64 {
        super::SystemTime::now()
            .duration_since(super::UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_secs_f64()
    }

    /// return elapsed nanoseconds since a process-relative epoch (exact integer).
    ///
    /// the epoch is set on first call and remains constant within a program run,
    /// per r7rs. uses `Instant` for monotonic timing.
    #[tein_fn(name = "current-jiffy")]
    pub fn current_jiffy() -> i64 {
        let epoch = super::JIFFY_EPOCH.get_or_init(super::Instant::now);
        epoch.elapsed().as_nanos() as i64
    }
}
