//! Watch ring buffer — "what changed in the last N minutes" (M1.4).
//!
//! The *cheap form* of the time-machine differentiator: an in-memory, bounded ring of
//! resource changes derived from the **watch stream** (not polling, not persistence).
//! Each `WatchEvent` (Added/Modified/Deleted) is reduced to a compact `ChangeRecord` and
//! pushed into a fixed-capacity buffer; old records are evicted once the capacity is
//! reached. No redb, no compaction, no disk — this validates the differentiator's shape
//! a year before the storage subsystem exists.
//!
//! The ring buffer is `Send + Sync + Clone` (shared handle), so it can be driven by a
//! spawned watch task and read concurrently by the landing view and any frontend.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Api, WatchEvent, WatchParams};
use kube::core::DynamicObject;
use kube::{Client, ResourceExt};

use crate::Error;

/// A compact, display-neutral record of a single resource change.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeRecord {
    /// The event kind: `Added`, `Modified`, or `Deleted`.
    pub event: String,
    /// The resource kind (e.g. `Pod`, `Deployment`).
    pub kind: String,
    /// The resource namespace (empty for cluster-scoped).
    pub namespace: String,
    /// The resource name.
    pub name: String,
    /// When the change was observed (wall clock, for the "last N minutes" window).
    pub observed_at_ms: i64,
}

/// A bounded, in-memory ring of the most recent `ChangeRecord`s.
///
/// `capacity` is the maximum number of records retained; pushes beyond that evict the
/// oldest record first.
#[derive(Clone)]
pub struct WatchRing {
    inner: Arc<Mutex<VecDeque<ChangeRecord>>>,
    capacity: usize,
}

impl WatchRing {
    /// Create an empty ring with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity))),
            capacity: capacity.max(1),
        }
    }

    /// Push a change record, evicting the oldest if at capacity. Returns the number of
    /// records now held.
    pub fn push(&self, record: ChangeRecord) -> usize {
        let mut buf = self.inner.lock().expect("watch ring poisoned");
        if buf.len() >= self.capacity {
            buf.pop_front();
        }
        buf.push_back(record);
        buf.len()
    }

    /// Snapshot the current records, oldest first.
    pub fn snapshot(&self) -> Vec<ChangeRecord> {
        self.inner
            .lock()
            .expect("watch ring poisoned")
            .iter()
            .cloned()
            .collect()
    }

    /// Records with `observed_at_ms >= since_ms` (the "last N minutes" window).
    pub fn since(&self, since_ms: i64) -> Vec<ChangeRecord> {
        self.snapshot()
            .into_iter()
            .filter(|r| r.observed_at_ms >= since_ms)
            .collect()
    }

    /// Number of records currently held.
    pub fn len(&self) -> usize {
        self.inner.lock().expect("watch ring poisoned").len()
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Reduce a single `WatchEvent<DynamicObject>` into a `ChangeRecord`.
pub fn reduce_event(event: WatchEvent<DynamicObject>, observed_at_ms: i64) -> Option<ChangeRecord> {
    let (event_name, obj) = match event {
        WatchEvent::Added(o) => ("Added", o),
        WatchEvent::Modified(o) => ("Modified", o),
        WatchEvent::Deleted(o) => ("Deleted", o),
        // Bookmarks and errors carry no resource change.
        WatchEvent::Bookmark(_) | WatchEvent::Error(_) => return None,
    };
    let kind = obj
        .types
        .as_ref()
        .map(|t| t.kind.clone())
        .unwrap_or_default();
    Some(ChangeRecord {
        event: event_name.to_string(),
        kind,
        namespace: obj.namespace().unwrap_or_default(),
        name: obj.name_any(),
        observed_at_ms,
    })
}

/// Start a watch on a `group/version/kind` and feed `ChangeRecord`s into the ring until
/// the watch ends (the caller drives this with a timeout or Ctrl-C). This is the
/// informer-based "cheap what-changed" primitive — no polling, no persistence.
///
/// The initial state is captured via a `list` (reduced to `Added` records), then a watch
/// from that list's resource version streams deltas. Returns the number of records pushed.
pub async fn watch_into_ring(
    client: &Client,
    gvk: &kube::core::GroupVersionKind,
    namespace: Option<&str>,
    ring: &WatchRing,
    max_events: usize,
) -> Result<usize, Error> {
    let ar = kube::core::ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let mut pushed = 0usize;

    // 1. Capture the current state as `Added` records (the "what exists now" baseline).
    let list = api.list(&Default::default()).await.map_err(Error::Api)?;
    let rv = list
        .metadata
        .resource_version
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let observed_at_ms = now_ms();
    for obj in list.items {
        let record = ChangeRecord {
            event: "Added".to_string(),
            kind: obj
                .types
                .as_ref()
                .map(|t| t.kind.clone())
                .unwrap_or_else(|| gvk.kind.clone()),
            namespace: obj.namespace().unwrap_or_default(),
            name: obj.name_any(),
            observed_at_ms,
        };
        ring.push(record);
        pushed += 1;
        if pushed >= max_events {
            return Ok(pushed);
        }
    }

    // 2. Watch for deltas from the list's resource version.
    let wp = WatchParams::default();
    let mut stream = Box::pin(api.watch(&wp, &rv).await.map_err(Error::Api)?);

    use futures_util::StreamExt;
    while let Some(event) = stream.next().await {
        let observed_at_ms = now_ms();
        match event {
            Ok(ev) => {
                if let Some(record) = reduce_event(ev, observed_at_ms) {
                    ring.push(record);
                    pushed += 1;
                }
            }
            Err(_) => break,
        }
        if pushed >= max_events {
            break;
        }
    }
    Ok(pushed)
}

/// Capture the current state of a resource kind into the ring **without** waiting on the
/// watch stream — the non-blocking "snapshot now" form. Useful for the landing view,
/// which wants "what exists now" immediately rather than a blocking watch.
pub async fn snapshot_into_ring(
    client: &Client,
    gvk: &kube::core::GroupVersionKind,
    namespace: Option<&str>,
    ring: &WatchRing,
    max_events: usize,
) -> Result<usize, Error> {
    let ar = kube::core::ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let list = api.list(&Default::default()).await.map_err(Error::Api)?;
    let observed_at_ms = now_ms();
    let mut pushed = 0usize;
    for obj in list.items {
        let record = ChangeRecord {
            event: "Added".to_string(),
            kind: obj
                .types
                .as_ref()
                .map(|t| t.kind.clone())
                .unwrap_or_else(|| gvk.kind.clone()),
            namespace: obj.namespace().unwrap_or_default(),
            name: obj.name_any(),
            observed_at_ms,
        };
        ring.push(record);
        pushed += 1;
        if pushed >= max_events {
            break;
        }
    }
    Ok(pushed)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A helper to build a `ChangeRecord` from a raw timestamp (used by tests).
#[allow(dead_code)]
fn record_with_time(
    event: &str,
    kind: &str,
    namespace: &str,
    name: &str,
    t: Option<Time>,
) -> ChangeRecord {
    let ms = t.map(|t| t.0.as_millisecond()).unwrap_or(0);
    ChangeRecord {
        event: event.to_string(),
        kind: kind.to_string(),
        namespace: namespace.to_string(),
        name: name.to_string(),
        observed_at_ms: ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_evicts_oldest_at_capacity() {
        let ring = WatchRing::new(3);
        for i in 0..5 {
            ring.push(ChangeRecord {
                event: "Added".into(),
                kind: "Pod".into(),
                namespace: "ns".into(),
                name: format!("pod-{i}"),
                observed_at_ms: i,
            });
        }
        let snap = ring.snapshot();
        assert_eq!(snap.len(), 3);
        // The oldest two (0, 1) were evicted.
        assert_eq!(snap[0].name, "pod-2");
        assert_eq!(snap[2].name, "pod-4");
    }

    #[test]
    fn since_filters_by_window() {
        let ring = WatchRing::new(10);
        for ms in [100, 200, 300, 400] {
            ring.push(ChangeRecord {
                event: "Added".into(),
                kind: "Pod".into(),
                namespace: "ns".into(),
                name: format!("t{ms}"),
                observed_at_ms: ms,
            });
        }
        let recent = ring.since(300);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].name, "t300");
    }

    #[test]
    fn reduce_event_ignores_bookmarks_and_errors() {
        use kube::core::response::Status;
        use kube::core::watch::{Bookmark, BookmarkMeta};
        let bookmark = WatchEvent::Bookmark(Bookmark {
            types: kube::core::TypeMeta::default(),
            metadata: BookmarkMeta {
                resource_version: "1".into(),
                annotations: Default::default(),
            },
        });
        assert!(reduce_event(bookmark, 0).is_none());
        let err = WatchEvent::Error(Box::<Status>::default());
        assert!(reduce_event(err, 0).is_none());
    }

    #[test]
    fn reduce_event_keeps_added_modified_deleted() {
        let obj = DynamicObject {
            types: None,
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some("p".into()),
                namespace: Some("n".into()),
                ..Default::default()
            },
            data: serde_json::Value::Null,
        };
        let rec = reduce_event(WatchEvent::Added(obj), 42).unwrap();
        assert_eq!(rec.event, "Added");
        assert_eq!(rec.name, "p");
        assert_eq!(rec.namespace, "n");
        assert_eq!(rec.observed_at_ms, 42);
    }
}
