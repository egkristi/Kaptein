//! Layer 2 — the semantic layer.
//!
//! The genuinely renderer-agnostic part: actions, RBAC state, status inference, blast
//! radius. Identical for every frontend.

/// An action the user (or an agent) can take, and whether it is currently allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    /// RBAC-preflight result: `Allowed` (enabled) vs `Forbidden` (greyed out *before*
    /// the user tries), possibly with a reason.
    pub state: ActionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionState {
    Allowed,
    /// Disallowed by RBAC preflight — shown greyed out, never a post-hoc 403.
    Forbidden {
        reason: String,
    },
    /// Allowed but gated behind a guardrail (e.g. prod "break glass").
    Gated {
        reason: String,
    },
}

/// The overall status of the current view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warning(String),
    Error(String),
    /// Read-only because the context is unknown or `impersonate` is unavailable.
    ReadOnly(String),
}
