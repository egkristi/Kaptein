//! Landing view — "is anything broken, and what changed recently?"
//!
//! The cluster-overview surface (M1.5). It composes three data sources:
//!
//! - warning events from the Events API (what is broken / has been failing)
//! - recent activity from the **watch ring buffer** (M1.4) plus the events stream
//!   (what changed)
//! - unhealthy pods from the M1.6 diagnostics engine (what is broken, directly — a pod
//!   that is not ready, with the *why*)
//!
//! The third operator question — "what is about to break" (certificates, quota,
//! capacity) — needs subsystems from Phase 3a and lands later.

use k8s_openapi::api::core::v1::Pod;
use kube::api::ListParams;
use kube::{Api, Client, ResourceExt};

use crate::Error;
use crate::events::{EventSummary, recent_events};
use crate::watchring::ChangeRecord;

/// A composed landing view: the warning events, recent activity, and unhealthy pods.
#[derive(Debug, Clone)]
pub struct Overview {
    /// Warning-type events, newest first (what is broken or failing).
    pub warnings: Vec<EventSummary>,
    /// Total number of events in the window (activity level).
    pub total_events: usize,
    /// Namespaces that produced warning events (deduplicated).
    pub affected_namespaces: Vec<String>,
    /// Recent resource changes from the watch ring (what changed, via the informer).
    pub recent_changes: Vec<ChangeRecord>,
    /// Pods that are not ready, with their diagnostics findings (what is broken, directly).
    pub unhealthy_pods: Vec<UnhealthyPod>,
}

/// A pod that is not ready, plus the M1.6 findings explaining why.
#[derive(Debug, Clone)]
pub struct UnhealthyPod {
    pub namespace: String,
    pub name: String,
    /// The diagnostics findings (e.g. crash_loop_backoff, image_pull, unschedulable).
    pub findings: Vec<String>,
}

/// Build the landing view from a `since_ms` window, optionally combining a snapshot of
/// the watch ring (the M1.4 in-memory "what changed").
pub async fn overview(
    client: &Client,
    namespace: Option<&str>,
    since_ms: i64,
) -> Result<Overview, Error> {
    let events = recent_events(client, namespace, Some(since_ms)).await?;
    Ok(summarize(events, Vec::new(), Vec::new()))
}

/// Build the landing view from the events API **and** the watch ring buffer — the full
/// M1.5 composition ("is anything broken" from events, "what changed" from the ring).
pub async fn overview_with_ring(
    client: &Client,
    namespace: Option<&str>,
    since_ms: i64,
    ring_changes: Vec<ChangeRecord>,
) -> Result<Overview, Error> {
    let events = recent_events(client, namespace, Some(since_ms)).await?;
    Ok(summarize(events, ring_changes, Vec::new()))
}

/// Build the landing view from events + the ring **+ the diagnostics engine**: list the
/// pods and diagnose each, so the overview answers "is anything broken" directly (not
/// just "were there warning events").
pub async fn overview_with_health(
    client: &Client,
    namespace: Option<&str>,
    since_ms: i64,
    ring_changes: Vec<ChangeRecord>,
) -> Result<Overview, Error> {
    let events = recent_events(client, namespace, Some(since_ms)).await?;
    let unhealthy = unhealthy_pods(client, namespace).await.unwrap_or_default();
    Ok(summarize(events, ring_changes, unhealthy))
}

/// List pods and diagnose each, returning only those that are **not ready** with their
/// findings. A best-effort read: on error (e.g. no pod list RBAC) it returns an empty
/// list so the overview degrades gracefully to events-only.
async fn unhealthy_pods(
    client: &Client,
    namespace: Option<&str>,
) -> Result<Vec<UnhealthyPod>, Error> {
    let pods: Api<Pod> = match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    let list = pods
        .list(&ListParams::default())
        .await
        .map_err(Error::Api)?;

    let mut out = Vec::new();
    for pod in list {
        let findings = crate::diagnostics::diagnose(&pod);
        if findings.is_empty() {
            continue;
        }
        out.push(UnhealthyPod {
            namespace: pod.namespace().unwrap_or_default(),
            name: pod.name_any(),
            findings: findings
                .into_iter()
                .map(|f| format!("{}: {}", f.code, f.summary))
                .collect(),
        });
    }
    // Namespace, then name, for a stable presentation.
    out.sort_by(|a, b| {
        (a.namespace.clone(), a.name.clone()).cmp(&(b.namespace.clone(), b.name.clone()))
    });
    Ok(out)
}

/// Pure aggregation over already-fetched events, ring changes, and unhealthy pods
/// (testable without a client).
fn summarize(
    events: Vec<EventSummary>,
    recent_changes: Vec<ChangeRecord>,
    unhealthy_pods: Vec<UnhealthyPod>,
) -> Overview {
    let total_events = events.len();
    let warnings: Vec<EventSummary> = events
        .into_iter()
        .filter(|e| e.type_ == "Warning")
        .collect();
    let mut affected = warnings
        .iter()
        .map(|e| e.namespace.clone())
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>();
    affected.sort();
    affected.dedup();
    Overview {
        warnings,
        total_events,
        affected_namespaces: affected,
        recent_changes,
        unhealthy_pods,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(ns: &str, ty: &str, name: &str) -> EventSummary {
        EventSummary {
            namespace: ns.into(),
            kind: "Pod".into(),
            name: name.into(),
            type_: ty.into(),
            reason: "R".into(),
            message: "".into(),
            count: 1,
            last_timestamp_ms: 100,
        }
    }

    #[test]
    fn summarize_separates_warnings_and_dedups_namespaces() {
        let events = [
            ev("a", "Warning", "p1"),
            ev("a", "Warning", "p2"),
            ev("b", "Normal", "p3"),
        ]
        .into_iter()
        .collect();
        let o = summarize(events, vec![], vec![]);
        assert_eq!(o.total_events, 3);
        assert_eq!(o.warnings.len(), 2);
        assert_eq!(o.affected_namespaces, ["a"]);
        assert!(o.recent_changes.is_empty());
    }

    #[test]
    fn summarize_no_warnings() {
        let events = [ev("b", "Normal", "p3")].into_iter().collect();
        let o = summarize(events, vec![], vec![]);
        assert_eq!(o.total_events, 1);
        assert!(o.warnings.is_empty());
        assert!(o.affected_namespaces.is_empty());
    }

    #[test]
    fn summarize_keeps_recent_changes() {
        let events = [ev("b", "Normal", "p3")].into_iter().collect();
        let changes = vec![crate::watchring::ChangeRecord {
            event: "Added".into(),
            kind: "Pod".into(),
            namespace: "b".into(),
            name: "p4".into(),
            observed_at_ms: 100,
        }];
        let o = summarize(events, changes, vec![]);
        assert_eq!(o.recent_changes.len(), 1);
        assert_eq!(o.recent_changes[0].name, "p4");
    }
}
