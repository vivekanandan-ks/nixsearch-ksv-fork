use std::fs::File;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;

use nix_search_config::AppConfig;
use nix_search_core::IngestContext;
use nix_search_index::SearchIndex;
use nix_search_ingest::parse_options_json;

#[derive(Debug, Parser)]
#[command(name = "nix-search")]
#[command(about = "Search Nix packages and options")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    IndexOptions {
        #[arg(long)]
        options_json: PathBuf,

        #[arg(long)]
        index_dir: PathBuf,

        #[arg(long)]
        project: String,

        #[arg(long)]
        dataset: String,

        #[arg(long)]
        ref_id: String,

        #[arg(long)]
        revision: Option<String>,
    },

    Search {
        query: String,

        #[arg(long)]
        index_dir: PathBuf,

        #[arg(long, default_value_t = 20)]
        limit: usize,
    },

    CheckConfig {
        #[arg(long)]
        config: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    match args.command {
        Command::IndexOptions {
            options_json,
            index_dir,
            project,
            dataset,
            ref_id,
            revision,
        } => index_options(options_json, index_dir, project, dataset, ref_id, revision),

        Command::Search {
            query,
            index_dir,
            limit,
        } => search(index_dir, &query, limit),

        Command::CheckConfig { config } => check_config(config),
    }
}

fn index_options(
    options_json: PathBuf,
    index_dir: PathBuf,
    project: String,
    dataset: String,
    ref_id: String,
    revision: Option<String>,
) -> Result<()> {
    let file = File::open(&options_json)
        .with_context(|| format!("failed to open {}", options_json.display()))?;

    let context = IngestContext {
        project,
        dataset,
        ref_id,
        revision,
        repo: None,
    };

    let documents = parse_options_json(file, &context)
        .with_context(|| format!("failed to ingest {}", options_json.display()))?;

    info!(count = documents.len(), "parsed options");

    let index = SearchIndex::create_or_replace(&index_dir)?;
    let mut writer = index.writer()?;

    for document in &documents {
        writer.add_document(document)?;
    }

    writer.commit()?;

    info!(
        count = documents.len(),
        index_dir = %index_dir.display(),
        "indexed options"
    );

    Ok(())
}

fn search(index_dir: PathBuf, query: &str, limit: usize) -> Result<()> {
    let index = SearchIndex::open(&index_dir)?;
    let hits = index.search(query, limit)?;

    for hit in hits {
        let common = hit.document.common();

        println!(
            "{score:.3}  {kind}  {name}",
            score = hit.score,
            kind = common.kind.as_str(),
            name = common.name,
        );

        let nix_search_core::SearchDocument::Option(option) = hit.document;

        if let Some(description) = option.description {
            let summary = description.lines().next().unwrap_or("").trim();

            if !summary.is_empty() {
                println!("       {summary}");
            }
        }
    }

    Ok(())
}

fn check_config(config: Option<PathBuf>) -> Result<()> {
    let loaded = AppConfig::load(config.as_deref()).context("configuration check failed")?;

    println!("configuration is valid");
    println!("artifact_url = {}", loaded.data.artifact_url);
    println!("index_dir = {}", loaded.data.index_dir.display());
    println!("listen = {}", loaded.server.listen);
    println!("projects = {}", loaded.projects.len());

    for (project_id, project) in &loaded.projects {
        let name = project.name.as_deref().unwrap_or(project_id);
        println!("  project {project_id}: {name}");

        for dataset in &project.datasets {
            let name = dataset.name.as_deref().unwrap_or(&dataset.id);
            println!("    dataset {}: {} ({:?})", dataset.id, name, dataset.kind);

            for ref_config in &dataset.refs {
                println!(
                    "      ref {}: producer={:?}",
                    ref_config.id,
                    ref_config.producer.kind()
                );
            }
        }
    }

    Ok(())
}
