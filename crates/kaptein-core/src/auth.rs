//! RBAC preflight — "what am I allowed to do here?"
//!
//! On context switch, run `SelfSubjectRulesReview` and grey out disallowed actions
//! *before* the user tries them (not a 403 afterwards). This is the first-class RBAC
//! preflight feature (M1.1).

use k8s_openapi::api::authorization::v1::{
    SelfSubjectRulesReview, SelfSubjectRulesReviewSpec, SubjectRulesReviewStatus,
};
use kube::api::PostParams;
use kube::{Api, Client};

use crate::Error;

/// A permission check for a single `verb` on a `resource` in a `group` and `namespace`.
///
/// Returns whether the action is allowed and, if not, nothing more (the UI greys it
/// out). This is the display-neutral form the view-model consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    pub verb: String,
    pub resource: String,
    pub group: String,
    pub namespace: String,
    pub allowed: bool,
}

/// Query the API server for the current user's rules in a namespace.
async fn subject_rules(
    client: &Client,
    namespace: &str,
) -> Result<SubjectRulesReviewStatus, Error> {
    let api: Api<SelfSubjectRulesReview> = Api::all(client.clone());
    let review = SelfSubjectRulesReview {
        spec: SelfSubjectRulesReviewSpec {
            namespace: Some(namespace.to_string()),
        },
        ..Default::default()
    };
    let resp = api
        .create(&PostParams::default(), &review)
        .await
        .map_err(Error::Api)?;
    Ok(resp.status.unwrap_or_default())
}

/// Check whether the current user may perform `verb` on `resource` (plural) in `group`
/// within `namespace`. Matches against the resource rules returned by the review,
/// handling `*` wildcards for group, resource, and verb.
pub async fn can(
    client: &Client,
    verb: &str,
    resource: &str,
    group: &str,
    namespace: &str,
) -> Result<Permission, Error> {
    let status = subject_rules(client, namespace).await?;
    let allowed = status.resource_rules.iter().any(|rule| {
        let group_ok = rule
            .api_groups
            .as_ref()
            .is_none_or(|groups| groups.iter().any(|g| g == "*" || g == group));
        let resource_ok = rule.resources.as_ref().is_none_or(|resources| {
            resources
                .iter()
                .any(|r| r == "*" || r == resource || r == &format!("*/{resource}"))
        });
        let verb_ok = rule.verbs.iter().any(|v| v == "*" || v == verb);
        group_ok && resource_ok && verb_ok
    });
    Ok(Permission {
        verb: verb.to_string(),
        resource: resource.to_string(),
        group: group.to_string(),
        namespace: namespace.to_string(),
        allowed,
    })
}

/// A batch RBAC preflight: check a standard set of verbs against a resource in a
/// namespace, so the frontend can grey out disallowed actions *before* the user tries
/// them (the "RBAC-preflight-greyed actions" k9s-parity item).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub resource: String,
    pub group: String,
    pub namespace: String,
    /// `(verb, allowed)` pairs in a stable order.
    pub actions: Vec<(String, bool)>,
}

/// Check the standard action set (`get`, `list`, `watch`, `create`, `update`, `patch`,
/// `delete`, `deletecollection`) for a resource. One `SelfSubjectRulesReview` call is
/// made per namespace and reused across all verbs.
pub async fn preflight(
    client: &Client,
    resource: &str,
    group: &str,
    namespace: &str,
) -> Result<Preflight, Error> {
    const VERBS: [&str; 8] = [
        "get",
        "list",
        "watch",
        "create",
        "update",
        "patch",
        "delete",
        "deletecollection",
    ];

    // Fetch the rules once.
    let status = subject_rules(client, namespace).await?;

    let mut actions = Vec::with_capacity(VERBS.len());
    for verb in VERBS {
        let allowed = status.resource_rules.iter().any(|rule| {
            let group_ok = rule
                .api_groups
                .as_ref()
                .is_none_or(|groups| groups.iter().any(|g| g == "*" || g == group));
            let resource_ok = rule.resources.as_ref().is_none_or(|resources| {
                resources
                    .iter()
                    .any(|r| r == "*" || r == resource || r == &format!("*/{resource}"))
            });
            let verb_ok = rule.verbs.iter().any(|v| v == "*" || v == verb);
            group_ok && resource_ok && verb_ok
        });
        actions.push((verb.to_string(), allowed));
    }

    Ok(Preflight {
        resource: resource.to_string(),
        group: group.to_string(),
        namespace: namespace.to_string(),
        actions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_is_clone_eq() {
        let p = Permission {
            verb: "get".into(),
            resource: "pods".into(),
            group: "".into(),
            namespace: "default".into(),
            allowed: true,
        };
        assert_eq!(p, p.clone());
        assert!(p.allowed);
    }
}
