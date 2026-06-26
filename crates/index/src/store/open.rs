use anyhow::{Context, Result};

use crate::generation::{
    StructurallyCompleteGeneration, open_seo_complete_generation,
    open_structurally_complete_generation, validate_manifest_invariants,
};
use crate::manifest::{validate_generation_id, validate_index_schema_version};
use crate::search::SearchIndex;
use crate::seo::SeoSidecar;

use super::{IndexStore, LeasedPublishedGeneration, PublishedGeneration};

impl IndexStore {
    fn open_valid_published_generation(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<(SearchIndex, SeoSidecar)> {
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let sidecar = self.read_seo_sidecar(generation)?;
        if self.validate_integrity(generation, true).is_ok() {
            let index_path = self.index_path(&generation.path);
            let index = SearchIndex::open(&index_path)
                .with_context(|| format!("failed to open search index {index_path}"))?;
            return Ok((index, sidecar));
        }

        let complete = open_seo_complete_generation(
            &self.index_path(&generation.path),
            &generation.manifest,
            sidecar,
        )
        .context("failed to validate SEO-complete generation")?;

        Ok((complete.index, complete.sidecar))
    }

    /// Opens a published generation after cheap manifest and Tantivy checks.
    ///
    /// This intentionally avoids scanning or hashing generation contents. Startup and
    /// request paths use it when they only need an index that can be served quickly;
    /// maintenance and repair paths use the complete openers when they need full
    /// generation validation.
    pub fn open_structurally_valid_published_generation(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<SearchIndex> {
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;
        validate_generation_id(&generation.manifest)
            .context("failed to validate supplied index generation manifest id")?;
        validate_manifest_invariants(&generation.manifest)
            .context("failed to validate supplied index generation manifest invariants")?;

        let index_path = self.index_path(&generation.path);
        SearchIndex::open(&index_path)
            .with_context(|| format!("failed to open search index {index_path}"))
    }

    pub fn open_structurally_complete_published_generation(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<StructurallyCompleteGeneration> {
        if self.validate_integrity(generation, false).is_ok() {
            let index_path = self.index_path(&generation.path);
            let index = SearchIndex::open(&index_path)
                .with_context(|| format!("failed to open search index {index_path}"))?;
            let seo_sidecar = crate::seo::SeoSidecarAccumulator::from_index(&index)?
                .into_sidecar_for_manifest(&generation.manifest);

            return Ok(StructurallyCompleteGeneration {
                index,
                scan: crate::generation::GenerationScan {
                    document_count: generation.manifest.document_count,
                    seo_sidecar,
                },
            });
        }

        open_structurally_complete_generation(
            &self.index_path(&generation.path),
            &generation.manifest,
        )
    }

    pub fn open_valid_leased_generation(
        &self,
        generation: &LeasedPublishedGeneration,
    ) -> Result<(SearchIndex, SeoSidecar)> {
        self.open_valid_published_generation(generation.published_generation())
    }

    /// Validates a published generation without acquiring a generation lease.
    ///
    /// Callers must already prevent concurrent deletion, for example by holding the
    /// update lock. Request and startup paths should use leased validation instead.
    pub fn validate_unleased_published_generation(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<()> {
        self.open_valid_published_generation(generation).map(|_| ())
    }

    pub fn validate_leased_generation(&self, generation: &LeasedPublishedGeneration) -> Result<()> {
        self.open_valid_leased_generation(generation).map(|_| ())
    }
}
