use anyhow::Result;

use crate::integrity::{self as generation_integrity, GenerationIntegrityPaths};

use super::{IndexStore, PublishedGeneration};

impl IndexStore {
    pub fn write_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<()> {
        let generation_path = self.validate_generation_path(&generation.path)?;
        let paths = GenerationIntegrityPaths {
            manifest_path: self.manifest_path(&generation.path),
            seo_sidecar_path: self.seo_sidecar_path(&generation.path),
            index_path: self.index_path(&generation.path),
            integrity_path: self.integrity_path(&generation_path),
        };

        generation_integrity::write_integrity(
            &generation_path,
            &generation.manifest.generation_id,
            &paths,
            seo_sidecar_required,
        )
    }
}
