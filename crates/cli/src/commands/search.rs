use std::sync::Arc;

use anyhow::Result;

use nixsearch_service::{SearchRequest, SearchService};

use crate::args::SearchArgs;
use crate::output::print_search_hit;

use super::load_required_config;

pub(super) fn search(args: SearchArgs) -> Result<()> {
    let config = Arc::new(load_required_config(&args.config)?);
    let service = SearchService::open_current(Arc::clone(&config))?;

    let hits = service.search_current(SearchRequest {
        query: args.query,
        sources: args.source.into_iter().collect(),
        ref_id: args.ref_id,
        ref_set: args.ref_set,
        limit: args.limit,
        ..Default::default()
    })?;

    for hit in hits.hits {
        print_search_hit(config.as_ref(), hit);
    }

    Ok(())
}
