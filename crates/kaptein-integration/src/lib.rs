//! Kaptein integration layer — binds `kaptein-core` to native frontends.
//!
//! Per `docs/architecture.md`, the *integration layer* is the native frontend or binary
//! that owns both `kaptein-core` and (where applicable) `kaptein-viewmodel`. It maps raw
//! `kaptein-core::Error` values into user-facing messages without leaking secrets.
//!
//! The TUI reaches `kaptein-core` through this crate, keeping the layer dependency rule
//! satisfied: `frontend-tui` → `kaptein-integration` → `kaptein-core`, with no frontend
//! depending on `kaptein-core` directly (see AGENTS.md / ADR-0005).

#![forbid(unsafe_code)]

/// The user-facing error type for native frontends: a `kaptein-core::Error` whose
/// display is safe to show to a user (no secrets, no raw stack traces).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct IntegrationError(#[from] kaptein_core::Error);

/// Re-export the entire core data plane so frontends reach `kaptein-core` through this
/// crate rather than as a direct dependency. The integration layer owns `kaptein-core`;
/// frontends own only geometry.
pub use kaptein_core;

/// Build a Kubernetes client for the default context.
pub async fn client() -> Result<kube::Client, IntegrationError> {
    Ok(kaptein_core::discovery::client().await?)
}

/// Build a Kubernetes client for a specific named context.
pub async fn client_for_context(context: Option<&str>) -> Result<kube::Client, IntegrationError> {
    Ok(kaptein_core::discovery::client_for_context(context).await?)
}
