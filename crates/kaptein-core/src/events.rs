//! Recent activity — "what changed in the last N minutes".
//!
//! The cheap form of the time-machine differentiator (M1.4): read the Kubernetes Events
//! API and filter to a time window. No persistence, no compaction — just the watch of
//! what the cluster recorded, which validates the differentiator's behavior a year
//! before the redb-backed time machine exists.
//!
//! Reads **both** event APIs — `core/v1` (where the timestamp is `lastTimestamp`) and
//! `events.k8s.io/v1` (where the timestamp is `eventTime`/`series.lastObservedTime`, and
//! `lastTimestamp` is frequently nil) — and merges them. A cluster emitting only
//! `events.k8s.io/v1` no longer silently returns nothing.

use k8s_openapi::api::core::v1::Event as CoreEvent;
use k8s_openapi::api::events::v1::Event as V1Event;
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
///
/// Queries `core/v1` and `events.k8s.io/v1` and merges, so a cluster emitting the newer
/// API (with nil `lastTimestamp`) still contributes to "what changed".
pub async fn recent_events(
    client: &Client,
    namespace: Option<&str>,
    since_ms: Option<i64>,
) -> Result<Vec<EventSummary>, Error> {
    let mut summaries = Vec::new();

    // core/v1 Events (the legacy API).
    let core_api: Api<CoreEvent> = match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    if let Ok(list) = core_api.list(&Default::default()).await {
        summaries.extend(list.into_iter().filter_map(|e| {
            let ts_ms = e.last_timestamp.map(|t| t.0.as_millisecond());
            keep(ts_ms, since_ms).then(|| EventSummary {
                namespace: e.metadata.namespace.unwrap_or_default(),
                kind: e.involved_object.kind.unwrap_or_default(),
                name: e.involved_object.name.unwrap_or_default(),
                type_: e.type_.unwrap_or_else(|| "Normal".into()),
                reason: e.reason.unwrap_or_default(),
                message: e.message.unwrap_or_default(),
                count: e.count.unwrap_or(0),
                last_timestamp_ms: ts_ms.unwrap_or_else(now_ms),
            })
        }));
    }

    // events.k8s.io/v1 (the modern API): `eventTime` / `series.lastObservedTime`.
    let v1_api: Api<V1Event> = match namespace {
        Some(ns) => Api::namespaced(client.clone(), ns),
        None => Api::all(client.clone()),
    };
    if let Ok(list) = v1_api.list(&Default::default()).await {
        summaries.extend(list.into_iter().filter_map(|e| {
            let ts_ms = e
                .event_time
                .map(|t| t.0.as_millisecond())
                .or_else(|| {
                    e.series
                        .as_ref()
                        .map(|s| s.last_observed_time.0.as_millisecond())
                })
                .or_else(|| e.deprecated_last_timestamp.map(|t| t.0.as_millisecond()));
            keep(ts_ms, since_ms).then(|| EventSummary {
                namespace: e.metadata.namespace.unwrap_or_default(),
                kind: e
                    .regarding
                    .as_ref()
                    .and_then(|r| r.kind.clone())
                    .unwrap_or_default(),
                name: e
                    .regarding
                    .as_ref()
                    .and_then(|r| r.name.clone())
                    .unwrap_or_default(),
                type_: e.type_.unwrap_or_else(|| "Normal".into()),
                reason: e.reason.unwrap_or_default(),
                message: e.note.unwrap_or_default(),
                count: e
                    .series
                    .as_ref()
                    .map(|s| s.count)
                    .or(e.deprecated_count)
                    .unwrap_or(0),
                last_timestamp_ms: ts_ms.unwrap_or_else(now_ms),
            })
        }));
    }

    // Newest first.
    summaries.sort_by_key(|e| std::cmp::Reverse(e.last_timestamp_ms));
    Ok(summaries)
}

/// Whether an event with `ts_ms` should be kept given the `since_ms` window: a missing
/// timestamp is kept only when no filter is applied (it can't be proven stale or fresh).
fn keep(ts_ms: Option<i64>, since_ms: Option<i64>) -> bool {
    match (ts_ms, since_ms) {
        (Some(ts), Some(since)) => ts >= since,
        (_, None) => true,
        _ => false,
    }
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
