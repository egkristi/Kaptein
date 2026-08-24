//! Ephemeral containers — `kubectl debug`-style profiling (M1.2).
//!
//! Ephemeral containers are the safe way to attach a debug container to a running pod
//! without restarting it (Kubernetes ≥1.25). They are **additive** — they cannot be
//! removed from a pod once attached — so this module is deliberately **read-only by
//! default**: `list` reports current ephemeral containers, and `add` requires an
//! explicit `confirm` (and is gated by the caller's break-glass guardrail).

use k8s_openapi::api::core::v1::{EphemeralContainer, Pod};
use kube::Client;
use kube::api::{Api, PostParams};

use crate::Error;

/// A single ephemeral container summary.
#[derive(Debug, Clone)]
pub struct EphemeralSummary {
    /// The ephemeral container name.
    pub name: String,
    /// Its image.
    pub image: String,
    /// The command it runs (if any).
    pub command: Vec<String>,
}

/// The outcome of an `add` operation.
#[derive(Debug, Clone)]
pub struct EphemeralOutcome {
    /// Human-readable result.
    pub message: String,
}

/// List the ephemeral containers currently attached to a pod.
pub async fn list(
    client: &Client,
    namespace: &str,
    pod: &str,
) -> Result<Vec<EphemeralSummary>, Error> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod_obj = pods
        .get_ephemeral_containers(pod)
        .await
        .map_err(Error::Api)?;
    let containers = pod_obj
        .spec
        .and_then(|s| s.ephemeral_containers)
        .unwrap_or_default();
    Ok(containers
        .into_iter()
        .map(|c| EphemeralSummary {
            name: c.name,
            image: c.image.unwrap_or_default(),
            command: c.command.unwrap_or_default(),
        })
        .collect())
}

/// Attach an ephemeral container to a running pod. `confirm` must be true to actually
/// attach; otherwise this returns the intended action without touching the API server.
///
/// Ephemeral containers are additive and cannot be removed, so the read-only-default
/// guardrail is especially important here.
pub async fn add(
    client: &Client,
    namespace: &str,
    pod: &str,
    container_name: &str,
    image: &str,
    command: &[String],
    confirm: bool,
) -> Result<EphemeralOutcome, Error> {
    if !confirm {
        return Ok(EphemeralOutcome {
            message: format!(
                "dry-run: {namespace}/{pod} would gain ephemeral container '{container_name}' ({image})"
            ),
        });
    }

    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);

    // Merge the new container into the existing ephemeral container list (the subresource
    // is additive and a replace would drop previously-attached containers).
    let existing = pods
        .get_ephemeral_containers(pod)
        .await
        .map_err(Error::Api)?;
    let mut containers = existing
        .spec
        .and_then(|s| s.ephemeral_containers)
        .unwrap_or_default();

    let mut new_container = EphemeralContainer {
        name: container_name.to_string(),
        ..Default::default()
    };
    new_container.image = Some(image.to_string());
    if !command.is_empty() {
        new_container.command = Some(command.to_vec());
    }
    containers.push(new_container);

    let patch = Pod {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(pod.to_string()),
            namespace: Some(namespace.to_string()),
            ..Default::default()
        },
        spec: Some(k8s_openapi::api::core::v1::PodSpec {
            ephemeral_containers: Some(containers),
            ..Default::default()
        }),
        ..Default::default()
    };

    pods.replace_ephemeral_containers(pod, &PostParams::default(), &patch)
        .await
        .map_err(Error::Api)?;

    Ok(EphemeralOutcome {
        message: format!(
            "attached ephemeral container '{container_name}' ({image}) to {namespace}/{pod}"
        ),
    })
}
