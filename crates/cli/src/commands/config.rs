use anyhow::{Context, Result};

use nixsearch_config::app::AppConfig;

use crate::args::ConfigArgs;
use crate::output::print_source;

pub(super) fn check_config(args: ConfigArgs) -> Result<()> {
    let config = AppConfig::load(args.config.as_deref()).context("configuration check failed")?;

    println!("configuration is valid");
    println!("artifact_url = {}", config.data.artifact_url);
    println!("index_dir = {}", config.data.index_dir);
    println!("listen = {}", config.server.listen);
    println!("bootstrap = {}", config.server.bootstrap);
    println!("schedule.enabled = {}", config.server.schedule.enabled);
    println!("schedule.interval = {}", config.server.schedule.interval);
    println!("sources = {}", config.sources.len());

    for (source_id, source) in &config.sources {
        print_source(source_id, source);
    }

    Ok(())
}
