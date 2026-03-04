//! `(tein time)` — sandbox-safe r7rs time procedures.
//!
//! provides:
//! - `current-second` — wall-clock POSIX time as inexact seconds since epoch
//! - `current-jiffy` — monotonic nanosecond counter (exact integer)
//! - `jiffies-per-second` — constant 10⁹
//! - `timezone-offset-seconds` — local timezone UTC offset in seconds (via libc `localtime_r`)

use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tein_macros::tein_module;

/// monotonic epoch for jiffies — set on first `current-jiffy` call,
/// constant for the rest of the program run (per r7rs).
static JIFFY_EPOCH: OnceLock<Instant> = OnceLock::new();

/// query local timezone offset via libc.
///
/// uses `localtime_r` to read `tm_gmtoff` from the system timezone database.
/// returns UTC offset in seconds (e.g. UTC+1 → 3600, UTC-5 → -18000, UTC → 0).
/// on failure, returns 0 (UTC) as a safe fallback.
fn local_utc_offset_seconds() -> i64 {
    use std::mem::MaybeUninit;

    use std::ffi::{c_char, c_long};

    unsafe extern "C" {
        fn time(tloc: *mut i64) -> i64;
        fn localtime_r(timep: *const i64, result: *mut Tm) -> *mut Tm;
    }

    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: c_long,
        tm_zone: *const c_char,
    }

    unsafe {
        let mut t: i64 = 0;
        time(&mut t);
        let mut tm = MaybeUninit::<Tm>::uninit();
        let result = localtime_r(&t, tm.as_mut_ptr());
        if result.is_null() {
            return 0;
        }
        (*result).tm_gmtoff as i64
    }
}

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

    /// return local timezone's UTC offset in seconds.
    ///
    /// e.g. UTC+1 → 3600, UTC-5 → -18000, UTC → 0.
    /// uses libc `localtime_r` to query the system timezone.
    /// returns real timezone even in sandboxed contexts.
    #[tein_fn(name = "timezone-offset-seconds")]
    pub fn timezone_offset_seconds() -> i64 {
        super::local_utc_offset_seconds()
    }
}
