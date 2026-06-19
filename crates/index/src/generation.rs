use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use camino::Utf8Path;

use nixsearch_core::artifact::ArtifactKind;
use nixsearch_core::document::{DocumentKind, SearchDocument};

use crate::annotation::SearchHitAnnotation;
use crate::manifest::{IndexGenerationManifest, validate_generation_id};
use crate::search::{IndexedSearchDocument, SearchIndex};
use crate::seo::{SeoSidecar, SeoSidecarAccumulator};

pub struct StructurallyCompleteGeneration {
    pub index: SearchIndex,
    pub scan: GenerationScan,
}

pub struct SeoCompleteGeneration {
    pub index: SearchIndex,
    pub sidecar: SeoSidecar,
    pub scan: GenerationScan,
}

#[derive(Debug, Clone)]
pub struct GenerationScan {
    pub document_count: usize,
    pub seo_sidecar: SeoSidecar,
}

pub fn open_structurally_complete_generation(
    path: &Utf8Path,
    manifest: &IndexGenerationManifest,
) -> Result<StructurallyCompleteGeneration> {
    validate_manifest_invariants(manifest)?;
    validate_generation_id(manifest).context("failed to validate index generation manifest id")?;

    let index =
        SearchIndex::open(path).with_context(|| format!("failed to open search index {path}"))?;
    let scan = scan_generation(&index, manifest)?;

    Ok(StructurallyCompleteGeneration { index, scan })
}

pub fn open_seo_complete_generation(
    path: &Utf8Path,
    manifest: &IndexGenerationManifest,
    sidecar: SeoSidecar,
) -> Result<SeoCompleteGeneration> {
    let complete = open_structurally_complete_generation(path, manifest)?;

    sidecar
        .validate_for_manifest(manifest)
        .context("failed to validate SEO sidecar against manifest")?;

    if sidecar != complete.scan.seo_sidecar {
        bail!("SEO sidecar facts do not match indexed documents");
    }

    Ok(SeoCompleteGeneration {
        index: complete.index,
        sidecar,
        scan: complete.scan,
    })
}

pub fn validate_manifest_invariants(manifest: &IndexGenerationManifest) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut target_sum = 0usize;

    for target in &manifest.targets {
        if !seen.insert((
            target.source.as_str(),
            target.ref_id.as_str(),
            target.artifact_kind,
        )) {
            bail!(
                "duplicate index generation target {}/{}/{}",
                target.source,
                target.ref_id,
                target.artifact_kind.as_str()
            );
        }

        target_sum = target_sum
            .checked_add(target.document_count)
            .context("index generation target document counts overflowed")?;
    }

    if manifest.document_count != target_sum {
        bail!(
            "index generation document_count mismatch: manifest {}, target sum {}",
            manifest.document_count,
            target_sum
        );
    }

    Ok(())
}

fn scan_generation(
    index: &SearchIndex,
    manifest: &IndexGenerationManifest,
) -> Result<GenerationScan> {
    let documents = index.scan_indexed_documents()?;
    if documents.len() != manifest.document_count {
        bail!(
            "indexed document count mismatch: index {}, manifest {}",
            documents.len(),
            manifest.document_count
        );
    }

    let expected = manifest_target_counts(manifest);
    let mut actual = BTreeMap::<TargetCountKey, usize>::new();
    let mut annotations = EntryAnnotationCounts::default();
    let mut seo = SeoSidecarAccumulator::new();

    for document in &documents {
        annotations.observe(&document.document);
    }

    for indexed in &documents {
        validate_annotation(indexed, &annotations)?;
        let key = target_key_for_document(&indexed.document);
        if !expected.contains_key(&key) {
            let common = indexed.document.common();
            bail!(
                "indexed document {}/{}/{} has no matching manifest target {}",
                common.source,
                common.ref_id,
                common.name,
                key.artifact_kind.as_str()
            );
        }

        *actual.entry(key).or_default() += 1;
        seo.observe(&indexed.document);
    }

    for (key, expected_count) in expected {
        let actual_count = actual.remove(&key).unwrap_or(0);
        if actual_count != expected_count {
            bail!(
                "manifest target count mismatch for {}/{}/{}: index {}, manifest {}",
                key.source,
                key.ref_id,
                key.artifact_kind.as_str(),
                actual_count,
                expected_count
            );
        }
    }

    if let Some((key, count)) = actual.into_iter().next() {
        bail!(
            "indexed documents for unmanifested target {}/{}/{}: {}",
            key.source,
            key.ref_id,
            key.artifact_kind.as_str(),
            count
        );
    }

    Ok(GenerationScan {
        document_count: documents.len(),
        seo_sidecar: seo.into_sidecar_for_manifest(manifest),
    })
}

fn manifest_target_counts(manifest: &IndexGenerationManifest) -> BTreeMap<TargetCountKey, usize> {
    manifest
        .targets
        .iter()
        .map(|target| {
            (
                TargetCountKey {
                    source: target.source.clone(),
                    ref_id: target.ref_id.clone(),
                    artifact_kind: target.artifact_kind,
                },
                target.document_count,
            )
        })
        .collect()
}

fn target_key_for_document(document: &SearchDocument) -> TargetCountKey {
    let common = document.common();
    TargetCountKey {
        source: common.source.clone(),
        ref_id: common.ref_id.clone(),
        artifact_kind: match document.kind() {
            DocumentKind::Option => ArtifactKind::OptionsJson,
            DocumentKind::Package => ArtifactKind::PackagesJson,
            DocumentKind::App | DocumentKind::Service => ArtifactKind::FlakeInfoJson,
        },
    }
}

fn validate_annotation(
    indexed: &IndexedSearchDocument,
    annotations: &EntryAnnotationCounts,
) -> Result<()> {
    let expected = annotations.annotation_for(&indexed.document);
    if indexed.annotation != expected {
        let common = indexed.document.common();
        bail!(
            "stored entry annotation mismatch for {}/{}/{}",
            common.source,
            common.ref_id,
            common.name
        );
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TargetCountKey {
    source: String,
    ref_id: String,
    artifact_kind: ArtifactKind,
}

#[derive(Debug, Default)]
struct EntryAnnotationCounts {
    entries: BTreeMap<EntryAnnotationKey, EntryKindCounts>,
}

impl EntryAnnotationCounts {
    fn observe(&mut self, document: &SearchDocument) {
        if !document.kind().is_supported_indexed_entry() {
            return;
        }

        let common = document.common();
        self.entries
            .entry(EntryAnnotationKey {
                source: common.source.clone(),
                ref_id: common.ref_id.clone(),
                name: common.name.clone(),
            })
            .or_default()
            .observe(document.kind());
    }

    fn annotation_for(&self, document: &SearchDocument) -> SearchHitAnnotation {
        let common = document.common();
        let counts = self.entries.get(&EntryAnnotationKey {
            source: common.source.clone(),
            ref_id: common.ref_id.clone(),
            name: common.name.clone(),
        });

        SearchHitAnnotation {
            ambiguous_entry_url: counts
                .map(|counts| counts.package_count > 0 && counts.option_count > 0)
                .unwrap_or(false),
            unique_within_kind: counts
                .map(|counts| counts.count_for_kind(document.kind()) == 1)
                .unwrap_or(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EntryAnnotationKey {
    source: String,
    ref_id: String,
    name: String,
}

#[derive(Debug, Clone, Default)]
struct EntryKindCounts {
    package_count: u8,
    option_count: u8,
}

impl EntryKindCounts {
    fn observe(&mut self, kind: &DocumentKind) {
        match kind {
            DocumentKind::Package => self.package_count = capped_increment(self.package_count),
            DocumentKind::Option => self.option_count = capped_increment(self.option_count),
            DocumentKind::App | DocumentKind::Service => {}
        }
    }

    fn count_for_kind(&self, kind: &DocumentKind) -> u8 {
        match kind {
            DocumentKind::Package => self.package_count,
            DocumentKind::Option => self.option_count,
            DocumentKind::App | DocumentKind::Service => 0,
        }
    }
}

fn capped_increment(value: u8) -> u8 {
    value.saturating_add(1).min(2)
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use nixsearch_core::artifact::ArtifactKind;

    use crate::manifest::{IndexGenerationManifest, IndexTargetManifest};

    use super::validate_manifest_invariants;

    const SOURCE: &str = "fixtures";
    const REF: &str = "small";

    #[test]
    fn manifest_invariants_accept_nonzero_flake_info_targets() {
        let manifest = IndexGenerationManifest::with_generated_at(
            2,
            vec![target(ArtifactKind::FlakeInfoJson, 2)],
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();

        validate_manifest_invariants(&manifest).unwrap();
    }

    #[test]
    fn manifest_invariants_still_reject_flake_info_count_mismatch() {
        let manifest = IndexGenerationManifest::with_generated_at(
            1,
            vec![target(ArtifactKind::FlakeInfoJson, 2)],
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();

        let error = validate_manifest_invariants(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("document_count mismatch"));
    }

    fn target(artifact_kind: ArtifactKind, document_count: usize) -> IndexTargetManifest {
        IndexTargetManifest {
            source: SOURCE.to_owned(),
            ref_id: REF.to_owned(),
            artifact_kind,
            document_count,
            artifact_hash: None,
            revision: None,
        }
    }
}
