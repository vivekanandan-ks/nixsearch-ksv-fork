use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use time::OffsetDateTime;

use nix_search_config::AppConfig;
use nix_search_index::{IndexGenerationManifest, IndexStore};
use nix_search_ops::generate;
use nix_search_ops::lock;
use nix_search_ops::targets::all_targets;

const RECONCILE_INTERVAL: Duration = Duration::from_secs(5 * 60);
const MANIFEST_ERROR_RETRY: Duration = Duration::from_secs(60);
const MIN_LOCK_BUSY_RETRY: Duration = Duration::from_secs(60);
const MAX_LOCK_BUSY_RETRY: Duration = Duration::from_secs(10 * 60);
const MIN_FAILURE_RETRY: Duration = Duration::from_secs(60);
const MAX_FAILURE_RETRY: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone)]
pub(crate) struct PublishedGeneration {
    pub path: PathBuf,
    pub manifest: IndexGenerationManifest,
}

#[derive(Debug, Clone)]
pub(crate) enum CurrentGeneration {
    Missing,
    Found(PublishedGeneration),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaintenanceOutcome {
    Completed,
    LockBusy,
    Failed,
}

pub(crate) fn spawn(config: Arc<AppConfig>, index_path: Arc<RwLock<PathBuf>>) {
    let interval = config
        .server
        .schedule
        .parse_interval()
        .expect("schedule interval already validated");

    tokio::spawn(async move {
        run_loop(config, index_path, interval).await;
    });
}

async fn run_loop(config: Arc<AppConfig>, index_path: Arc<RwLock<PathBuf>>, interval: Duration) {
    let index_store = IndexStore::new(&config.data.index_dir);
    let regeneration_enabled = config.server.schedule.enabled && has_configured_targets(&config);

    loop {
        let generation = match read_current_generation(&index_store) {
            Ok(CurrentGeneration::Found(generation)) => generation,
            Ok(CurrentGeneration::Missing) => {
                tracing::warn!("current index disappeared during maintenance loop");
                tokio::time::sleep(MANIFEST_ERROR_RETRY.min(RECONCILE_INTERVAL)).await;
                continue;
            }
            Err(error) => {
                tracing::warn!("failed to read current index generation: {error:#}");
                tokio::time::sleep(MANIFEST_ERROR_RETRY.min(RECONCILE_INTERVAL)).await;
                continue;
            }
        };

        reconcile_served_generation(&index_path, &generation.path);

        if !regeneration_enabled {
            tokio::time::sleep(RECONCILE_INTERVAL).await;
            continue;
        }

        let Some(next_due) = next_due(generation.manifest.generated_at, interval) else {
            tracing::error!("failed to compute next scheduled regeneration time");
            tokio::time::sleep(MANIFEST_ERROR_RETRY.min(RECONCILE_INTERVAL)).await;
            continue;
        };

        let now = OffsetDateTime::now_utc();

        if now < next_due {
            tokio::time::sleep(duration_until(next_due, now).min(RECONCILE_INTERVAL)).await;
            continue;
        }

        match run_scheduled_regeneration(&config).await {
            MaintenanceOutcome::Completed => {
                continue;
            }
            MaintenanceOutcome::LockBusy => {
                tracing::info!("scheduled regeneration skipped; maintenance lock is held");
                let delay = clamp_duration(interval, MIN_LOCK_BUSY_RETRY, MAX_LOCK_BUSY_RETRY)
                    .min(RECONCILE_INTERVAL);
                tokio::time::sleep(delay).await;
            }
            MaintenanceOutcome::Failed => {
                let delay = clamp_duration(interval, MIN_FAILURE_RETRY, MAX_FAILURE_RETRY)
                    .min(RECONCILE_INTERVAL);
                tokio::time::sleep(delay).await;
            }
        }
    }
}

async fn run_scheduled_regeneration(config: &AppConfig) -> MaintenanceOutcome {
    let update_lock = match lock::try_acquire_update_lock(&config.data.index_dir) {
        Ok(Some(update_lock)) => update_lock,
        Ok(None) => return MaintenanceOutcome::LockBusy,
        Err(error) => {
            tracing::error!("failed to acquire maintenance lock: {error:#}");
            return MaintenanceOutcome::Failed;
        }
    };

    let start = Instant::now();

    let result = generate::regenerate_all(config).await;

    drop(update_lock);

    match result {
        Ok(_) => {
            tracing::info!(
                elapsed_secs = start.elapsed().as_secs_f64(),
                "scheduled regeneration completed"
            );
            MaintenanceOutcome::Completed
        }
        Err(error) => {
            tracing::error!("scheduled regeneration failed: {error:#}");
            MaintenanceOutcome::Failed
        }
    }
}

pub(crate) fn read_current_generation(index_store: &IndexStore) -> Result<CurrentGeneration> {
    let current_file = index_store.current_file();

    let raw = match fs::read_to_string(&current_file) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(CurrentGeneration::Missing),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "failed to read current index file {}",
                    current_file.display()
                )
            });
        }
    };

    let path = PathBuf::from(raw.trim());

    if path.as_os_str().is_empty() {
        bail!("current index file is empty")
    }

    let manifest = index_store.read_manifest(&path)?;

    Ok(CurrentGeneration::Found(PublishedGeneration {
        path,
        manifest,
    }))
}

pub(crate) fn reconcile_served_generation(
    index_path: &Arc<RwLock<PathBuf>>,
    published_path: &Path,
) {
    let mut served_path = index_path.write().expect("index path lock poisoned");

    if served_path.as_path() != published_path {
        tracing::info!(
            old = %served_path.display(),
                new = %published_path.display(),
                "detected published index generation change"
        );

        *served_path = published_path.to_path_buf();
    }
}

pub(crate) fn has_configured_targets(config: &AppConfig) -> bool {
    !all_targets(config).is_empty()
}

pub(crate) fn next_due(generated_at: OffsetDateTime, interval: Duration) -> Option<OffsetDateTime> {
    let interval = time::Duration::try_from(interval).ok()?;
    generated_at.checked_add(interval)
}

pub(crate) fn duration_until(next: OffsetDateTime, now: OffsetDateTime) -> Duration {
    if next <= now {
        return Duration::ZERO;
    }

    (next - now).try_into().unwrap_or(Duration::ZERO)
}

pub(crate) fn clamp_duration(value: Duration, min: Duration, max: Duration) -> Duration {
    value.max(min).min(max)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, RwLock};

    use time::Duration as TimeDuration;

    use super::{clamp_duration, duration_until, next_due, reconcile_served_generation};

    #[test]
    fn next_due_adds_interval() {
        let generated_at = time::OffsetDateTime::UNIX_EPOCH;
        let next = next_due(generated_at, std::time::Duration::from_secs(60)).unwrap();

        assert_eq!(next, generated_at + TimeDuration::seconds(60));
    }

    #[test]
    fn duration_until_floors_past_times_to_zero() {
        let now = time::OffsetDateTime::UNIX_EPOCH + TimeDuration::seconds(60);
        let next = time::OffsetDateTime::UNIX_EPOCH;

        assert_eq!(duration_until(next, now), std::time::Duration::ZERO);
    }

    #[test]
    fn clamp_duration_applies_bounds() {
        assert_eq!(
            clamp_duration(
                std::time::Duration::from_secs(5),
                std::time::Duration::from_secs(10),
                std::time::Duration::from_secs(20)
            ),
            std::time::Duration::from_secs(10)
        );

        assert_eq!(
            clamp_duration(
                std::time::Duration::from_secs(30),
                std::time::Duration::from_secs(10),
                std::time::Duration::from_secs(20)
            ),
            std::time::Duration::from_secs(20)
        );
    }

    #[test]
    fn reconcile_updates_changed_path() {
        let path = Arc::new(RwLock::new(std::path::PathBuf::from("/old")));

        reconcile_served_generation(&path, std::path::Path::new("/new"));

        assert_eq!(*path.read().unwrap(), std::path::PathBuf::from("/new"));
    }

    #[test]
    fn reconcile_keeps_current_path() {
        let path = Arc::new(RwLock::new(std::path::PathBuf::from("/current")));

        reconcile_served_generation(&path, std::path::Path::new("/current"));

        assert_eq!(*path.read().unwrap(), std::path::PathBuf::from("/current"));
    }
}
