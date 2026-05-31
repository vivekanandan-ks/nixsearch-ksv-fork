use std::sync::Arc;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use nixsearch_config::app::AppConfig;
use nixsearch_core::document::DocumentKind;
use nixsearch_index::search::{
    EntryLookup, EntryLookupResult, SearchIndex, SearchOptions, SearchResult, SearchScope,
};
use nixsearch_index::store::IndexStore;

#[derive(Debug, Clone)]
pub struct SearchService {
    config: Arc<AppConfig>,
}

#[derive(Debug, Clone, Default)]
pub struct SearchRequest {
    pub query: String,
    pub source: Option<String>,
    pub ref_id: Option<String>,
    pub ref_set: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

#[derive(Debug, Clone)]
pub struct EntryRequest {
    pub source: String,
    pub ref_id: Option<String>,
    pub name: String,
    pub kind: Option<DocumentKind>,
}

impl SearchService {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &AppConfig {
        &self.config
    }

    pub fn config_arc(&self) -> Arc<AppConfig> {
        Arc::clone(&self.config)
    }

    pub fn current_index_path(&self) -> Result<Utf8PathBuf> {
        let index_store = IndexStore::new(&self.config.data.index_dir);

        index_store.current_path().with_context(|| {
            format!(
                "failed to locate current index in {}",
                self.config.data.index_dir
            )
        })
    }

    pub fn open_current_index(&self) -> Result<SearchIndex> {
        let current_path = self.current_index_path()?;
        self.open_index(&current_path)
    }

    pub fn open_index(&self, path: impl AsRef<Utf8Path>) -> Result<SearchIndex> {
        let path = path.as_ref();

        SearchIndex::open(path)
            .with_context(|| format!("failed to open search index {}", path.as_str()))
    }

    pub fn search_current(&self, request: SearchRequest) -> Result<SearchResult> {
        let index = self.open_current_index()?;
        self.search_with_index(&index, request)
    }

    pub fn search_with_index(
        &self,
        index: &SearchIndex,
        request: SearchRequest,
    ) -> Result<SearchResult> {
        let scopes = self.search_scopes(
            request.source.as_deref(),
            request.ref_id.as_deref(),
            request.ref_set.as_deref(),
        )?;

        index
            .search(SearchOptions {
                query: request.query,
                limit: request.limit,
                offset: request.offset,
                scopes,
            })
            .context("search failed")
    }

    pub fn find_entry_current(&self, request: EntryRequest) -> Result<EntryLookupResult> {
        let index = self.open_current_index()?;
        self.find_entry_with_index(&index, request)
    }

    pub fn find_entry_with_index(
        &self,
        index: &SearchIndex,
        request: EntryRequest,
    ) -> Result<EntryLookupResult> {
        let ref_id = self.resolve_entry_ref(&request.source, request.ref_id.as_deref())?;

        index
            .find_entry(EntryLookup {
                source: request.source,
                ref_id,
                name: request.name,
                kind: request.kind,
            })
            .context("entry lookup failed")
    }

    pub fn search_scopes(
        &self,
        source: Option<&str>,
        ref_id: Option<&str>,
        ref_set: Option<&str>,
    ) -> Result<Vec<SearchScope>> {
        let scopes = self
            .config
            .resolve_search_scopes(source, ref_id, ref_set)
            .context("failed to resolve search scope")?
            .into_iter()
            .map(|scope| SearchScope {
                source: scope.source,
                ref_id: scope.ref_id,
            })
            .collect();

        Ok(scopes)
    }

    pub fn resolve_entry_ref(&self, source_id: &str, ref_id: Option<&str>) -> Result<String> {
        if let Some(ref_id) = ref_id.and_then(non_empty) {
            return Ok(ref_id.to_owned());
        }

        let source = self
            .config
            .sources
            .get(source_id)
            .with_context(|| format!("unknown source {source_id:?}"))?;

        source
            .default_ref
            .clone()
            .with_context(|| format!("source {source_id:?} has no default ref"))
    }
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tempfile::tempdir;

    use nixsearch_core::document::DocumentKind;
    use nixsearch_index::search::EntryLookupResult;
    use nixsearch_index_test_support::publish_canonical_index;
    use nixsearch_test_support::{REF_SMALL, SOURCE_FIXTURES, app_config, utf8_path_buf};

    use super::{EntryRequest, SearchRequest, SearchService};

    #[test]
    fn search_current_uses_configured_default_scopes() {
        let tempdir = tempdir().unwrap();
        let index_dir = utf8_path_buf(tempdir.path().join("indexes"));
        publish_canonical_index(&index_dir);

        let config = Arc::new(app_config(&index_dir));
        let service = SearchService::new(config);

        let result = service
            .search_current(SearchRequest {
                query: "git".to_owned(),
                limit: 10,
                ..SearchRequest::default()
            })
            .unwrap();

        assert!(result.total > 0);
        assert!(!result.hits.is_empty());
    }

    #[test]
    fn search_with_explicit_source_and_ref() {
        let tempdir = tempdir().unwrap();
        let index_dir = utf8_path_buf(tempdir.path().join("indexes"));
        publish_canonical_index(&index_dir);

        let config = Arc::new(app_config(&index_dir));
        let service = SearchService::new(config);

        let result = service
            .search_current(SearchRequest {
                query: "programs.git.enable".to_owned(),
                source: Some(SOURCE_FIXTURES.to_owned()),
                ref_id: Some(REF_SMALL.to_owned()),
                limit: 10,
                ..SearchRequest::default()
            })
            .unwrap();

        assert!(result.total > 0);
        assert!(
            result
                .hits
                .iter()
                .any(|hit| hit.document.name() == "programs.git.enable")
        );
    }

    #[test]
    fn find_entry_current_resolves_entry() {
        let tempdir = tempdir().unwrap();
        let index_dir = utf8_path_buf(tempdir.path().join("indexes"));
        publish_canonical_index(&index_dir);

        let config = Arc::new(app_config(&index_dir));
        let service = SearchService::new(config);

        let result = service
            .find_entry_current(EntryRequest {
                source: SOURCE_FIXTURES.to_owned(),
                ref_id: Some(REF_SMALL.to_owned()),
                name: "programs.git.enable".to_owned(),
                kind: Some(DocumentKind::Option),
            })
            .unwrap();

        assert!(matches!(result, EntryLookupResult::Found(_)));
    }
}
