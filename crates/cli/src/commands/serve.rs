use anyhow::{Context, Result};

use nixsearch_config::app::AppConfig;

use crate::args::ConfigArgs;

pub(super) async fn serve(args: ConfigArgs) -> Result<()> {
    let config = AppConfig::load(args.config.as_deref()).context("failed to load config")?;

    nixsearch_web::serve(config).await
}
