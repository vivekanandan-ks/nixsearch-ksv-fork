use std::collections::HashMap;

use nixsearch_core::document::{DocumentKind, SearchDocument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHitAnnotation {
    pub current_hit_kind: DocumentKind,
    pub ambiguous_entry_url: bool,
    pub unique_within_kind: bool,
}

#[derive(Debug, Default)]
pub struct EntryAnnotationIndex {
    entries: HashMap<EntryAnnotationKey, EntryKindCounts>,
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
        };

        self.entries.entry(key).or_default().observe(kind);
    }

    pub fn annotation_for(&self, document: &SearchDocument) -> SearchHitAnnotation {
        let common = document.common();
        let counts = self.entries.get(&EntryAnnotationKey {
            source: common.source.clone(),
            ref_id: common.ref_id.clone(),
            name: common.name.clone(),
        });

        SearchHitAnnotation {
            current_hit_kind: document.kind().clone(),
            ambiguous_entry_url: counts
                .map(|counts| counts.package_count > 0 && counts.option_count > 0)
                .unwrap_or(false),
            unique_within_kind: counts
                .map(|counts| counts.count_for_kind(document.kind()) == 1)
                .unwrap_or(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct EntryAnnotationKey {
    source: String,
    ref_id: String,
    name: String,
}

#[derive(Debug, Clone, Default)]
struct EntryKindCounts {
    package_count: u8,
    option_count: u8,
}

impl EntryKindCounts {
    fn observe(&mut self, kind: &DocumentKind) {
        match kind {
            DocumentKind::Package => self.package_count = capped_increment(self.package_count),
            DocumentKind::Option => self.option_count = capped_increment(self.option_count),
            DocumentKind::App | DocumentKind::Service => {}
        }
    }

    fn count_for_kind(&self, kind: &DocumentKind) -> u8 {
        match kind {
            DocumentKind::Package => self.package_count,
            DocumentKind::Option => self.option_count,
            DocumentKind::App | DocumentKind::Service => 0,
        }
    }
}

fn capped_increment(value: u8) -> u8 {
    value.saturating_add(1).min(2)
}
