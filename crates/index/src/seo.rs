use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use nixsearch_core::artifact::ArtifactKind;
use nixsearch_core::document::{DocumentKind, SearchDocument};

use crate::manifest::IndexGenerationManifest;

pub const SEO_SIDECAR_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SeoSidecar {
    pub schema_version: u32,
    pub generation_id: String,
    pub refs: Vec<SeoRefFacts>,
    pub entries: Vec<SeoEntryFacts>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SeoRefFacts {
    pub source: String,
    pub ref_id: String,
    pub total_supported_indexed_count: usize,
    pub package_supported_count: usize,
    pub option_supported_count: usize,
    pub package_eligible_count: usize,
    pub option_eligible_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SeoEntryFacts {
    pub source: String,
    pub ref_id: String,
    pub name: String,
    pub total_supported_indexed_count: usize,
    pub package_supported_count: usize,
    pub option_supported_count: usize,
    pub package_eligible_count: usize,
    pub option_eligible_count: usize,
}

#[derive(Debug, Default)]
pub struct SeoSidecarAccumulator {
    entries: BTreeMap<SeoEntryKey, SeoCounts>,
}

impl SeoSidecarAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, document: &SearchDocument) {
        if !document.kind().is_supported_indexed_entry() {
            return;
        }

        let common = document.common();
        let key = SeoEntryKey {
            source: common.source.clone(),
            ref_id: common.ref_id.clone(),
            name: common.name.clone(),
        };

        self.entries
            .entry(key)
            .or_default()
            .observe(document.kind(), document.is_seo_eligible_entry());
    }

    pub fn into_sidecar(self, generation_id: impl Into<String>) -> SeoSidecar {
        let mut refs = BTreeMap::<SeoRefKey, SeoCounts>::new();
        let mut entries = Vec::with_capacity(self.entries.len());

        for (key, counts) in self.entries {
            refs.entry(SeoRefKey {
                source: key.source.clone(),
                ref_id: key.ref_id.clone(),
            })
            .or_default()
            .merge(&counts);

            entries.push(SeoEntryFacts {
                source: key.source,
                ref_id: key.ref_id,
                name: key.name,
                total_supported_indexed_count: counts.total_supported_indexed_count,
                package_supported_count: counts.package_supported_count,
                option_supported_count: counts.option_supported_count,
                package_eligible_count: counts.package_eligible_count,
                option_eligible_count: counts.option_eligible_count,
            });
        }

        let refs = refs
            .into_iter()
            .map(|(key, counts)| SeoRefFacts {
                source: key.source,
                ref_id: key.ref_id,
                total_supported_indexed_count: counts.total_supported_indexed_count,
                package_supported_count: counts.package_supported_count,
                option_supported_count: counts.option_supported_count,
                package_eligible_count: counts.package_eligible_count,
                option_eligible_count: counts.option_eligible_count,
            })
            .collect();

        SeoSidecar {
            schema_version: SEO_SIDECAR_SCHEMA_VERSION,
            generation_id: generation_id.into(),
            refs,
            entries,
        }
    }
}

impl SeoSidecar {
    pub fn validate_for_manifest(&self, manifest: &IndexGenerationManifest) -> Result<()> {
        if self.schema_version != SEO_SIDECAR_SCHEMA_VERSION {
            bail!(
                "unsupported SEO sidecar schema version {}",
                self.schema_version
            );
        }

        if self.generation_id != manifest.generation_id {
            bail!(
                "SEO sidecar generation_id mismatch: stored {:?}, manifest {:?}",
                self.generation_id,
                manifest.generation_id
            );
        }

        let manifest_expected = manifest_supported_counts(manifest);
        let mut ref_sums = BTreeMap::<SeoRefKey, SeoCounts>::new();
        let mut seen_entries = BTreeSet::<SeoEntryKey>::new();

        for entry in &self.entries {
            entry.validate_counts().with_context(|| {
                format!(
                    "invalid SEO sidecar entry {}/{}/{}",
                    entry.source, entry.ref_id, entry.name
                )
            })?;

            let key = SeoEntryKey {
                source: entry.source.clone(),
                ref_id: entry.ref_id.clone(),
                name: entry.name.clone(),
            };

            if !seen_entries.insert(key) {
                bail!(
                    "duplicate SEO sidecar entry {}/{}/{}",
                    entry.source,
                    entry.ref_id,
                    entry.name
                );
            }

            validate_manifest_targets(
                &manifest_expected,
                &entry.source,
                &entry.ref_id,
                Some(&entry.name),
                entry.package_supported_count,
                entry.option_supported_count,
            )?;

            let counts = entry.counts();
            ref_sums
                .entry(SeoRefKey {
                    source: entry.source.clone(),
                    ref_id: entry.ref_id.clone(),
                })
                .or_default()
                .merge(&counts);
        }

        let mut seen_refs = BTreeSet::<SeoRefKey>::new();

        for ref_facts in &self.refs {
            ref_facts.validate_counts().with_context(|| {
                format!(
                    "invalid SEO sidecar ref {}/{}",
                    ref_facts.source, ref_facts.ref_id
                )
            })?;

            let key = SeoRefKey {
                source: ref_facts.source.clone(),
                ref_id: ref_facts.ref_id.clone(),
            };

            if !seen_refs.insert(key.clone()) {
                bail!(
                    "duplicate SEO sidecar ref {}/{}",
                    ref_facts.source,
                    ref_facts.ref_id
                );
            }

            let Some(sum) = ref_sums.remove(&key) else {
                bail!(
                    "SEO sidecar ref {}/{} has no entries",
                    ref_facts.source,
                    ref_facts.ref_id
                );
            };

            let actual = ref_facts.counts();
            if actual != sum {
                bail!(
                    "SEO sidecar ref totals mismatch for {}/{}",
                    ref_facts.source,
                    ref_facts.ref_id
                );
            }

            let Some(expected) = manifest_expected.get(&key) else {
                bail!(
                    "SEO sidecar ref {}/{} references ref missing from manifest",
                    ref_facts.source,
                    ref_facts.ref_id
                );
            };

            if actual.package_supported_count != expected.package_supported_count {
                bail!(
                    "SEO sidecar ref {}/{} package count mismatch: sidecar {}, manifest {}",
                    ref_facts.source,
                    ref_facts.ref_id,
                    actual.package_supported_count,
                    expected.package_supported_count
                );
            }

            if actual.option_supported_count != expected.option_supported_count {
                bail!(
                    "SEO sidecar ref {}/{} option count mismatch: sidecar {}, manifest {}",
                    ref_facts.source,
                    ref_facts.ref_id,
                    actual.option_supported_count,
                    expected.option_supported_count
                );
            }

            validate_manifest_targets(
                &manifest_expected,
                &ref_facts.source,
                &ref_facts.ref_id,
                None,
                ref_facts.package_supported_count,
                ref_facts.option_supported_count,
            )?;
        }

        if let Some((key, _)) = ref_sums.into_iter().next() {
            bail!(
                "SEO sidecar entries for {}/{} are missing ref totals",
                key.source,
                key.ref_id
            );
        }

        for (key, expected) in &manifest_expected {
            if expected.supported_count() > 0 && !seen_refs.contains(key) {
                bail!(
                    "SEO sidecar missing ref totals for {}/{} expected package={} option={}",
                    key.source,
                    key.ref_id,
                    expected.package_supported_count,
                    expected.option_supported_count
                );
            }
        }

        Ok(())
    }
}

impl SeoEntryFacts {
    fn counts(&self) -> SeoCounts {
        SeoCounts {
            total_supported_indexed_count: self.total_supported_indexed_count,
            package_supported_count: self.package_supported_count,
            option_supported_count: self.option_supported_count,
            package_eligible_count: self.package_eligible_count,
            option_eligible_count: self.option_eligible_count,
        }
    }

    fn validate_counts(&self) -> Result<()> {
        self.counts().validate()
    }
}

impl SeoRefFacts {
    fn counts(&self) -> SeoCounts {
        SeoCounts {
            total_supported_indexed_count: self.total_supported_indexed_count,
            package_supported_count: self.package_supported_count,
            option_supported_count: self.option_supported_count,
            package_eligible_count: self.package_eligible_count,
            option_eligible_count: self.option_eligible_count,
        }
    }

    fn validate_counts(&self) -> Result<()> {
        self.counts().validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SeoEntryKey {
    source: String,
    ref_id: String,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SeoRefKey {
    source: String,
    ref_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SeoCounts {
    total_supported_indexed_count: usize,
    package_supported_count: usize,
    option_supported_count: usize,
    package_eligible_count: usize,
    option_eligible_count: usize,
}

#[derive(Debug, Clone, Default)]
struct SeoManifestExpectedCounts {
    has_package_target: bool,
    has_option_target: bool,
    package_supported_count: usize,
    option_supported_count: usize,
}

impl SeoManifestExpectedCounts {
    fn supported_count(&self) -> usize {
        self.package_supported_count + self.option_supported_count
    }
}

impl SeoCounts {
    fn observe(&mut self, kind: &DocumentKind, eligible: bool) {
        match kind {
            DocumentKind::Package => {
                self.total_supported_indexed_count += 1;
                self.package_supported_count += 1;

                if eligible {
                    self.package_eligible_count += 1;
                }
            }
            DocumentKind::Option => {
                self.total_supported_indexed_count += 1;
                self.option_supported_count += 1;

                if eligible {
                    self.option_eligible_count += 1;
                }
            }
            DocumentKind::App | DocumentKind::Service => {}
        }
    }

    fn merge(&mut self, other: &Self) {
        self.total_supported_indexed_count += other.total_supported_indexed_count;
        self.package_supported_count += other.package_supported_count;
        self.option_supported_count += other.option_supported_count;
        self.package_eligible_count += other.package_eligible_count;
        self.option_eligible_count += other.option_eligible_count;
    }

    fn validate(&self) -> Result<()> {
        if self.total_supported_indexed_count
            != self.package_supported_count + self.option_supported_count
        {
            bail!("total_supported_indexed_count does not match package+option counts");
        }

        if self.package_eligible_count > self.package_supported_count {
            bail!("package_eligible_count exceeds package_supported_count");
        }

        if self.option_eligible_count > self.option_supported_count {
            bail!("option_eligible_count exceeds option_supported_count");
        }

        Ok(())
    }
}

fn manifest_supported_counts(
    manifest: &IndexGenerationManifest,
) -> BTreeMap<SeoRefKey, SeoManifestExpectedCounts> {
    let mut refs = BTreeMap::<SeoRefKey, SeoManifestExpectedCounts>::new();

    for target in &manifest.targets {
        let expected = refs
            .entry(SeoRefKey {
                source: target.source.clone(),
                ref_id: target.ref_id.clone(),
            })
            .or_default();

        match target.artifact_kind {
            ArtifactKind::PackagesJson => {
                expected.has_package_target = true;
                expected.package_supported_count += target.document_count;
            }
            ArtifactKind::OptionsJson => {
                expected.has_option_target = true;
                expected.option_supported_count += target.document_count;
            }
            ArtifactKind::FlakeInfoJson => {}
        }
    }

    refs
}

fn validate_manifest_targets(
    manifest_expected: &BTreeMap<SeoRefKey, SeoManifestExpectedCounts>,
    source: &str,
    ref_id: &str,
    name: Option<&str>,
    package_supported_count: usize,
    option_supported_count: usize,
) -> Result<()> {
    let label = sidecar_record_label(source, ref_id, name);
    let key = SeoRefKey {
        source: source.to_owned(),
        ref_id: ref_id.to_owned(),
    };

    let Some(expected) = manifest_expected.get(&key) else {
        bail!("{label} references ref missing from manifest");
    };

    if package_supported_count > 0 && !expected.has_package_target {
        bail!("{label} has package facts without package target");
    }

    if option_supported_count > 0 && !expected.has_option_target {
        bail!("{label} has option facts without option target");
    }

    Ok(())
}

fn sidecar_record_label(source: &str, ref_id: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("SEO sidecar entry {source}/{ref_id}/{name}"),
        None => format!("SEO sidecar ref {source}/{ref_id}"),
    }
}

#[cfg(test)]
mod tests {
    use nixsearch_core::artifact::ArtifactKind;
    use nixsearch_core::document::{OptionDoc, PackageDoc, SearchDocument};
    use nixsearch_core::ingest::IngestContext;

    use crate::manifest::{IndexGenerationManifest, IndexTargetManifest};
    use crate::seo::{SEO_SIDECAR_SCHEMA_VERSION, SeoSidecar, SeoSidecarAccumulator};

    const SOURCE: &str = "fixtures";
    const REF: &str = "small";

    fn context() -> IngestContext {
        IngestContext {
            source: SOURCE.to_owned(),
            ref_id: REF.to_owned(),
            revision: None,
            repo: None,
        }
    }

    fn manifest(package_count: usize, option_count: usize) -> IndexGenerationManifest {
        let mut targets = Vec::new();

        if package_count > 0 {
            targets.push(target(ArtifactKind::PackagesJson, package_count));
        }

        if option_count > 0 {
            targets.push(target(ArtifactKind::OptionsJson, option_count));
        }

        IndexGenerationManifest::with_generated_at(
            package_count + option_count,
            targets,
            time::OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap()
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

    fn package(name: &str) -> SearchDocument {
        SearchDocument::Package(PackageDoc::new(&context(), name))
    }

    fn option(name: &str) -> SearchDocument {
        SearchDocument::Option(OptionDoc::new(&context(), name))
    }

    fn hidden_option(name: &str) -> SearchDocument {
        let mut doc = OptionDoc::new(&context(), name);
        doc.visible = Some(false);
        SearchDocument::Option(doc)
    }

    fn sidecar_for(docs: &[SearchDocument], manifest: &IndexGenerationManifest) -> SeoSidecar {
        let mut accumulator = SeoSidecarAccumulator::new();

        for doc in docs {
            accumulator.observe(doc);
        }

        accumulator.into_sidecar(manifest.generation_id.clone())
    }

    #[test]
    fn accumulator_counts_supported_and_eligible_documents() {
        let manifest = manifest(1, 2);
        let docs = vec![
            package("git"),
            option("programs.git.enable"),
            hidden_option("internal.hidden"),
        ];

        let sidecar = sidecar_for(&docs, &manifest);

        sidecar.validate_for_manifest(&manifest).unwrap();

        assert_eq!(sidecar.refs.len(), 1);
        assert_eq!(sidecar.refs[0].total_supported_indexed_count, 3);
        assert_eq!(sidecar.refs[0].package_supported_count, 1);
        assert_eq!(sidecar.refs[0].option_supported_count, 2);
        assert_eq!(sidecar.refs[0].package_eligible_count, 1);
        assert_eq!(sidecar.refs[0].option_eligible_count, 1);
    }

    #[test]
    fn validation_rejects_generation_id_mismatch() {
        let manifest = manifest(1, 0);
        let mut sidecar = sidecar_for(&[package("git")], &manifest);
        sidecar.generation_id = "sha256:wrong".to_owned();

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("generation_id mismatch"));
    }

    #[test]
    fn validation_rejects_duplicate_entries() {
        let manifest = manifest(1, 0);
        let mut sidecar = sidecar_for(&[package("git")], &manifest);
        sidecar.entries.push(sidecar.entries[0].clone());

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("duplicate SEO sidecar entry"));
    }

    #[test]
    fn validation_rejects_duplicate_refs() {
        let manifest = manifest(1, 0);
        let mut sidecar = sidecar_for(&[package("git")], &manifest);
        sidecar.refs.push(sidecar.refs[0].clone());

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("duplicate SEO sidecar ref"));
    }

    #[test]
    fn validation_rejects_ref_totals_mismatch() {
        let manifest = manifest(1, 0);
        let mut sidecar = sidecar_for(&[package("git")], &manifest);
        sidecar.refs[0].package_supported_count += 1;
        sidecar.refs[0].total_supported_indexed_count += 1;

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("ref totals mismatch"));
    }

    #[test]
    fn validation_rejects_package_facts_without_package_target() {
        let manifest = manifest(0, 1);
        let sidecar = sidecar_for(&[package("git")], &manifest);

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("package facts without package target"));
    }

    #[test]
    fn validation_rejects_option_facts_without_option_target() {
        let manifest = manifest(1, 0);
        let sidecar = sidecar_for(&[option("programs.git.enable")], &manifest);

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("option facts without option target"));
    }

    #[test]
    fn validation_rejects_incomplete_sidecar_totals() {
        let manifest = manifest(2, 0);
        let sidecar = sidecar_for(&[package("git")], &manifest);

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("package count mismatch"));
    }

    #[test]
    fn validation_rejects_missing_ref_totals_for_manifest_documents() {
        let manifest = manifest(1, 0);
        let sidecar = SeoSidecar {
            schema_version: SEO_SIDECAR_SCHEMA_VERSION,
            generation_id: manifest.generation_id.clone(),
            refs: Vec::new(),
            entries: Vec::new(),
        };

        let error = sidecar.validate_for_manifest(&manifest).unwrap_err();

        assert!(format!("{error:#}").contains("missing ref totals"));
    }

    #[test]
    fn validation_accepts_flake_info_target_without_sidecar_refs() {
        let manifest = IndexGenerationManifest::with_generated_at(
            1,
            vec![target(ArtifactKind::FlakeInfoJson, 1)],
            time::OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();

        let sidecar = SeoSidecar {
            schema_version: SEO_SIDECAR_SCHEMA_VERSION,
            generation_id: manifest.generation_id.clone(),
            refs: Vec::new(),
            entries: Vec::new(),
        };

        sidecar.validate_for_manifest(&manifest).unwrap();
    }

    #[test]
    fn validation_accepts_zero_document_supported_targets_without_sidecar_refs() {
        let manifest = IndexGenerationManifest::with_generated_at(
            0,
            vec![
                target(ArtifactKind::PackagesJson, 0),
                target(ArtifactKind::OptionsJson, 0),
            ],
            time::OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();

        let sidecar = SeoSidecar {
            schema_version: SEO_SIDECAR_SCHEMA_VERSION,
            generation_id: manifest.generation_id.clone(),
            refs: Vec::new(),
            entries: Vec::new(),
        };

        sidecar.validate_for_manifest(&manifest).unwrap();
    }
}
