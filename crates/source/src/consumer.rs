use anyhow::{Context, Result, bail};
use async_trait::async_trait;

use nixsearch_core::artifact::ArtifactKind;
use nixsearch_core::document::SearchDocument;
use nixsearch_core::ingest::IngestContext;
use nixsearch_ingest::{
    parse_options_json_with_strip_prefixes, parse_packages_json_with_strip_prefixes,
};
use nixsearch_store::ArtifactStore;

use crate::artifact::ProducedArtifact;

#[async_trait]
pub trait Consumer: Send + Sync {
    async fn consume(
        &self,
        store: &ArtifactStore,
        artifact: &ProducedArtifact,
    ) -> Result<Vec<SearchDocument>>;
}

#[derive(Debug, Default, Clone)]
pub struct OptionsJsonConsumer {
    strip_prefixes: Vec<String>,
}

impl OptionsJsonConsumer {
    pub fn new(strip_prefixes: Vec<String>) -> Self {
        Self { strip_prefixes }
    }
}

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
            source: artifact.metadata.source.clone(),
            ref_id: artifact.metadata.ref_id.clone(),
            revision: artifact.metadata.revision.clone(),
            repo: None,
        };

        parse_options_json_with_strip_prefixes(bytes.as_ref(), &context, &self.strip_prefixes)
            .context("failed to parse options artifact")
    }
}

#[derive(Debug, Default, Clone)]
pub struct PackagesJsonConsumer {
    strip_prefixes: Vec<String>,
}

impl PackagesJsonConsumer {
    pub fn new(strip_prefixes: Vec<String>) -> Self {
        Self { strip_prefixes }
    }
}

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
            source: artifact.metadata.source.clone(),
            ref_id: artifact.metadata.ref_id.clone(),
            revision: artifact.metadata.revision.clone(),
            repo: None,
        };

        parse_packages_json_with_strip_prefixes(bytes.as_ref(), &context, &self.strip_prefixes)
            .context("failed to parse packages artifact")
    }
}

#[derive(Debug, Default, Clone)]
pub struct FlakeInfoJsonConsumer;

#[async_trait]
impl Consumer for FlakeInfoJsonConsumer {
    async fn consume(
        &self,
        store: &ArtifactStore,
        artifact: &ProducedArtifact,
    ) -> Result<Vec<SearchDocument>> {
        if artifact.artifact_ref.kind != ArtifactKind::FlakeInfoJson {
            anyhow::bail!(
                "FlakeInfoJsonConsumer cannot consume artifact kind {:?}",
                artifact.artifact_ref.kind
            );
        }

        let bytes = store
            .get_artifact(&artifact.artifact_ref)
            .await
            .context("failed to read flake-info artifact")?;

        let value = serde_json::from_slice::<serde_json::Value>(bytes.as_ref())
            .context("failed to parse flake-info artifact as JSON")?;

        validate_flake_info_json(&value).context("invalid flake-info artifact shape")?;

        Ok(Vec::new())
    }
}

fn validate_flake_info_json(value: &serde_json::Value) -> Result<()> {
    let items = value
        .as_array()
        .context("expected top-level array of flake-info exports")?;

    for (index, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .with_context(|| format!("expected export at index {index} to be an object"))?;

        let item_type = object
            .get("type")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .with_context(|| {
                format!("expected export at index {index} to have a non-empty string type")
            })?;

        let identity_field = match item_type {
            "package" => "package_attr_name",
            "app" => "app_attr_name",
            "option" | "service" | "home-manager-option" => "option_name",
            other => bail!("unsupported flake-info export type {other:?} at index {index}"),
        };

        object
            .get(identity_field)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .with_context(|| {
                format!(
                    "expected export at index {index} to have a non-empty string {identity_field}"
                )
            })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use bytes::Bytes;
    use tempfile::tempdir;

    use nixsearch_core::artifact::ArtifactKind;
    use nixsearch_store::{ArtifactMetadataInput, ArtifactRef, ArtifactStore};
    use nixsearch_test_support::{
        OPTION_GIT_ENABLE, OPTION_NGINX_ENABLE, REF_SMALL, SOURCE_FIXTURES,
    };

    use crate::artifact::{ProduceRequest, ProducedArtifact};
    use crate::consumer::{Consumer, FlakeInfoJsonConsumer, OptionsJsonConsumer};
    use crate::producers::{ExistingFileProducer, Producer};

    async fn put_flake_info_artifact(store: &ArtifactStore, bytes: Bytes) -> ProducedArtifact {
        let artifact_ref =
            ArtifactRef::latest(SOURCE_FIXTURES, REF_SMALL, ArtifactKind::FlakeInfoJson);
        let metadata = store
            .put_artifact(
                &artifact_ref,
                bytes,
                ArtifactMetadataInput::new("test-producer"),
            )
            .await
            .unwrap();

        ProducedArtifact {
            artifact_ref,
            metadata,
        }
    }

    #[tokio::test]
    async fn options_json_consumer_reads_produced_artifact() {
        let tempdir = tempdir().unwrap();
        let artifact_path = tempdir.path().join("options.json");
        let store_path = tempdir.path().join("store");

        fs::write(
            &artifact_path,
            include_bytes!("../../../fixtures/search-small/options.json"),
        )
        .unwrap();

        let store = ArtifactStore::local(&store_path).unwrap();

        let producer = ExistingFileProducer::new(&artifact_path, ArtifactKind::OptionsJson);
        let request = ProduceRequest {
            source: SOURCE_FIXTURES.into(),
            ref_id: REF_SMALL.into(),
        };

        let produced = producer.produce(&store, &request).await.unwrap();

        let consumer = OptionsJsonConsumer::default();
        let docs = consumer.consume(&store, &produced).await.unwrap();

        assert_eq!(docs.len(), 4);
        assert!(docs.iter().any(|doc| doc.name() == OPTION_GIT_ENABLE));
        assert!(docs.iter().any(|doc| doc.name() == OPTION_NGINX_ENABLE));
    }

    #[tokio::test]
    async fn flake_info_json_consumer_validates_minimal_exports_to_zero_documents() {
        let tempdir = tempdir().unwrap();
        let store = ArtifactStore::local(tempdir.path()).unwrap();
        let produced = put_flake_info_artifact(
            &store,
            Bytes::from_static(
                br#"[
                    {"type":"package","package_attr_name":"hello"},
                    {"type":"app","app_attr_name":"hello"},
                    {"type":"option","option_name":"programs.git.enable"},
                    {"type":"service","option_name":"services.example.enable"},
                    {"type":"home-manager-option","option_name":"programs.git.enable"}
                ]"#,
            ),
        )
        .await;

        let consumer = FlakeInfoJsonConsumer;
        let docs = consumer.consume(&store, &produced).await.unwrap();

        assert!(docs.is_empty());
    }

    #[tokio::test]
    async fn flake_info_json_consumer_rejects_invalid_shapes() {
        let invalid_shapes: &[(&str, &[u8])] = &[
            ("null", br#"null"#),
            ("string", br#""not an export list""#),
            ("number", br#"42"#),
            ("object", br#"{}"#),
            ("items object", br#"{"items":[]}"#),
            ("scalar item", br#"[null]"#),
            ("empty item", br#"[{}]"#),
            ("unknown type", br#"[{"type":"unknown","option_name":"x"}]"#),
            ("non-string type", br#"[{"type":1,"option_name":"x"}]"#),
            ("empty type", br#"[{"type":"","option_name":"x"}]"#),
            ("missing identity", br#"[{"type":"app"}]"#),
            (
                "non-string identity",
                br#"[{"type":"app","app_attr_name":1}]"#,
            ),
            ("empty identity", br#"[{"type":"app","app_attr_name":""}]"#),
        ];

        for &(name, bytes) in invalid_shapes {
            let tempdir = tempdir().unwrap();
            let store = ArtifactStore::local(tempdir.path()).unwrap();
            let produced = put_flake_info_artifact(&store, Bytes::from_static(bytes)).await;
            let consumer = FlakeInfoJsonConsumer;

            let error = consumer.consume(&store, &produced).await.unwrap_err();

            assert!(
                format!("{error:#}").contains("invalid flake-info artifact shape"),
                "expected invalid shape error for {name}"
            );
        }
    }
}
