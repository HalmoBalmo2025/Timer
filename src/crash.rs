//! The panic hook. The release build aborts on a panic, and an app started
//! from the Finder has no terminal, so the hook also writes where the panic
//! happened to a file of its own under `~/Library/Logs/Timer/`, which
//! Console.app lists. It records the location and the thread only: the
//! panic message may carry user data.

use chrono::{DateTime, TimeZone};
use std::fmt::Display;
use std::fs::File;
use std::io::{self, Write};
use std::panic::{self, Location};
use std::path::{Path, PathBuf};
use std::thread;

use crate::clock::local_now;

/// Installs the hook; called first thing in `main`. Debug builds keep the
/// default output (message and backtrace) as well.
pub(crate) fn install() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let report = report(info.location(), thread::current().name());
        record(
            &report,
            dirs::home_dir().as_deref(),
            &local_now(),
            &mut io::stderr(),
        );
        if cfg!(debug_assertions) {
            default_hook(info);
        }
    }));
}

/// Writes `report` to its file under `home`, then tells `stderr` where it
/// went. The file comes first and a failing `stderr` is ignored: started
/// with a closed pipe as stderr, `eprintln!` would panic inside the hook
/// and abort before the file is written.
pub(crate) fn record<Tz: TimeZone>(
    report: &str,
    home: Option<&Path>,
    now: &DateTime<Tz>,
    stderr: &mut impl Write,
) where
    Tz::Offset: Display,
{
    let written = home.map(|home| write_report(&log_folder(home), now, report));
    let told = match written {
        Some(Ok(path)) => writeln!(
            stderr,
            "{report}\nTimer wrote the crash report to {}",
            path.display()
        ),
        Some(Err(error)) => writeln!(
            stderr,
            "{report}\nTimer could not write the crash report: {error}"
        ),
        None => writeln!(
            stderr,
            "{report}\nTimer found no home folder for the crash report"
        ),
    };
    match told {
        // Nobody reads a stderr that fails; the file is all there is.
        Ok(()) | Err(_) => {}
    }
}

/// One line saying where the panic happened and on which thread.
pub(crate) fn report(location: Option<&Location<'_>>, thread: Option<&str>) -> String {
    let place = location.map_or_else(
        || "an unknown location".to_owned(),
        |location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        },
    );
    let thread = thread.unwrap_or("an unnamed thread");
    format!("Timer panicked at {place} on thread '{thread}'")
}

/// The app's folder under `~/Library/Logs`, for the home folder `home`.
pub(crate) fn log_folder(home: &Path) -> PathBuf {
    home.join("Library").join("Logs").join("Timer")
}

/// Writes `report` to a new file in `folder`, named after the local time
/// `now`, and returns its path. An existing file is never written over: a
/// second panic within the same second is reported on stderr only.
pub(crate) fn write_report<Tz: TimeZone>(
    folder: &Path,
    now: &DateTime<Tz>,
    report: &str,
) -> io::Result<PathBuf>
where
    Tz::Offset: Display,
{
    std::fs::create_dir_all(folder)?;
    let path = folder.join(format!("panic-{}.log", now.format("%Y-%m-%d-%H%M%S")));
    File::create_new(&path)?.write_all(format!("{report}\n").as_bytes())?;
    Ok(path)
}
