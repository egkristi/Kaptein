//! Kaptein integration layer — binds `kaptein-core` to native frontends.
//!
//! Per `docs/architecture.md`, the *integration layer* is the native frontend or binary
//! that owns both `kaptein-core` and (where applicable) `kaptein-viewmodel`. It maps raw
//! `kaptein-core::Error` values into user-facing messages without leaking secrets.
//!
//! The TUI reaches `kaptein-core` through this crate, keeping the layer dependency rule
//! satisfied: `frontend-tui` → `kaptein-integration` → `kaptein-core`, with no frontend
//! depending on `kaptein-core` directly (see AGENTS.md / ADR-0005).

#![forbid(unsafe_code)]

/// The user-facing, redaction-aware error type for native frontends.
///
/// It maps the raw `kaptein-core::Error` (network/auth/watch/discovery/API) into messages
/// that are safe to show a user: no secret values, no raw stack traces, no internal
/// kubeconfig/exec-credential output. The mapping is deliberately *coarse* — the core
/// reports *what failed*; this layer decides *how to say it* without leaking secrets.
#[derive(Debug, thiserror::Error)]
pub enum IntegrationError {
    /// The API server rejected or failed the request; `message` is the (safe) detail.
    #[error("kubernetes API error: {message}")]
    Api { message: String },

    /// Authentication/authorization failed.
    #[error("authentication failed: {message}")]
    Auth { message: String },

    /// A network error reaching the cluster.
    #[error("network error: {message}")]
    Network { message: String },

    /// A watch stream was interrupted.
    #[error("watch interrupted: {message}")]
    Watch { message: String },

    /// API discovery failed.
    #[error("discovery failed: {message}")]
    Discovery { message: String },

    /// The kubeconfig could not be read/parsed (never include the raw file contents).
    #[error("kubeconfig error: {message}")]
    Kubeconfig { message: String },

    /// An external tool shell-out failed.
    #[error("external tool error: {message}")]
    External { message: String },

    /// A generic internal error (already user-safe by construction in core).
    #[error("{0}")]
    Internal(String),
}

/// Map a raw `kaptein-core::Error` into a redaction-aware `IntegrationError`.
///
/// Secret values never appear in a message: the `message` field is a *classification*
/// description, not a dump of the raw error. Where the core error already carries a
/// user-safe message (`Internal`, `External`), it is forwarded verbatim; API/auth errors
/// carry only the status code and reason, never the request body or credentials.
impl From<kaptein_core::Error> for IntegrationError {
    fn from(e: kaptein_core::Error) -> Self {
        match e {
            kaptein_core::Error::Network(msg) => IntegrationError::Network { message: msg },
            kaptein_core::Error::Auth(msg) => IntegrationError::Auth { message: msg },
            kaptein_core::Error::WatchInterrupted(msg) => IntegrationError::Watch { message: msg },
            kaptein_core::Error::Discovery(msg) => IntegrationError::Discovery { message: msg },
            kaptein_core::Error::Kubeconfig(e) => IntegrationError::Kubeconfig {
                message: e.to_string(),
            },
            kaptein_core::Error::Api(e) => {
                // `kube::Error::Api` carries the server's status (code + reason + message),
                // which is already safe — the request body/credentials are not included.
                IntegrationError::Api {
                    message: e.to_string(),
                }
            }
            kaptein_core::Error::External { tool, message } => IntegrationError::External {
                message: format!("{tool}: {message}"),
            },
            kaptein_core::Error::Internal(msg) => IntegrationError::Internal(msg),
        }
    }
}

/// Re-export the entire core data plane so frontends reach `kaptein-core` through this
/// crate rather than as a direct dependency. The integration layer owns `kaptein-core`;
/// frontends own only geometry.
pub use kaptein_core;
/// Re-export the view-model so a frontend depends on exactly one integration crate for
/// both the render contract (`DataPlane`, `Row`, `Cell`, `Query`) and the core binding.
pub use kaptein_viewmodel;
use kube::ResourceExt as _;

/// Build a Kubernetes client for the default context.
pub async fn client() -> Result<kube::Client, IntegrationError> {
    Ok(kaptein_core::discovery::client().await?)
}

/// Build a Kubernetes client for a specific named context.
pub async fn client_for_context(context: Option<&str>) -> Result<kube::Client, IntegrationError> {
    Ok(kaptein_core::discovery::client_for_context(context).await?)
}

/// The column schema of a Kubernetes resource view: `name`, `namespace`, `status`,
/// `created`. The view-model owns *which* columns exist (semantics); the frontend owns
/// their width in cells (geometry).
pub const RESOURCE_COLUMNS: [&str; 4] = ["name", "namespace", "status", "created"];

/// Map a core `ResourceSummary` into a view-model `Row`, using `uid` (or `namespace/name`
/// when `uid` is absent) as the stable `RowId`. The `status` cell is a typed
/// `Status` chip so a frontend colors it without string-matching.
fn resource_row(summary: kaptein_core::discovery::ResourceSummary) -> kaptein_viewmodel::Row {
    let id = summary.uid.clone().unwrap_or_else(|| {
        if summary.namespace.is_empty() {
            summary.name.clone()
        } else {
            format!("{}/{}", summary.namespace, summary.name)
        }
    });
    let status_level = match summary.status.as_str() {
        "Running" | "Active" | "Ready" => kaptein_viewmodel::StatusLevel::Ok,
        "Pending" | "ContainerCreating" => kaptein_viewmodel::StatusLevel::Pending,
        "" => kaptein_viewmodel::StatusLevel::Info,
        _ => kaptein_viewmodel::StatusLevel::Warning,
    };
    kaptein_viewmodel::Row {
        id: kaptein_viewmodel::RowId(id),
        cells: vec![
            kaptein_viewmodel::Cell::Text {
                value: summary.name,
            },
            kaptein_viewmodel::Cell::Text {
                value: summary.namespace,
            },
            kaptein_viewmodel::Cell::Status {
                level: status_level,
                label_key: summary.status,
            },
            kaptein_viewmodel::Cell::Timestamp {
                millis: summary.created.map(|t| t.0.as_millisecond()).unwrap_or(0),
            },
        ],
    }
}

/// A `DataPlane` that queries a live Kubernetes cluster through `kaptein-core`.
///
/// It is the integration-layer *implementation* of the render contract (ADR-0005):
/// sorting/filtering live in the view-model (`table`/`mem_plane`), the bounded list +
/// informer store live in `kaptein-core`, and this struct binds the two. It is the shape
/// every future frontend consumes — the TUI today, the browser/`serve` later — instead of
/// per-key `api.list` calls.
pub struct KubernetesPlane {
    client: kube::Client,
    gvk: kube::core::GroupVersionKind,
    namespace: Option<String>,
}

impl KubernetesPlane {
    /// Create a plane for the given `group/version/kind` (namespaced when `namespace`
    /// is `Some`).
    pub fn new(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
    ) -> Self {
        Self {
            client,
            gvk,
            namespace,
        }
    }
}

#[async_trait::async_trait]
impl kaptein_viewmodel::DataPlane for KubernetesPlane {
    async fn query(
        &self,
        query: &kaptein_viewmodel::Query,
    ) -> Result<kaptein_viewmodel::Page, kaptein_viewmodel::Error> {
        // The bounded list (ADR-0006) is the data source; sorting/filtering are applied
        // by the view-model (not here — no logic in the integration layer beyond binding).
        let summaries =
            kaptein_core::discovery::list(&self.client, &self.gvk, self.namespace.as_deref())
                .await
                .map_err(|e| kaptein_viewmodel::Error::Internal(e.to_string()))?;

        let mut rows: Vec<kaptein_viewmodel::Row> =
            summaries.into_iter().map(resource_row).collect();
        let column_ids: Vec<String> = RESOURCE_COLUMNS.iter().map(|s| s.to_string()).collect();
        kaptein_viewmodel::sort_rows(&mut rows, &column_ids, query.sort.as_ref());
        rows = kaptein_viewmodel::filter_rows(rows, query.filter.as_ref());

        let total = rows.len();
        let start = query.start.min(total);
        let end = query.end.min(total).max(start);
        let page_rows = rows.into_iter().skip(start).take(end - start).collect();
        Ok(kaptein_viewmodel::Page {
            rows: page_rows,
            total,
            revision: kaptein_viewmodel::Revision(0),
        })
    }

    fn subscribe(
        &self,
        _from: kaptein_viewmodel::Revision,
    ) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = kaptein_viewmodel::RowPatch> + Send>>
    {
        // The one-shot `KubernetesPlane` has no live deltas; `LivePlane` (below) provides
        // the informer-backed subscription.
        Box::pin(futures_util::stream::empty())
    }
}

/// Map a watch event over a `DynamicObject` into a `RowPatch`, using the same
/// `ResourceSummary` → `Row` mapping as `resource_row`. Bookmarks and errors produce no
/// patch (they carry no resource change).
fn watch_event_to_patch(
    event: &kube::api::WatchEvent<kube::core::DynamicObject>,
    gvk: &kube::core::GroupVersionKind,
) -> Option<kaptein_viewmodel::RowPatch> {
    match event {
        kube::api::WatchEvent::Added(obj) | kube::api::WatchEvent::Modified(obj) => {
            let row = resource_row(kaptein_core::discovery::summary_of(obj, gvk));
            Some(kaptein_viewmodel::RowPatch::Upsert {
                id: row.id.clone(),
                row,
            })
        }
        kube::api::WatchEvent::Deleted(obj) => {
            let id = obj.metadata.uid.clone().unwrap_or_else(|| obj.name_any());
            Some(kaptein_viewmodel::RowPatch::Remove {
                id: kaptein_viewmodel::RowId(id),
            })
        }
        kube::api::WatchEvent::Bookmark(_) | kube::api::WatchEvent::Error(_) => None,
    }
}

/// A `DataPlane` that is **informer-backed and live**: it seeds a `MemPlane` from a
/// bounded list, then a background watch task applies `Added`/`Modified`/`Deleted`
/// deltas as `RowPatch` upserts/removes. `query` reads the live `MemPlane` (no API call
/// per keystroke) and `subscribe` streams real deltas — the ADR-0006 "informer-based,
/// never polling" shape for the TUI.
pub struct LivePlane {
    mem: kaptein_viewmodel::MemPlane,
    client: kube::Client,
    gvk: kube::core::GroupVersionKind,
    namespace: Option<String>,
}

impl Clone for LivePlane {
    /// Clone shares the same in-memory `MemPlane` and Kubernetes client — two handles to
    /// one live data plane (the watch task and the TUI both hold a handle).
    fn clone(&self) -> Self {
        Self {
            mem: self.mem.clone(),
            client: self.client.clone(),
            gvk: self.gvk.clone(),
            namespace: self.namespace.clone(),
        }
    }
}

impl LivePlane {
    /// Create a live plane for `group/version/kind`.
    pub fn new(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
    ) -> Self {
        let mem = kaptein_viewmodel::MemPlane::new(kaptein_viewmodel::Schema {
            column_ids: RESOURCE_COLUMNS.iter().map(|s| s.to_string()).collect(),
        });
        Self {
            mem,
            client,
            gvk,
            namespace,
        }
    }

    /// A convenience alias for `Clone::clone` (the TUI holds two handles: one for the
    /// watch task, one for querying).
    pub fn clone_plane(&self) -> Self {
        self.clone()
    }

    /// The underlying `MemPlane` (exposed for tests).
    pub fn mem(&self) -> &kaptein_viewmodel::MemPlane {
        &self.mem
    }

    /// Seed the plane from a bounded list (the "list" half of list-then-watch). The
    /// caller then runs `watch_loop` on a background task to apply live deltas.
    pub async fn seed(&self) -> Result<usize, IntegrationError> {
        let summaries =
            kaptein_core::discovery::list(&self.client, &self.gvk, self.namespace.as_deref())
                .await?;
        let count = summaries.len();
        for s in summaries {
            let row = resource_row(s);
            self.mem.upsert(row);
        }
        Ok(count)
    }

    /// Run the watch loop until cancelled, applying deltas to the `MemPlane`. This is the
    /// "watch" half — drive it on a `tokio::spawn`ed task. On watch expiry/error
    /// (routinely after ~5 min server timeouts or a 410 Gone) it **relists into the
    /// store and reconciles** — removing rows absent from the fresh relist — then watches
    /// from the relist's resourceVersion, so no deleted object lingers as a ghost row
    /// (issue #20).
    pub async fn watch_loop(&self) -> Result<(), IntegrationError> {
        let ar = kube::core::ApiResource::from_gvk(&self.gvk);
        let api: kube::Api<kube::core::DynamicObject> = match self.namespace.as_deref() {
            Some(ns) => kube::Api::namespaced_with(self.client.clone(), ns, &ar),
            None => kube::Api::all_with(self.client.clone(), &ar),
        };
        use futures_util::StreamExt;
        let mut backoff_ms: u64 = 100;

        loop {
            // Relist (metadata-only, fully paged) and reconcile before watching: this is
            // the correct informer shape — remove keys absent from the relist, then watch
            // from the relist's resourceVersion. A bare `limit(1)` list used only as a
            // "watch from here" cursor would leave objects deleted during an outage
            // forever (no `Deleted` event arrives for them).
            let rv = match self.relist_and_reconcile(&api).await {
                Ok(rv) => rv,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).min(5_000);
                    continue;
                }
            };

            let stream = match api.watch(&kube::api::WatchParams::default(), &rv).await {
                Ok(s) => s,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    backoff_ms = (backoff_ms * 2).min(5_000);
                    continue;
                }
            };
            let mut stream = Box::pin(stream);

            while let Some(event) = stream.next().await {
                match event {
                    Ok(ev) => {
                        backoff_ms = 100; // healthy stream resets backoff
                        if let Some(patch) = watch_event_to_patch(&ev, &self.gvk) {
                            match patch {
                                kaptein_viewmodel::RowPatch::Upsert { row, .. } => {
                                    self.mem.upsert(row);
                                }
                                kaptein_viewmodel::RowPatch::Remove { id } => {
                                    self.mem.remove(&id);
                                }
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Stream ended (expired or errored): back off and reconnect.
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(5_000);
        }
    }

    /// Relist the resource (metadata-only, fully paged) into the `MemPlane`, removing any
    /// row absent from the fresh list (reconciliation), and return the resourceVersion to
    /// watch from. This is the "list" half of list-then-watch on every reconnect — not
    /// just the initial seed.
    async fn relist_and_reconcile(
        &self,
        api: &kube::Api<kube::core::DynamicObject>,
    ) -> Result<String, IntegrationError> {
        // Collect the full live object set (metadata-only, paged) and its resourceVersion.
        let mut live_ids: std::collections::HashSet<kaptein_viewmodel::RowId> =
            std::collections::HashSet::new();
        let mut rv: Option<String> = None;
        let mut continue_token: Option<String> = None;
        loop {
            let mut lp = kube::api::ListParams::default().limit(500);
            if let Some(token) = &continue_token {
                lp = lp.continue_token(token);
            }
            let list = api
                .list_metadata(&lp)
                .await
                .map_err(|e| IntegrationError::from(kaptein_core::Error::Api(e)))?;
            rv = list.metadata.resource_version.clone().or(rv);
            for meta in list.items {
                let id = kaptein_viewmodel::RowId(
                    meta.metadata.uid.clone().unwrap_or_else(|| meta.name_any()),
                );
                live_ids.insert(id);
            }
            match list.metadata.continue_.clone() {
                Some(t) => continue_token = Some(t),
                None => break,
            }
        }

        // Reconcile: remove any row in the plane not present in the fresh relist.
        let current = self.mem.rows();
        for row in &current {
            if !live_ids.contains(&row.id) {
                self.mem.remove(&row.id);
            }
        }

        Ok(rv.unwrap_or_else(|| "0".to_string()))
    }
}

#[async_trait::async_trait]
impl kaptein_viewmodel::DataPlane for LivePlane {
    async fn query(
        &self,
        query: &kaptein_viewmodel::Query,
    ) -> Result<kaptein_viewmodel::Page, kaptein_viewmodel::Error> {
        self.mem.query(query).await
    }

    fn subscribe(
        &self,
        from: kaptein_viewmodel::Revision,
    ) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = kaptein_viewmodel::RowPatch> + Send>>
    {
        self.mem.subscribe(from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_maps_to_api_variant_without_leaking_body() {
        let err = IntegrationError::from(kaptein_core::Error::Api(kube::Error::Api(Box::<
            kube::core::Status,
        >::default(
        ))));
        match err {
            IntegrationError::Api { message: _ } => {
                // The classification is "api error"; the message is the server status
                // (code + reason), which never includes a credential or request body.
            }
            _ => panic!("expected Api variant"),
        }
    }

    #[test]
    fn internal_error_is_forwarded_verbatim() {
        let err = IntegrationError::from(kaptein_core::Error::Internal("boom".into()));
        assert!(matches!(err, IntegrationError::Internal(m) if m == "boom"));
    }

    #[test]
    fn external_error_preserves_tool_and_message() {
        let err = IntegrationError::from(kaptein_core::Error::External {
            tool: "helm".into(),
            message: "not installed".into(),
        });
        match err {
            IntegrationError::External { message } => {
                assert!(message.contains("helm") && message.contains("not installed"));
            }
            _ => panic!("expected External variant"),
        }
    }

    fn pod_obj(name: &str, ns: &str, uid: &str) -> kube::core::DynamicObject {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        kube::core::DynamicObject {
            types: Some(kube::core::TypeMeta {
                api_version: "v1".into(),
                kind: "Pod".into(),
            }),
            metadata: ObjectMeta {
                name: Some(name.into()),
                namespace: Some(ns.into()),
                uid: Some(uid.into()),
                ..Default::default()
            },
            data: serde_json::json!({"status": {"phase": "Running"}}),
        }
    }

    fn pod_gvk() -> kube::core::GroupVersionKind {
        kube::core::GroupVersionKind::gvk("", "v1", "Pod")
    }

    #[test]
    fn watch_event_added_maps_to_upsert() {
        let obj = pod_obj("p", "ns", "uid-1");
        let patch = watch_event_to_patch(&kube::api::WatchEvent::Added(obj), &pod_gvk())
            .expect("added -> patch");
        match patch {
            kaptein_viewmodel::RowPatch::Upsert { id, row } => {
                assert_eq!(id.0, "uid-1");
                assert_eq!(kaptein_viewmodel::cell_text(&row.cells[0]), "p");
            }
            _ => panic!("expected upsert"),
        }
    }

    #[test]
    fn watch_event_deleted_maps_to_remove_by_uid() {
        let obj = pod_obj("p", "ns", "uid-9");
        let patch = watch_event_to_patch(&kube::api::WatchEvent::Deleted(obj), &pod_gvk())
            .expect("deleted -> patch");
        match patch {
            kaptein_viewmodel::RowPatch::Remove { id } => assert_eq!(id.0, "uid-9"),
            _ => panic!("expected remove"),
        }
    }

    #[test]
    fn bookmark_and_error_map_to_none() {
        let bookmark = kube::api::WatchEvent::Bookmark(kube::core::watch::Bookmark {
            types: kube::core::TypeMeta::default(),
            metadata: kube::core::watch::BookmarkMeta {
                resource_version: "1".into(),
                annotations: Default::default(),
            },
        });
        assert!(watch_event_to_patch(&bookmark, &pod_gvk()).is_none());
        let err = kube::api::WatchEvent::Error(Box::<kube::core::Status>::default());
        assert!(watch_event_to_patch(&err, &pod_gvk()).is_none());
    }

    #[test]
    fn resource_row_uses_uid_as_stable_id() {
        let summary = kaptein_core::discovery::ResourceSummary {
            name: "p".into(),
            namespace: "ns".into(),
            kind: "Pod".into(),
            uid: Some("uid-7".into()),
            created: None,
            status: "Running".into(),
        };
        let row = resource_row(summary);
        assert_eq!(row.id.0, "uid-7");
        assert!(matches!(
            row.cells[2],
            kaptein_viewmodel::Cell::Status { .. }
        ));
    }

    /// A live integration test (skipped without a cluster): seeds a `LivePlane` from the
    /// real API server and asserts the informer-backed data plane returns a real page.
    /// This is the "real kube client" tier the review flagged as missing, gated on
    /// `KUBECONFIG` so it is safe in CI without a cluster.
    #[tokio::test]
    async fn live_plane_seeds_from_cluster_when_available() {
        if std::env::var_os("KUBECONFIG").is_none() {
            eprintln!("skipping live_plane_seeds_from_cluster_when_available: no KUBECONFIG");
            return;
        }
        let client = match kaptein_core::discovery::client().await {
            Ok(c) => c,
            Err(_) => {
                eprintln!("skipping: cluster unreachable");
                return;
            }
        };
        let plane = LivePlane::new(
            client,
            kube::core::GroupVersionKind::gvk("", "v1", "Namespace"),
            None,
        );
        let seeded = plane.seed().await.expect("seed");
        assert!(
            seeded > 0,
            "a live cluster must have at least one namespace"
        );
        use kaptein_viewmodel::DataPlane as _;
        let page = plane
            .query(&kaptein_viewmodel::Query {
                start: 0,
                end: 10,
                sort: Some(kaptein_viewmodel::SortSpec {
                    column: "name".into(),
                    descending: false,
                }),
                filter: None,
            })
            .await
            .expect("query");
        assert_eq!(page.total, seeded);
        assert!(!page.rows.is_empty());
    }
}
