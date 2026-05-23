use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const UPDATE_LOCK_FILENAME: &str = "update.lock";

#[derive(Debug)]
pub struct UpdateLock {
    path: PathBuf,
    _file: File,
}

impl UpdateLock {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub fn update_lock_path(index_dir: &Path) -> PathBuf {
    index_dir.join(UPDATE_LOCK_FILENAME)
}

pub fn acquire_update_lock(index_dir: &Path) -> Result<UpdateLock> {
    let path = prepare_lock_file(index_dir)?;
    let file = open_lock_file(&path)?;

    tracing::info!("waiting for maintenance lock {}", path.display());
    file.lock()
        .with_context(|| format!("failed to acquire maintenance lock {}", path.display()))?;
    tracing::info!("acquired maintenance lock {}", path.display());

    Ok(UpdateLock { path, _file: file })
}

pub fn try_acquire_update_lock(index_dir: &Path) -> Result<Option<UpdateLock>> {
    let path = prepare_lock_file(index_dir)?;
    let file = open_lock_file(&path)?;

    match file.try_lock() {
        Ok(()) => {
            tracing::info!("acquired maintenance lock {}", path.display());
            Ok(Some(UpdateLock { path, _file: file }))
        }
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error)
            .with_context(|| format!("failed to acquire maintenance lock {}", path.display())),
    }
}

fn prepare_lock_file(index_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(index_dir)
        .with_context(|| format!("failed to create index dir {}", index_dir.display()))?;

    Ok(update_lock_path(index_dir))
}

fn open_lock_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open maintenance lock {}", path.display()))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{try_acquire_update_lock, update_lock_path};

    #[test]
    fn update_lock_path_uses_index_dir() {
        let dir = tempdir().unwrap();

        assert_eq!(update_lock_path(dir.path()), dir.path().join("update.lock"));
    }

    #[test]
    fn try_acquire_update_lock_reports_contention() {
        let dir = tempdir().unwrap();

        let first = try_acquire_update_lock(dir.path()).unwrap();
        assert!(first.is_some());

        let second = try_acquire_update_lock(dir.path()).unwrap();
        assert!(second.is_none());

        drop(first);

        let third = try_acquire_update_lock(dir.path()).unwrap();
        assert!(third.is_some());
    }
}
