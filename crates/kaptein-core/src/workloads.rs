//! Workload writes — scale and restart (M1.2 k9s parity).
//!
//! The two remaining *write* primitives for k9s parity, both governed by the
//! "read-only default, explicit opt-in for writes" guardrail:
//!
//! - `scale` patches the `scale` subresource (server-side dry-run unless confirmed).
//! - `restart` triggers a rollout by annotating the pod template with
//!   `kube.kubernetes.io/restartedAt` (the same mechanism as `kubectl rollout restart`).
//!
//! Neither touches etcd on a dry-run; both go through the API server so RBAC and
//! admission are enforced exactly as a real write would be.

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use kube::Client;
use kube::api::{Api, DynamicObject, Patch, PatchParams};
use kube::core::{ApiResource, GroupVersionKind};

use crate::Error;

/// The outcome of a scale operation.
#[derive(Debug, Clone)]
pub struct ScaleOutcome {
    /// `true` if the replicas were actually changed (vs. dry-run).
    pub scaled: bool,
    /// Human-readable result.
    pub message: String,
}

/// Scale a workload by patching its `scale` subresource. When `confirm` is false, a
/// server-side dry-run validates the patch without changing anything.
pub async fn scale(
    client: &Client,
    gvk: &GroupVersionKind,
    name: &str,
    namespace: Option<&str>,
    replicas: i32,
    confirm: bool,
) -> Result<ScaleOutcome, Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let pp = PatchParams {
        dry_run: !confirm,
        field_manager: Some("kaptein".into()),
        ..PatchParams::default()
    };
    let patch = Patch::Merge(serde_json::json!({ "spec": { "replicas": replicas } }));

    match api.patch_scale(name, &pp, &patch).await {
        Ok(scale) => {
            let actual = scale
                .spec
                .as_ref()
                .and_then(|s| s.replicas)
                .unwrap_or(replicas);
            let message = if confirm {
                format!("scaled {name} to {actual} replicas")
            } else {
                format!("dry-run: {name} would scale to {replicas} replicas")
            };
            Ok(ScaleOutcome {
                scaled: confirm,
                message,
            })
        }
        Err(e) => Err(Error::Api(e)),
    }
}

/// The outcome of a restart operation.
#[derive(Debug, Clone)]
pub struct RestartOutcome {
    /// Human-readable result.
    pub message: String,
}

/// Trigger a rollout restart for a Deployment, StatefulSet, or DaemonSet.
///
/// There is no dry-run for a restart (it is not a spec change that the server can
/// validate) — the guardrail is enforced by the *caller* (the CLI/TUI require an
/// explicit `--confirm`/break-glass before invoking this).
pub async fn restart(
    client: &Client,
    gvk: &GroupVersionKind,
    name: &str,
    namespace: &str,
) -> Result<RestartOutcome, Error> {
    match gvk.kind.as_str() {
        "Deployment" => {
            let api: Api<Deployment> = Api::namespaced(client.clone(), namespace);
            api.restart(name).await.map_err(Error::Api)?;
        }
        "StatefulSet" => {
            let api: Api<StatefulSet> = Api::namespaced(client.clone(), namespace);
            api.restart(name).await.map_err(Error::Api)?;
        }
        "DaemonSet" => {
            let api: Api<DaemonSet> = Api::namespaced(client.clone(), namespace);
            api.restart(name).await.map_err(Error::Api)?;
        }
        other => {
            return Err(Error::Internal(format!(
                "restart is not supported for kind '{other}' (supported: Deployment, StatefulSet, DaemonSet)"
            )));
        }
    }
    Ok(RestartOutcome {
        message: format!(
            "restarted {name} (annotated the pod template with kube.kubernetes.io/restartedAt)"
        ),
    })
}
