use anyhow::Result;
use camino::Utf8Path;

use nixsearch_index::search::SearchIndex;
use nixsearch_index::store::IndexStore;

use super::documents::SpooledDocumentSet;

pub(crate) fn write_tantivy_index(
    index_store: &IndexStore,
    generation_path: &Utf8Path,
    documents: &SpooledDocumentSet,
) -> Result<()> {
    let index_path = index_store.index_path(generation_path);
    let index = SearchIndex::create_or_replace(&index_path)?;
    let mut writer = index.writer()?;

    for document in documents.spool.reader()? {
        let document = document?;
        writer.add_document(&document, &documents.annotations.annotation_for(&document))?;
    }

    writer.commit()?;

    Ok(())
}
