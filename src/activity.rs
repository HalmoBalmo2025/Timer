//! Keeps macOS from throttling the app's timers.
//!
//! macOS saves energy in apps that are not in front: after a minute or two
//! in the background, App Nap and timer coalescing let their timers fire up
//! to about 100 ms late, a different amount each time. The countdown window
//! floats above other apps exactly while they are in front, so one second
//! is shown too long and the next too short. An activity marked
//! user-initiated and latency-critical opts the app out of both; measured,
//! wake-ups then stay within a few milliseconds.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSActivityOptions, NSObjectProtocol, NSProcessInfo, NSString};

/// The running activity. It lasts until the process ends: ending it early
/// would need the `unsafe` `endActivity`, and an idle timer has no timers
/// to throttle anyway. Dropping the token does not end the activity.
pub(crate) struct PreciseTimers {
    _token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

/// Starts the activity; keep the result alive for the whole run.
pub(crate) fn keep_timers_precise() -> PreciseTimers {
    let token = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep
            | NSActivityOptions::LatencyCritical,
        &NSString::from_str("Countdown display must change exactly on the second"),
    );
    PreciseTimers { _token: token }
}
