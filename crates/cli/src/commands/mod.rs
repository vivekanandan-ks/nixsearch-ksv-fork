use std::path::Path;

use anyhow::{Context, Result};

use nixsearch_config::app::AppConfig;

use crate::args::{ArtifactCommand, Command, IndexCommand};

mod artifact;
mod config;
mod index;
mod search;
mod serve;
mod update;

pub(crate) async fn run(command: Command) -> Result<()> {
    match command {
        Command::CheckConfig(args) => config::check_config(args),
        Command::Update(args) => update::update(args).await,
        Command::Search(args) => search::search(args),
        Command::Serve(args) => serve::serve(args).await,
        Command::Artifact { command } => match command {
            ArtifactCommand::Produce(args) => artifact::produce(args).await,
            ArtifactCommand::Inspect(args) => artifact::inspect(args).await,
        },
        Command::Index { command } => match command {
            IndexCommand::Rebuild(args) => index::rebuild(args).await,
            IndexCommand::Inspect(args) => index::inspect(args),
        },
    }
}

fn load_required_config(path: &Path) -> Result<AppConfig> {
    AppConfig::load(Some(path)).with_context(|| format!("failed to load {}", path.display()))
}
