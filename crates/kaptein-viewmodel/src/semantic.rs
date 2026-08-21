//! Layer 2 — the semantic layer.
//!
//! The genuinely renderer-agnostic part: actions, RBAC state, status inference, blast
//! radius. Identical for every frontend.
//!
//! The view-model emits **message keys + args**, never localized strings (see ADR-0005);
//! the frontend resolves keys for i18n. Structured data (e.g. which verb/resource is
//! forbidden) is carried as fields so the MCP surface can reason about it programmatically.

use serde::{Deserialize, Serialize};

/// An action the user (or an agent) can take, and whether it is currently allowed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    /// Message key resolved by the frontend for i18n.
    pub label_key: String,
    /// RBAC-preflight result: `Allowed` (enabled) vs `Forbidden` (greyed out *before*
    /// the user tries), possibly with a structured reason.
    pub state: ActionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionState {
    Allowed,
    /// Disallowed by RBAC preflight — shown greyed out, never a post-hoc 403. Carries
    /// the specific missing permission so the MCP surface can act on it.
    Forbidden {
        verb: String,
        resource: String,
        namespace: Option<String>,
    },
    /// Allowed but gated behind a guardrail (e.g. prod "break glass").
    Gated {
        /// Message key (localized by the frontend), not a pre-formatted sentence.
        reason_key: String,
    },
}

/// The overall status of the current view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Ok,
    Warning {
        message_key: String,
    },
    Error {
        message_key: String,
    },
    /// Read-only because the context is unknown or `impersonate` is unavailable.
    ReadOnly {
        message_key: String,
    },
}
