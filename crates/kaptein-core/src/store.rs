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
/// `watchring::watch_into_ring`: it seeds the store from a bounded `list`, then applies
/// watch deltas. Returns the number of watch events applied (the seed is not counted).
pub async fn run_informer(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    store: &InformerStore,
    limit: u32,
) -> Result<usize, Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    // 1. Seed from a bounded list (continue-token paging so a large kind is bounded).
    let mut continue_token: Option<String> = None;
    loop {
        let mut lp = ListParams::default().limit(limit.max(1));
        if let Some(token) = &continue_token {
            lp = lp.continue_token(token);
        }
        let page: ObjectList<DynamicObject> = api.list(&lp).await.map_err(Error::Api)?;
        let rv = page.metadata.resource_version.clone();
        let next = page.metadata.continue_.clone();
        store.seed(page, gvk);
        match next {
            Some(next) => continue_token = Some(next),
            None => {
                // Watch from the last page's resource version.
                let rv = rv.unwrap_or_else(|| "0".to_string());
                return watch_from(client, gvk, namespace, store, &rv).await;
            }
        }
    }
}

async fn watch_from(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    store: &InformerStore,
    resource_version: &str,
) -> Result<usize, Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };
    let wp = WatchParams::default();
    let mut stream = Box::pin(api.watch(&wp, resource_version).await.map_err(Error::Api)?);
    let mut applied = 0usize;
    use futures_util::StreamExt;
    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => {
                store.apply(&ev, gvk);
                applied += 1;
            }
            Err(_) => break,
        }
    }
    Ok(applied)
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
}
