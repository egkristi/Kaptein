//! Landing view — "is anything broken, and what changed recently?"
//!
//! The cluster-overview surface (M1.5). It composes two existing data sources:
//!
//! - warning events from the Events API (what is broken / has been failing)
//! - recent activity from the same stream (what changed)
//!
//! The third operator question — "what is about to break" (certificates, quota,
//! capacity) — needs subsystems from Phase 3a and lands later.

use crate::Error;
use crate::events::{EventSummary, recent_events};
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
}

/// Build the landing view from a `since_ms` window.
pub async fn overview(
    client: &Client,
    namespace: Option<&str>,
    since_ms: i64,
) -> Result<Overview, Error> {
    let events = recent_events(client, namespace, Some(since_ms)).await?;
    Ok(summarize(events))
}

/// Pure aggregation over already-fetched events (testable without a client).
fn summarize(events: Vec<EventSummary>) -> Overview {
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
        let o = summarize(events);
        assert_eq!(o.total_events, 3);
        assert_eq!(o.warnings.len(), 2);
        assert_eq!(o.affected_namespaces, ["a"]);
    }

    #[test]
    fn summarize_no_warnings() {
        let events = [ev("b", "Normal", "p3")].into_iter().collect();
        let o = summarize(events);
        assert_eq!(o.total_events, 1);
        assert!(o.warnings.is_empty());
        assert!(o.affected_namespaces.is_empty());
    }
}
