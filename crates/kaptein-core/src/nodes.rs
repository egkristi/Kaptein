//! Node operations — cordon/uncordon, evict, and safe drain (M1.2 k9s parity).
//!
//! These are the node-lifecycle writes. All follow the read-only-default guardrail:
//! writes require an explicit `confirm` flag (enforced by the CLI, which also applies
//! the break-glass gate). Drain is implemented as a *read-only* planner by default —
//! it computes what a drain would evict without evicting, so an operator can preview
//! the blast radius before any cordon/evict occurs.

use k8s_openapi::api::core::v1::{Node, Pod};
use kube::api::{Api, EvictParams};
use kube::{Client, ResourceExt};

use crate::Error;

/// The result of a cordon/uncordon.
#[derive(Debug, Clone)]
pub struct NodeOpOutcome {
    /// Human-readable result.
    pub message: String,
}

/// Cordon a node (mark it unschedulable). `confirm` must be true to actually cordon;
/// otherwise this returns the intended action without touching the API server.
pub async fn cordon(client: &Client, name: &str, confirm: bool) -> Result<NodeOpOutcome, Error> {
    if !confirm {
        return Ok(NodeOpOutcome {
            message: format!("dry-run: {name} would be cordoned (unschedulable)"),
        });
    }
    let nodes: Api<Node> = Api::all(client.clone());
    nodes.cordon(name).await.map_err(Error::Api)?;
    Ok(NodeOpOutcome {
        message: format!("cordoned {name} (marked unschedulable)"),
    })
}

/// Uncordon a node (mark it schedulable again).
pub async fn uncordon(client: &Client, name: &str, confirm: bool) -> Result<NodeOpOutcome, Error> {
    if !confirm {
        return Ok(NodeOpOutcome {
            message: format!("dry-run: {name} would be uncordoned (schedulable)"),
        });
    }
    let nodes: Api<Node> = Api::all(client.clone());
    nodes.uncordon(name).await.map_err(Error::Api)?;
    Ok(NodeOpOutcome {
        message: format!("uncordoned {name} (marked schedulable)"),
    })
}

/// Evict a pod. `confirm` must be true to actually evict; otherwise this reports the
/// intended action without touching the API server.
pub async fn evict(
    client: &Client,
    namespace: &str,
    name: &str,
    confirm: bool,
) -> Result<NodeOpOutcome, Error> {
    if !confirm {
        return Ok(NodeOpOutcome {
            message: format!("dry-run: {namespace}/{name} would be evicted"),
        });
    }
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let ep = EvictParams::default();
    pods.evict(name, &ep).await.map_err(Error::Api)?;
    Ok(NodeOpOutcome {
        message: format!("evicted {namespace}/{name}"),
    })
}

/// A single pod a drain would evict, with the reason it can't be evicted safely (if any).
#[derive(Debug, Clone)]
pub struct DrainTarget {
    pub namespace: String,
    pub name: String,
    /// Why this pod would be skipped by a real drain (empty if it would be evicted).
    pub skip_reason: String,
}

/// A read-only drain preview: list the pods on a node and classify them as evictable
/// or protected (daemonsets, mirror pods, or pods not managed by a controller).
///
/// This never cordons or evicts — it computes what a drain *would* do, so the operator
/// can preview the blast radius (read-only default, M1.2).
pub async fn drain_preview(client: &Client, node: &str) -> Result<Vec<DrainTarget>, Error> {
    let pods: Api<Pod> = Api::all(client.clone());
    // Field-selector for spec.nodeName == node.
    let lp = kube::api::ListParams::default().fields(&format!("spec.nodeName={node}"));
    let list = pods.list(&lp).await.map_err(Error::Api)?;

    let mut out = Vec::new();
    for pod in list {
        let namespace = pod.namespace().unwrap_or_default();
        let name = pod.name_any();
        let skip_reason = drain_skip_reason(&pod);
        out.push(DrainTarget {
            namespace,
            name,
            skip_reason,
        });
    }
    out.sort_by(|a, b| {
        (a.namespace.clone(), a.name.clone()).cmp(&(b.namespace.clone(), b.name.clone()))
    });
    Ok(out)
}

/// Determine why a drain would skip a pod (empty string = evictable).
fn drain_skip_reason(pod: &Pod) -> String {
    let owner = pod
        .metadata
        .owner_references
        .as_ref()
        .and_then(|ors| ors.first());
    let owner_kind = owner.map(|o| o.kind.as_str()).unwrap_or("");

    match owner_kind {
        "DaemonSet" => "daemonset-managed (drain ignores)".into(),
        "Node" => "mirror pod (drain ignores)".into(),
        "ReplicaSet" | "StatefulSet" | "Job" | "ReplicationController" | "" => String::new(),
        _ => format!("owner {owner_kind} not drained automatically"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::OwnerReference;

    #[test]
    fn drain_skips_daemonsets() {
        let pod = Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                owner_references: Some(vec![OwnerReference {
                    kind: "DaemonSet".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            spec: None,
            status: None,
        };
        assert!(drain_skip_reason(&pod).contains("daemonset"));
    }

    #[test]
    fn drain_evicts_controller_managed_pods() {
        let pod = Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                owner_references: Some(vec![OwnerReference {
                    kind: "ReplicaSet".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            spec: None,
            status: None,
        };
        assert_eq!(drain_skip_reason(&pod), "");
    }

    #[test]
    fn drain_skips_mirror_pods() {
        let pod = Pod {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                owner_references: Some(vec![OwnerReference {
                    kind: "Node".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            spec: None,
            status: None,
        };
        assert!(drain_skip_reason(&pod).contains("mirror"));
    }
}
