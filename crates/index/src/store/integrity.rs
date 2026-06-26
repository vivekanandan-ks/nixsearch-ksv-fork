use anyhow::Result;
use camino::Utf8Path;

use crate::integrity::{self as generation_integrity, GenerationIntegrityPaths};

use super::{IndexStore, PublishedGeneration};

impl IndexStore {
    pub fn write_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<()> {
        let generation_path = self.validate_generation_path(&generation.path)?;
        let paths = self.integrity_paths(&generation.path, &generation_path);

        generation_integrity::write_integrity(
            &generation_path,
            &generation.manifest.generation_id,
            &paths,
            seo_sidecar_required,
        )
    }

    pub(super) fn validate_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<()> {
        let paths = self.integrity_paths(&generation.path, &generation.path);

        generation_integrity::validate_integrity(
            &generation.manifest.generation_id,
            &paths,
            seo_sidecar_required,
        )
    }

    fn integrity_paths(
        &self,
        generation_path: &Utf8Path,
        integrity_generation_path: &Utf8Path,
    ) -> GenerationIntegrityPaths {
        GenerationIntegrityPaths {
            manifest_path: self.manifest_path(generation_path),
            seo_sidecar_path: self.seo_sidecar_path(generation_path),
            index_path: self.index_path(generation_path),
            integrity_path: self.integrity_path(integrity_generation_path),
        }
    }
}
