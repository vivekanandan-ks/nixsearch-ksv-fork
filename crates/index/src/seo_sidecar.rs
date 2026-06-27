use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use nixsearch_core::document::SearchDocument;

use crate::atomic_file;
use crate::manifest::{
    IndexGenerationManifest, validate_generation_id, validate_index_schema_version,
};
use crate::search::SearchIndex;
use crate::seo::{SeoSidecar, SeoSidecarAccumulator};
use crate::store::{IndexStore, PublishedGeneration};

const SEO_SIDECAR_FILE: &str = "seo-facts.json";
const SEO_SIDECAR_TEMP_PREFIX: &str = "seo-facts.json.tmp";

#[derive(Debug, Clone, Copy)]
pub struct SeoFactsArtifact;

impl SeoFactsArtifact {
    pub fn path(generation_path: &Utf8Path) -> Utf8PathBuf {
        generation_path.join(SEO_SIDECAR_FILE)
    }

    pub fn derive_from_index(
        manifest: &IndexGenerationManifest,
        index: &SearchIndex,
    ) -> Result<SeoSidecar> {
        Ok(SeoSidecarAccumulator::from_index(index)?.into_sidecar_for_manifest(manifest))
    }

    pub fn derive_from_documents<'a>(
        manifest: &IndexGenerationManifest,
        documents: impl IntoIterator<Item = &'a SearchDocument>,
    ) -> SeoSidecar {
        let mut accumulator = SeoSidecarAccumulator::new();

        for document in documents {
            accumulator.observe(document);
        }

        accumulator.into_sidecar_for_manifest(manifest)
    }

    pub fn write(
        store: &IndexStore,
        generation: &PublishedGeneration,
        sidecar: &SeoSidecar,
    ) -> Result<()> {
        let generation_path = store.validate_generation_path(&generation.path)?;
        let path = Self::path(&generation_path);

        validate_generation_id(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let index_path = store.index_path(&generation_path);
        let index = SearchIndex::open(&index_path)
            .with_context(|| format!("failed to open search index {index_path}"))?;

        sidecar
            .validate_for_index(&generation.manifest, &index)
            .with_context(|| format!("failed to validate SEO sidecar {}", path.as_str()))?;

        Self::write_serialized(&generation_path, &path, sidecar)
    }

    pub fn write_derived(
        store: &IndexStore,
        generation: &PublishedGeneration,
        index: &SearchIndex,
    ) -> Result<SeoSidecar> {
        validate_generation_id(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let sidecar = Self::derive_from_index(&generation.manifest, index)?;
        Self::write_validated_unchecked(store, generation, &sidecar)?;

        Ok(sidecar)
    }

    pub fn write_validated_unchecked(
        store: &IndexStore,
        generation: &PublishedGeneration,
        sidecar: &SeoSidecar,
    ) -> Result<()> {
        let generation_path = store.validate_generation_path(&generation.path)?;
        let path = Self::path(&generation_path);

        validate_generation_id(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;
        sidecar
            .validate_for_manifest(&generation.manifest)
            .with_context(|| format!("failed to validate SEO sidecar {}", path.as_str()))?;

        Self::write_serialized(&generation_path, &path, sidecar)
    }

    fn write_serialized(
        generation_path: &Utf8Path,
        path: &Utf8Path,
        sidecar: &SeoSidecar,
    ) -> Result<()> {
        let bytes =
            serde_json::to_vec_pretty(sidecar).context("failed to serialize SEO sidecar")?;
        let temp_path =
            atomic_file::create_temp_file(generation_path, SEO_SIDECAR_TEMP_PREFIX, &bytes)?;

        if let Err(error) = fs::rename(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);

            return Err(error)
                .with_context(|| format!("failed to write SEO sidecar {}", path.as_str()));
        }

        atomic_file::sync_dir(generation_path)?;

        Ok(())
    }

    pub fn read(generation: &PublishedGeneration) -> Result<SeoSidecar> {
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let path = Self::path(&generation.path);
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read SEO sidecar {}", path.as_str()))?;

        let sidecar: SeoSidecar = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse SEO sidecar {}", path.as_str()))?;

        sidecar
            .validate_for_manifest(&generation.manifest)
            .with_context(|| format!("failed to validate SEO sidecar {}", path.as_str()))?;

        Ok(sidecar)
    }
}
