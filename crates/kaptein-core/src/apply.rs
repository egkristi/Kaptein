//! Dry-run apply — validate a manifest against the API server without mutating state.
//!
//! The safe half of M1.3 ("server-side dry-run and diff before apply"): parse a YAML
//! manifest into a `DynamicObject`, submit it with `PostParams { dry_run: true }`, and
//! return the server's dry-run result. No write ever reaches etcd — the API server runs
//! admission and validation, then returns the object it *would* have persisted.

use kube::api::{Api, DynamicObject, Patch, PatchParams, PostParams};
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

/// Server-side-apply (patch) a YAML manifest as a **dry-run** — the edit path.
///
/// Unlike `dry_run_apply` (a create), this uses a `Patch::Apply` against an existing
/// object, which is the correct semantic for "edit then validate" (M1.3): the object
/// already exists, so a create would fail with `AlreadyExists`.
///
/// Server-managed fields (`metadata.managedFields`, `metadata.resourceVersion`,
/// `metadata.creationTimestamp`, `metadata.uid`, `status`) are stripped before the
/// patch — they are read-only and would otherwise be rejected by the API server.
///
/// # Phase 2 guardrail (issue #16)
///
/// `force: true` is **correct for dry-run only** — it lets the dry-run succeed against
/// resources created by other field managers (`kubectl create`, Flux/Argo). The Phase 2
/// **real write path must NOT reuse this function or carry `force: true` forward**: a
/// real apply with `force` silently steals field ownership from Flux/Argo/GitOps, which
/// would then drop those fields on their next reconcile. The write path needs a *new*
/// function with `force: false` (or an explicit field-ownership negotiation), never this
/// one.
pub async fn dry_run_apply_patch(client: &Client, manifest: &str) -> Result<DryRun, Error> {
    let mut obj: DynamicObject =
        serde_yaml::from_str(manifest).map_err(|e| Error::Internal(e.to_string()))?;

    strip_server_managed(&mut obj);

    let namespace = obj.namespace();
    let name = obj.name_any();
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

    let pp = PatchParams {
        dry_run: true,
        field_manager: Some("kaptein".into()),
        // Force ownership of fields previously managed by other clients (e.g.
        // `kubectl create`) so edits don't fail with FieldManagerConflict.
        force: true,
        ..PatchParams::default()
    };
    let patch = Patch::Apply(&obj);

    match api.patch(&name, &pp, &patch).await {
        Ok(patched) => Ok(DryRun {
            response_yaml: serde_yaml::to_string(&patched)
                .map_err(|e| Error::Internal(e.to_string()))?,
            accepted: true,
        }),
        Err(kube::Error::Api(ae)) if ae.code == 422 || ae.code == 400 => Ok(DryRun {
            response_yaml: format!("rejected: {}", ae.message),
            accepted: false,
        }),
        Err(e) => Err(Error::Api(e)),
    }
}

/// Strip the read-only, server-managed metadata fields and status from a dynamic object
/// before submitting it as an apply patch. `managedFields` in particular is rejected by
/// the API server ("metadata.managedFields must be nil") if present.
fn strip_server_managed(obj: &mut DynamicObject) {
    obj.metadata.managed_fields = None;
    obj.metadata.resource_version = None;
    obj.metadata.creation_timestamp = None;
    obj.metadata.uid = None;
    obj.metadata.generation = None;
    obj.metadata.self_link = None;
    // `status` and `data`-adjacent fields differ per kind; `DynamicObject`'s `data`
    // field carries everything else, but `status` is a well-known top-level key.
    obj.data.as_object_mut().map(|m| m.remove("status"));
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
