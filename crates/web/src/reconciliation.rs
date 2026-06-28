use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use nixsearch_service::{ReconcileReport, ServedGenerationSnapshot};

use crate::AppState;

const REQUEST_RECONCILE_ATTEMPTS: usize = 3;

pub(crate) struct RequestGeneration {
    snapshot: ServedGenerationSnapshot,
}

impl RequestGeneration {
    pub(crate) fn reconcile(state: &AppState) -> Self {
        for _ in 0..REQUEST_RECONCILE_ATTEMPTS {
            let report = match state.search.reconcile_current_generation() {
                Ok(report) => report,
                Err(error) => {
                    tracing::warn!(
                        "failed to reconcile published index generation during request; continuing to serve previous generation: {error:#}"
                    );
                    return Self::from_current_snapshot(state);
                }
            };

            if matches!(report, ReconcileReport::Superseded) {
                continue;
            }

            return Self::from_current_snapshot(state);
        }

        tracing::warn!(
            attempts = REQUEST_RECONCILE_ATTEMPTS,
            "published index generation changed repeatedly during request reconciliation; continuing with current snapshot"
        );

        Self::from_current_snapshot(state)
    }

    fn from_current_snapshot(state: &AppState) -> Self {
        Self {
            snapshot: state.search.snapshot(),
        }
    }

    pub(crate) fn snapshot(&self) -> &ServedGenerationSnapshot {
        &self.snapshot
    }

    pub(crate) fn generation_id(&self) -> &str {
        self.snapshot.manifest().generation_id.as_str()
    }

    pub(crate) fn client_status(
        &self,
        client_generation_id: Option<&str>,
    ) -> ClientGenerationStatus {
        if client_generation_id == Some(self.generation_id()) {
            ClientGenerationStatus::Current
        } else {
            ClientGenerationStatus::Changed
        }
    }

    pub(crate) fn stale_json_response(&self) -> Response {
        (
            StatusCode::CONFLICT,
            Json(stale_generation_payload(self.generation_id())),
        )
            .into_response()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientGenerationStatus {
    Current,
    Changed,
}

impl ClientGenerationStatus {
    pub(crate) fn changed(self) -> bool {
        matches!(self, Self::Changed)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StaleGenerationPayload<'a> {
    error: &'static str,
    reload: bool,
    generation_id: &'a str,
}

fn stale_generation_payload(generation_id: &str) -> StaleGenerationPayload<'_> {
    StaleGenerationPayload {
        error: "stale_generation",
        reload: true,
        generation_id,
    }
}

#[cfg(test)]
mod tests {
    use super::{ClientGenerationStatus, stale_generation_payload};

    #[test]
    fn client_generation_status_reports_changed() {
        assert!(!ClientGenerationStatus::Current.changed());
        assert!(ClientGenerationStatus::Changed.changed());
    }

    #[test]
    fn stale_generation_payload_uses_existing_json_contract() {
        let json = serde_json::to_string(&stale_generation_payload("sha256:abc")).unwrap();

        assert_eq!(
            json,
            r#"{"error":"stale_generation","reload":true,"generationId":"sha256:abc"}"#
        );
    }
}
