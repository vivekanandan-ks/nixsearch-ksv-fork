use nixsearch_core::document::SearchDocument;
use nixsearch_index::annotation::SearchHitAnnotation;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AnnotatedEntryDocument {
    pub document: Box<SearchDocument>,
    pub annotation: SearchHitAnnotation,
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
