//! The JSON data file: tracked seconds per day and the last countdown
//! duration.

use chrono::{DateTime, NaiveDate, TimeDelta, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write as _;
use std::iter::Sum;
use std::path::{Path, PathBuf};
use std::{fmt, io};

use crate::duration::CountdownDuration;

const ONE_SECOND: TimeDelta = TimeDelta::seconds(1);

/// A number of whole seconds of tracked time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Seconds(u64);

impl Seconds {
    pub(crate) const ZERO: Self = Self(0);

    pub(crate) const fn new(seconds: u64) -> Self {
        Self(seconds)
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }

    pub(crate) const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub(crate) const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl Sum for Seconds {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::ZERO, Self::saturating_add)
    }
}

/// Everything the app remembers between runs. It is written as
/// [`FileContents`], which adds the format version.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(into = "FileContents", from = "FileContents")]
pub(crate) struct Storage {
    days: BTreeMap<NaiveDate, Seconds>,
    countdown: CountdownDuration,
}

/// No tracked time and the default duration, for a first run.
impl Default for Storage {
    fn default() -> Self {
        Self {
            days: BTreeMap::new(),
            countdown: CountdownDuration::DEFAULT,
        }
    }
}

/// The data file as JSON.
#[derive(Serialize, Deserialize)]
struct FileContents {
    /// Always the current version when written. Files written before it
    /// was recorded are version 1; an unknown version does not load.
    #[serde(default = "FormatVersion::unrecorded")]
    version: FormatVersion,
    /// Tracked time per day, written as `"2026-08-25": 3600`.
    #[serde(default)]
    days: BTreeMap<NaiveDate, Seconds>,
    /// The duration last started with.
    #[serde(rename = "countdown_secs", default)]
    countdown: CountdownDuration,
}

impl From<Storage> for FileContents {
    fn from(storage: Storage) -> Self {
        Self {
            version: FormatVersion::CURRENT,
            days: storage.days,
            countdown: storage.countdown,
        }
    }
}

/// Every version this app knows has the shape of the current one.
impl From<FileContents> for Storage {
    fn from(contents: FileContents) -> Self {
        match contents.version {
            FormatVersion::V1 => Self {
                days: contents.days,
                countdown: contents.countdown,
            },
        }
    }
}

/// Only the version, read before the rest so that an unknown version is
/// told apart from a damaged file.
#[derive(Deserialize)]
struct VersionProbe {
    version: Option<u32>,
}

/// The versions of the data file this app can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
enum FormatVersion {
    /// Seconds per day under `days`, the countdown under `countdown_secs`.
    V1,
}

impl FormatVersion {
    /// The version files are written in.
    const CURRENT: Self = Self::V1;

    /// The version of a file that does not say: written by Timer 1.0.0.
    const fn unrecorded() -> Self {
        Self::V1
    }
}

/// A data file version this app does not know.
#[derive(Debug)]
pub(crate) struct UnknownVersion(u32);

/// Where an unknown version comes from.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum VersionOrigin {
    /// Above the current version: a newer Timer wrote the file.
    NewerApp,
    /// No Timer writes it (0): the file was edited.
    Invalid,
}

impl UnknownVersion {
    pub(crate) const fn get(&self) -> u32 {
        self.0
    }

    pub(crate) fn origin(&self) -> VersionOrigin {
        if self.0 > u32::from(FormatVersion::CURRENT) {
            VersionOrigin::NewerApp
        } else {
            VersionOrigin::Invalid
        }
    }
}

impl fmt::Display for UnknownVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "data file version {} is unknown to this app", self.0)
    }
}

impl TryFrom<u32> for FormatVersion {
    type Error = UnknownVersion;

    fn try_from(version: u32) -> Result<Self, Self::Error> {
        match version {
            1 => Ok(Self::V1),
            other => Err(UnknownVersion(other)),
        }
    }
}

impl From<FormatVersion> for u32 {
    fn from(version: FormatVersion) -> Self {
        match version {
            FormatVersion::V1 => 1,
        }
    }
}

/// What went wrong reading or writing the data file.
#[derive(Debug)]
pub(crate) enum StorageError {
    /// The system names no folder for application data.
    NoDataFolder,
    /// The data file exists but cannot be read.
    Read { path: PathBuf, source: io::Error },
    /// The data file is not valid JSON of the expected shape.
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    /// The data file is in a format version this app does not know.
    UnknownVersion {
        path: PathBuf,
        version: UnknownVersion,
    },
    /// Another running Timer holds the data file's lock.
    InUse { path: PathBuf },
    /// The lock file beside the data file could not be opened or locked.
    Lock { path: PathBuf, source: io::Error },
    /// The data could not be turned into JSON.
    Serialize(serde_json::Error),
    /// The data file or its folder could not be written.
    Write { path: PathBuf, source: io::Error },
}

impl Storage {
    /// Reads the data file. A file that does not exist yet is an empty
    /// storage; one that cannot be read, is in an unknown version or does
    /// not parse is an error.
    pub(crate) fn load(path: &Path) -> Result<Self, StorageError> {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => {
                return Err(StorageError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let parse_error = |source| StorageError::Parse {
            path: path.to_path_buf(),
            source,
        };
        let probe: VersionProbe = serde_json::from_str(&contents).map_err(parse_error)?;
        if let Some(Err(version)) = probe.version.map(FormatVersion::try_from) {
            return Err(StorageError::UnknownVersion {
                path: path.to_path_buf(),
                version,
            });
        }
        serde_json::from_str(&contents).map_err(parse_error)
    }

    /// Writes the data file. The JSON goes to a temporary file first, which
    /// is flushed to the disk and then replaces the old one, and the folder
    /// is flushed after the rename: neither a crash nor a power loss leaves
    /// a half-written or empty data file behind.
    pub(crate) fn save(&self, path: &Path) -> Result<(), StorageError> {
        let write_error = |source| StorageError::Write {
            path: path.to_path_buf(),
            source,
        };
        let json = serde_json::to_string_pretty(self).map_err(StorageError::Serialize)?;
        // A bare file name lies in the current folder.
        let folder = path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(folder).map_err(write_error)?;
        let temporary = path.with_extension("json.tmp");
        let mut file = File::create(&temporary).map_err(write_error)?;
        file.write_all(json.as_bytes()).map_err(write_error)?;
        // `sync_all` is `F_FULLFSYNC` on macOS: the bytes are on the disk,
        // not only in its cache.
        file.sync_all().map_err(write_error)?;
        std::fs::rename(&temporary, path).map_err(write_error)?;
        File::open(folder)
            .and_then(|folder| folder.sync_all())
            .map_err(write_error)
    }

    pub(crate) fn seconds_on(&self, date: NaiveDate) -> Seconds {
        self.days.get(&date).copied().unwrap_or(Seconds::ZERO)
    }

    pub(crate) const fn countdown(&self) -> CountdownDuration {
        self.countdown
    }

    pub(crate) const fn set_countdown(&mut self, countdown: CountdownDuration) {
        self.countdown = countdown;
    }

    /// Books every whole second from `from` to `to` to the local date it
    /// falls on, so time past midnight lands on the new day, also where a
    /// clock change skips or repeats midnight or a whole day. The seconds
    /// are walked one by one: the timer books one checkpoint interval at a
    /// time. A reversed interval books nothing.
    pub(crate) fn book<Tz: TimeZone>(&mut self, from: &DateTime<Tz>, to: &DateTime<Tz>) {
        let length = to.clone().signed_duration_since(from).num_seconds();
        let Ok(seconds) = u64::try_from(length) else {
            return;
        };
        let mut second = from.clone();
        for _ in 0..seconds {
            let booked = self.days.entry(second.date_naive()).or_default();
            *booked = booked.saturating_add(Seconds::new(1));
            let next = second.checked_add_signed(ONE_SECOND);
            debug_assert!(
                next.is_some(),
                "invariant: booked times are far from chrono's calendar edge"
            );
            // Should one not be, the seconds beyond the edge stay unbooked.
            let Some(next) = next else {
                return;
            };
            second = next;
        }
    }
}

/// The lock on the data file, held for as long as this value lives: a
/// second Timer finds it taken and does not write the file, so the two
/// cannot overwrite each other's times. The system releases it when the
/// process ends, also on a crash.
#[derive(Debug)]
pub(crate) struct DataFileLock {
    _file: File,
}

/// The data file's path and its lock.
#[derive(Debug)]
pub(crate) struct LockedDataFile {
    pub(crate) path: PathBuf,
    pub(crate) lock: DataFileLock,
}

impl DataFileLock {
    /// Locks `data_file` through `<data_file>.lock` beside it.
    pub(crate) fn acquire(data_file: &Path) -> Result<Self, StorageError> {
        let path = data_file.with_extension("json.lock");
        let lock_error = |source| StorageError::Lock {
            path: path.clone(),
            source,
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(lock_error)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(lock_error)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(StorageError::InUse {
                path: data_file.to_path_buf(),
            }),
            Err(TryLockError::Error(source)) => Err(lock_error(source)),
        }
    }
}

/// Finds the data file and locks it.
pub(crate) fn lock_data_file() -> Result<LockedDataFile, StorageError> {
    let path = data_file_path()?;
    let lock = DataFileLock::acquire(&path)?;
    Ok(LockedDataFile { path, lock })
}

/// Where the data file lives: `~/Library/Application Support/timer` on
/// macOS.
fn data_file_path() -> Result<PathBuf, StorageError> {
    let folder = dirs::data_dir().ok_or(StorageError::NoDataFolder)?;
    Ok(folder.join("timer").join("data.json"))
}
