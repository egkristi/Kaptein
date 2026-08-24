//! Landing view — "is anything broken, and what changed recently?"
//!
//! The cluster-overview surface (M1.5). It composes two data sources:
//!
//! - warning events from the Events API (what is broken / has been failing)
//! - recent activity from the **watch ring buffer** (M1.4) plus the events stream
//!   (what changed)
//!
//! The third operator question — "what is about to break" (certificates, quota,
//! capacity) — needs subsystems from Phase 3a and lands later.

use crate::Error;
use crate::events::{EventSummary, recent_events};
use crate::watchring::ChangeRecord;
use kube::Client;

/// A composed landing view: the warning events and a count of recent activity.
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
}

/// Build the landing view from a `since_ms` window, optionally combining a snapshot of
/// the watch ring (the M1.4 in-memory "what changed").
pub async fn overview(
    client: &Client,
    namespace: Option<&str>,
    since_ms: i64,
) -> Result<Overview, Error> {
    let events = recent_events(client, namespace, Some(since_ms)).await?;
    Ok(summarize(events, Vec::new()))
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
    Ok(summarize(events, ring_changes))
}

/// Pure aggregation over already-fetched events and ring changes (testable without a
/// client).
fn summarize(events: Vec<EventSummary>, recent_changes: Vec<ChangeRecord>) -> Overview {
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
        let o = summarize(events, vec![]);
        assert_eq!(o.total_events, 3);
        assert_eq!(o.warnings.len(), 2);
        assert_eq!(o.affected_namespaces, ["a"]);
        assert!(o.recent_changes.is_empty());
    }

    #[test]
    fn summarize_no_warnings() {
        let events = [ev("b", "Normal", "p3")].into_iter().collect();
        let o = summarize(events, vec![]);
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
        let o = summarize(events, changes);
        assert_eq!(o.recent_changes.len(), 1);
        assert_eq!(o.recent_changes[0].name, "p4");
    }
}
