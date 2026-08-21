//! The raw, non-user-facing error type for `kaptein-core`.
//!
//! `kaptein-core` cannot depend on `kaptein-viewmodel` (layer rule), so it owns its own
//! error type. The view-model maps this into its own redaction-aware, user-facing
//! `Error` via a `From` impl (see `kaptein-viewmodel::error`). This split is deliberate:
//! the core reports *what failed* (network, auth, watch, discovery); the view-model
//! decides *how to say it* to a user without leaking secrets.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Network(String),

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("watch interrupted: {0}")]
    WatchInterrupted(String),

    #[error("discovery failed: {0}")]
    Discovery(String),

    #[error("internal error: {0}")]
    Internal(String),
}
