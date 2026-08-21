//! Delete — remove a resource with explicit cascade selection.
//!
//! The write operation (M1.2) with guardrails: deletes are **dry-run by default** and
//! require `--confirm` to actually remove anything, with an explicit `PropagationPolicy`
//! for cascade selection. This matches the "read-only default, explicit opt-in for
//! writes" guardrail.

use kube::Client;
use kube::api::{Api, DeleteParams, DynamicObject, PropagationPolicy};
use kube::core::{ApiResource, GroupVersionKind};

use crate::Error;

/// The outcome of a delete request.
#[derive(Debug, Clone)]
pub struct DeleteOutcome {
    /// `true` if the object was actually deleted (vs. dry-run).
    pub deleted: bool,
    /// Human-readable result.
    pub message: String,
}

/// Delete a resource by `group/version/kind`, name, and namespace (optional for
/// cluster-scoped). When `confirm` is false, performs a server-side dry-run only.
pub async fn delete(
    client: &Client,
    gvk: &GroupVersionKind,
    name: &str,
    namespace: Option<&str>,
    cascade: PropagationPolicy,
    confirm: bool,
) -> Result<DeleteOutcome, Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let params = DeleteParams {
        dry_run: !confirm,
        propagation_policy: Some(cascade),
        ..DeleteParams::default()
    };

    match api.delete(name, &params).await {
        Ok(_) => {
            let msg = if confirm {
                format!("deleted {name}")
            } else {
                format!("dry-run: {name} would be deleted")
            };
            Ok(DeleteOutcome {
                deleted: confirm,
                message: msg,
            })
        }
        Err(kube::Error::Api(ae)) => Err(Error::Api(kube::Error::Api(ae))),
        Err(e) => Err(Error::Api(e)),
    }
}

/// Parse a cascade policy string.
pub fn parse_propagation(s: &str) -> PropagationPolicy {
    match s {
        "orphan" => PropagationPolicy::Orphan,
        "foreground" => PropagationPolicy::Foreground,
        _ => PropagationPolicy::Background,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_propagation_defaults_to_background() {
        assert_eq!(
            parse_propagation("background"),
            PropagationPolicy::Background
        );
        assert_eq!(
            parse_propagation("foreground"),
            PropagationPolicy::Foreground
        );
        assert_eq!(parse_propagation("orphan"), PropagationPolicy::Orphan);
        assert_eq!(parse_propagation("bogus"), PropagationPolicy::Background);
    }
}
