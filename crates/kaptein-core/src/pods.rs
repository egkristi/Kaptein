//! Pod listing — the first real Kubernetes access.
//!
//! Lists pods in a namespace from a live cluster, using `kube-rs`. This is the
//! minimal end-to-end proof that `kaptein-core` can talk to an API server.

use k8s_openapi::api::core::v1::Pod;
use kube::api::ObjectList;
use kube::config::KubeConfigOptions;
use kube::{Api, Client, Config, ResourceExt};

use crate::Error;

/// A single pod's identifying and status information, in a stable, display-neutral form.
#[derive(Debug, Clone)]
pub struct PodSummary {
    pub name: String,
    pub namespace: String,
    pub phase: String,
    pub ready: String,
    pub restarts: i32,
}

/// Build a Kubernetes client from the default kubeconfig (the same discovery rules as
/// `kubectl`: `KUBECONFIG` env, then `~/.kube/config`).
pub async fn client() -> Result<Client, Error> {
    let config = Config::from_kubeconfig(&KubeConfigOptions::default()).await?;
    Ok(Client::try_from(config)?)
}

/// List pods in the given namespace and reduce them to `PodSummary`s.
pub async fn list_pods(client: &Client, namespace: &str) -> Result<Vec<PodSummary>, Error> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let list: ObjectList<Pod> = pods.list(&Default::default()).await.map_err(Error::Api)?;

    Ok(list
        .into_iter()
        .map(|pod| PodSummary {
            name: pod.name_any(),
            namespace: pod.namespace().unwrap_or_else(|| namespace.to_string()),
            phase: pod
                .status
                .as_ref()
                .and_then(|s| s.phase.clone())
                .unwrap_or_else(|| "Unknown".into()),
            ready: pod_ready(&pod),
            restarts: pod
                .status
                .as_ref()
                .and_then(|s| s.container_statuses.as_ref())
                .map(|cs| cs.iter().map(|c| c.restart_count).sum())
                .unwrap_or(0),
        })
        .collect())
}

/// Compute a `ready/total` string from a pod's container statuses, or "—" if unknown.
fn pod_ready(pod: &Pod) -> String {
    let Some(status) = &pod.status else {
        return "—".into();
    };
    let Some(container_statuses) = &status.container_statuses else {
        return "—".into();
    };
    let total = container_statuses.len();
    let ready = container_statuses.iter().filter(|c| c.ready).count();
    format!("{ready}/{total}")
}

/// Fetch a single pod by name in a namespace.
pub async fn get_pod(client: &Client, namespace: &str, name: &str) -> Result<Pod, Error> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    pods.get(name).await.map_err(Error::Api)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pod_ready_unknown_when_no_status() {
        let pod = Pod {
            metadata: Default::default(),
            spec: None,
            status: None,
        };
        assert_eq!(pod_ready(&pod), "—");
    }
}
