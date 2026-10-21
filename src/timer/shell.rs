//! The Dioxus side of the timer: its state in a signal that every component
//! reaches through the context, the effects performed, and the ticker that
//! wakes it while it runs.

use chrono::NaiveDate;
use dioxus::prelude::*;

use super::{Effect, Event, NumberView, Ticking, Timer, Tracked};
use crate::clock::{monotonic_now, now, sleep_until};
use crate::storage::{DataFileLock, LockedDataFile, Storage, lock_data_file};
use crate::storage_problem::storage_problem_text;

/// The timer and the duration field, shared by the countdown card, the
/// tracker card and the window.
#[derive(Clone, Copy)]
pub(crate) struct TimerHandle {
    timer: Signal<Timer>,
    /// The text in the duration field; set by typing and by
    /// [`Effect::ShowDuration`].
    duration_input: Signal<String>,
    /// Held while the app runs: `Some` exactly when the data file could be
    /// locked, so the timer may write it.
    _data_file_lock: Signal<Option<DataFileLock>>,
}

/// Opens the data file, provides the timer to every component below, and
/// runs its ticker. Called once, in `App`.
pub(crate) fn use_timer() -> TimerHandle {
    let handle = use_context_provider(|| {
        let (path, lock) = match lock_data_file() {
            Ok(LockedDataFile { path, lock }) => (Ok(path), Some(lock)),
            Err(error) => (Err(error), None),
        };
        let timer = Timer::opened(path, Storage::load);
        let duration_input = Signal::new(timer.countdown().to_string());
        TimerHandle {
            timer: Signal::new(timer),
            duration_input,
            _data_file_lock: Signal::new(lock),
        }
    });
    let ticking = use_memo(move || handle.timer.read().ticking());
    // Reading `ticking` restarts this task on Start, Stop and Reset; the
    // deadlines in between come from the timer itself. The task belongs to
    // `App` and ends with it.
    use_resource(move || async move {
        match ticking() {
            Ticking::Idle => {}
            Ticking::Round(_) => loop {
                let next = handle.timer.peek().next_tick(monotonic_now());
                let Some(deadline) = next else { break };
                sleep_until(deadline).await;
                handle.dispatch(Event::Tick);
            },
        }
    });
    handle
}

impl TimerHandle {
    /// Steps the timer with `event`, happening now, and performs the
    /// effects.
    pub(crate) fn dispatch(mut self, event: Event) {
        let effects = self.timer.write().step(event, &now());
        for effect in effects {
            match effect {
                Effect::Save(request) => {
                    let result = request.storage.save(&request.path);
                    self.dispatch(Event::Saved(result));
                }
                Effect::ShowDuration(total) => self.duration_input.set(total.to_string()),
            }
        }
    }

    /// Start/Stop with what the duration field holds.
    pub(crate) fn start_stop(self) {
        // Parsed before the step: the step writes the field.
        let input = self.duration_input.peek().parse();
        self.dispatch(Event::StartStop(input));
    }

    pub(crate) const fn duration_input(self) -> Signal<String> {
        self.duration_input
    }

    pub(crate) fn number(self) -> NumberView {
        self.timer.read().number()
    }

    pub(crate) fn tracked(self, today: NaiveDate) -> Tracked {
        self.timer.read().tracked(today)
    }

    /// Why tracked time is not being saved, in words for the user.
    pub(crate) fn storage_problem(self) -> Option<String> {
        storage_problem_text(self.timer.read().data_file())
    }
}
