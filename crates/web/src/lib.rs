use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use anyhow::{Context, Result, bail};
use axum::Router;
use axum::routing::get;
use tower_http::trace::TraceLayer;

use nix_search_config::AppConfig;
use nix_search_index::IndexStore;
use nix_search_ops::generate;
use nix_search_ops::lock;

mod handlers;
mod maintenance;
mod request;
mod scripts;
mod templates;
mod urls;

const DEFAULT_LIMIT: usize = 50;
const RECONCILE_EVENTS_URL: &str = "/-/state/events";
const MORE_RESULTS_URL: &str = "/-/more";

#[derive(Debug, Clone)]
struct AppState {
    config: Arc<AppConfig>,
    index_path: Arc<RwLock<PathBuf>>,
}

pub async fn serve(config: AppConfig) -> Result<()> {
    let generation = ensure_current_generation(&config).await?;

    let addr: SocketAddr =
        config.server.listen.parse().with_context(|| {
            format!("failed to parse listen address {:?}", config.server.listen)
        })?;

    log_startup_maintenance_state(&config, &generation);

    let config = Arc::new(config);
    let index_path = Arc::new(RwLock::new(generation.path));

    maintenance::spawn(Arc::clone(&config), Arc::clone(&index_path));

    let state = AppState { config, index_path };

    let app = Router::new()
        .route("/-/health", get(handlers::health))
        .route(RECONCILE_EVENTS_URL, get(handlers::state_events))
        .route(MORE_RESULTS_URL, get(handlers::more_results))
        .route("/", get(handlers::root_page))
        .route("/{source}", get(handlers::source_page))
        .route("/{source}/{*entry}", get(handlers::entry_page))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind {addr}"))?;

    tracing::info!("serving nix-search web UI at http://{addr}");

    axum::serve(listener, app)
        .await
        .context("web server failed")?;

    Ok(())
}

async fn ensure_current_generation(config: &AppConfig) -> Result<maintenance::PublishedGeneration> {
    let index_store = IndexStore::new(&config.data.index_dir);

    match maintenance::read_current_generation(&index_store)? {
        maintenance::CurrentGeneration::Found(generation) => return Ok(generation),
        maintenance::CurrentGeneration::Missing => {}
    }

    if !config.server.bootstrap {
        bail!(
            "failed to locate current index in {}; run `nix-search update` first",
            config.data.index_dir.display()
        );
    }

    if !maintenance::has_configured_targets(config) {
        bail!("cannot bootstrap missing index: no configured refs to index");
    }

    tracing::info!(
        index_dir = %config.data.index_dir.display(),
        "no current index found; bootstrap enabled, building initial index"
    );

    let index_dir = config.data.index_dir.clone();
    let _lock = tokio::task::spawn_blocking(move || lock::acquire_update_lock(&index_dir))
        .await
        .context("failed to join maintenance lock task")??;

    match maintenance::read_current_generation(&index_store)? {
        maintenance::CurrentGeneration::Found(generation) => {
            tracing::info!("current index was created by another process while waiting for lock");
            return Ok(generation);
        }
        maintenance::CurrentGeneration::Missing => {}
    }

    generate::regenerate_all(config)
        .await
        .context("failed to bootstrap missing index")?;

    match maintenance::read_current_generation(&index_store)? {
        maintenance::CurrentGeneration::Found(generation) => Ok(generation),
        maintenance::CurrentGeneration::Missing => {
            bail!("bootstrap completed without publishing a current index")
        }
    }
}

fn log_startup_maintenance_state(
    config: &AppConfig,
    generation: &maintenance::PublishedGeneration,
) {
    tracing::info!("background index reconciliation enabled");
    tracing::info!(
        enabled = config.server.bootstrap,
        "server bootstrap setting"
    );

    if config.server.schedule.enabled {
        if maintenance::has_configured_targets(config) {
            let interval = config
                .server
                .schedule
                .parse_interval()
                .expect("schedule interval already validated");

            if let Some(next_due) =
                maintenance::next_due(generation.manifest.generated_at, interval)
            {
                tracing::info!(
                    interval = %config.server.schedule.interval,
                    generated_at = %generation.manifest.generated_at,
                    next_due = %next_due,
                    "scheduled regeneration enabled"
                );
            } else {
                tracing::error!(
                    interval = %config.server.schedule.interval,
                    generated_at = %generation.manifest.generated_at,
                    "scheduled regeneration enabled but next due time could not be computed"
                );
            }
        } else {
            tracing::warn!(
                "scheduled regeneration enabled but no refs are configured; reconciliation will continue"
            );
        }
    } else {
        tracing::info!("scheduled regeneration disabled");
    }
}
