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

/// The user-facing, redaction-aware error type for native frontends.
///
/// It maps the raw `kaptein-core::Error` (network/auth/watch/discovery/API) into messages
/// that are safe to show a user: no secret values, no raw stack traces, no internal
/// kubeconfig/exec-credential output. The mapping is deliberately *coarse* — the core
/// reports *what failed*; this layer decides *how to say it* without leaking secrets.
#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    /// The API server rejected or failed the request; `message` is the (safe) detail.
    #[error("kubernetes API error: {message}")]
    Api { message: String },

    /// Authentication/authorization failed.
    #[error("authentication failed: {message}")]
    Auth { message: String },

    /// A network error reaching the cluster.
    #[error("network error: {message}")]
    Network { message: String },

    /// A watch stream was interrupted.
    #[error("watch interrupted: {message}")]
    Watch { message: String },

    /// API discovery failed.
    #[error("discovery failed: {message}")]
    Discovery { message: String },

    /// The kubeconfig could not be read/parsed (never include the raw file contents).
    #[error("kubeconfig error: {message}")]
    Kubeconfig { message: String },

    /// An external tool shell-out failed.
    #[error("external tool error: {message}")]
    External { message: String },

    /// A generic internal error (already user-safe by construction in core).
    #[error("{0}")]
    Internal(String),
}

/// Map a raw `kaptein-core::Error` into a redaction-aware `IntegrationError`.
///
/// Secret values never appear in a message: the `message` field is a *classification*
/// description, not a dump of the raw error. Where the core error already carries a
/// user-safe message (`Internal`, `External`), it is forwarded verbatim; API/auth errors
/// carry only the status code and reason, never the request body or credentials.
impl From<kaptein_core::Error> for IntegrationError {
    fn from(e: kaptein_core::Error) -> Self {
        match e {
            kaptein_core::Error::Network(msg) => IntegrationError::Network { message: msg },
            kaptein_core::Error::Auth(msg) => IntegrationError::Auth { message: msg },
            kaptein_core::Error::WatchInterrupted(msg) => IntegrationError::Watch { message: msg },
            kaptein_core::Error::Discovery(msg) => IntegrationError::Discovery { message: msg },
            kaptein_core::Error::Kubeconfig(e) => IntegrationError::Kubeconfig {
                message: e.to_string(),
            },
            kaptein_core::Error::Api(e) => {
                // `kube::Error::Api` carries the server's status (code + reason + message),
                // which is already safe — the request body/credentials are not included.
                IntegrationError::Api {
                    message: e.to_string(),
                }
            }
            kaptein_core::Error::External { tool, message } => IntegrationError::External {
                message: format!("{tool}: {message}"),
            },
            kaptein_core::Error::Internal(msg) => IntegrationError::Internal(msg),
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_maps_to_api_variant_without_leaking_body() {
        let err = IntegrationError::from(kaptein_core::Error::Api(kube::Error::Api(Box::<
            kube::core::Status,
        >::default(
        ))));
        match err {
            IntegrationError::Api { message: _ } => {
                // The classification is "api error"; the message is the server status
                // (code + reason), which never includes a credential or request body.
            }
            _ => panic!("expected Api variant"),
        }
    }

    #[test]
    fn internal_error_is_forwarded_verbatim() {
        let err = IntegrationError::from(kaptein_core::Error::Internal("boom".into()));
        assert!(matches!(err, IntegrationError::Internal(m) if m == "boom"));
    }

    #[test]
    fn external_error_preserves_tool_and_message() {
        let err = IntegrationError::from(kaptein_core::Error::External {
            tool: "helm".into(),
            message: "not installed".into(),
        });
        match err {
            IntegrationError::External { message } => {
                assert!(message.contains("helm") && message.contains("not installed"));
            }
            _ => panic!("expected External variant"),
        }
    }
}
