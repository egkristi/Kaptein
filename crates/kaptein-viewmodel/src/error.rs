//! The unified error enum for the view-model.
//!
//! Raw `kube::Error` and subprocess failures are mapped here into redaction-aware,
//! user-facing variants (see `ROADMAP.md` cross-cutting commitments).

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Error {
    #[error("query failed: {0}")]
    Query(String),

    #[error("the store is at revision {0} and cannot serve a subscription from {1}")]
    StaleSubscription(u64, u64),

    #[error("resource not found: {0}")]
    NotFound(String),

    #[error("operation forbidden: {verb} on {resource}")]
    Forbidden { verb: String, resource: String },

    #[error("external tool failed: {0}")]
    ExternalTool(String),

    #[error("internal error: {0}")]
    Internal(String),
}
