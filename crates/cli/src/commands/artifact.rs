use anyhow::{Context, Result, bail};

use nixsearch_ops::lock::acquire_update_lock;
use nixsearch_ops::produce::{
    artifact_store_from_config, latest_artifact_ref_for_target, produce_target,
};
use nixsearch_ops::targets::select_targets;

use crate::args::SelectionArgs;
use crate::output::{print_artifact_metadata, print_produced_artifact};

use super::load_required_config;

pub(super) async fn produce(args: SelectionArgs) -> Result<()> {
    let config = load_required_config(&args.config)?;
    let _lock = acquire_update_lock(&config.data.index_dir)?;

    let store = artifact_store_from_config(&config)?;
    let targets = select_targets(&config, args.source.as_deref(), args.ref_id.as_deref())?;

    if targets.is_empty() {
        bail!("no refs matched selection");
    }

    for target in targets {
        let produced = produce_target(&store, &target).await?;
        print_produced_artifact(&produced);
    }

    Ok(())
}

pub(super) async fn inspect(args: SelectionArgs) -> Result<()> {
    let config = load_required_config(&args.config)?;
    let store = artifact_store_from_config(&config)?;
    let targets = select_targets(&config, args.source.as_deref(), args.ref_id.as_deref())?;

    if targets.is_empty() {
        bail!("no refs matched selection");
    }

    for target in targets {
        let artifact_ref = latest_artifact_ref_for_target(&target);
        let metadata = store.get_metadata(&artifact_ref).await.with_context(|| {
            format!(
                "failed to read metadata for {}/{}",
                target.source_id, target.ref_config.id
            )
        })?;

        print_artifact_metadata(&metadata);
    }

    Ok(())
}
