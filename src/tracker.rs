//! The tracker card: the running session, today, the week and the month,
//! and why tracked time is not being saved.

use chrono::{Datelike, NaiveDate};
use dioxus::prelude::*;
use std::time::Duration;

use crate::calendar::weekday_label;
use crate::clock::{local_now, monotonic_now, sleep_until};
use crate::storage::Seconds;
use crate::timer::TimerHandle;

/// `hh:mm:ss`; hours take a third digit only from 100 on.
pub(crate) fn format_hms(total: Seconds) -> String {
    let total = total.get();
    let hours = total / 3600;
    let minutes = total % 3600 / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

/// How often the card checks whether the date changed.
const DATE_CHECK: Duration = Duration::from_secs(60);

/// Today's date, kept current while the app runs. The card would otherwise
/// show the old day, week and month after midnight while stopped, since
/// nothing else re-renders it then. Checked once a minute instead of
/// sleeping until midnight: the monotonic clock stops while the Mac sleeps,
/// and the wall clock moves with time zone changes.
fn use_today() -> Signal<NaiveDate> {
    let mut today = use_signal(|| local_now().date_naive());
    use_future(move || async move {
        while let Some(deadline) = monotonic_now().checked_add(DATE_CHECK) {
            sleep_until(deadline).await;
            let date = local_now().date_naive();
            if *today.peek() != date {
                today.set(date);
            }
        }
    });
    today
}

#[component]
pub(crate) fn TimeTracker() -> Element {
    let timer = use_context::<TimerHandle>();
    let today = use_today()();
    let tracked = timer.tracked(today);
    let storage_problem = timer.storage_problem();

    rsx! {
        section { class: "tracker",
            div { class: "time-display", "{format_hms(tracked.session)}" }
            div { class: "week-list",
                div { class: "week-row total",
                    span { class: "week-day", "Week" }
                    span { class: "week-time", "{format_hms(tracked.week_total)}" }
                }
                for day in tracked.week {
                    div { class: if day.date == today { "week-row today" } else { "week-row" },
                        span { class: "week-day", "{weekday_label(day.date.weekday())}" }
                        span { class: "week-time", "{format_hms(day.seconds)}" }
                    }
                }
                div { class: "week-row total month",
                    span { class: "week-day", "Month" }
                    span { class: "week-time", "{format_hms(tracked.month_total)}" }
                }
            }
            if let Some(problem) = storage_problem {
                p { class: "storage-problem", "{problem}" }
            }
        }
    }
}
