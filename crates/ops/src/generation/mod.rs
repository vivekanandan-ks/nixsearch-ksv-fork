use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use camino::Utf8PathBuf;

use nixsearch_core::artifact::ArtifactKind;
use nixsearch_core::document::SearchDocument;
use nixsearch_index::manifest::{IndexGenerationManifest, IndexTargetManifest};
use nixsearch_index::search::SearchIndex;
use nixsearch_index::store::IndexStore;
use nixsearch_store::ArtifactStore;

use crate::targets::{TargetKey, TargetRef};

mod documents;
mod index;
mod policy;
mod production;
mod publish;

use self::documents::{
    SpooledDocumentSetBuilder, build_generation_manifest, consume_and_spool_target,
};
use self::index::write_tantivy_index;
use self::policy::{validate_generation_policy, validate_generation_success_requirements};
use self::production::produce_or_retain_target;
use self::publish::{
    IncompleteGenerationGuard, publish_completed_generation, write_generation_artifacts,
};

pub(crate) use self::policy::GenerationFailurePolicy;
pub(crate) use self::production::TargetProducer;

#[derive(Debug, Clone)]
pub struct GenerationBuildResult {
    pub path: Utf8PathBuf,
    pub successful_targets: Vec<TargetKey>,
    pub skipped_targets: Vec<TargetKey>,
    pub failed_refresh_targets: Vec<TargetKey>,
}

impl GenerationBuildResult {
    pub fn is_degraded(&self) -> bool {
        !self.skipped_targets.is_empty() || !self.failed_refresh_targets.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct RetainedGeneration {
    targets: BTreeMap<TargetKey, RetainedTarget>,
}

#[derive(Debug, Clone)]
pub(crate) struct RetainedTarget {
    pub(crate) manifest_target: IndexTargetManifest,
    pub(crate) documents: Vec<SearchDocument>,
}

impl RetainedGeneration {
    pub fn from_index(manifest: &IndexGenerationManifest, index: &SearchIndex) -> Result<Self> {
        let mut document_target_keys = BTreeMap::<(String, String, ArtifactKind), TargetKey>::new();
        let mut targets = BTreeMap::<TargetKey, RetainedTarget>::new();

        for manifest_target in &manifest.targets {
            let key = TargetKey::from(manifest_target);
            document_target_keys.insert(
                (
                    manifest_target.source.clone(),
                    manifest_target.ref_id.clone(),
                    manifest_target.artifact_kind,
                ),
                key.clone(),
            );
            targets.insert(
                key,
                RetainedTarget {
                    manifest_target: manifest_target.clone(),
                    documents: Vec::new(),
                },
            );
        }

        for indexed in index.scan_indexed_documents()? {
            let common = indexed.document.common();
            let artifact_kind = ArtifactKind::for_indexed_document_kind(indexed.document.kind())
                .with_context(|| {
                    format!(
                        "indexed document {}/{}/{} has unsupported document kind {}",
                        common.source,
                        common.ref_id,
                        common.name,
                        indexed.document.kind().as_str()
                    )
                })?;
            let key = document_target_keys
                .get(&(common.source.clone(), common.ref_id.clone(), artifact_kind))
                .with_context(|| {
                    format!(
                        "indexed document {}/{}/{} has no retained manifest target {}",
                        common.source,
                        common.ref_id,
                        common.name,
                        artifact_kind.as_str()
                    )
                })?
                .clone();
            targets
                .get_mut(&key)
                .expect("retained target key came from retained target map")
                .documents
                .push(indexed.document);
        }

        for (key, retained) in &targets {
            let actual = retained.documents.len();
            let expected = retained.manifest_target.document_count;
            if actual != expected {
                bail!(
                    "retained target {key} document count mismatch: index {actual}, manifest {expected}"
                );
            }
        }

        Ok(Self { targets })
    }

    pub(crate) fn target(&self, key: &TargetKey) -> Option<&RetainedTarget> {
        self.targets.get(key)
    }
}

pub(crate) struct GenerationBuildPlan<'a> {
    pub(crate) targets: Vec<TargetRef>,
    pub(crate) refresh_keys: &'a BTreeSet<TargetKey>,
    pub(crate) policy: GenerationFailurePolicy,
    pub(crate) required_success_targets: Option<&'a BTreeSet<TargetKey>>,
    pub(crate) retained_generation: Option<&'a RetainedGeneration>,
}

pub(crate) async fn build_and_publish_generation_with_policy(
    index_store: &IndexStore,
    artifact_store: &ArtifactStore,
    plan: GenerationBuildPlan<'_>,
    producer: &(dyn TargetProducer + Send + Sync),
) -> Result<GenerationBuildResult> {
    validate_generation_policy(&plan.targets, plan.refresh_keys, plan.policy)?;

    let mut generation = IncompleteGenerationGuard::create(index_store)?;

    let mut documents_builder = SpooledDocumentSetBuilder::create()?;
    let mut skipped_targets = Vec::new();
    let mut failed_refresh_targets = Vec::new();

    for target in plan.targets {
        let key = TargetKey::from(&target);

        if !plan.refresh_keys.contains(&key) {
            let retained = plan
                .retained_generation
                .and_then(|generation| generation.target(&key))
                .with_context(|| {
                    format!("target {key} was not refreshed and is not available to retain")
                })?;
            documents_builder.append_retained_target(&key, retained)?;
            continue;
        }

        match produce_or_retain_target(
            artifact_store,
            target,
            plan.refresh_keys,
            plan.policy,
            producer,
        )
        .await?
        {
            Some(produced_target) => {
                consume_and_spool_target(artifact_store, &produced_target, &mut documents_builder)
                    .await?;
            }
            None => {
                failed_refresh_targets.push(key.clone());
                skipped_targets.push(key);
            }
        }
    }

    validate_generation_success_requirements(
        plan.policy,
        plan.required_success_targets,
        documents_builder.successful_targets(),
    )?;

    let documents = documents_builder.finish()?;
    write_tantivy_index(index_store, generation.path(), &documents)?;

    let manifest = build_generation_manifest(&documents)?;
    write_generation_artifacts(index_store, generation.path(), manifest)?;

    publish_completed_generation(index_store, generation.path(), documents.total_documents)?;
    generation.mark_published();

    Ok(GenerationBuildResult {
        path: generation.path().to_owned(),
        successful_targets: documents.successful_targets,
        skipped_targets,
        failed_refresh_targets,
    })
}
