use std::collections::BTreeSet;

use anyhow::{Result, bail};

use nixsearch_index::store::IndexStore;
use nixsearch_ops::generate::build_and_publish_generation;
use nixsearch_ops::lock::acquire_update_lock;
use nixsearch_ops::produce::artifact_store_from_config;
use nixsearch_ops::targets::{TargetKey, current_manifest_targets, select_targets};

use crate::args::SelectionArgs;

use super::load_required_config;

pub(super) async fn update(args: SelectionArgs) -> Result<()> {
    let config = load_required_config(&args.config)?;
    let _lock = acquire_update_lock(&config.data.index_dir)?;

    let store = artifact_store_from_config(&config)?;
    let selected_targets = select_targets(&config, args.source.as_deref(), args.ref_id.as_deref())?;

    if selected_targets.is_empty() {
        bail!("no refs matched selection");
    }

    let index_store = IndexStore::new(&config.data.index_dir);

    let mut included_targets = current_manifest_targets(&config, &index_store)?;
    let selected_keys: BTreeSet<TargetKey> = selected_targets.iter().map(TargetKey::from).collect();

    for target in selected_targets {
        included_targets.insert(TargetKey::from(&target), target);
    }

    if included_targets.is_empty() {
        bail!("no refs available to index");
    }

    build_and_publish_generation(
        &index_store,
        &store,
        included_targets.into_values().collect(),
        &selected_keys,
    )
    .await?;

    Ok(())
}
