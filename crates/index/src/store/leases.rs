use std::fs::{self, File, OpenOptions, TryLockError};
use std::sync::Arc;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use crate::manifest::{IndexGenerationManifest, validate_generation_id};

use super::layout::validate_generation_name;
use super::{IndexStore, PublishedGeneration};

#[derive(Debug)]
pub struct GenerationLease {
    _file: File,
}

#[derive(Debug, Clone)]
pub struct LeasedPublishedGeneration {
    pub(super) generation: PublishedGeneration,
    pub(super) _lease: Arc<GenerationLease>,
}

impl LeasedPublishedGeneration {
    pub fn path(&self) -> &Utf8Path {
        &self.generation.path
    }

    pub fn manifest(&self) -> &IndexGenerationManifest {
        &self.generation.manifest
    }

    pub fn published_generation(&self) -> &PublishedGeneration {
        &self.generation
    }

    pub fn to_published_generation(&self) -> PublishedGeneration {
        self.generation.clone()
    }
}

impl IndexStore {
    pub fn try_acquire_existing_exclusive_generation_lock(
        &self,
        generation_name: &str,
    ) -> Result<Option<GenerationLease>> {
        validate_generation_name(generation_name)?;

        let lock_path = self.generation_lock_path(generation_name);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("failed to open generation lease {}", lock_path.as_str()))?;

        match file.try_lock() {
            Ok(()) => Ok(Some(GenerationLease { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error).with_context(|| {
                format!("failed to acquire exclusive generation lease {lock_path}")
            }),
        }
    }

    fn generation_lease_path(
        &self,
        generation_path: &Utf8Path,
    ) -> Result<(Utf8PathBuf, Utf8PathBuf)> {
        let generation_path = self.validate_generation_path(generation_path)?;
        let name = generation_path
            .file_name()
            .with_context(|| {
                format!(
                    "failed to derive generation lease name for {}",
                    generation_path.as_str()
                )
            })?
            .to_owned();

        let lock_path = self.generation_lock_path(&name);

        Ok((generation_path, lock_path))
    }

    fn open_generation_lease_file(&self, generation_path: &Utf8Path) -> Result<File> {
        let (_, lock_path) = self.generation_lease_path(generation_path)?;

        fs::create_dir_all(self.generation_locks_dir()).with_context(|| {
            format!(
                "failed to create generation locks dir {}",
                self.generation_locks_dir()
            )
        })?;

        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("failed to open generation lease {}", lock_path.as_str()))
    }

    pub fn acquire_shared_generation_lease(
        &self,
        generation_path: &Utf8Path,
    ) -> Result<GenerationLease> {
        let file = self.open_generation_lease_file(generation_path)?;

        file.lock_shared().with_context(|| {
            format!("failed to acquire shared generation lease for {generation_path}")
        })?;

        self.validate_generation_path(generation_path)?;

        Ok(GenerationLease { _file: file })
    }

    #[cfg(test)]
    pub(super) fn acquire_shared_generation_lease_with_hook(
        &self,
        generation_path: &Utf8Path,
        before_lock: impl FnOnce(),
    ) -> Result<GenerationLease> {
        let file = self.open_generation_lease_file(generation_path)?;
        before_lock();

        file.lock_shared().with_context(|| {
            format!("failed to acquire shared generation lease for {generation_path}")
        })?;

        self.validate_generation_path(generation_path)?;

        Ok(GenerationLease { _file: file })
    }

    pub fn try_acquire_exclusive_generation_lease(
        &self,
        generation_path: &Utf8Path,
    ) -> Result<Option<GenerationLease>> {
        let file = self.open_generation_lease_file(generation_path)?;

        match file.try_lock() {
            Ok(()) => Ok(Some(GenerationLease { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error).with_context(|| {
                format!("failed to acquire exclusive generation lease for {generation_path}")
            }),
        }
    }

    pub fn lease_published_generation(
        &self,
        generation: PublishedGeneration,
    ) -> Result<LeasedPublishedGeneration> {
        validate_generation_id(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let lease = Arc::new(self.acquire_shared_generation_lease(&generation.path)?);

        Ok(LeasedPublishedGeneration {
            generation,
            _lease: lease,
        })
    }
}
