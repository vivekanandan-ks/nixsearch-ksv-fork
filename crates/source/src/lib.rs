use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use tokio::process::Command;

use nix_search_core::{ArtifactKind, IngestContext, SearchDocument};
use nix_search_ingest::{parse_options_json, parse_packages_json};
use nix_search_store::{ArtifactMetadata, ArtifactMetadataInput, ArtifactRef, ArtifactStore};

#[derive(Debug, Clone)]
pub struct ProduceRequest {
    pub project: String,
    pub dataset: String,
    pub ref_id: String,
}

impl ProduceRequest {
    pub fn artifact_ref(&self, kind: ArtifactKind) -> ArtifactRef {
        ArtifactRef::latest(
            self.project.clone(),
            self.dataset.clone(),
            self.ref_id.clone(),
            kind,
        )
    }

    pub fn ingest_context(&self, revision: Option<String>) -> IngestContext {
        IngestContext {
            project: self.project.clone(),
            dataset: self.dataset.clone(),
            ref_id: self.ref_id.clone(),
            revision,
            repo: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProducedArtifact {
    pub artifact_ref: ArtifactRef,
    pub metadata: ArtifactMetadata,
}

#[async_trait]
pub trait Producer: Send + Sync {
    async fn produce(
        &self,
        store: &ArtifactStore,
        request: &ProduceRequest,
    ) -> Result<ProducedArtifact>;
}

#[async_trait]
pub trait Consumer: Send + Sync {
    async fn consume(
        &self,
        store: &ArtifactStore,
        artifact: &ProducedArtifact,
    ) -> Result<Vec<SearchDocument>>;
}

#[derive(Debug, Clone)]
pub struct ExistingFileProducer {
    path: PathBuf,
    kind: ArtifactKind,
    producer_name: String,
}

impl ExistingFileProducer {
    pub fn new(path: impl Into<PathBuf>, kind: ArtifactKind) -> Self {
        Self {
            path: path.into(),
            kind,
            producer_name: "existing-file".to_owned(),
        }
    }

    pub fn with_name(
        path: impl Into<PathBuf>,
        kind: ArtifactKind,
        producer_name: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            kind,
            producer_name: producer_name.into(),
        }
    }
}

#[async_trait]
impl Producer for ExistingFileProducer {
    async fn produce(
        &self,
        store: &ArtifactStore,
        request: &ProduceRequest,
    ) -> Result<ProducedArtifact> {
        let bytes = tokio::fs::read(&self.path)
            .await
            .with_context(|| format!("failed to read artifact file {}", self.path.display()))?;

        let artifact_ref = request.artifact_ref(self.kind);

        let mut metadata_input = ArtifactMetadataInput::new(self.producer_name.clone());
        metadata_input.source = Some(self.path.display().to_string());

        let metadata = store
            .put_artifact(&artifact_ref, Bytes::from(bytes), metadata_input)
            .await
            .context("failed to write artifact to store")?;

        Ok(ProducedArtifact {
            artifact_ref,
            metadata,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NixBuildOptionsJsonProducer {
    source_ref: String,
    attribute: String,
    import_path: String,
    output_path: String,
    producer_name: String,
}

impl NixBuildOptionsJsonProducer {
    pub fn new(
        source_ref: impl Into<String>,
        attribute: impl Into<String>,
        import_path: impl Into<String>,
        output_path: impl Into<String>,
    ) -> Self {
        Self {
            source_ref: source_ref.into(),
            attribute: attribute.into(),
            import_path: import_path.into(),
            output_path: output_path.into(),
            producer_name: "nix-build-options-json".to_owned(),
        }
    }
}

#[async_trait]
impl Producer for NixBuildOptionsJsonProducer {
    async fn produce(
        &self,
        store: &ArtifactStore,
        request: &ProduceRequest,
    ) -> Result<ProducedArtifact> {
        let nix_path_source = normalize_nix_path_source(&self.source_ref);
        let expression = format!("<nixpkgs/{}>", self.import_path);

        let output = Command::new("nix-build")
            .arg("--no-build-output")
            .arg(&expression)
            .arg("--attr")
            .arg(&self.attribute)
            .arg("--no-out-link")
            .arg("-I")
            .arg(format!("nixpkgs={nix_path_source}"))
            .output()
            .await
            .with_context(|| {
                format!(
                    "failed to run nix-build for source ref {:?}",
                    self.source_ref
                )
            })?;

        if !output.status.success() {
            anyhow::bail!(
                "nix-build failed with status {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }

        let result_path = String::from_utf8(output.stdout)
            .context("nix-build stdout was not valid UTF-8")?
            .trim()
            .to_owned();

        if result_path.is_empty() {
            anyhow::bail!("nix-build succeeded but did not print an output path");
        }

        let artifact_path = PathBuf::from(result_path).join(&self.output_path);

        let bytes = tokio::fs::read(&artifact_path).await.with_context(|| {
            format!(
                "failed to read generated options JSON {}",
                artifact_path.display()
            )
        })?;

        let artifact_ref = request.artifact_ref(ArtifactKind::OptionsJson);

        let mut metadata_input = ArtifactMetadataInput::new(self.producer_name.clone());
        metadata_input.source = Some(self.source_ref.clone());

        let metadata = store
            .put_artifact(&artifact_ref, Bytes::from(bytes), metadata_input)
            .await
            .context("failed to write options artifact to store")?;

        Ok(ProducedArtifact {
            artifact_ref,
            metadata,
        })
    }
}

fn normalize_nix_path_source(source_ref: &str) -> String {
    if let Some(rest) = source_ref.strip_prefix("github:") {
        let mut parts = rest.splitn(3, '/');

        if let (Some(owner), Some(repo), Some(branch_or_ref)) =
            (parts.next(), parts.next(), parts.next())
        {
            return format!(
                "https://github.com/{owner}/{repo}/archive/refs/heads/{branch_or_ref}.tar.gz"
            );
        }
    }

    source_ref.to_owned()
}

#[derive(Debug, Clone)]
pub struct ChannelPackagesJsonProducer {
    channel: String,
    url: Option<String>,
    producer_name: String,
}

impl ChannelPackagesJsonProducer {
    pub fn new(channel: impl Into<String>, url: Option<String>) -> Self {
        Self {
            channel: channel.into(),
            url,
            producer_name: "channel-packages-json".to_owned(),
        }
    }

    fn url(&self) -> String {
        self.url.clone().unwrap_or_else(|| {
            format!(
                "https://channels.nixos.org/{}/packages.json.br",
                self.channel
            )
        })
    }
}

#[async_trait]
impl Producer for ChannelPackagesJsonProducer {
    async fn produce(
        &self,
        store: &ArtifactStore,
        request: &ProduceRequest,
    ) -> Result<ProducedArtifact> {
        let url = self.url();

        let compressed = reqwest::get(&url)
            .await
            .with_context(|| format!("failed to fetch {url}"))?
            .error_for_status()
            .with_context(|| format!("HTTP error fetching {url}"))?
            .bytes()
            .await
            .with_context(|| format!("failed to read response body from {url}"))?;

        let decompressed = decompress_brotli(&compressed)
            .with_context(|| format!("failed to decompress Brotli response from {url}"))?;

        let artifact_ref = request.artifact_ref(ArtifactKind::PackagesJson);

        let mut metadata_input = ArtifactMetadataInput::new(self.producer_name.clone());
        metadata_input.source = Some(url);

        let metadata = store
            .put_artifact(&artifact_ref, Bytes::from(decompressed), metadata_input)
            .await
            .context("failed to write packages artifact to store")?;

        Ok(ProducedArtifact {
            artifact_ref,
            metadata,
        })
    }
}

fn decompress_brotli(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut decompressor = brotli::Decompressor::new(bytes, 4096);
    let mut output = Vec::new();

    decompressor
        .read_to_end(&mut output)
        .context("Brotli decompression failed")?;

    Ok(output)
}

#[derive(Debug, Default, Clone)]
pub struct OptionsJsonConsumer;

#[async_trait]
impl Consumer for OptionsJsonConsumer {
    async fn consume(
        &self,
        store: &ArtifactStore,
        artifact: &ProducedArtifact,
    ) -> Result<Vec<SearchDocument>> {
        if artifact.artifact_ref.kind != ArtifactKind::OptionsJson {
            anyhow::bail!(
                "OptionsJsonConsumer cannot consume artifact kind {:?}",
                artifact.artifact_ref.kind
            );
        }

        let bytes = store
            .get_artifact(&artifact.artifact_ref)
            .await
            .context("failed to read options artifact")?;

        let context = IngestContext {
            project: artifact.metadata.project.clone(),
            dataset: artifact.metadata.dataset.clone(),
            ref_id: artifact.metadata.ref_id.clone(),
            revision: artifact.metadata.revision.clone(),
            repo: None,
        };

        parse_options_json(bytes.as_ref(), &context).context("failed to parse options artifact")
    }
}

#[derive(Debug, Default, Clone)]
pub struct PackagesJsonConsumer;

#[async_trait]
impl Consumer for PackagesJsonConsumer {
    async fn consume(
        &self,
        store: &ArtifactStore,
        artifact: &ProducedArtifact,
    ) -> Result<Vec<SearchDocument>> {
        if artifact.artifact_ref.kind != ArtifactKind::PackagesJson {
            anyhow::bail!(
                "PackagesJsonConsumer cannot consume artifact kind {:?}",
                artifact.artifact_ref.kind
            );
        }

        let bytes = store
            .get_artifact(&artifact.artifact_ref)
            .await
            .context("failed to read packages artifact")?;

        let context = IngestContext {
            project: artifact.metadata.project.clone(),
            dataset: artifact.metadata.dataset.clone(),
            ref_id: artifact.metadata.ref_id.clone(),
            revision: artifact.metadata.revision.clone(),
            repo: None,
        };

        parse_packages_json(bytes.as_ref(), &context).context("failed to parse packages artifact")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use nix_search_core::ArtifactKind;
    use nix_search_store::ArtifactStore;
    use tempfile::tempdir;

    use crate::ChannelPackagesJsonProducer;

    use super::{
        Consumer, ExistingFileProducer, OptionsJsonConsumer, ProduceRequest, Producer,
        normalize_nix_path_source,
    };

    #[tokio::test]
    async fn existing_file_producer_writes_artifact_to_store() {
        let tempdir = tempdir().unwrap();
        let artifact_path = tempdir.path().join("options.json");
        let store_path = tempdir.path().join("store");

        fs::write(
            &artifact_path,
            r#"
               {
                 "programs.git.enable": {
                   "description": "Whether to enable Git."
                 }
               }
               "#,
        )
        .unwrap();

        let store = ArtifactStore::local(&store_path).unwrap();

        let producer = ExistingFileProducer::new(&artifact_path, ArtifactKind::OptionsJson);
        let request = ProduceRequest {
            project: "fixtures".into(),
            dataset: "options".into(),
            ref_id: "small".into(),
        };

        let produced = producer.produce(&store, &request).await.unwrap();

        assert_eq!(produced.artifact_ref.kind, ArtifactKind::OptionsJson);
        assert_eq!(produced.metadata.project, "fixtures");
        assert_eq!(produced.metadata.dataset, "options");
        assert_eq!(produced.metadata.ref_id, "small");
        assert_eq!(produced.metadata.producer, "existing-file");
        assert_eq!(
            produced.metadata.source.as_deref(),
            Some(artifact_path.to_string_lossy().as_ref())
        );

        assert!(store.exists(&produced.artifact_ref).await.unwrap());
    }

    #[tokio::test]
    async fn options_json_consumer_reads_produced_artifact() {
        let tempdir = tempdir().unwrap();
        let artifact_path = tempdir.path().join("options.json");
        let store_path = tempdir.path().join("store");

        fs::write(
            &artifact_path,
            r#"
               {
                 "programs.git.enable": {
                   "description": "Whether to enable Git.",
                   "loc": ["programs", "git", "enable"]
                 },
                 "services.nginx.enable": {
                   "description": "Whether to enable Nginx.",
                   "loc": ["services", "nginx", "enable"]
                 }
               }
               "#,
        )
        .unwrap();

        let store = ArtifactStore::local(&store_path).unwrap();

        let producer = ExistingFileProducer::new(&artifact_path, ArtifactKind::OptionsJson);
        let request = ProduceRequest {
            project: "fixtures".into(),
            dataset: "options".into(),
            ref_id: "small".into(),
        };

        let produced = producer.produce(&store, &request).await.unwrap();

        let consumer = OptionsJsonConsumer;
        let docs = consumer.consume(&store, &produced).await.unwrap();

        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].name(), "programs.git.enable");
        assert_eq!(docs[1].name(), "services.nginx.enable");
    }

    #[test]
    fn normalizes_github_flake_ref_to_tarball_url() {
        let normalized = normalize_nix_path_source("github:NixOS/nixpkgs/nixos-unstable");

        assert_eq!(
            normalized,
            "https://github.com/NixOS/nixpkgs/archive/refs/heads/nixos-unstable.tar.gz"
        );
    }

    #[test]
    fn leaves_non_github_sources_unchanged() {
        let source = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.xz";

        assert_eq!(normalize_nix_path_source(source), source);
    }

    #[test]
    fn channel_packages_json_producer_builds_default_url() {
        let producer = ChannelPackagesJsonProducer::new("nixos-unstable", None);

        assert_eq!(
            producer.url(),
            "https://channels.nixos.org/nixos-unstable/packages.json.br"
        );
    }
}
