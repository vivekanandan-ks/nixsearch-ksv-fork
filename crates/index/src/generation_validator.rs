use std::fs;

use anyhow::{Context, Result};

use crate::generation::{
    SeoCompleteGeneration, StructurallyCompleteGeneration, open_seo_complete_generation,
    open_structurally_complete_generation, validate_manifest_invariants,
};
use crate::integrity::{self as generation_integrity, GenerationIntegrityPaths};
use crate::manifest::{validate_generation_id, validate_index_schema_version};
use crate::search::SearchIndex;
use crate::seo::SeoSidecarAccumulator;
use crate::store::{IndexStore, LeasedPublishedGeneration, PublishedGeneration};

#[derive(Debug, Clone)]
pub struct GenerationValidator {
    store: IndexStore,
}

impl GenerationValidator {
    pub fn new(store: IndexStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &IndexStore {
        &self.store
    }

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

        let index_path = self.store.index_path(&generation.path);
        SearchIndex::open(&index_path)
            .with_context(|| format!("failed to open search index {index_path}"))
    }

    pub fn open_structurally_complete_published_generation(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<StructurallyCompleteGeneration> {
        if self.validate_integrity(generation, false).is_ok() {
            let index_path = self.store.index_path(&generation.path);
            let index = SearchIndex::open(&index_path)
                .with_context(|| format!("failed to open search index {index_path}"))?;
            let seo_sidecar = SeoSidecarAccumulator::from_index(&index)?
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
            &self.store.index_path(&generation.path),
            &generation.manifest,
        )
    }

    pub fn open_seo_complete_published_generation(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<SeoCompleteGeneration> {
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let sidecar = self.store.read_seo_sidecar(generation)?;
        if self.validate_integrity(generation, true).is_ok() {
            let index_path = self.store.index_path(&generation.path);
            let index = SearchIndex::open(&index_path)
                .with_context(|| format!("failed to open search index {index_path}"))?;
            let scan = crate::generation::GenerationScan {
                document_count: generation.manifest.document_count,
                seo_sidecar: sidecar.clone(),
            };

            return Ok(SeoCompleteGeneration {
                index,
                sidecar,
                scan,
            });
        }

        open_seo_complete_generation(
            &self.store.index_path(&generation.path),
            &generation.manifest,
            sidecar,
        )
        .context("failed to validate SEO-complete generation")
    }

    pub fn open_seo_complete_leased_generation(
        &self,
        generation: &LeasedPublishedGeneration,
    ) -> Result<SeoCompleteGeneration> {
        self.open_seo_complete_published_generation(generation.published_generation())
    }

    /// Validates a published generation without acquiring a generation lease.
    ///
    /// Callers must already prevent concurrent deletion, for example by holding the
    /// update lock. Request and startup paths should use leased validation instead.
    pub fn validate_seo_complete_published_generation_unleased(
        &self,
        generation: &PublishedGeneration,
    ) -> Result<()> {
        self.open_seo_complete_published_generation(generation)
            .map(|_| ())
    }

    pub fn validate_seo_complete_leased_generation(
        &self,
        generation: &LeasedPublishedGeneration,
    ) -> Result<()> {
        self.open_seo_complete_leased_generation(generation)
            .map(|_| ())
    }

    pub fn require_seo_sidecar_file(&self, generation: &PublishedGeneration) -> Result<()> {
        validate_index_schema_version(&generation.manifest)
            .context("failed to validate supplied index generation manifest")?;

        let path = self.store.seo_sidecar_path(&generation.path);
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to read SEO sidecar metadata {path}"))?;

        if !metadata.is_file() {
            anyhow::bail!("SEO sidecar is not a file {path}");
        }

        Ok(())
    }

    fn validate_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<()> {
        let paths = GenerationIntegrityPaths {
            manifest_path: self.store.manifest_path(&generation.path),
            seo_sidecar_path: self.store.seo_sidecar_path(&generation.path),
            index_path: self.store.index_path(&generation.path),
            integrity_path: self.store.integrity_path(&generation.path),
        };

        generation_integrity::validate_integrity(
            &generation.manifest.generation_id,
            &paths,
            seo_sidecar_required,
        )
    }
}
