//! Describe — a YAML dump of a single resource, and logs — tail a pod's containers.
//!
//! The "describe" primitive returns the raw object (which the frontend renders as YAML),
//! and the logs primitive streams recent log lines from a pod (M1.2).

use k8s_openapi::api::core::v1::Pod;
use kube::api::{Api, ListParams, LogParams};
use kube::{Client, ResourceExt};

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

/// A single log line from a multi-pod stream.
#[derive(Debug, Clone)]
pub struct LogLine {
    pub pod: String,
    pub namespace: String,
    pub container: String,
    pub line: String,
}

/// Stream recent logs from all pods matching a label selector, optionally filtered by a
/// regex. Each matching line is prefixed with the pod/container it came from.
///
/// This is the "multi-pod/multi-container log streaming with regex filter" primitive
/// (M1.2). `follow` is not implemented — this returns a bounded tail, not an open stream.
pub async fn multi_pod_logs(
    client: &Client,
    namespace: &str,
    label_selector: Option<&str>,
    regex_filter: Option<&str>,
    tail_lines: Option<i64>,
) -> Result<Vec<LogLine>, Error> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let lp = ListParams::default().labels(label_selector.unwrap_or(""));
    let list = pods.list(&lp).await.map_err(Error::Api)?;

    let re = regex_filter
        .map(regex::Regex::new)
        .transpose()
        .map_err(|e| Error::Internal(e.to_string()))?;

    let mut out = Vec::new();
    for pod in list.into_iter() {
        let pod_name = pod.name_any();
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
            let logs = pods.logs(&pod_name, &lp).await.map_err(Error::Api)?;
            for line in logs.lines() {
                if let Some(re) = &re
                    && !re.is_match(line)
                {
                    continue;
                }
                out.push(LogLine {
                    pod: pod_name.clone(),
                    namespace: namespace.to_string(),
                    container: cname.clone(),
                    line: line.to_string(),
                });
            }
        }
    }
    Ok(out)
}
