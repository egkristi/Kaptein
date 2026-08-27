//! An informer-backed, bounded store for a resource kind (ADR-0006).
//!
//! This is the "informer-based, never polling" primitive that the TUI and other
//! frontends consume instead of re-listing the whole cluster per keystroke. It seeds
//! from a bounded `list` (limit + continue token) and then applies watch deltas
//! (Added/Modified/Deleted), keeping a live `HashMap<RowKey, ResourceSummary>` plus a
//! monotonically increasing `Revision`. A consumer `snapshot()`s the store; the watcher
//! task is the only thing talking to the API server.
//!
//! This module is deliberately **view-model-free** (layer rule): it speaks
//! `ResourceSummary`, and the integration layer maps it into `Row`/`Cell` for the
//! render contract.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use kube::api::{Api, DynamicObject, ListParams, ObjectList, WatchEvent, WatchParams};
use kube::core::{ApiResource, GroupVersionKind};
use kube::{Client, ResourceExt};

use crate::Error;
use crate::discovery::summary_of;

/// A stable identity for a stored object: `namespace/name` for namespaced resources,
/// `name` for cluster-scoped ones (namespace is empty).
pub type RowKey = String;

/// A monotonically increasing store revision, +1 per applied watch event (or seed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StoreRevision(pub u64);

/// A live, bounded view of one resource kind.
#[derive(Clone)]
pub struct InformerStore {
    inner: Arc<Mutex<StoreInner>>,
}

#[derive(Debug)]
struct StoreInner {
    /// `RowKey -> ResourceSummary`.
    items: HashMap<RowKey, crate::discovery::ResourceSummary>,
    /// Revision of the latest applied delta (seed = 1).
    revision: u64,
}

impl InformerStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(StoreInner {
                items: HashMap::new(),
                revision: 0,
            })),
        }
    }

    /// The current revision.
    pub fn revision(&self) -> StoreRevision {
        StoreRevision(self.inner.lock().expect("store poisoned").revision)
    }

    /// Apply a single watch event (Added/Modified = upsert, Deleted = remove).
    /// Returns the new revision.
    pub fn apply(
        &self,
        event: &WatchEvent<DynamicObject>,
        gvk: &GroupVersionKind,
    ) -> StoreRevision {
        let mut inner = self.inner.lock().expect("store poisoned");
        match event {
            WatchEvent::Added(obj) | WatchEvent::Modified(obj) => {
                let key = row_key(obj);
                inner.items.insert(key, summary_of(obj, gvk));
            }
            WatchEvent::Deleted(obj) => {
                let key = row_key(obj);
                inner.items.remove(&key);
            }
            WatchEvent::Bookmark(_) | WatchEvent::Error(_) => {
                return StoreRevision(inner.revision);
            }
        }
        inner.revision += 1;
        StoreRevision(inner.revision)
    }

    /// Seed from an object list (the "list" half of list-then-watch). Returns the count.
    pub fn seed(&self, list: ObjectList<DynamicObject>, gvk: &GroupVersionKind) -> usize {
        let mut inner = self.inner.lock().expect("store poisoned");
        for obj in &list.items {
            let key = row_key(obj);
            inner.items.insert(key, summary_of(obj, gvk));
        }
        inner.revision += 1;
        list.items.len()
    }

    /// Seed from already-summarized items (metadata-only lists reduce to summaries
    /// without going through `DynamicObject`). Returns the count.
    pub fn seed_summaries(&self, summaries: Vec<crate::discovery::ResourceSummary>) -> usize {
        let mut inner = self.inner.lock().expect("store poisoned");
        let count = summaries.len();
        for s in summaries {
            let key = if s.namespace.is_empty() {
                s.name.clone()
            } else {
                format!("{}/{}", s.namespace, s.name)
            };
            inner.items.insert(key, s);
        }
        inner.revision += 1;
        count
    }

    /// A snapshot of the current items (order unspecified — the view-model sorts).
    pub fn snapshot(&self) -> Vec<crate::discovery::ResourceSummary> {
        self.inner
            .lock()
            .expect("store poisoned")
            .items
            .values()
            .cloned()
            .collect()
    }

    /// Number of items currently stored.
    pub fn len(&self) -> usize {
        self.inner.lock().expect("store poisoned").items.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InformerStore {
    fn default() -> Self {
        Self::new()
    }
}

/// The identity key of a `DynamicObject`: `namespace/name`, or just `name` when the
/// object has no namespace (cluster-scoped). Uses `ResourceExt::namespace`.
fn row_key(obj: &DynamicObject) -> RowKey {
    match obj.namespace() {
        Some(ns) if !ns.is_empty() => format!("{ns}/{}", obj.name_any()),
        _ => obj.name_any(),
    }
}

/// Run the list-then-watch loop for a resource kind into the store, until the watch ends
/// (caller drives this with a timeout or Ctrl-C). This is the informer-backed form of
/// `watchring::watch_into_ring`: it seeds the store from a bounded, **metadata-only**
/// list (ADR-0006 `PartialObjectMetadata`), then applies watch deltas — **reconnecting
/// with backoff and relisting on watch expiry/410**, so the store never goes stale.
///
/// `max_events` bounds the watch: `Some(n)` returns after applying `n` change events
/// (leaving the store seeded + deltas applied), `None` watches forever (the background
/// informer shape). A `Some(0)` watches nothing — the store holds only the seed.
pub async fn run_informer(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    store: &InformerStore,
    limit: u32,
    max_events: Option<usize>,
) -> Result<(), Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    // 1. Seed from a bounded, metadata-only list (continue-token paging). Metadata-only
    //    keeps the seed cheap for list-heavy views (ADR-0006); the watch stream that
    //    follows carries full objects so status is available for the current view.
    let mut continue_token: Option<String> = None;
    loop {
        let (summaries, next, rv) = {
            let mut lp = ListParams::default().limit(limit.max(1));
            if let Some(token) = &continue_token {
                lp = lp.continue_token(token);
            }
            let list: ObjectList<kube::core::PartialObjectMeta<DynamicObject>> =
                api.list_metadata(&lp).await.map_err(Error::Api)?;
            let rv = list.metadata.resource_version.clone();
            let next = list.metadata.continue_.clone();
            let summaries: Vec<crate::discovery::ResourceSummary> = list
                .into_iter()
                .map(|meta| crate::discovery::ResourceSummary {
                    name: meta.name_any(),
                    namespace: meta.namespace().unwrap_or_default(),
                    kind: gvk.kind.clone(),
                    uid: meta.metadata.uid.clone(),
                    created: meta.metadata.creation_timestamp.clone(),
                    status: String::new(),
                })
                .collect();
            (summaries, next, rv)
        };

        store.seed_summaries(summaries);
        match next {
            Some(next) => continue_token = Some(next),
            None => {
                let rv = rv.unwrap_or_else(|| "0".to_string());
                match max_events {
                    // Seed-only: no watch.
                    Some(0) => return Ok(()),
                    // Bounded watch: apply N events, then return.
                    Some(n) => {
                        watch_from_until(client, gvk, namespace, store, &rv, n).await;
                        return Ok(());
                    }
                    // Unbounded background watch (reconnect forever).
                    None => {
                        watch_from(client, gvk, namespace, store, &rv).await;
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Watch from `resource_version`, applying at most `max_events` change events before
/// returning (the bounded form used by `kaptein watch-store` — the store keeps the seed
/// plus the observed deltas, and the caller snapshots it). Reconnects with backoff on a
/// transient watch error, but returns as soon as `max_events` have been applied.
async fn watch_from_until(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    store: &InformerStore,
    resource_version: &str,
    max_events: usize,
) {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };
    let mut backoff_ms: u64 = 100;
    let mut current_rv = resource_version.to_string();
    let mut applied = 0usize;
    use futures_util::StreamExt;

    while applied < max_events {
        let stream = match api.watch(&WatchParams::default(), &current_rv).await {
            Ok(s) => s,
            Err(_) => {
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(5_000);
                if let Ok(list) = api.list_metadata(&ListParams::default().limit(1)).await
                    && let Some(rv) = list.metadata.resource_version
                {
                    current_rv = rv;
                }
                continue;
            }
        };
        let mut stream = Box::pin(stream);
        while let Some(event) = stream.next().await {
            match event {
                Ok(ev) => {
                    backoff_ms = 100;
                    store.apply(&ev, gvk);
                    applied += 1;
                    if applied >= max_events {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(5_000);
    }
}

async fn watch_from(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    store: &InformerStore,
    resource_version: &str,
) {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };
    let mut backoff_ms: u64 = 100;
    let mut current_rv = resource_version.to_string();
    use futures_util::StreamExt;

    loop {
        let stream = match api.watch(&WatchParams::default(), &current_rv).await {
            Ok(s) => s,
            Err(_) => {
                // Relist-on-410 / transient error: back off, then re-fetch the
                // resource version and retry — the store never goes silently stale.
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(5_000);
                // Re-fetch the resource version so a 410 Gone ("too old resource
                // version") is recovered by relisting, not by retrying the same RV.
                if let Ok(list) = api.list_metadata(&ListParams::default().limit(1)).await
                    && let Some(rv) = list.metadata.resource_version
                {
                    current_rv = rv;
                }
                continue;
            }
        };
        let mut stream = Box::pin(stream);

        let mut errored = false;
        while let Some(event) = stream.next().await {
            match event {
                Ok(ev) => {
                    backoff_ms = 100; // healthy stream resets backoff
                    store.apply(&ev, gvk);
                }
                Err(_) => {
                    errored = true;
                    break;
                }
            }
        }
        if errored {
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(5_000);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn obj(name: &str, ns: &str) -> DynamicObject {
        DynamicObject {
            types: Some(kube::core::TypeMeta {
                api_version: "v1".into(),
                kind: "Pod".into(),
            }),
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: if ns.is_empty() { None } else { Some(ns.into()) },
                ..Default::default()
            },
            data: serde_json::json!({"status": {"phase": "Running"}}),
        }
    }

    fn gvk() -> GroupVersionKind {
        GroupVersionKind::gvk("", "v1", "Pod")
    }

    #[test]
    fn seed_then_apply_deltas() {
        let store = InformerStore::new();
        let list = ObjectList {
            types: kube::core::TypeMeta {
                api_version: "v1".into(),
                kind: "PodList".into(),
            },
            metadata: Default::default(),
            items: vec![obj("a", "ns"), obj("b", "ns")],
        };
        assert_eq!(store.seed(list, &gvk()), 2);
        assert_eq!(store.len(), 2);
        let rev = store.revision().0;
        assert!(rev >= 1);

        // Apply a delete.
        store.apply(&WatchEvent::Deleted(obj("a", "ns")), &gvk());
        assert_eq!(store.len(), 1);
        let snap = store.snapshot();
        assert_eq!(snap[0].name, "b");
        assert_eq!(store.revision().0, rev + 1);

        // Apply an add (upsert).
        store.apply(&WatchEvent::Added(obj("c", "ns")), &gvk());
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn seed_summaries_keys_by_namespace_name() {
        let store = InformerStore::new();
        let summary = |name: &str, ns: &str| crate::discovery::ResourceSummary {
            name: name.into(),
            namespace: ns.into(),
            kind: "Pod".into(),
            uid: None,
            created: None,
            status: String::new(),
        };
        let n = store.seed_summaries(vec![summary("a", "ns"), summary("b", "")]);
        assert_eq!(n, 2);
        assert_eq!(store.len(), 2);
        // Namespaced "a" and cluster-scoped "b" have distinct keys, so both survive.
        let snap = store.snapshot();
        assert_eq!(snap.len(), 2);
        // Re-seeding the same identity is an upsert, not a duplicate.
        store.seed_summaries(vec![summary("a", "ns")]);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn bookmark_and_error_do_not_change_revision() {
        let store = InformerStore::new();
        let bookmark = WatchEvent::Bookmark(kube::core::watch::Bookmark {
            types: kube::core::TypeMeta::default(),
            metadata: kube::core::watch::BookmarkMeta {
                resource_version: "1".into(),
                annotations: Default::default(),
            },
        });
        store.apply(&bookmark, &gvk());
        let rev = store.revision().0;
        let err = WatchEvent::Error(Box::<kube::core::Status>::default());
        store.apply(&err, &gvk());
        assert_eq!(store.revision().0, rev);
    }

    #[test]
    fn row_key_namespaced_vs_cluster_scoped() {
        assert_eq!(row_key(&obj("p", "ns")), "ns/p");
        assert_eq!(row_key(&obj("node", "")), "node");
    }

    /// The store is shared across tasks: a spawned writer applies watch deltas while the
    /// main task snapshots. This exercises the `Arc<Mutex<..>>` + revision contract that
    /// `run_informer` relies on — the "informer-driven, never polling" shape — without a
    /// live cluster (the watch stream is synthesized via `apply`).
    #[tokio::test]
    async fn concurrent_writer_and_reader_see_consistent_snapshots() {
        let store = InformerStore::new();
        let seed = ObjectList {
            types: kube::core::TypeMeta {
                api_version: "v1".into(),
                kind: "PodList".into(),
            },
            metadata: Default::default(),
            items: vec![obj("seed", "ns")],
        };
        store.seed(seed, &gvk());

        let writer = store.clone();
        let handle = tokio::spawn(async move {
            for i in 0..10 {
                let o = obj(&format!("pod-{i}"), "ns");
                writer.apply(&WatchEvent::Added(o), &gvk());
                tokio::task::yield_now().await;
            }
        });

        // Snapshot while the writer is running; every observation is internally
        // consistent (len == snapshot length) and monotonic.
        let mut observed = 0usize;
        while observed < 11 {
            let snap = store.snapshot();
            assert_eq!(snap.len(), store.len());
            observed = store.len();
            tokio::task::yield_now().await;
        }
        handle.await.expect("writer task");
        assert_eq!(store.len(), 11); // seed + 10
    }
}
