use std::collections::HashMap;

use nixsearch_core::document::{DocumentKind, SearchDocument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHitAnnotation {
    pub ambiguous_entry_url: bool,
    pub unique_within_kind: bool,
}

#[derive(Debug, Default)]
pub struct EntryAnnotationIndex {
    entries: HashMap<EntryAnnotationKey, u8>,
}

impl EntryAnnotationIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&mut self, document: &SearchDocument) {
        let kind = document.kind();

        if !kind.is_supported_indexed_entry() {
            return;
        }

        let common = document.common();
        let key = EntryAnnotationKey {
            source: common.source.clone(),
            ref_id: common.ref_id.clone(),
            name: common.name.clone(),
            kind: kind.clone(),
        };

        let count = self.entries.entry(key).or_default();
        *count = capped_increment(*count);
    }

    pub fn annotation_for(&self, document: &SearchDocument) -> SearchHitAnnotation {
        let common = document.common();
        let count = self.entries.get(&EntryAnnotationKey {
            source: common.source.clone(),
            ref_id: common.ref_id.clone(),
            name: common.name.clone(),
            kind: document.kind().clone(),
        });

        SearchHitAnnotation {
            ambiguous_entry_url: false,
            unique_within_kind: count.map(|count| *count == 1).unwrap_or(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EntryAnnotationKey {
    source: String,
    ref_id: String,
    name: String,
    kind: DocumentKind,
}

fn capped_increment(value: u8) -> u8 {
    value.saturating_add(1).min(2)
}
