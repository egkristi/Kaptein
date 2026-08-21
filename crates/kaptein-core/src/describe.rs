//! Describe — a YAML dump of a single resource, and logs — tail a pod's containers.
//!
//! The "describe" primitive returns the raw object (which the frontend renders as YAML),
//! and the logs primitive streams recent log lines from a pod (M1.2).

use k8s_openapi::api::core::v1::Pod;
use kube::Client;
use kube::api::{Api, LogParams};

use crate::Error;

/// Fetch a resource and serialize it to YAML for a "describe"-style dump.
///
/// Uses `serde_yaml` for a human-readable representation. `DynamicObject` serializes to
/// YAML directly, so any resource (built-in or CRD) is described uniformly.
pub async fn describe_dynamic(
    client: &Client,
    gvk: &kube::core::GroupVersionKind,
    namespace: Option<&str>,
    name: &str,
) -> Result<String, Error> {
    let ar = kube::core::ApiResource::from_gvk(gvk);
    let api: Api<kube::api::DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };
    let obj = api.get(name).await.map_err(Error::Api)?;
    serde_yaml::to_string(&obj).map_err(|e| Error::Internal(e.to_string()))
}

/// Fetch the most recent log lines from every container of a pod.
///
/// Returns a `(container, line)` list. `tail_lines` caps the volume; `follow` is not yet
/// implemented (that requires a streaming response and belongs to a later milestone).
pub async fn pod_logs(
    client: &Client,
    namespace: &str,
    name: &str,
    tail_lines: Option<i64>,
) -> Result<Vec<(String, String)>, Error> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod = pods.get(name).await.map_err(Error::Api)?;

    let mut out = Vec::new();
    let container_names: Vec<String> = pod
        .spec
        .as_ref()
        .map(|s| s.containers.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();

    for cname in container_names {
        let lp = LogParams {
            container: Some(cname.clone()),
            tail_lines,
            ..Default::default()
        };
        let logs = pods.logs(name, &lp).await.map_err(Error::Api)?;
        for line in logs.lines() {
            out.push((cname.clone(), line.to_string()));
        }
    }
    Ok(out)
}
