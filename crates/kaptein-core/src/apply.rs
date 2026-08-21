//! Dry-run apply — validate a manifest against the API server without mutating state.
//!
//! The safe half of M1.3 ("server-side dry-run and diff before apply"): parse a YAML
//! manifest into a `DynamicObject`, submit it with `PostParams { dry_run: true }`, and
//! return the server's dry-run result. No write ever reaches etcd — the API server runs
//! admission and validation, then returns the object it *would* have persisted.

use kube::api::{Api, DynamicObject, PostParams};
use kube::{Client, ResourceExt};

use crate::Error;

/// The outcome of a dry-run: either the server-validated object, or a serializable error.
#[derive(Debug, Clone)]
pub struct DryRun {
    /// The API server's response to the dry-run (the object it would persist).
    pub response_yaml: String,
    /// `true` if the server accepted the manifest (dry-run succeeded).
    pub accepted: bool,
}

/// Submit a YAML manifest (single document) as a **dry-run** create/apply.
///
/// The object's `apiVersion`/`kind` are read from the manifest and used to target the
/// correct API resource. A dry-run never persists anything.
pub async fn dry_run_apply(client: &Client, manifest: &str) -> Result<DryRun, Error> {
    // Parse as DynamicObject so arbitrary built-ins and CRDs work uniformly.
    let obj: DynamicObject =
        serde_yaml::from_str(manifest).map_err(|e| Error::Internal(e.to_string()))?;

    let namespace = obj.namespace();
    let api_version = obj
        .types
        .as_ref()
        .map(|t| t.api_version.clone())
        .unwrap_or_default();
    let kind = obj
        .types
        .as_ref()
        .map(|t| t.kind.clone())
        .unwrap_or_default();
    let gvk = parse_gvk(&api_version, &kind);
    let ar = kube::core::ApiResource::from_gvk(&gvk);

    let api: Api<DynamicObject> = match namespace.as_deref() {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    // Server-side dry-run create.
    let pp = PostParams {
        dry_run: true,
        field_manager: Some("kaptein".into()),
    };
    let result = api.create(&pp, &obj).await;

    match result {
        Ok(created) => Ok(DryRun {
            response_yaml: serde_yaml::to_string(&created)
                .map_err(|e| Error::Internal(e.to_string()))?,
            accepted: true,
        }),
        Err(kube::Error::Api(ae)) if ae.code == 422 || ae.code == 400 => {
            // Validation/admission failure — this is exactly what dry-run exists to
            // surface before a real apply.
            Ok(DryRun {
                response_yaml: format!("rejected: {}", ae.message),
                accepted: false,
            })
        }
        Err(e) => Err(Error::Api(e)),
    }
}

/// Construct a `GroupVersionKind` from an `apiVersion` and `kind` string.
fn parse_gvk(api_version: &str, kind: &str) -> kube::core::GroupVersionKind {
    let parts: Vec<&str> = api_version.split('/').collect();
    match parts.as_slice() {
        [version] => kube::core::GroupVersionKind::gvk("", version, kind),
        [group, version] => kube::core::GroupVersionKind::gvk(group, version, kind),
        _ => kube::core::GroupVersionKind::gvk("", "v1", kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gvk_core_group() {
        let gvk = parse_gvk("v1", "Pod");
        assert_eq!(gvk.group, "");
        assert_eq!(gvk.version, "v1");
        assert_eq!(gvk.kind, "Pod");
    }

    #[test]
    fn parse_gvk_named_group() {
        let gvk = parse_gvk("apps/v1", "Deployment");
        assert_eq!(gvk.group, "apps");
        assert_eq!(gvk.version, "v1");
        assert_eq!(gvk.kind, "Deployment");
    }
}
