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

/// Map the raw `kaptein-core` error into a user-facing, redaction-aware error.
///
/// The core reports *what failed*; the view-model decides *how to say it* without
/// leaking secrets. This `From` impl is the single mapping point (see ADR-0009 /
/// `architecture.md`).
impl From<kaptein_core::Error> for Error {
    fn from(err: kaptein_core::Error) -> Self {
        match err {
            kaptein_core::Error::Auth(msg) => Error::Forbidden {
                verb: "authenticate".into(),
                resource: msg,
            },
            kaptein_core::Error::Network(msg)
            | kaptein_core::Error::WatchInterrupted(msg)
            | kaptein_core::Error::Discovery(msg)
            | kaptein_core::Error::Internal(msg) => Error::Internal(msg),
        }
    }
}
