//! Recent activity — "what changed in the last N minutes".
//!
//! The cheap form of the time-machine differentiator (M1.4): read the Kubernetes Events
//! API and filter to a time window. No persistence, no compaction — just the watch of
//! what the cluster recorded, which validates the differentiator's behavior a year
//! before the redb-backed time machine exists.

use k8s_openapi::api::core::v1::Event;
use kube::api::ObjectList;
use kube::{Api, Client};

use crate::Error;

/// A display-neutral summary of a cluster event.
#[derive(Debug, Clone)]
pub struct EventSummary {
    pub namespace: String,
    pub kind: String,
    pub name: String,
    pub type_: String,
    pub reason: String,
    pub message: String,
    pub count: i32,
    /// Last occurrence as unix epoch milliseconds.
    pub last_timestamp_ms: i64,
}

/// Fetch events from the given namespace (or all namespaces) and reduce them to
/// summaries, optionally filtering to events newer than `since_ms` (unix epoch millis).
pub async fn recent_events(
    client: &Client,
    namespace: Option<&str>,
    since_ms: Option<i64>,
) -> Result<Vec<EventSummary>, Error> {
    let api: Api<Event> = match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    let list: ObjectList<Event> = api.list(&Default::default()).await.map_err(Error::Api)?;

    let now_ms = now_ms();
    let summaries = list
        .into_iter()
        .filter_map(|e| {
            let ts_ms = e.last_timestamp.map(|t| t.0.as_millisecond());
            // Keep the event if it is newer than `since_ms`; if no timestamp, keep it
            // only when no filter is applied.
            let keep = match (ts_ms, since_ms) {
                (Some(ts), Some(since)) => ts >= since,
                (_, None) => true,
                _ => false,
            };
            keep.then(|| EventSummary {
                namespace: e.metadata.namespace.unwrap_or_default(),
                kind: e.involved_object.kind.unwrap_or_default(),
                name: e.involved_object.name.unwrap_or_default(),
                type_: e.type_.unwrap_or_else(|| "Normal".into()),
                reason: e.reason.unwrap_or_default(),
                message: e.message.unwrap_or_default(),
                count: e.count.unwrap_or(0),
                last_timestamp_ms: ts_ms.unwrap_or(now_ms),
            })
        })
        .collect::<Vec<_>>();

    // Newest first.
    let mut sorted = summaries;
    sorted.sort_by_key(|e| std::cmp::Reverse(e.last_timestamp_ms));
    Ok(sorted)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_events_sorts_newest_first() {
        let mut v = [
            EventSummary {
                namespace: "a".into(),
                kind: "Pod".into(),
                name: "p1".into(),
                type_: "Normal".into(),
                reason: "Started".into(),
                message: "".into(),
                count: 1,
                last_timestamp_ms: 100,
            },
            EventSummary {
                namespace: "a".into(),
                kind: "Pod".into(),
                name: "p2".into(),
                type_: "Warning".into(),
                reason: "Failed".into(),
                message: "".into(),
                count: 1,
                last_timestamp_ms: 200,
            },
        ];
        v.sort_by_key(|e| std::cmp::Reverse(e.last_timestamp_ms));
        assert_eq!(v[0].name, "p2");
        assert_eq!(v[1].name, "p1");
    }
}
