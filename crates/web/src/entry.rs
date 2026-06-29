use nixsearch_core::document::SearchDocument;
use nixsearch_index::annotation::SearchHitAnnotation;
use nixsearch_index::search::EntryFacts;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnnotatedEntryDocument {
    pub document: Box<SearchDocument>,
    pub annotation: SearchHitAnnotation,
}

impl AnnotatedEntryDocument {
    pub(crate) fn from_facts(document: SearchDocument, facts: &EntryFacts) -> Self {
        let annotation = facts.annotation_for_document(&document);

        Self {
            document: Box::new(document),
            annotation,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EntryData {
    Empty,
    Found(AnnotatedEntryDocument),
    NotFound { entry: String },
    Ambiguous(Vec<AnnotatedEntryDocument>),
    Error(String),
}

impl EntryData {
    pub fn document(&self) -> Option<&SearchDocument> {
        match self {
            Self::Found(entry) => Some(&entry.document),
            Self::Empty | Self::NotFound { .. } | Self::Ambiguous(_) | Self::Error(_) => None,
        }
    }
}
