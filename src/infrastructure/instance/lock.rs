//! The data-folder lock: one server per `DATA_DIR`.
//!
//! Two copies on different ports can share a data folder (an absolute
//! `DATA_DIR`, or the same folder with another `PORT`). Only the copy holding
//! `DATA_DIR/instance.lock` may treat unfinished recordings as interrupted:
//! another live copy's recordings would otherwise be failed under it. The lock
//! is held until the process exits, so a new copy cannot take over while this
//! one's recording tasks still write. The file stays empty: a locked range
//! cannot be read on Windows.

use std::{
    fs::{File, OpenOptions, TryLockError},
    io,
    path::Path,
    sync::OnceLock,
};

const LOCK_FILE: &str = "instance.lock";

/// The lock file of this process, kept open (and locked) until it exits.
static HELD: OnceLock<File> = OnceLock::new();

/// What became of the data-folder lock.
#[derive(Debug)]
pub enum DataLock {
    /// This process holds it.
    Held,
    /// Another process holds it.
    InUse,
    /// The file system cannot lock files (some network shares): the server
    /// runs, but leaves unfinished recordings alone.
    Unsupported(io::Error),
}

/// Takes the lock on `data_dir` for the rest of the process. Calling it again
/// after `Held` answers `Held` without locking twice.
pub fn lock_data_folder(data_dir: &Path) -> Result<DataLock, String> {
    if HELD.get().is_some() {
        return Ok(DataLock::Held);
    }
    let (outcome, file) = try_lock(data_dir)
        .map_err(|e| format!("Cannot create {}: {e}", data_dir.join(LOCK_FILE).display()))?;
    if let Some(file) = file {
        // Only one path in `startup` takes the lock, once.
        let _ = HELD.set(file);
    }
    Ok(outcome)
}

/// Opens (creating it if needed) and tries to lock `instance.lock`. The file
/// comes back only with `Held`: dropping it releases the lock.
fn try_lock(data_dir: &Path) -> io::Result<(DataLock, Option<File>)> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(data_dir.join(LOCK_FILE))?;
    Ok(match file.try_lock() {
        Ok(()) => (DataLock::Held, Some(file)),
        Err(TryLockError::WouldBlock) => (DataLock::InUse, None),
        Err(TryLockError::Error(error)) => (DataLock::Unsupported(error), None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_on_the_same_folder_fails_until_the_first_is_released() {
        let dir = tempfile::tempdir().unwrap();
        let (first, held) = try_lock(dir.path()).unwrap();
        assert!(matches!(first, DataLock::Held), "{first:?}");
        assert!(held.is_some());
        // A second handle, as another process would open it.
        let (second, none) = try_lock(dir.path()).unwrap();
        assert!(matches!(second, DataLock::InUse), "{second:?}");
        assert!(none.is_none());
        // Another data folder is free.
        let other = tempfile::tempdir().unwrap();
        let (third, _other) = try_lock(other.path()).unwrap();
        assert!(matches!(third, DataLock::Held), "{third:?}");
        drop(held);
        let (again, _held) = try_lock(dir.path()).unwrap();
        assert!(matches!(again, DataLock::Held), "{again:?}");
        // The lock file stays empty.
        let size = std::fs::metadata(dir.path().join(LOCK_FILE)).unwrap().len();
        assert_eq!(size, 0);
    }

    #[test]
    fn a_missing_data_folder_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(try_lock(&dir.path().join("missing")).is_err());
    }
}
