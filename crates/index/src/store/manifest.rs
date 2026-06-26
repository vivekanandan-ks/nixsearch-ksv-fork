use std::fs;

use anyhow::{Context, Result};
use camino::Utf8Path;

use crate::generation::validate_manifest_invariants;
use crate::manifest::{
    IndexGenerationManifest, validate_generation_id, validate_index_schema_version,
};

use super::IndexStore;
use super::layout::MANIFEST_TEMP_PREFIX;

impl IndexStore {
    pub fn write_manifest(
        &self,
        generation_path: &Utf8Path,
        manifest: &IndexGenerationManifest,
    ) -> Result<()> {
        let generation_path = self.validate_generation_path(generation_path)?;
        let path = self.manifest_path(&generation_path);

        validate_generation_id(manifest)
            .context("failed to validate supplied index generation manifest id")?;
        validate_manifest_invariants(manifest)
            .context("failed to validate supplied index generation manifest invariants")?;

        let bytes = serde_json::to_vec_pretty(manifest)
            .context("failed to serialize index generation manifest")?;

        let temp_path = self.create_temp_file(&generation_path, MANIFEST_TEMP_PREFIX, &bytes)?;

        if let Err(error) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);

            return Err(error)
                .with_context(|| format!("failed to write index metadata {}", path.as_str()));
        }

        Self::sync_dir(&generation_path)?;

        Ok(())
    }

    pub fn read_manifest(&self, generation_path: &Utf8Path) -> Result<IndexGenerationManifest> {
        let path = self.manifest_path(generation_path);
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read index metadata {}", path.as_str()))?;

        let manifest: IndexGenerationManifest = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse index metadata {}", path.as_str()))?;

        validate_generation_id(&manifest)
            .with_context(|| format!("failed to validate index metadata {}", path.as_str()))?;
        validate_index_schema_version(&manifest)
            .with_context(|| format!("failed to validate index metadata {}", path.as_str()))?;

        Ok(manifest)
    }
}
