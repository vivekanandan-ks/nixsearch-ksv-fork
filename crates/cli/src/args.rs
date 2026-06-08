use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "nixsearch")]
#[command(about = "Search Nix packages and options")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Validate and display configuration.
    CheckConfig(ConfigArgs),

    /// Produce artifacts and build indexes for configured refs.
    Update(SelectionArgs),

    /// Search configured indexes.
    Search(SearchArgs),

    /// Serve the web UI.
    Serve(ConfigArgs),

    /// Debug artifact production and metadata.
    Artifact {
        #[command(subcommand)]
        command: ArtifactCommand,
    },

    /// Debug index building and inspection.
    Index {
        #[command(subcommand)]
        command: IndexCommand,
    },

    /// Run maintenance tasks.
    Maintenance {
        #[command(subcommand)]
        command: MaintenanceCommand,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum ArtifactCommand {
    /// Produce raw artifacts only, without indexing.
    Produce(SelectionArgs),

    /// Inspect artifact metadata.
    Inspect(SelectionArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum IndexCommand {
    /// Rebuild the current index from exactly the selected refs.
    Rebuild(SelectionArgs),

    /// Inspect the current published index generation.
    Inspect(ConfigArgs),
}

#[derive(Debug, Subcommand)]
pub(crate) enum MaintenanceCommand {
    /// Prune old indexes and run configured store cleanup.
    Cleanup(ConfigArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ConfigArgs {
    /// Path to config file. If omitted, only defaults and env vars are loaded.
    #[arg(long, env = "NIXSEARCH_CONFIG")]
    pub(crate) config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct SelectionArgs {
    /// Path to config file.
    #[arg(long, env = "NIXSEARCH_CONFIG")]
    pub(crate) config: PathBuf,

    /// Restrict to one source.
    #[arg(long)]
    pub(crate) source: Option<String>,

    /// Restrict to one ref.
    #[arg(long = "ref")]
    pub(crate) ref_id: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct SearchArgs {
    /// Search query.
    pub(crate) query: String,

    /// Path to config file.
    #[arg(long, env = "NIXSEARCH_CONFIG")]
    pub(crate) config: PathBuf,

    /// Restrict to one source.
    #[arg(long)]
    pub(crate) source: Option<String>,

    /// Restrict to one ref. Requires --source.
    #[arg(long = "ref")]
    pub(crate) ref_id: Option<String>,

    /// Restrict All search to one configured ref set.
    #[arg(long = "ref-set")]
    pub(crate) ref_set: Option<String>,

    /// Maximum number of results.
    #[arg(long, default_value_t = 20)]
    pub(crate) limit: usize,
}
