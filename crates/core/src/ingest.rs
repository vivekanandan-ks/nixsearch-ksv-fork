use serde::{Deserialize, Serialize};

use crate::source_link::Repo;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestContext {
    pub source: String,
    pub ref_id: String,
    pub revision: Option<String>,
    pub repo: Option<Repo>,
}
