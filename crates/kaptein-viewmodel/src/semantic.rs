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

/// The Kubernetes RBAC `verb` an action id requires. This is the single, renderer-agnostic
/// mapping that lets the core's RBAC preflight grey out a lens-declared action *before*
/// the user tries it (M2.2 "per-action RBAC grey-out"). It lives here, not in a frontend,
/// so the TUI, GUI, and MCP surface all map an action id to the same verb.
///
/// `describe` needs `get` (describe reads the object); `logs`, `exec`, `port-forward`
/// need `get` too (each reads the pod); `scale`/`restart` need `update` (or `patch`);
/// `delete` needs `delete`. Unknown action ids map to `get` (the read-only default, so an
/// unknown action is never *less* restricted than a read).
pub fn action_verb(action_id: &str) -> &'static str {
    match action_id {
        "delete" => "delete",
        "scale" | "restart" | "update" | "apply" | "edit" | "cordon" | "drain" | "uncordon" => {
            "update"
        }
        "describe" | "logs" | "exec" | "port-forward" | "diagnose" => "get",
        _ => "get",
    }
}

/// Downgrade an action's state to `Forbidden` when the RBAC preflight denies the verb it
/// needs. This is the renderer-agnostic grey-out: `Allowed`/`Gated` become `Forbidden`
/// (with the structured verb/resource/namespace the frontend and MCP surface can act on);
/// an already-`Forbidden` action is left unchanged. A `None` preflight result means "no
/// preflight was run" and leaves the action untouched (the caller decides whether that is
/// fail-open or fail-closed — the shipped frontend runs preflight before rendering).
pub fn downgrade_forbidden(
    action: &mut Action,
    verb_allowed: Option<bool>,
    resource: &str,
    namespace: Option<&str>,
) {
    let Some(allowed) = verb_allowed else {
        return;
    };
    if allowed {
        return;
    }
    if !matches!(action.state, ActionState::Forbidden { .. }) {
        action.state = ActionState::Forbidden {
            verb: action_verb(&action.id).to_string(),
            resource: resource.to_string(),
            namespace: namespace.map(str::to_string),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_verb_maps_read_write_and_delete() {
        assert_eq!(action_verb("describe"), "get");
        assert_eq!(action_verb("logs"), "get");
        assert_eq!(action_verb("exec"), "get");
        assert_eq!(action_verb("diagnose"), "get");
        assert_eq!(action_verb("scale"), "update");
        assert_eq!(action_verb("restart"), "update");
        assert_eq!(action_verb("delete"), "delete");
        // Unknown ids default to the read-only verb (never *less* restricted than a read).
        assert_eq!(action_verb("something-new"), "get");
    }

    #[test]
    fn downgrade_allowed_to_forbidden_carries_structured_reason() {
        let mut a = Action {
            id: "delete".into(),
            label_key: "action.delete".into(),
            state: ActionState::Allowed,
        };
        downgrade_forbidden(&mut a, Some(false), "clusters", Some("default"));
        assert!(matches!(
            a.state,
            ActionState::Forbidden { ref verb, ref resource, ref namespace }
                if verb == "delete" && resource == "clusters" && namespace.as_deref() == Some("default")
        ));
    }

    #[test]
    fn downgrade_leaves_allowed_when_preflight_grants() {
        let mut a = Action {
            id: "describe".into(),
            label_key: "action.describe".into(),
            state: ActionState::Allowed,
        };
        downgrade_forbidden(&mut a, Some(true), "clusters", Some("default"));
        assert!(matches!(a.state, ActionState::Allowed));
    }

    #[test]
    fn downgrade_leaves_existing_forbidden_untouched() {
        let mut a = Action {
            id: "describe".into(),
            label_key: "action.describe".into(),
            state: ActionState::Forbidden {
                verb: "get".into(),
                resource: "clusters".into(),
                namespace: Some("ns".into()),
            },
        };
        downgrade_forbidden(&mut a, Some(false), "clusters", Some("default"));
        // The pre-existing structured reason is preserved, not overwritten.
        assert!(matches!(
            a.state,
            ActionState::Forbidden { ref namespace, .. } if namespace.as_deref() == Some("ns")
        ));
    }

    #[test]
    fn downgrade_is_a_noop_without_preflight() {
        let mut a = Action {
            id: "scale".into(),
            label_key: "action.scale".into(),
            state: ActionState::Allowed,
        };
        downgrade_forbidden(&mut a, None, "clusters", Some("default"));
        assert!(matches!(a.state, ActionState::Allowed));
    }
}
