//! The one place that reads the clock or waits on it. Everything else
//! takes the time it works with as a parameter.

use chrono::{DateTime, Local, TimeZone};
use std::time::Instant;

/// The moment an event happened, on both clocks.
#[derive(Clone, Debug)]
pub(crate) struct Now<Tz: TimeZone> {
    /// For durations within a run. It stops while the Mac sleeps, so time
    /// asleep is neither counted down nor tracked.
    pub(crate) monotonic: Instant,
    /// For booking tracked time to calendar days.
    pub(crate) wall: DateTime<Tz>,
}

/// Now on both clocks.
pub(crate) fn now() -> Now<Local> {
    Now {
        monotonic: monotonic_now(),
        wall: local_now(),
    }
}

/// Now on the monotonic clock, for measuring how long something runs.
pub(crate) fn monotonic_now() -> Instant {
    Instant::now()
}

/// Now in local wall-clock time, for booking tracked time to calendar days.
pub(crate) fn local_now() -> DateTime<Local> {
    Local::now()
}

/// Waits until `deadline` on the monotonic clock, which stops while the Mac
/// sleeps.
pub(crate) async fn sleep_until(deadline: Instant) {
    tokio::time::sleep_until(deadline.into()).await;
}
