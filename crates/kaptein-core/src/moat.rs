//! The diagnostic moat (M1b.3) — the differentiating read-only MCP tools backed by the
//! M1.6 rule engine and the Events API (ADR-0013).
//!
//! Four tools distinguish Kaptein's MCP surface from a raw `kubectl` wrapper:
//!
//! - `explain_pod_failure` — the M1.6 "why isn't this pod ready" over events + status.
//! - `why_is_job_pending` — job admission analysis over conditions and pod status.
//! - `blast_radius` — what else breaks if a resource is removed (ownerRef/selector).
//! - `what_changed_between` — events in a time window, scoped to a namespace.
//!
//! Each is **read-only** and reuses the same primitives the CLI/TUI use, so the moat is
//! not a parallel implementation — it is the same engine, exposed to agents.

use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::Pod;
use kube::api::ListParams;
use kube::{Api, Client, ResourceExt};

use crate::Error;
use crate::events::recent_events;

/// A structured explanation of why a pod is not ready, backed by diagnostics + events.
#[derive(Debug, Clone)]
pub struct PodFailureExplanation {
    pub namespace: String,
    pub name: String,
    /// The rule-engine findings (M1.6).
    pub findings: Vec<crate::diagnostics::Finding>,
    /// Related warning events for this pod (newest first).
    pub related_events: Vec<crate::events::EventSummary>,
}

/// Explain why a pod is failing/not ready: run the diagnostics engine over its status and
/// attach the pod's recent warning events as evidence.
pub async fn explain_pod_failure(
    client: &Client,
    namespace: &str,
    name: &str,
    event_window_minutes: i64,
) -> Result<PodFailureExplanation, Error> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod = pods.get(name).await.map_err(Error::Api)?;

    let findings = crate::diagnostics::diagnose(&pod);

    // Related warning events for this pod in the window.
    let since_ms = now_ms().saturating_sub(event_window_minutes * 60 * 1000);
    let mut related = recent_events(client, Some(namespace), Some(since_ms)).await?;
    related.retain(|e| e.kind == "Pod" && e.name == name && e.type_ == "Warning");

    Ok(PodFailureExplanation {
        namespace: namespace.into(),
        name: name.into(),
        findings,
        related_events: related,
    })
}

/// A structured explanation of why a Job is pending/stuck.
#[derive(Debug, Clone)]
pub struct JobExplanation {
    pub namespace: String,
    pub name: String,
    /// The most recent job conditions (reason + message).
    pub conditions: Vec<(String, String, String)>, // (type, status, message)
    /// Failed/active/succeeded counters from the job status.
    pub failed: i32,
    pub active: i32,
    pub succeeded: i32,
    /// The job's pods and their diagnostics (the evidence behind the job's state).
    pub pods: Vec<String>,
}

/// Analyze why a Job is pending or stuck: report its conditions and the diagnostics of
/// its pods.
pub async fn why_is_job_pending(
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<JobExplanation, Error> {
    let jobs: Api<Job> = Api::namespaced(client.clone(), namespace);
    let job = jobs.get(name).await.map_err(Error::Api)?;

    let status = job.status.as_ref();
    let conditions = status
        .and_then(|s| s.conditions.as_ref())
        .map(|cs| {
            cs.iter()
                .map(|c| {
                    (
                        c.type_.clone(),
                        c.status.clone(),
                        c.message.clone().unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // List the job's pods via label selector (job-name label).
    let label = format!("job-name={name}");
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod_list = pods
        .list(&ListParams::default().labels(&label))
        .await
        .map_err(Error::Api)?;

    let mut pod_lines = Vec::new();
    for pod in pod_list {
        let findings = crate::diagnostics::diagnose(&pod);
        let pname = pod.name_any();
        if findings.is_empty() {
            pod_lines.push(format!("{pname}: ready"));
        } else {
            for f in findings {
                pod_lines.push(format!("{pname}: {} — {}", f.code, f.summary));
            }
        }
    }

    Ok(JobExplanation {
        namespace: namespace.into(),
        name: name.into(),
        conditions,
        failed: status.and_then(|s| s.failed).unwrap_or(0),
        active: status.and_then(|s| s.active).unwrap_or(0),
        succeeded: status.and_then(|s| s.succeeded).unwrap_or(0),
        pods: pod_lines,
    })
}

/// What would break if a resource were removed: its owner references (upstream) and the
/// resources that are owned by it (downstream, via ownerRef matching its uid).
#[derive(Debug, Clone)]
pub struct BlastRadius {
    pub namespace: String,
    pub kind: String,
    pub name: String,
    /// The resource's own owner references (who would notice if *this* vanished).
    pub owners: Vec<String>,
    /// Resources that would be garbage-collected/affected if this vanished.
    pub dependents: Vec<String>,
}

/// Compute a read-only blast radius for a resource: report its owners and the resources
/// that reference it via `ownerReferences` (cascade-delete would remove those).
///
/// The traversal is **generic over the ownership chain**, not hardcoded to a single
/// shape. It handles every common workload controller:
///
/// - `Deployment → ReplicaSet → Pod` (two levels via an intermediate controller)
/// - `StatefulSet → Pod` and `DaemonSet → Pod` (direct `ownerRef`)
/// - `CronJob → Job → Pod` (two levels via an intermediate controller)
///
/// The intermediate controllers that can own Pods and be owned by a workload
/// (`ReplicaSet`, `Job`) are listed and their UIDs added to the transitive "owned" set,
/// so a Pod owned by a ReplicaSet owned by the target is still matched. A full
/// cross-kind topology scan (volumes, selectors, RBAC) is a Phase 3a fleet feature; this
/// covers the ownership/cascade-delete chain.
pub async fn blast_radius(
    client: &Client,
    namespace: Option<&str>,
    gvk: &kube::core::GroupVersionKind,
    name: &str,
) -> Result<BlastRadius, Error> {
    // `None`/empty namespace = cluster-scoped target (Node, Namespace, cluster-scoped
    // CRD). The target is fetched via the cluster-scoped API; the dependents traversal
    // lists namespaced controllers/pods cluster-wide.
    let ar = kube::core::ApiResource::from_gvk(gvk);
    let api: Api<kube::api::DynamicObject> = match namespace {
        Some(ns) if !ns.is_empty() => Api::namespaced_with(client.clone(), ns, &ar),
        _ => Api::all_with(client.clone(), &ar),
    };
    let obj = api.get(name).await.map_err(Error::Api)?;

    let uid = obj.metadata.uid.clone().unwrap_or_default();
    let owners = obj
        .metadata
        .owner_references
        .unwrap_or_default()
        .iter()
        .map(|o| format!("{}/{}", o.kind, o.name))
        .collect::<Vec<_>>();

    // The set of UIDs transitively owned by this resource (the resource's own uid plus
    // any intermediate controllers it owns), used to match dependents at any depth.
    let mut owned_uids: std::collections::HashSet<String> = std::collections::HashSet::new();
    owned_uids.insert(uid.clone());
    let mut dependents = Vec::new();

    // Intermediate controllers that can be owned by a workload and, in turn, own Pods.
    // Listing them generically lets Deployment→ReplicaSet→Pod and CronJob→Job→Pod both
    // resolve without hardcoding the caller's kind.
    let intermediates: [(&str, &str, &str); 2] =
        [("apps", "v1", "ReplicaSet"), ("batch", "v1", "Job")];
    for (group, version, kind) in intermediates {
        let ar = kube::core::ApiResource::from_gvk(&kube::core::GroupVersionKind::gvk(
            group, version, kind,
        ));
        let api: Api<kube::api::DynamicObject> = match namespace {
            Some(ns) if !ns.is_empty() => Api::namespaced_with(client.clone(), ns, &ar),
            _ => Api::all_with(client.clone(), &ar),
        };
        let list = match api.list(&ListParams::default()).await {
            Ok(l) => l,
            // The group/kind may not be served (no batch API, RBAC deny) — degrade
            // gracefully: skip the level rather than failing the whole blast radius.
            Err(_) => continue,
        };
        for item in list.items {
            let is_owned = item
                .metadata
                .owner_references
                .as_ref()
                .is_some_and(|ors| ors.iter().any(|o| owned_uids.contains(&o.uid)));
            if is_owned {
                let item_name = item.metadata.name.clone().unwrap_or_default();
                dependents.push(format!("{kind}/{item_name}"));
                if let Some(child_uid) = item.metadata.uid.clone() {
                    owned_uids.insert(child_uid);
                }
            }
        }
    }

    // Match Pods against any transitively-owned uid (the target's own uid, or an
    // intermediate controller it owns).
    let pods: Api<Pod> = match namespace {
        Some(ns) if !ns.is_empty() => Api::namespaced(client.clone(), ns),
        _ => Api::all(client.clone()),
    };
    let pod_list = pods
        .list(&ListParams::default())
        .await
        .map_err(Error::Api)?;
    for pod in pod_list {
        let has_owner = pod
            .metadata
            .owner_references
            .as_ref()
            .is_some_and(|ors| ors.iter().any(|o| owned_uids.contains(&o.uid)));
        if has_owner {
            dependents.push(format!("Pod/{}", pod.name_any()));
        }
    }

    Ok(BlastRadius {
        namespace: namespace.unwrap_or_default().into(),
        kind: gvk.kind.clone(),
        name: name.into(),
        owners,
        dependents,
    })
}

/// Events between two timestamps (or "the last N minutes"), scoped to a namespace.
#[derive(Debug, Clone)]
pub struct WhatChanged {
    pub namespace: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub events: Vec<crate::events::EventSummary>,
}

/// "What changed between T1 and T2" (or the last `minutes` when timestamps are omitted) —
/// the read-only time-window primitive of the moat.
pub async fn what_changed_between(
    client: &Client,
    namespace: &str,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
    minutes: Option<i64>,
) -> Result<WhatChanged, Error> {
    let to = to_ms.unwrap_or_else(now_ms);
    let from = from_ms.unwrap_or_else(|| to.saturating_sub(minutes.unwrap_or(15) * 60 * 1000));

    let mut events = recent_events(client, Some(namespace), Some(from)).await?;
    events.retain(|e| e.last_timestamp_ms <= to);

    Ok(WhatChanged {
        namespace: namespace.into(),
        from_ms: from,
        to_ms: to,
        events,
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
