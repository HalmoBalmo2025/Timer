//! The timer as one pure state machine: the repeating countdown, the time
//! tracking that runs with it, the tracked times it books, and whether they
//! can be saved. [`Timer::step`] takes each event with the moment it
//! happened and returns the effects for the shell to perform; nothing here
//! reads the clock or touches a file.

mod shell;

pub(crate) use shell::{TimerHandle, use_timer};

use chrono::{DateTime, NaiveDate, SubsecRound, TimeDelta, TimeZone};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::calendar::{month_dates, week_dates};
use crate::clock::Now;
use crate::duration::{
    CountdownDuration, DurationError, countdown_remaining, next_countdown_change,
};
use crate::storage::{Seconds, Storage, StorageError};

/// Time between checkpoints of a running session to disk. A crash or
/// force-quit loses at most this much; a normal close loses nothing.
pub(crate) const CHECKPOINT_INTERVAL: Seconds = Seconds::new(10);

/// Everything the countdown card and the tracker card show, and the data
/// behind it.
#[derive(Debug)]
pub(crate) struct Timer {
    phase: Phase,
    ledger: Ledger,
}

/// What the timer is doing. The duration can only be edited while stopped;
/// a round and a session only exist while running.
#[derive(Debug)]
enum Phase {
    Stopped(Field),
    Running(Run),
}

/// Countdown and tracking run together: Start begins both, Stop ends both.
#[derive(Debug)]
struct Run {
    cycle: Cycle,
    session: Session,
}

/// The countdown round in progress.
#[derive(Debug)]
struct Cycle {
    total: CountdownDuration,
    /// When this round began; Reset begins a new one.
    start: Instant,
    /// Whole seconds shown.
    remaining: u64,
}

/// The tracking session in progress.
#[derive(Debug)]
struct Session {
    start: Instant,
    /// Whole seconds shown.
    elapsed: Seconds,
    /// Seconds already booked to the storage, so checkpoints and the final
    /// booking on Stop never count a second twice. Never more than
    /// `elapsed`.
    booked: Seconds,
}

/// The tracked times and where they are saved.
#[derive(Debug)]
struct Ledger {
    storage: Storage,
    data_file: DataFile,
}

/// While stopped, the number is shown styled (with a slim decimal point)
/// and only turns into a text field once it is clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Showing,
    Editing,
}

/// Whether the storage can be written back to disk.
#[derive(Debug)]
pub(crate) enum DataFile {
    /// Saved to `path`; `last_error` is set while saving fails.
    Writable {
        path: PathBuf,
        last_error: Option<StorageError>,
    },
    /// Never written: there is no place for it, or the existing file could
    /// not be read, and writing it would replace the times it holds.
    Unavailable(StorageError),
}

/// Something that happened to the timer.
#[derive(Debug)]
pub(crate) enum Event {
    /// Start/Stop was pressed (button, `S` or Ctrl+#), with the duration
    /// field's content.
    StartStop(Result<CountdownDuration, DurationError>),
    /// Reset was pressed: the countdown starts its round again, tracking
    /// runs on.
    Reset,
    /// The duration was clicked or left: show it or edit it.
    Field(Field),
    /// The ticker woke up: the shown values may have changed.
    Tick,
    /// The window closes: book everything not on disk yet.
    Checkpoint,
    /// How an [`Effect::Save`] went.
    Saved(Result<(), StorageError>),
}

/// Whether an event is a moment to bring the disk up to date.
enum Flush {
    Yes,
    No,
}

impl Flush {
    const fn of(event: &Event) -> Self {
        match event {
            Event::StartStop(_) | Event::Checkpoint => Self::Yes,
            Event::Reset | Event::Field(_) | Event::Tick | Event::Saved(_) => Self::No,
        }
    }
}

/// What the shell does after a step.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    /// Write the storage to the data file, then report back with
    /// [`Event::Saved`].
    Save(SaveRequest),
    /// Put this duration into the duration field.
    ShowDuration(CountdownDuration),
}

/// The storage to write and where.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SaveRequest {
    pub(crate) storage: Storage,
    pub(crate) path: PathBuf,
}

/// What the big number shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumberView {
    /// Running: the whole seconds left.
    Remaining(u64),
    /// Stopped: the duration, turning into a text field when clicked.
    Duration,
    /// Stopped: the text field for the duration.
    Editing,
}

/// What the ticker follows. It starts over whenever this changes, so a new
/// round never waits for a deadline of the old one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ticking {
    Idle,
    /// Running, in the round that began at this instant.
    Round(Instant),
}

/// The tracked times the tracker card shows, the running session included.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Tracked {
    /// Whole seconds of the running session, zero when stopped.
    pub(crate) session: Seconds,
    /// Monday to Sunday of the current week.
    pub(crate) week: Vec<TrackedDay>,
    pub(crate) week_total: Seconds,
    /// The month the current week is booked to (`calendar::month_dates`).
    pub(crate) month_total: Seconds,
}

/// The time tracked on one day.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TrackedDay {
    pub(crate) date: NaiveDate,
    pub(crate) seconds: Seconds,
}

impl Timer {
    /// A stopped timer with the data file found at `path` and read by
    /// `load`. If either fails, it starts empty and never writes the file.
    pub(crate) fn opened(
        path: Result<PathBuf, StorageError>,
        load: impl FnOnce(&Path) -> Result<Storage, StorageError>,
    ) -> Self {
        let ledger = match path {
            Err(error) => Ledger::unavailable(error),
            Ok(path) => match load(&path) {
                Ok(storage) => Ledger {
                    storage,
                    data_file: DataFile::Writable {
                        path,
                        last_error: None,
                    },
                },
                Err(error) => Ledger::unavailable(error),
            },
        };
        Self {
            phase: Phase::Stopped(Field::Showing),
            ledger,
        }
    }

    /// Applies `event`, which happened at `now`, and returns what the shell
    /// has to do.
    pub(crate) fn step<Tz: TimeZone>(&mut self, event: Event, now: &Now<Tz>) -> Vec<Effect> {
        let mut effects = Vec::new();
        let flush = Flush::of(&event);
        let Self { phase, ledger } = self;
        match phase {
            Phase::Stopped(field) => match event {
                Event::StartStop(input) => {
                    // An empty or zero entry, the only invalid ones the field
                    // holds, starts the duration used last time.
                    let total = input.unwrap_or_else(|_: DurationError| ledger.storage.countdown());
                    effects.push(Effect::ShowDuration(total));
                    ledger.set_countdown(total, &mut effects);
                    *phase = Phase::Running(Run {
                        cycle: Cycle::begin(total, now.monotonic),
                        session: Session::begin(now.monotonic),
                    });
                }
                Event::Field(new) => *field = new,
                Event::Saved(result) => ledger.saved(result),
                // Nothing runs: nothing to rewind, show or book.
                Event::Reset | Event::Tick | Event::Checkpoint => {}
            },
            Phase::Running(run) => match event {
                Event::StartStop(_) => {
                    run.session.catch_up(now.monotonic);
                    ledger.book(&mut run.session, &now.wall, &mut effects);
                    *phase = Phase::Stopped(Field::Showing);
                }
                Event::Reset => run.cycle = Cycle::begin(run.cycle.total, now.monotonic),
                Event::Tick => {
                    run.cycle.catch_up(now.monotonic);
                    run.session.catch_up(now.monotonic);
                    if run.session.unbooked() >= CHECKPOINT_INTERVAL {
                        ledger.book(&mut run.session, &now.wall, &mut effects);
                    }
                }
                Event::Checkpoint => {
                    run.session.catch_up(now.monotonic);
                    ledger.book(&mut run.session, &now.wall, &mut effects);
                }
                Event::Saved(result) => ledger.saved(result),
                // The field vanishing on Start blurs it; that must not
                // touch the running round.
                Event::Field(_) => {}
            },
        }
        // Start, Stop and closing the window also retry a save that failed,
        // so times that never reached the disk are not lost when nothing
        // new was booked since.
        match flush {
            Flush::Yes => ledger.retry_failed_save(&mut effects),
            Flush::No => {}
        }
        effects
    }

    /// The duration the countdown was last started with.
    pub(crate) const fn countdown(&self) -> CountdownDuration {
        self.ledger.storage.countdown()
    }

    pub(crate) const fn number(&self) -> NumberView {
        match &self.phase {
            Phase::Running(run) => NumberView::Remaining(run.cycle.remaining),
            Phase::Stopped(Field::Showing) => NumberView::Duration,
            Phase::Stopped(Field::Editing) => NumberView::Editing,
        }
    }

    pub(crate) const fn data_file(&self) -> &DataFile {
        &self.ledger.data_file
    }

    pub(crate) const fn ticking(&self) -> Ticking {
        match &self.phase {
            Phase::Running(run) => Ticking::Round(run.cycle.start),
            Phase::Stopped(_) => Ticking::Idle,
        }
    }

    /// When the shown values next change after `now`: the countdown's next
    /// step or the session's next whole second, whichever comes first.
    /// `None` while stopped.
    ///
    /// Deadlines are anchored to the starts, so a late wake-up delays one
    /// change but never the next.
    pub(crate) fn next_tick(&self, now: Instant) -> Option<Instant> {
        match &self.phase {
            Phase::Running(run) => run
                .cycle
                .next_change(now)
                .into_iter()
                .chain(run.session.next_second(now))
                .min(),
            Phase::Stopped(_) => None,
        }
    }

    /// The tracked times of the week and month around `today`, with the
    /// unbooked seconds of the running session added to `today`.
    pub(crate) fn tracked(&self, today: NaiveDate) -> Tracked {
        let (session, unbooked) = match &self.phase {
            Phase::Running(run) => (run.session.elapsed, run.session.unbooked()),
            Phase::Stopped(_) => (Seconds::ZERO, Seconds::ZERO),
        };
        let seconds_on = |date: NaiveDate| {
            let stored = self.ledger.storage.seconds_on(date);
            if date == today {
                stored.saturating_add(unbooked)
            } else {
                stored
            }
        };
        // Both are `None` only hundreds of thousands of years away, where
        // there is nothing tracked to show.
        let week: Vec<TrackedDay> = week_dates(today)
            .unwrap_or_default()
            .into_iter()
            .map(|date| TrackedDay {
                date,
                seconds: seconds_on(date),
            })
            .collect();
        let month_total = month_dates(today)
            .unwrap_or_default()
            .into_iter()
            .map(seconds_on)
            .sum();
        Tracked {
            session,
            week_total: week.iter().map(|day| day.seconds).sum(),
            week,
            month_total,
        }
    }

}

impl Cycle {
    fn begin(total: CountdownDuration, now: Instant) -> Self {
        Self {
            total,
            start: now,
            remaining: countdown_remaining(total, 0),
        }
    }

    /// Derives the shown value from the clock instead of counting ticks.
    fn catch_up(&mut self, now: Instant) {
        self.remaining = countdown_remaining(self.total, tenths_between(self.start, now));
    }

    fn next_change(&self, now: Instant) -> Option<Instant> {
        let next = next_countdown_change(self.total, tenths_between(self.start, now));
        self.start
            .checked_add(Duration::from_millis(next).checked_mul(100)?)
    }
}

impl Session {
    const fn begin(now: Instant) -> Self {
        Self {
            start: now,
            elapsed: Seconds::ZERO,
            booked: Seconds::ZERO,
        }
    }

    fn catch_up(&mut self, now: Instant) {
        self.elapsed = Seconds::new(since(self.start, now).as_secs());
    }

    fn next_second(&self, now: Instant) -> Option<Instant> {
        let next = since(self.start, now).as_secs().checked_add(1)?;
        self.start.checked_add(Duration::from_secs(next))
    }

    fn unbooked(&self) -> Seconds {
        debug_assert!(
            self.booked <= self.elapsed,
            "invariant: booked seconds never exceed the elapsed ones"
        );
        // Should the invariant break, nothing counts as unbooked.
        self.elapsed.saturating_sub(self.booked)
    }
}

impl Ledger {
    fn unavailable(error: StorageError) -> Self {
        Self {
            storage: Storage::default(),
            data_file: DataFile::Unavailable(error),
        }
    }

    /// Books the session's unbooked seconds as the wall-clock interval
    /// ending at `wall`, so seconds after midnight land on the new day, and
    /// saves them.
    fn book<Tz: TimeZone>(
        &mut self,
        session: &mut Session,
        wall: &DateTime<Tz>,
        effects: &mut Vec<Effect>,
    ) {
        let unbooked = session.unbooked();
        if unbooked == Seconds::ZERO {
            return;
        }
        // Whole seconds, so that splitting at midnight leaves no fractions.
        let to = wall.clone().trunc_subsecs(0);
        let from = i64::try_from(unbooked.get())
            .ok()
            .and_then(TimeDelta::try_seconds)
            .and_then(|length| to.clone().checked_sub_signed(length));
        debug_assert!(
            from.is_some(),
            "invariant: a session is shorter than millions of years"
        );
        // Should it not be, the seconds stay unbooked for the next attempt.
        let Some(from) = from else {
            return;
        };
        self.storage.book(&from, &to);
        session.booked = session.elapsed;
        self.save(effects);
    }

    /// Remembers the duration the countdown was started with.
    fn set_countdown(&mut self, total: CountdownDuration, effects: &mut Vec<Effect>) {
        if self.storage.countdown() != total {
            self.storage.set_countdown(total);
            self.save(effects);
        }
    }

    fn save(&self, effects: &mut Vec<Effect>) {
        match &self.data_file {
            DataFile::Writable {
                path,
                last_error: _,
            } => effects.push(Effect::Save(SaveRequest {
                storage: self.storage.clone(),
                path: path.clone(),
            })),
            DataFile::Unavailable(_) => {}
        }
    }

    /// Saves again if the last save failed and this step saves nothing:
    /// every save writes the whole storage, so one success catches up.
    fn retry_failed_save(&self, effects: &mut Vec<Effect>) {
        let saving = effects.iter().any(|effect| match effect {
            Effect::Save(_) => true,
            Effect::ShowDuration(_) => false,
        });
        match &self.data_file {
            DataFile::Writable {
                last_error: Some(_),
                path: _,
            } => {
                if !saving {
                    self.save(effects);
                }
            }
            DataFile::Writable {
                last_error: None,
                path: _,
            }
            | DataFile::Unavailable(_) => {}
        }
    }

    fn saved(&mut self, result: Result<(), StorageError>) {
        match &mut self.data_file {
            // A success clears the error: saving works again.
            DataFile::Writable {
                path: _,
                last_error,
            } => *last_error = result.err(),
            // Nothing is saved there, so no result belongs to it; the error
            // that keeps it unavailable stays.
            DataFile::Unavailable(_) => {}
        }
    }
}

/// How long after `start` the moment `now` is. Events are stamped after
/// the start they are measured from, so an earlier `now` cannot occur; it
/// would count as the start itself.
fn since(start: Instant, now: Instant) -> Duration {
    now.saturating_duration_since(start)
}

/// Whole tenths of a second from `start` to `now`.
fn tenths_between(start: Instant, now: Instant) -> u64 {
    // u64 tenths last 58 billion years; the countdown would stop there.
    u64::try_from(since(start, now).as_millis() / 100).unwrap_or(u64::MAX)
}
