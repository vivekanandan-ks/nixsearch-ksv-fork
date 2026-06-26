use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use sha2::{Digest, Sha256};

use super::layout::INTEGRITY_TEMP_PREFIX;
use super::{IndexStore, PublishedGeneration};

const INTEGRITY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct GenerationIntegrity {
    schema_version: u32,
    manifest_generation_id: String,
    manifest_hash: String,
    seo_sidecar_hash: Option<String>,
    index_fingerprint: String,
}

impl IndexStore {
    pub fn write_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<()> {
        let generation_path = self.validate_generation_path(&generation.path)?;
        let integrity = self.build_integrity(generation, seo_sidecar_required)?;
        let bytes = serde_json::to_vec_pretty(&integrity)
            .context("failed to serialize index generation integrity metadata")?;
        let path = self.integrity_path(&generation_path);
        let temp_path = self.create_temp_file(&generation_path, INTEGRITY_TEMP_PREFIX, &bytes)?;

        if let Err(error) = fs::rename(&temp_path, &path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error)
                .with_context(|| format!("failed to write integrity metadata {path}"));
        }

        Self::sync_file(&path)?;
        Self::sync_dir(&generation_path)?;

        Ok(())
    }

    fn build_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<GenerationIntegrity> {
        let manifest_path = self.manifest_path(&generation.path);
        let sidecar_path = self.seo_sidecar_path(&generation.path);
        let seo_sidecar_hash = match hash_file_if_present(&sidecar_path)? {
            Some(hash) => Some(hash),
            None if seo_sidecar_required => {
                anyhow::bail!("SEO sidecar is required before writing generation integrity")
            }
            None => None,
        };

        Ok(GenerationIntegrity {
            schema_version: INTEGRITY_SCHEMA_VERSION,
            manifest_generation_id: generation.manifest.generation_id.clone(),
            manifest_hash: hash_file(&manifest_path)?,
            seo_sidecar_hash,
            index_fingerprint: self.index_fingerprint(&generation.path)?,
        })
    }

    fn read_integrity(&self, generation: &PublishedGeneration) -> Result<GenerationIntegrity> {
        let path = self.integrity_path(&generation.path);
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read integrity metadata {path}"))?;
        let integrity: GenerationIntegrity = serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse integrity metadata {path}"))?;

        if integrity.schema_version != INTEGRITY_SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported integrity schema version {} (current {})",
                integrity.schema_version,
                INTEGRITY_SCHEMA_VERSION
            );
        }

        Ok(integrity)
    }

    pub(super) fn validate_integrity(
        &self,
        generation: &PublishedGeneration,
        seo_sidecar_required: bool,
    ) -> Result<()> {
        let expected = self.build_integrity(generation, seo_sidecar_required)?;
        let actual = self.read_integrity(generation)?;

        if actual != expected {
            anyhow::bail!("index generation integrity metadata does not match generation files");
        }

        Ok(())
    }

    fn index_fingerprint(&self, generation_path: &Utf8Path) -> Result<String> {
        let index_path = self.index_path(generation_path);
        let mut entries = Vec::new();
        collect_index_files(&index_path, &index_path, &mut entries)?;
        entries.sort_by(|left, right| left.0.cmp(&right.0));

        let mut hasher = Sha256::new();
        for (relative, size, hash) in entries {
            hasher.update(relative.as_bytes());
            hasher.update(b"\0");
            hasher.update(size.to_string().as_bytes());
            hasher.update(b"\0");
            hasher.update(hash.as_bytes());
            hasher.update(b"\0");
        }

        Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
    }
}

fn hash_file(path: &Utf8Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("failed to read file {path}"))?;
    Ok(hash_bytes(&bytes))
}

fn hash_file_if_present(path: &Utf8Path) -> Result<Option<String>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(hash_bytes(&bytes))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read file {path}")),
    }
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn collect_index_files(
    root: &Utf8Path,
    path: &Utf8Path,
    entries: &mut Vec<(String, u64, String)>,
) -> Result<()> {
    for entry in fs::read_dir(path).with_context(|| format!("failed to read index dir {path}"))? {
        let entry = entry.with_context(|| format!("failed to read index dir entry in {path}"))?;
        let entry_path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|path| anyhow::anyhow!("index path is not valid UTF-8: {}", path.display()))?;
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to stat index file {entry_path}"))?;

        if metadata.is_dir() {
            collect_index_files(root, &entry_path, entries)?;
            continue;
        }

        if !metadata.is_file() {
            anyhow::bail!("unexpected non-regular index path {entry_path}");
        }

        let relative = entry_path
            .strip_prefix(root)
            .with_context(|| format!("failed to relativize index path {entry_path}"))?
            .as_str()
            .to_owned();
        entries.push((relative, metadata.len(), hash_file(&entry_path)?));
    }

    Ok(())
}
