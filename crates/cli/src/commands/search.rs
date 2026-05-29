use anyhow::{Context, Result};

use nixsearch_index::search::{SearchIndex, SearchOptions, SearchScope};
use nixsearch_index::store::IndexStore;

use crate::args::SearchArgs;
use crate::output::print_search_hit;

use super::load_required_config;

pub(super) fn search(args: SearchArgs) -> Result<()> {
    let config = load_required_config(&args.config)?;

    let index_store = IndexStore::new(&config.data.index_dir);
    let current_path = index_store.current_path()?;

    let index = SearchIndex::open(&current_path)
        .with_context(|| format!("failed to open current index {}", current_path.as_str()))?;

    let scopes = config
        .resolve_search_scopes(
            args.source.as_deref(),
            args.ref_id.as_deref(),
            args.ref_set.as_deref(),
        )
        .context("failed to resolve search scope")?
        .into_iter()
        .map(|scope| SearchScope {
            source: scope.source,
            ref_id: scope.ref_id,
        })
        .collect();

    let hits = index.search(SearchOptions {
        query: args.query,
        limit: args.limit,
        scopes,
        ..Default::default()
    })?;

    for hit in hits.hits {
        print_search_hit(&config, hit);
    }

    Ok(())
}
