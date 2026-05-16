use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, STORED, STRING, Schema, TEXT, TantivyDocument, Value as _};
use tantivy::{Index, IndexReader, IndexWriter, doc};

use nix_search_core::{OptionDoc, SearchDocument};

const WRITER_MEMORY_BYTES: usize = 50_000_000;

#[derive(Debug, Clone)]
struct IndexFields {
    id: Field,
    project: Field,
    dataset: Field,
    ref_id: Field,
    kind: Field,
    name_exact: Field,
    name_text: Field,
    description: Field,
    option_set: Field,
    parents: Field,
    option_type: Field,
    stored_json: Field,
}

impl IndexFields {
    fn from_schema(schema: &Schema) -> Result<Self> {
        Ok(Self {
            id: schema.get_field("id").context("missing field id")?,
            project: schema
                .get_field("project")
                .context("missing field project")?,
            dataset: schema
                .get_field("dataset")
                .context("missing field dataset")?,
            ref_id: schema.get_field("ref").context("missing field ref")?,
            kind: schema.get_field("kind").context("missing field kind")?,
            name_exact: schema
                .get_field("name_exact")
                .context("missing field name_exact")?,
            name_text: schema
                .get_field("name_text")
                .context("missing field name_text")?,
            description: schema
                .get_field("description")
                .context("missing field description")?,
            option_set: schema
                .get_field("option_set")
                .context("missing field option_set")?,
            parents: schema
                .get_field("parents")
                .context("missing field parents")?,
            option_type: schema
                .get_field("option_type")
                .context("missing field option_type")?,
            stored_json: schema
                .get_field("stored_json")
                .context("missing field stored_json")?,
        })
    }
}

pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    fields: IndexFields,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub score: f32,
    pub document: SearchDocument,
}

impl SearchIndex {
    pub fn create_or_replace(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if path.exists() {
            fs::remove_dir_all(path)
                .with_context(|| format!("failed to remove existing index {}", path.display()))?;
        }

        fs::create_dir_all(path)
            .with_context(|| format!("failed to create index directory {}", path.display()))?;

        let schema = build_schema();
        let fields = IndexFields::from_schema(&schema)?;
        let index = Index::create_in_dir(path, schema)
            .with_context(|| format!("failed to create Tantivy index at {}", path.display()))?;
        let reader = index.reader().context("failed to create index reader")?;

        Ok(Self {
            index,
            reader,
            fields,
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let index = Index::open_in_dir(path)
            .with_context(|| format!("failed to open Tantivy index at {}", path.display()))?;
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
            .writer(WRITER_MEMORY_BYTES)
            .context("failed to create index writer")?;

        Ok(SearchIndexWriter {
            writer,
            fields: self.fields.clone(),
        })
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let searcher = self.reader.searcher();

        let mut parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.name_exact,
                self.fields.name_text,
                self.fields.description,
                self.fields.option_set,
                self.fields.parents,
                self.fields.option_type,
            ],
        );

        parser.set_field_boost(self.fields.name_exact, 20.0);
        parser.set_field_boost(self.fields.name_text, 10.0);
        parser.set_field_boost(self.fields.option_set, 3.0);
        parser.set_field_boost(self.fields.parents, 2.0);
        parser.set_field_boost(self.fields.option_type, 1.5);
        parser.set_field_boost(self.fields.description, 1.0);

        let query = parser
            .parse_query(query)
            .with_context(|| format!("failed to parse query {query:?}"))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit).order_by_score())
            .context("search failed")?;

        let mut hits = Vec::with_capacity(top_docs.len());

        for (score, address) in top_docs {
            let retrieved: TantivyDocument = searcher
                .doc(address)
                .context("failed to retrieve search result document")?;

            let stored_json = retrieved
                .get_first(self.fields.stored_json)
                .and_then(|value| value.as_str())
                .context("search result did not contain stored_json")?;

            let document = serde_json::from_str(stored_json)
                .context("failed to deserialize stored search document")?;

            hits.push(SearchHit { score, document });
        }

        Ok(hits)
    }
}

pub struct SearchIndexWriter {
    writer: IndexWriter,
    fields: IndexFields,
}

impl SearchIndexWriter {
    pub fn add_document(&mut self, document: &SearchDocument) -> Result<()> {
        match document {
            SearchDocument::Option(option) => self.add_option(option),
        }
    }

    pub fn commit(mut self) -> Result<()> {
        self.writer
            .commit()
            .context("failed to commit index writer")?;
        Ok(())
    }

    fn add_option(&mut self, option: &OptionDoc) -> Result<()> {
        let common = &option.common;
        let stored_json = serde_json::to_string(&SearchDocument::Option(option.clone()))
            .context("failed to serialize search document")?;

        let mut document = doc!(
            self.fields.id => common.id.clone(),
            self.fields.project => common.project.clone(),
            self.fields.dataset => common.dataset.clone(),
            self.fields.ref_id => common.ref_id.clone(),
            self.fields.kind => common.kind.as_str(),
            self.fields.name_exact => common.name.clone(),
            self.fields.name_text => common.name.clone(),
            self.fields.stored_json => stored_json,
        );

        if let Some(description) = &option.description {
            document.add_text(self.fields.description, description);
        }

        if let Some(option_set) = &option.option_set {
            document.add_text(self.fields.option_set, option_set);
        }

        if let Some(option_type) = &option.option_type {
            document.add_text(self.fields.option_type, option_type);
        }

        for parent in &option.parents {
            document.add_text(self.fields.parents, parent);
        }

        self.writer
            .add_document(document)
            .context("failed to add document to index")?;

        Ok(())
    }
}

fn build_schema() -> Schema {
    let mut builder = Schema::builder();

    builder.add_text_field("id", STRING | STORED);
    builder.add_text_field("project", STRING | STORED);
    builder.add_text_field("dataset", STRING | STORED);
    builder.add_text_field("ref", STRING | STORED);
    builder.add_text_field("kind", STRING | STORED);

    builder.add_text_field("name_exact", STRING | STORED);
    builder.add_text_field("name_text", TEXT | STORED);
    builder.add_text_field("description", TEXT);
    builder.add_text_field("option_set", STRING | STORED);
    builder.add_text_field("parents", STRING | STORED);
    builder.add_text_field("option_type", TEXT | STORED);

    builder.add_text_field("stored_json", STORED);

    builder.build()
}
