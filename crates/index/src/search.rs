use std::collections::HashSet;
use std::fs;

use anyhow::{Context, Result, bail};
use camino::Utf8Path;
use tantivy::collector::{Count, TopDocs};
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, TantivyDocument, Value as _};
use tantivy::{DocAddress, Index, IndexReader, Searcher, Term};

use nixsearch_core::document::{DocumentKind, SearchDocument};

use crate::annotation::SearchHitAnnotation;
use crate::ranking::{QueryAnalysis, SearchCandidate, rerank_candidate_limit, rerank_candidates};
use crate::schema::{IndexFields, build_schema};
use crate::writer::SearchIndexWriter;

pub struct SearchIndex {
    pub(crate) index: Index,
    reader: IndexReader,
    pub(crate) fields: IndexFields,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub score: f32,
    pub document: SearchDocument,
    pub annotation: SearchHitAnnotation,
}

#[derive(Debug, Clone)]
struct StoredSearchDocument {
    document: SearchDocument,
    annotation: SearchHitAnnotation,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchScope {
    pub source: String,
    pub ref_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchOptions {
    pub query: String,
    pub limit: usize,
    pub offset: usize,
    pub scopes: Vec<SearchScope>,
}

#[derive(Debug, Clone)]
pub struct EntryLookup {
    pub source: String,
    pub ref_id: String,
    pub name: String,
    pub kind: Option<DocumentKind>,
}

#[derive(Debug, Clone)]
pub enum EntryLookupResult {
    Found(Box<SearchDocument>),
    NotFound,
    Ambiguous(Vec<SearchDocument>),
}

#[derive(Debug, Clone)]
pub struct EntryRepresentative {
    pub document: SearchDocument,
    pub seo_eligible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFactsStatus {
    NotFound,
    Unique,
    Ambiguous,
}

#[derive(Debug, Clone)]
pub struct EntryFacts {
    pub requested_kind: Option<DocumentKind>,
    pub package_count: usize,
    pub option_count: usize,
    pub representative: Option<EntryRepresentative>,
}

impl EntryFacts {
    pub fn count_for_kind(&self, kind: &DocumentKind) -> usize {
        match kind {
            DocumentKind::Package => self.package_count,
            DocumentKind::Option => self.option_count,
            DocumentKind::App | DocumentKind::Service => 0,
        }
    }

    pub fn supported_count(&self) -> usize {
        match &self.requested_kind {
            Some(kind) => self.count_for_kind(kind),
            None => self.package_count + self.option_count,
        }
    }

    pub fn status(&self) -> EntryFactsStatus {
        match self.supported_count() {
            0 => EntryFactsStatus::NotFound,
            1 => EntryFactsStatus::Unique,
            _ => EntryFactsStatus::Ambiguous,
        }
    }

    pub fn unique_supported_kind(&self) -> Option<DocumentKind> {
        match self.requested_kind.as_ref() {
            Some(kind) if kind.is_supported_indexed_entry() && self.count_for_kind(kind) == 1 => {
                Some(kind.clone())
            }
            Some(_) => None,
            None if self.package_count == 1 && self.option_count == 0 => {
                Some(DocumentKind::Package)
            }
            None if self.option_count == 1 && self.package_count == 0 => Some(DocumentKind::Option),
            None => None,
        }
    }

    pub fn seo_eligible(&self) -> Option<bool> {
        self.representative
            .as_ref()
            .map(|representative| representative.seo_eligible)
    }

    pub fn annotation_for_document(&self, document: &SearchDocument) -> SearchHitAnnotation {
        SearchHitAnnotation {
            ambiguous_entry_url: self.package_count > 0 && self.option_count > 0,
            unique_within_kind: self.count_for_kind(document.kind()) == 1,
        }
    }
}

impl SearchIndex {
    pub fn create_or_replace(path: impl AsRef<Utf8Path>) -> Result<Self> {
        let path = path.as_ref();

        if path.exists() {
            fs::remove_dir_all(path)
                .with_context(|| format!("failed to remove existing index {path}"))?;
        }

        fs::create_dir_all(path)
            .with_context(|| format!("failed to create index directory {path}"))?;

        let schema = build_schema();
        let fields = IndexFields::from_schema(&schema)?;
        let index = Index::create_in_dir(path, schema)
            .with_context(|| format!("failed to create Tantivy index at {path}"))?;
        let reader = index.reader().context("failed to create index reader")?;

        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    pub fn open(path: impl AsRef<Utf8Path>) -> Result<Self> {
        let path = path.as_ref();
        let index = Index::open_in_dir(path)
            .with_context(|| format!("failed to open Tantivy index at {path}"))?;
        let schema = index.schema();
        let fields = IndexFields::from_schema(&schema)?;
        let reader = index.reader().context("failed to create index reader")?;

        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    pub fn writer(&self) -> Result<SearchIndexWriter> {
        let writer = self
            .index
            .writer(crate::WRITER_MEMORY_BYTES)
            .context("failed to create index writer")?;

        Ok(SearchIndexWriter {
            writer,
            fields: self.fields.clone(),
        })
    }

    pub fn search(&self, options: SearchOptions) -> Result<SearchResult> {
        let searcher = self.reader.searcher();
        let candidate_limit = rerank_candidate_limit(options.limit, options.offset);
        let diversify_sources = options.scopes.len() > 1;
        let analysis = QueryAnalysis::new(&self.index, self.fields.name_text, &options.query)?;

        let (candidates, total) = if diversify_sources {
            let mut candidates = Vec::new();
            let mut total = 0;

            for scope in &options.scopes {
                let (mut scope_candidates, scope_total) = self.search_candidates(
                    &searcher,
                    &options.query,
                    std::slice::from_ref(scope),
                    candidate_limit,
                )?;
                total += scope_total;
                candidates.append(&mut scope_candidates);
            }

            (candidates, total)
        } else {
            self.search_candidates(&searcher, &options.query, &options.scopes, candidate_limit)?
        };
        let candidate_window_len = candidates.len();
        let candidate_ids = candidates
            .iter()
            .map(|candidate| candidate.document.id().to_owned())
            .collect::<HashSet<_>>();

        let mut hits = rerank_candidates(
            candidates,
            &analysis,
            diversify_sources,
            options.offset,
            options.limit,
        );

        if hits.len() < options.limit && total > options.offset.saturating_add(hits.len()) {
            let remaining = options.limit - hits.len();
            let mut fallback_hits = if diversify_sources {
                self.search_native_filtered_hits(
                    &searcher,
                    &options,
                    &candidate_ids,
                    options.offset.saturating_sub(candidate_window_len),
                    remaining,
                    total,
                )?
            } else {
                self.search_native_hits(
                    &searcher,
                    &options.query,
                    &options.scopes,
                    options.offset.max(candidate_window_len),
                    remaining,
                )?
            };

            hits.append(&mut fallback_hits);
        }

        Ok(SearchResult { hits, total })
    }

    fn search_candidates(
        &self,
        searcher: &Searcher,
        query_text: &str,
        scopes: &[SearchScope],
        limit: usize,
    ) -> Result<(Vec<SearchCandidate>, usize)> {
        let query = self.build_query(query_text, scopes)?;

        let (total, top_docs) = searcher
            .search(
                &*query,
                &(Count, TopDocs::with_limit(limit).order_by_score()),
            )
            .context("search failed")?;

        let mut candidates = Vec::with_capacity(top_docs.len());

        for (score, address) in top_docs {
            let stored = self.stored_document_at_with_searcher(searcher, address)?;
            candidates.push(SearchCandidate {
                score,
                document: stored.document,
                annotation: stored.annotation,
            });
        }

        Ok((candidates, total))
    }

    fn search_native_hits(
        &self,
        searcher: &Searcher,
        query_text: &str,
        scopes: &[SearchScope],
        offset: usize,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let query = self.build_query(query_text, scopes)?;
        let top_docs = searcher
            .search(
                &*query,
                &TopDocs::with_limit(limit)
                    .and_offset(offset)
                    .order_by_score(),
            )
            .context("native fallback search failed")?;

        let mut hits = Vec::with_capacity(top_docs.len());

        for (score, address) in top_docs {
            let stored = self.stored_document_at_with_searcher(searcher, address)?;
            hits.push(SearchHit {
                score,
                document: stored.document,
                annotation: stored.annotation,
            });
        }

        Ok(hits)
    }

    fn search_native_filtered_hits(
        &self,
        searcher: &Searcher,
        options: &SearchOptions,
        excluded_ids: &HashSet<String>,
        skip: usize,
        limit: usize,
        total: usize,
    ) -> Result<Vec<SearchHit>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let query = self.build_query(&options.query, &options.scopes)?;
        let mut hits = Vec::with_capacity(limit);
        let mut skipped = 0;
        let mut native_offset = 0;

        while hits.len() < limit && native_offset < total {
            let batch_limit = skip
                .saturating_sub(skipped)
                .saturating_add(limit - hits.len())
                .saturating_add(excluded_ids.len())
                .clamp(200, 5_000)
                .min(total - native_offset);

            if batch_limit == 0 {
                break;
            }

            let top_docs = searcher
                .search(
                    &*query,
                    &TopDocs::with_limit(batch_limit)
                        .and_offset(native_offset)
                        .order_by_score(),
                )
                .context("filtered native fallback search failed")?;

            if top_docs.is_empty() {
                break;
            }

            native_offset += top_docs.len();

            for (score, address) in top_docs {
                let stored = self.stored_document_at_with_searcher(searcher, address)?;
                let document = stored.document;

                if excluded_ids.contains(document.id()) {
                    continue;
                }

                if skipped < skip {
                    skipped += 1;
                    continue;
                }

                hits.push(SearchHit {
                    score,
                    document,
                    annotation: stored.annotation,
                });

                if hits.len() == limit {
                    break;
                }
            }
        }

        Ok(hits)
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<SearchDocument>> {
        let searcher = self.reader.searcher();

        let query = tantivy::query::TermQuery::new(
            tantivy::Term::from_field_text(self.fields.id, id),
            tantivy::schema::IndexRecordOption::Basic,
        );

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(1).order_by_score())
            .context("entry lookup by id failed")?;

        let Some((_, address)) = top_docs.into_iter().next() else {
            return Ok(None);
        };

        self.document_at(address).map(Some)
    }

    pub fn find_entry(&self, lookup: EntryLookup) -> Result<EntryLookupResult> {
        let facts = self.entry_facts(lookup.clone())?;

        self.find_entry_with_facts(lookup, &facts)
    }

    pub fn find_entry_with_facts(
        &self,
        lookup: EntryLookup,
        facts: &EntryFacts,
    ) -> Result<EntryLookupResult> {
        match facts.status() {
            EntryFactsStatus::NotFound => Ok(EntryLookupResult::NotFound),
            EntryFactsStatus::Unique => {
                let representative = facts
                    .representative
                    .as_ref()
                    .context("unique entry facts did not include representative")?;

                Ok(EntryLookupResult::Found(Box::new(
                    representative.document.clone(),
                )))
            }
            EntryFactsStatus::Ambiguous => {
                let searcher = self.reader.searcher();
                let documents =
                    self.ambiguous_entry_documents(&searcher, &lookup, facts.supported_count())?;

                Ok(EntryLookupResult::Ambiguous(documents))
            }
        }
    }

    pub fn entry_facts(&self, lookup: EntryLookup) -> Result<EntryFacts> {
        let searcher = self.reader.searcher();

        let package_count =
            self.count_exact_entry_kind(&searcher, &lookup, &DocumentKind::Package)?;
        let option_count =
            self.count_exact_entry_kind(&searcher, &lookup, &DocumentKind::Option)?;

        let mut facts = EntryFacts {
            requested_kind: lookup.kind.clone(),
            package_count,
            option_count,
            representative: None,
        };

        if let Some(kind) = facts.unique_supported_kind() {
            let document = self.exact_entry_representative(&searcher, &lookup, &kind)?;
            let seo_eligible = document.is_seo_eligible_entry();

            facts.representative = Some(EntryRepresentative {
                document,
                seo_eligible,
            });
        }

        Ok(facts)
    }

    fn count_exact_entry_kind(
        &self,
        searcher: &Searcher,
        lookup: &EntryLookup,
        kind: &DocumentKind,
    ) -> Result<usize> {
        let query = self.exact_entry_kind_query(lookup, kind);

        searcher
            .search(&*query, &Count)
            .with_context(|| format!("failed to count exact {} entry facts", kind.as_str()))
    }

    fn exact_entry_representative(
        &self,
        searcher: &Searcher,
        lookup: &EntryLookup,
        kind: &DocumentKind,
    ) -> Result<SearchDocument> {
        let query = self.exact_entry_kind_query(lookup, kind);
        let top_docs = searcher
            .search(&*query, &TopDocs::with_limit(1).order_by_score())
            .with_context(|| {
                format!(
                    "failed to retrieve unique exact {} entry representative",
                    kind.as_str()
                )
            })?;

        let Some((_, address)) = top_docs.into_iter().next() else {
            bail!(
                "exact {} entry count was one but representative was not found",
                kind.as_str()
            );
        };

        let document = self.document_at_with_searcher(searcher, address)?;

        if !document_deserialized_as_kind(&document, kind) {
            bail!(
                "unique exact {} entry representative deserialized as {}",
                kind.as_str(),
                document_variant_name(&document)
            );
        }

        Ok(document)
    }

    fn ambiguous_entry_documents(
        &self,
        searcher: &Searcher,
        lookup: &EntryLookup,
        count: usize,
    ) -> Result<Vec<SearchDocument>> {
        let Some(query) = self.supported_entry_query(lookup) else {
            return Ok(Vec::new());
        };

        let top_docs = searcher
            .search(&*query, &TopDocs::with_limit(count).order_by_score())
            .context("failed to retrieve ambiguous entry documents")?;

        let mut documents = Vec::with_capacity(top_docs.len());

        for (_, address) in top_docs {
            documents.push(self.document_at_with_searcher(searcher, address)?);
        }

        Ok(documents)
    }

    fn supported_entry_query(&self, lookup: &EntryLookup) -> Option<Box<dyn Query>> {
        let kinds = supported_lookup_kinds(lookup);

        if kinds.is_empty() {
            return None;
        }

        let mut clauses = self.exact_entry_identity_clauses(lookup);

        if kinds.len() == 1 {
            clauses.push((Occur::Must, self.entry_kind_query(&kinds[0])));
        } else {
            let kind_clauses = kinds
                .iter()
                .map(|kind| (Occur::Should, self.entry_kind_query(kind)))
                .collect::<Vec<_>>();

            clauses.push((Occur::Must, Box::new(BooleanQuery::new(kind_clauses))));
        }

        Some(Box::new(BooleanQuery::new(clauses)))
    }

    fn exact_entry_kind_query(&self, lookup: &EntryLookup, kind: &DocumentKind) -> Box<dyn Query> {
        let mut clauses = self.exact_entry_identity_clauses(lookup);
        clauses.push((Occur::Must, self.entry_kind_query(kind)));

        Box::new(BooleanQuery::new(clauses))
    }

    fn exact_entry_identity_clauses(&self, lookup: &EntryLookup) -> Vec<(Occur, Box<dyn Query>)> {
        vec![
            (
                Occur::Must,
                exact_term_query(self.fields.source, &lookup.source),
            ),
            (
                Occur::Must,
                exact_term_query(self.fields.ref_id, &lookup.ref_id),
            ),
            (
                Occur::Must,
                exact_term_query(self.fields.name_exact, &lookup.name),
            ),
        ]
    }

    fn entry_kind_query(&self, kind: &DocumentKind) -> Box<dyn Query> {
        exact_term_query(self.fields.kind, kind.as_str())
    }

    fn document_at(&self, address: DocAddress) -> Result<SearchDocument> {
        let searcher = self.reader.searcher();
        self.document_at_with_searcher(&searcher, address)
    }

    fn document_at_with_searcher(
        &self,
        searcher: &Searcher,
        address: DocAddress,
    ) -> Result<SearchDocument> {
        let retrieved = tantivy_document_at(searcher, address)?;

        search_document_from_tantivy(&self.fields, &retrieved)
    }

    fn stored_document_at_with_searcher(
        &self,
        searcher: &Searcher,
        address: DocAddress,
    ) -> Result<StoredSearchDocument> {
        let retrieved = tantivy_document_at(searcher, address)?;
        let document = search_document_from_tantivy(&self.fields, &retrieved)?;
        let ambiguous_entry_url = stored_bool(
            &retrieved,
            self.fields.entry_ambiguous_entry_url,
            "entry_ambiguous_entry_url",
        )?;
        let unique_within_kind = stored_bool(
            &retrieved,
            self.fields.entry_unique_within_kind,
            "entry_unique_within_kind",
        )?;

        Ok(StoredSearchDocument {
            annotation: SearchHitAnnotation {
                ambiguous_entry_url,
                unique_within_kind,
            },
            document,
        })
    }
}

fn tantivy_document_at(searcher: &Searcher, address: DocAddress) -> Result<TantivyDocument> {
    searcher
        .doc(address)
        .context("failed to retrieve entry document")
}

fn search_document_from_tantivy(
    fields: &IndexFields,
    retrieved: &TantivyDocument,
) -> Result<SearchDocument> {
    let stored_json = retrieved
        .get_first(fields.stored_json)
        .and_then(|value| value.as_str())
        .context("entry document did not contain stored_json")?;

    serde_json::from_str(stored_json).context("failed to deserialize entry document")
}

fn stored_bool(retrieved: &TantivyDocument, field: Field, name: &'static str) -> Result<bool> {
    retrieved
        .get_first(field)
        .and_then(|value| value.as_bool())
        .with_context(|| format!("entry document did not contain {name}"))
}

fn exact_term_query(field: tantivy::schema::Field, value: &str) -> Box<dyn Query> {
    Box::new(TermQuery::new(
        Term::from_field_text(field, value),
        IndexRecordOption::Basic,
    ))
}

fn supported_lookup_kinds(lookup: &EntryLookup) -> Vec<DocumentKind> {
    match &lookup.kind {
        Some(kind) if kind.is_supported_indexed_entry() => vec![kind.clone()],
        Some(_) => Vec::new(),
        None => vec![DocumentKind::Package, DocumentKind::Option],
    }
}

fn document_deserialized_as_kind(document: &SearchDocument, kind: &DocumentKind) -> bool {
    matches!(
        (document, kind),
        (SearchDocument::Package(_), DocumentKind::Package)
            | (SearchDocument::Option(_), DocumentKind::Option)
    ) && document.kind() == kind
}

fn document_variant_name(document: &SearchDocument) -> &'static str {
    match document {
        SearchDocument::Package(_) => "package",
        SearchDocument::Option(_) => "option",
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use tantivy::doc;
    use tempfile::tempdir;

    use nixsearch_test_support::{REF_SMALL, SOURCE_FIXTURES};

    use super::*;

    #[test]
    fn corrupt_unique_entry_representative_is_an_error() {
        let tempdir = tempdir().unwrap();
        let index_path = Utf8PathBuf::from_path_buf(tempdir.path().to_path_buf())
            .expect("test path must be valid UTF-8");

        let index = SearchIndex::create_or_replace(&index_path).unwrap();
        let mut writer = index.index.writer(crate::WRITER_MEMORY_BYTES).unwrap();

        writer
            .add_document(doc!(
                index.fields.id => "fixtures/small/option/corrupt.entry",
                index.fields.source => SOURCE_FIXTURES,
                index.fields.ref_id => REF_SMALL,
                index.fields.kind => "option",
                index.fields.name_exact => "corrupt.entry",
                index.fields.name_text => "corrupt.entry",
                index.fields.stored_json => "{not valid json",
            ))
            .unwrap();
        writer.commit().unwrap();

        let index = SearchIndex::open(&index_path).unwrap();
        let error = index
            .entry_facts(EntryLookup {
                source: SOURCE_FIXTURES.to_owned(),
                ref_id: REF_SMALL.to_owned(),
                name: "corrupt.entry".to_owned(),
                kind: Some(DocumentKind::Option),
            })
            .unwrap_err();

        assert!(format!("{error:#}").contains("failed to deserialize entry document"));
    }
}
