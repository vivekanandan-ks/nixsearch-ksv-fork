use std::fs;
use std::sync::Arc;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use crate::manifest::IndexGenerationManifest;

use super::layout::CURRENT_TEMP_PREFIX;
use super::{GenerationLease, IndexStore, LeasedPublishedGeneration, PublishedGeneration};

impl IndexStore {
    // Atomically publishes generation_path as CURRENT. Update callers should still use the
    // update lock to avoid redundant concurrent generation work.
    pub fn publish(&self, generation_path: &Utf8Path) -> Result<()> {
        fs::create_dir_all(self.root())
            .with_context(|| format!("failed to create index root {}", self.root().as_str()))?;

        let generation_path = self.validate_generation_path(generation_path)?;

        // On Unix, rename atomically replaces CURRENT with this fully written file.
        let current_tmp = self.create_temp_file(
            self.root(),
            CURRENT_TEMP_PREFIX,
            generation_path.as_str().as_bytes(),
        )?;

        if let Err(error) = fs::rename(&current_tmp, self.current_file()) {
            let _ = fs::remove_file(&current_tmp);

            return Err(error).with_context(|| {
                format!(
                    "failed to publish current index {}",
                    self.current_file().as_str()
                )
            });
        }

        Self::sync_dir(self.root())?;

        Ok(())
    }

    pub fn current_path(&self) -> Result<Utf8PathBuf> {
        self.try_current_path()?.with_context(|| {
            format!(
                "failed to read current index file {}; run `nixsearch update` first",
                self.current_file().as_str()
            )
        })
    }

    pub fn try_current_path(&self) -> Result<Option<Utf8PathBuf>> {
        let raw = match fs::read_to_string(self.current_file()) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to read current index file {}",
                        self.current_file().as_str()
                    )
                });
            }
        };

        let path = Utf8PathBuf::from(raw);

        if path.as_str().is_empty() {
            anyhow::bail!("current index file is empty");
        }

        self.validate_generation_path(&path).map(Some)
    }

    pub fn current_manifest(&self) -> Result<IndexGenerationManifest> {
        let current = self.current_path()?;
        self.read_manifest(&current)
    }

    pub fn try_current_manifest(&self) -> Result<Option<IndexGenerationManifest>> {
        let Some(current) = self.try_current_path()? else {
            return Ok(None);
        };

        self.read_manifest(&current).map(Some)
    }

    pub fn try_current_generation_metadata(&self) -> Result<Option<PublishedGeneration>> {
        self.try_current_generation_metadata_with(|_, _| Ok(()))
    }

    pub(super) fn try_current_generation_metadata_with(
        &self,
        mut after_manifest_read: impl FnMut(&IndexStore, &Utf8Path) -> Result<()>,
    ) -> Result<Option<PublishedGeneration>> {
        for _ in 0..100 {
            let Some(path) = self.try_current_path()? else {
                return Ok(None);
            };

            let manifest = match self.read_manifest(&path) {
                Ok(manifest) => manifest,
                Err(error) => match self.try_current_path()? {
                    Some(current) if current != path => continue,
                    Some(_) => return Err(error),
                    None => return Ok(None),
                },
            };

            after_manifest_read(self, &path)?;

            match self.try_current_path()? {
                Some(current) if current == path => {
                    return Ok(Some(PublishedGeneration { path, manifest }));
                }
                Some(_) => continue,
                None => return Ok(None),
            }
        }

        anyhow::bail!("failed to read stable current generation metadata")
    }

    pub fn current_leased_generation(&self) -> Result<LeasedPublishedGeneration> {
        self.try_current_leased_generation()?.with_context(|| {
            format!(
                "failed to read current index file {}; run `nixsearch update` first",
                self.current_file().as_str()
            )
        })
    }

    pub fn try_current_leased_generation(&self) -> Result<Option<LeasedPublishedGeneration>> {
        self.try_current_leased_generation_with(
            IndexStore::acquire_shared_generation_lease,
            |_, _| Ok(()),
        )
    }

    pub(super) fn try_current_leased_generation_with(
        &self,
        mut acquire_shared_generation_lease: impl FnMut(
            &IndexStore,
            &Utf8Path,
        ) -> Result<GenerationLease>,
        mut after_manifest_read: impl FnMut(&IndexStore, &Utf8Path) -> Result<()>,
    ) -> Result<Option<LeasedPublishedGeneration>> {
        for _ in 0..100 {
            let Some(path) = self.try_current_path()? else {
                return Ok(None);
            };

            let lease = match acquire_shared_generation_lease(self, &path) {
                Ok(lease) => Arc::new(lease),
                Err(error) => match self.try_current_path()? {
                    Some(current) if current != path => continue,
                    Some(_) => return Err(error),
                    None => return Ok(None),
                },
            };

            match self.try_current_path()? {
                Some(current) if current == path => {
                    let manifest = match self.read_manifest(&path) {
                        Ok(manifest) => manifest,
                        Err(error) => match self.try_current_path()? {
                            Some(current) if current != path => continue,
                            Some(_) => return Err(error),
                            None => return Ok(None),
                        },
                    };

                    after_manifest_read(self, &path)?;

                    match self.try_current_path()? {
                        Some(current) if current == path => {}
                        Some(_) => continue,
                        None => return Ok(None),
                    }

                    return Ok(Some(LeasedPublishedGeneration {
                        generation: PublishedGeneration { path, manifest },
                        _lease: lease,
                    }));
                }
                Some(_) => continue,
                None => return Ok(None),
            }
        }

        anyhow::bail!("failed to acquire a stable current generation lease")
    }
}
