use anyhow::{Context, Result};

use nixsearch_config::app::AppConfig;
use nixsearch_index::generation_validator::GenerationValidator;
use nixsearch_index::store::{IndexStore, PublishedGeneration};

use crate::lock::UpdateLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeoSidecarRepairOutcome {
    AlreadySeoComplete {
        generation: PublishedGeneration,
    },
    Repaired {
        generation: PublishedGeneration,
    },
    MissingCurrent,
    SupersededBeforeRepair,
    SupersededAfterRepair,
    Unrepairable {
        generation: PublishedGeneration,
        error: String,
    },
    RepairFailed {
        generation: PublishedGeneration,
        error: String,
    },
}

pub fn repair_current_seo_sidecar_under_lock(
    config: &AppConfig,
    _update_lock: &UpdateLock,
) -> Result<SeoSidecarRepairOutcome> {
    let index_store = IndexStore::new(&config.data.index_dir);
    let validator = GenerationValidator::new(index_store.clone());
    let Some(candidate) = index_store.try_current_generation_metadata()? else {
        return Ok(SeoSidecarRepairOutcome::MissingCurrent);
    };

    if validator
        .validate_seo_complete_published_generation_unleased(&candidate)
        .is_ok()
    {
        return Ok(SeoSidecarRepairOutcome::AlreadySeoComplete {
            generation: candidate,
        });
    }

    let structural = match validator.open_structurally_complete_published_generation(&candidate) {
        Ok(structural) => structural,
        Err(error) => {
            return Ok(SeoSidecarRepairOutcome::Unrepairable {
                generation: candidate,
                error: format!("{error:#}"),
            });
        }
    };

    if !published_generation_is_current(&index_store, &candidate)? {
        return Ok(SeoSidecarRepairOutcome::SupersededBeforeRepair);
    }

    let sidecar = structural.scan.seo_sidecar;
    if let Err(error) = index_store.write_validated_seo_sidecar_unchecked(&candidate, &sidecar) {
        return Ok(SeoSidecarRepairOutcome::RepairFailed {
            generation: candidate,
            error: format!("{error:#}"),
        });
    }

    if !published_generation_is_current(&index_store, &candidate)? {
        return Ok(SeoSidecarRepairOutcome::SupersededAfterRepair);
    }

    validator
        .validate_seo_complete_published_generation_unleased(&candidate)
        .context("repaired SEO sidecar did not validate")?;

    Ok(SeoSidecarRepairOutcome::Repaired {
        generation: candidate,
    })
}

fn published_generation_is_current(
    index_store: &IndexStore,
    generation: &PublishedGeneration,
) -> Result<bool> {
    let Some(current) = index_store.try_current_generation_metadata()? else {
        return Ok(false);
    };

    Ok(current == *generation)
}
