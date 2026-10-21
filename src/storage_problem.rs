//! Why tracked time is not being saved, in words for the user.

use serde_json::error::Category;
use std::io;

use crate::storage::{StorageError, VersionOrigin};
use crate::timer::DataFile;

/// Why tracked time is not being saved, in words for the user; `None`
/// while saving works.
pub(crate) fn storage_problem_text(data_file: &DataFile) -> Option<String> {
    match data_file {
        DataFile::Writable {
            last_error: Some(error),
            path: _,
        } => Some(format!("Saving failed: {}", describe(error))),
        DataFile::Writable {
            last_error: None,
            path: _,
        } => None,
        DataFile::Unavailable(error) => Some(format!(
            "Times are not being saved: {}",
            describe(error)
        )),
    }
}

/// A storage error in words for the user. The causes the system and the
/// JSON parser report are put into plain words; their own messages stay in
/// the error.
fn describe(error: &StorageError) -> String {
    match error {
        StorageError::NoDataFolder => "no folder for app data found".to_owned(),
        StorageError::Read { path, source } => {
            format!("{} cannot be read: {}", path.display(), io_cause(source))
        }
        StorageError::Parse { path, source } => format!(
            "{} is damaged and is left unchanged: {}",
            path.display(),
            json_cause(source)
        ),
        StorageError::UnknownVersion { path, version } => match version.origin() {
            VersionOrigin::NewerApp => format!(
                "{} comes from a newer Timer version (format {}) and is left unchanged",
                path.display(),
                version.get()
            ),
            VersionOrigin::Invalid => format!(
                "{} has an unknown format version ({}) and is left unchanged",
                path.display(),
                version.get()
            ),
        },
        StorageError::InUse { path } => format!(
            "another Timer is already using {} and saves on its own",
            path.display()
        ),
        StorageError::Lock { path, source } => {
            format!(
                "{} cannot be locked: {}",
                path.display(),
                io_cause(source)
            )
        }
        StorageError::Serialize(source) => format!(
            "the data cannot be converted: {}",
            json_kind(source)
        ),
        StorageError::Write { path, source } => {
            format!(
                "{} cannot be written: {}",
                path.display(),
                io_cause(source)
            )
        }
    }
}

/// Why the system refused to read or write, for the user.
fn io_cause(error: &io::Error) -> String {
    let cause = match error.kind() {
        io::ErrorKind::PermissionDenied => "permission denied",
        io::ErrorKind::StorageFull => "the disk is full",
        io::ErrorKind::ReadOnlyFilesystem => "the disk is read-only",
        io::ErrorKind::NotFound => "the folder is missing",
        io::ErrorKind::IsADirectory => "a folder is in the way",
        io::ErrorKind::InvalidData => "the file is not text",
        _ => {
            return match error.raw_os_error() {
                Some(code) => format!("system error (number {code})"),
                None => "system error".to_owned(),
            };
        }
    };
    cause.to_owned()
}

/// Where and why the JSON parser stopped, for the user.
fn json_cause(error: &serde_json::Error) -> String {
    format!(
        "{} (line {}, column {})",
        json_kind(error),
        error.line(),
        error.column()
    )
}

/// What kind of JSON error this is, for the user.
fn json_kind(error: &serde_json::Error) -> &'static str {
    match error.classify() {
        Category::Syntax => "not valid JSON",
        Category::Data => "unexpected content",
        Category::Eof => "the content breaks off",
        Category::Io => "read error",
    }
}
