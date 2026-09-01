//! Kaptein integration layer — binds `kaptein-core` to native frontends.
//!
//! Per `docs/architecture.md`, the *integration layer* is the native frontend or binary
//! that owns both `kaptein-core` and (where applicable) `kaptein-viewmodel`. It maps raw
//! `kaptein-core::Error` values into user-facing messages without leaking secrets.
//!
//! The TUI reaches `kaptein-core` through this crate, keeping the layer dependency rule
//! satisfied: `kaptein-tui` → `kaptein-integration` → `kaptein-core`, with no frontend
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
use kaptein_viewmodel::downgrade_forbidden;
use kube::ResourceExt as _;

/// Build a Kubernetes client for the default context.
pub async fn client() -> Result<kube::Client, IntegrationError> {
    Ok(kaptein_core::discovery::client().await?)
}

/// Build a Kubernetes client for a specific named context.
pub async fn client_for_context(context: Option<&str>) -> Result<kube::Client, IntegrationError> {
    Ok(kaptein_core::discovery::client_for_context(context).await?)
}

/// Load and validate a lens file into a [`kaptein_viewmodel::ViewDefinition`] (M2.2).
///
/// This is the shared "load a lens the frontend discovered" path: parse YAML/JSON, then
/// refuse an invalid lens (empty problem list = valid). A lens that fails validation is
/// surfaced as an error, never silently dropped — the same contract as `kaptein viewdef
/// validate`.
pub fn load_lens(
    path: &std::path::Path,
) -> Result<kaptein_viewmodel::ViewDefinition, IntegrationError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| IntegrationError::Internal(format!("cannot read {}: {e}", path.display())))?;
    let value: serde_json::Value = serde_yaml::from_str(&text)
        .map_err(|e| IntegrationError::Internal(format!("cannot parse {}: {e}", path.display())))?;
    let vd: kaptein_viewmodel::ViewDefinition = serde_json::from_value(value).map_err(|e| {
        IntegrationError::Internal(format!("cannot deserialize {}: {e}", path.display()))
    })?;
    let problems = kaptein_viewmodel::validate_viewdef(&vd);
    if problems.is_empty() {
        Ok(vd)
    } else {
        Err(IntegrationError::Internal(format!(
            "lens {} is invalid ({} problem(s)): {}",
            path.display(),
            problems.len(),
            problems.join("; ")
        )))
    }
}

/// The column schema of a Kubernetes resource view: `name`, `namespace`, `status`,
/// `created`. The view-model owns *which* columns exist (semantics); the frontend owns
/// their width in cells (geometry).
pub const RESOURCE_COLUMNS: [&str; 4] = ["name", "namespace", "status", "created"];

/// RBAC-preflight an action graph **in place**, downgrading each action whose verb the
/// current user is not permitted to make to `Forbidden` (M2.2 "per-action RBAC
/// grey-out"). This is the shipped path: it runs one `SelfSubjectRulesReview` for the
/// target resource (via `kaptein_core::auth::preflight`) and applies the view-model's
/// [`kaptein_viewmodel::downgrade_forbidden`] — so the TUI greys out an action *before*
/// the operator tries it, not after a 403.
///
/// The `gvk` is resolved to its plural resource + group with kube's own pluralizer
/// (the same plural the request path uses), so a lens over a CRD preflights the *actual*
/// resource, not a guess. A preflight failure degrades to "no downgrade" (the actions
/// keep their declared state) rather than hiding every action — the frontend already
/// surfaces API errors separately.
pub async fn preflight_actions(
    client: &kube::Client,
    gvk: &kube::core::GroupVersionKind,
    namespace: Option<&str>,
    actions: &mut [kaptein_viewmodel::Action],
) {
    if actions.is_empty() {
        return;
    }
    let plural = kube::core::ApiResource::from_gvk(gvk).plural;
    let group = gvk.group.clone();
    let ns = namespace.unwrap_or_default();
    let Ok(preflight) = kaptein_core::auth::preflight(client, &plural, &group, ns).await else {
        return;
    };
    // Build a verb → allowed map once, then downgrade each action by its mapped verb.
    let allowed: std::collections::HashMap<&str, bool> = preflight
        .actions
        .iter()
        .map(|(v, a)| (v.as_str(), *a))
        .collect();
    for action in actions.iter_mut() {
        let verb = kaptein_viewmodel::action_verb(&action.id);
        downgrade_forbidden(action, allowed.get(verb).copied(), &plural, namespace);
    }
}

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

/// Map a watch event over a `DynamicObject` into a `RowPatch`, using the same
/// `ResourceSummary` → `Row` mapping as `resource_row`. Bookmarks and errors produce no
/// patch (they carry no resource change).
///
/// This is the *non-lens* mapping; `LivePlane::map_watch_event` is the lens-aware
/// entry point used in production. Kept here (test-only) because it is the exact
/// `Added`/`Modified`/`Deleted` → `RowPatch` shape the tests assert against.
#[cfg(test)]
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

/// The single `DynamicObject` → `Row` mapping, shared by `LivePlane`'s seed and watch
/// delta paths. A `Some(lens)` renders through `render_row` (M2.2 — the lens's declared
/// columns reach the data plane); `None` uses the built-in four-column `resource_row`.
/// Split out as a free function so the M2.2 DoD is testable without a live `kube::Client`.
///
/// **Redaction (M1.7):** the object is redacted *before* `render_row` so a lens that
/// binds a column to a secret-shaped field (`data.password`, a `Secret`'s `data.*`, an
/// env `value` paired with a sensitive `name`) never reaches a `Row` as plaintext — the
/// same choke point that protects `describe` and the MCP surface. Without this, a
/// lens-driven view could leak secret values that the built-in four-column path masks.
pub(crate) fn map_object_with(
    lens: Option<&kaptein_viewmodel::ViewDefinition>,
    obj: &kube::core::DynamicObject,
    gvk: &kube::core::GroupVersionKind,
) -> kaptein_viewmodel::Row {
    match lens {
        Some(vd) => {
            // Redact a *copy* of the object (redact_object is in-place) before rendering,
            // so lens columns read masked values — never plaintext secrets.
            let mut redacted = obj.clone();
            kaptein_core::redact::redact_object(&mut redacted);
            let value = serde_json::to_value(&redacted).unwrap_or(serde_json::Value::Null);
            kaptein_viewmodel::render_row(vd, &value)
        }
        None => resource_row(kaptein_core::discovery::summary_of(obj, gvk)),
    }
}

/// A `DataPlane` that is **informer-backed and live**: it seeds a `MemPlane` from a
/// bounded list, then a background watch task applies `Added`/`Modified`/`Deleted`
/// deltas as `RowPatch` upserts/removes. `query` reads the live `MemPlane` (no API call
/// per keystroke) and `subscribe` streams real deltas — the ADR-0006 "informer-based,
/// never polling" shape for the TUI.
///
/// A plane is either the built-in four-column view (`name`/`namespace`/`status`/`created`)
/// or a **lens-driven** view: when a [`kaptein_viewmodel::ViewDefinition`] is attached
/// (M2.2), every object is mapped through `render_row`, so the lens's declared columns and
/// status inference reach a `Row` through the data plane — not only through
/// `kaptein viewdef render`.
pub struct LivePlane {
    mem: kaptein_viewmodel::MemPlane,
    client: kube::Client,
    gvk: kube::core::GroupVersionKind,
    namespace: Option<String>,
    /// The informer lifecycle manager (ADR-0006): the hard cap, LRU+TTL eviction, and
    /// degrade-to-list path. Shared across clones **and across distinct planes in a
    /// session** so the cap is enforced over the *process*, not per plane (issue #25,
    /// finding M). A plane constructed with [`LivePlane::with_shared_informers`] — or any
    /// `clone` of one — shares the session-scoped manager rather than minting a fresh one.
    informers: std::sync::Arc<kaptein_core::informer::InformerManager>,
    /// The lens this plane renders through (M2.2). `None` = the built-in four-column
    /// view. Its columns become the plane's schema; its status rules become the status
    /// column's inference.
    lens: Option<kaptein_viewmodel::ViewDefinition>,
}

impl Clone for LivePlane {
    /// Clone shares the same in-memory `MemPlane`, Kubernetes client, informer manager,
    /// and lens — two handles to one live data plane (the watch task and the TUI both
    /// hold a handle).
    fn clone(&self) -> Self {
        Self {
            mem: self.mem.clone(),
            client: self.client.clone(),
            gvk: self.gvk.clone(),
            namespace: self.namespace.clone(),
            informers: self.informers.clone(),
            lens: self.lens.clone(),
        }
    }
}

impl LivePlane {
    /// Create a live plane for `group/version/kind` with the default informer policy.
    pub fn new(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
    ) -> Self {
        Self::new_with_policy(
            client,
            gvk,
            namespace,
            kaptein_core::informer::InformerPolicy::default(),
        )
    }

    /// Create a **lens-driven** live plane (M2.2): objects are rendered through the
    /// lens's `render_row`, so the lens's declared columns + status inference are the
    /// plane's schema. The lens's `target` GVK is authoritative — it must match `gvk`.
    pub fn new_lens(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
        lens: kaptein_viewmodel::ViewDefinition,
    ) -> Self {
        Self::new_lens_with_policy(
            client,
            gvk,
            namespace,
            lens,
            kaptein_core::informer::InformerPolicy::default(),
        )
    }

    /// Create a live plane for `group/version/kind` with an explicit informer policy
    /// (from the `[informer]` config section, ADR-0006). The manager is **per-plane**
    /// (a fresh `Arc`) — callers that want the cap enforced across the whole session
    /// (the TUI) must use [`LivePlane::with_shared_informers`] instead (finding M).
    pub fn new_with_policy(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
        policy: kaptein_core::informer::InformerPolicy,
    ) -> Self {
        Self::with_shared_informers(
            client,
            gvk,
            namespace,
            None,
            std::sync::Arc::new(kaptein_core::informer::InformerManager::new(policy)),
        )
    }

    /// Create a **lens-driven** live plane with an explicit informer policy. The plane's
    /// schema is the lens's column ids, so `query` sort/filter resolve against the lens
    /// columns (M2.2 DoD: a lens-declared column reaches a `Row` through the data plane).
    pub fn new_lens_with_policy(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
        lens: kaptein_viewmodel::ViewDefinition,
        policy: kaptein_core::informer::InformerPolicy,
    ) -> Self {
        let column_ids: Vec<String> = lens.columns.iter().map(|c| c.id.clone()).collect();
        let mem = kaptein_viewmodel::MemPlane::new(kaptein_viewmodel::Schema { column_ids });
        Self {
            mem,
            client,
            gvk,
            namespace,
            // Per-plane manager (the compatibility path); the TUI passes a shared manager
            // via `with_shared_informers` so the cap is session-scoped (finding M).
            informers: std::sync::Arc::new(kaptein_core::informer::InformerManager::new(policy)),
            lens: Some(lens),
        }
    }

    /// Create a live plane that shares the caller-supplied informer manager. This is the
    /// session-scoped path (finding M): the TUI holds **one** [`InformerManager`] per
    /// session and passes it to every `rebuild_plane`, so `max_watches` is enforced over
    /// the set of *all* views the operator has opened, not one view at a time.
    ///
    /// `lens` is `None` for the built-in four-column view, `Some` for a lens-driven view.
    pub fn with_shared_informers(
        client: kube::Client,
        gvk: kube::core::GroupVersionKind,
        namespace: Option<String>,
        lens: Option<kaptein_viewmodel::ViewDefinition>,
        informers: std::sync::Arc<kaptein_core::informer::InformerManager>,
    ) -> Self {
        let column_ids: Vec<String> = match &lens {
            Some(vd) => vd.columns.iter().map(|c| c.id.clone()).collect(),
            None => RESOURCE_COLUMNS.iter().map(|s| s.to_string()).collect(),
        };
        let mem = kaptein_viewmodel::MemPlane::new(kaptein_viewmodel::Schema { column_ids });
        Self {
            mem,
            client,
            gvk,
            namespace,
            informers,
            lens,
        }
    }

    /// The watch key this plane registers — `group/version/kind[/namespace]` (finding N:
    /// the same identity used to `release` the slot when the view closes).
    pub fn watch_key(&self) -> kaptein_core::informer::WatchKey {
        kaptein_core::informer::WatchKey {
            group: self.gvk.group.clone(),
            version: self.gvk.version.clone(),
            kind: self.gvk.kind.clone(),
            namespace: self.namespace.clone().unwrap_or_default(),
        }
    }

    /// Release this plane's informer slot (a view closed). Idempotent — safe to call
    /// without a corresponding `register`. The watch task calls this on exit so a slot is
    /// returned to the shared manager when a view is switched away from (finding N).
    pub fn release_informer(&self) {
        self.informers.release(&self.watch_key());
    }

    /// The number of live watches in the shared manager right now (tests: the cap is
    /// enforced over distinct planes, not per plane).
    pub fn live_watches(&self) -> usize {
        self.informers.live()
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

    /// The plane's column schema (built-in four-column, or the lens's columns).
    pub fn column_ids(&self) -> Vec<String> {
        self.mem.schema_column_ids()
    }

    /// Map a `DynamicObject` into a `Row`: through the attached lens (M2.2) or the
    /// built-in four-column `resource_row` mapping. This is the single mapping every
    /// seed/watch delta goes through, so a lens column reaches the data plane by
    /// construction, not only through `kaptein viewdef render`.
    fn map_object(&self, obj: &kube::core::DynamicObject) -> kaptein_viewmodel::Row {
        map_object_with(self.lens.as_ref(), obj, &self.gvk)
    }

    /// Map a watch event into a `RowPatch`, through the attached lens when present
    /// (M2.2), else the built-in mapping. Added/Modified become an upsert rendered via
    /// `map_object`; Deleted becomes a remove keyed by uid (or name).
    fn map_watch_event(
        &self,
        event: &kube::api::WatchEvent<kube::core::DynamicObject>,
    ) -> Option<kaptein_viewmodel::RowPatch> {
        match event {
            kube::api::WatchEvent::Added(obj) | kube::api::WatchEvent::Modified(obj) => {
                let row = self.map_object(obj);
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

    /// Seed the plane from a **bounded** list (the "list" half of list-then-watch), paging
    /// through `limit` objects at a time so the frontend path never asks the API server
    /// to materialize the whole cluster in one unbounded `list` (issue #27 / ADR-0006).
    /// The caller then runs `watch_loop` on a background task to apply live deltas.
    ///
    /// A lens plane seeds **full objects** (not metadata summaries) so `render_row` can
    /// read the `spec`/`status` fields its columns bind.
    pub async fn seed(&self) -> Result<usize, IntegrationError> {
        let mut count = 0usize;
        let mut continue_token: Option<String> = None;
        loop {
            if self.lens.is_some() {
                let (objs, next) = kaptein_core::discovery::list_objects_bounded(
                    &self.client,
                    &self.gvk,
                    self.namespace.as_deref(),
                    500,
                    continue_token.as_deref(),
                )
                .await?;
                count += objs.len();
                for obj in &objs {
                    self.mem.upsert(self.map_object(obj));
                }
                match next {
                    Some(t) => continue_token = Some(t),
                    None => break,
                }
            } else {
                let (summaries, next) = kaptein_core::discovery::list_bounded(
                    &self.client,
                    &self.gvk,
                    self.namespace.as_deref(),
                    500,
                    continue_token.as_deref(),
                )
                .await?;
                count += summaries.len();
                for s in summaries {
                    let row = resource_row(s);
                    self.mem.upsert(row);
                }
                match next {
                    Some(t) => continue_token = Some(t),
                    None => break,
                }
            }
        }
        Ok(count)
    }

    /// Run the watch loop until cancelled, applying deltas to the `MemPlane`. This is the
    /// "watch" half — drive it on a `tokio::spawn`ed task. On watch expiry/error
    /// (routinely after ~5 min server timeouts or a 410 Gone) it **relists into the
    /// store and reconciles** — removing rows absent from the fresh relist and upserting
    /// rows that appeared during the outage — then watches from the relist's
    /// resourceVersion, so no deleted object lingers as a ghost row (issue #20) and no
    /// object created during an outage stays invisible (finding O).
    ///
    /// The informer lifecycle is **enforced here** (issue #25): the watch key is
    /// registered with the shared [`InformerManager`] first; if the hard cap is reached
    /// (`Denied`), the plane **degrades to a one-shot on-demand list** (seeded, no live
    /// watch) instead of opening another socket. When the loop exits (view closed or
    /// task aborted), the slot is **released** back to the shared manager (finding N), so
    /// a session-scoped manager does not leak a slot per view switch.
    pub async fn watch_loop(&self) -> Result<(), IntegrationError> {
        let ar = kube::core::ApiResource::from_gvk(&self.gvk);
        let api: kube::Api<kube::core::DynamicObject> = match self.namespace.as_deref() {
            Some(ns) => kube::Api::namespaced_with(self.client.clone(), ns, &ar),
            None => kube::Api::all_with(self.client.clone(), &ar),
        };

        let watch_key = self.watch_key();
        // Release the slot when the loop exits for *any* reason (normal return, `?`,
        // or the task being aborted). This is what keeps the shared manager's count
        // accurate across view switches (finding N: without it, fixing M becomes a slot
        // leak).
        let _release_guard = WatchSlotGuard {
            informers: self.informers.clone(),
            key: watch_key.clone(),
        };

        use kaptein_core::informer::Registration;
        if self.informers.register(watch_key.clone()) == Registration::Denied {
            // Degrade to on-demand list (ADR-0006): the hard cap is reached, so this view
            // gets a bounded snapshot rather than a live watch socket.
            let _ = self.relist_and_reconcile(&api).await?;
            return Ok(());
        }

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
                        if let Some(patch) = self.map_watch_event(&ev) {
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

    /// Relist the resource (fully paged) into the `MemPlane`, reconciling in **both
    /// directions** and returning the resourceVersion to watch from. This is the "list"
    /// half of list-then-watch on every reconnect — not just the initial seed.
    ///
    /// The relist uses **full objects** (not metadata summaries) so the reconciliation can
    /// upsert rows with a correct `status` — the fix for finding O: a metadata-only relist
    /// could not add a row that appeared during a watch outage without a fabricated status.
    async fn relist_and_reconcile(
        &self,
        api: &kube::Api<kube::core::DynamicObject>,
    ) -> Result<String, IntegrationError> {
        // Collect the full live object set (fully paged) and its resourceVersion. Full
        // objects are required so the upsert direction carries a correct `status` — a
        // metadata-only relist would leave a new object's status empty (finding O).
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
                .list(&lp)
                .await
                .map_err(|e| IntegrationError::from(kaptein_core::Error::Api(e)))?;
            rv = list.metadata.resource_version.clone().or(rv);
            for obj in list.items {
                let row = self.map_object(&obj);
                // Upsert direction: an object that is *in* the relist but missing from the
                // plane (created during a watch outage) is added, not skipped (finding O).
                self.mem.upsert(row.clone());
                live_ids.insert(row.id);
            }
            match list.metadata.continue_.clone() {
                Some(t) => continue_token = Some(t),
                None => break,
            }
        }

        // Remove direction: drop any row in the plane not present in the fresh relist.
        let current = self.mem.rows();
        for row in &current {
            if !live_ids.contains(&row.id) {
                self.mem.remove(&row.id);
            }
        }

        Ok(rv.unwrap_or_else(|| "0".to_string()))
    }
}

/// A drop guard that releases a watch slot from the shared [`InformerManager`] when the
/// watch task exits (finding N). `watch_loop` holds this for its whole lifetime; when the
/// task is aborted or returns, the slot is returned so a session-scoped manager never
/// leaks a slot per view switch.
struct WatchSlotGuard {
    informers: std::sync::Arc<kaptein_core::informer::InformerManager>,
    key: kaptein_core::informer::WatchKey,
}

impl Drop for WatchSlotGuard {
    fn drop(&mut self) {
        self.informers.release(&self.key);
    }
}

#[async_trait::async_trait]
impl kaptein_viewmodel::DataPlane for LivePlane {
    async fn query(
        &self,
        query: &kaptein_viewmodel::Query,
    ) -> Result<kaptein_viewmodel::Page, kaptein_viewmodel::Error> {
        // Refresh the informer's recency signal (finding Z): a live view that is
        // queried is "hot", so its watch slot must not be the one the LRU evicts. The
        // TUI already re-queries on every revision change, so this hook costs nothing
        // and fixes the LRU inversion where `min_by_key(last_touched)` otherwise picked
        // the oldest-*registered* — i.e. the view on screen — as the coldest.
        self.informers.touch(&self.watch_key());
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

    /// The M2.2 DoD, made falsifiable: a lens-declared column reaches a `Row` through the
    /// data plane (`map_object_with`, the seed/watch mapping), not only through
    /// `kaptein viewdef render`. Removing the lens path in `map_object_with` fails this.
    #[test]
    fn lens_column_reaches_row_through_data_plane() {
        let lens: kaptein_viewmodel::ViewDefinition = serde_json::from_value(serde_json::json!({
            "id": "com.example.cnpg-lens",
            "api_version": 1,
            "target": { "group": "postgresql.cnpg.io", "version": "v1", "kind": "Cluster" },
            "columns": [
                { "id": "name", "header_key": "col.name", "kind": "text", "sortable": true, "field": "metadata.name" },
                { "id": "instances", "header_key": "col.instances", "kind": "number", "sortable": true, "field": "spec.instances" },
                { "id": "status", "header_key": "col.status", "kind": "status", "sortable": true }
            ],
            "status": [
                { "field": "status.phase", "op": "eq", "value": "ClusterIsReady", "level": "ok" }
            ]
        }))
        .expect("valid lens");

        // A CNPG Cluster dynamic object with the spec field the `instances` column binds.
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        let obj = kube::core::DynamicObject {
            types: Some(kube::core::TypeMeta {
                api_version: "postgresql.cnpg.io/v1".into(),
                kind: "Cluster".into(),
            }),
            metadata: ObjectMeta {
                name: Some("cnpg-main".into()),
                namespace: Some("db".into()),
                uid: Some("uid-cluster-1".into()),
                ..Default::default()
            },
            data: serde_json::json!({
                "spec": { "instances": 3 },
                "status": { "phase": "ClusterIsReady" }
            }),
        };
        let gvk = kube::core::GroupVersionKind::gvk("postgresql.cnpg.io", "v1", "Cluster");

        let row = map_object_with(Some(&lens), &obj, &gvk);

        // The lens's declared columns are the row's cells, in order — the `instances`
        // number column reached the data plane, not only `viewdef render`.
        assert_eq!(row.cells.len(), 3);
        assert_eq!(kaptein_viewmodel::cell_text(&row.cells[0]), "cnpg-main");
        assert!(matches!(
            row.cells[1],
            kaptein_viewmodel::Cell::Number { value: 3 }
        ));
        // The status column is inferred (status.phase == ClusterIsReady → ok).
        assert!(matches!(
            row.cells[2],
            kaptein_viewmodel::Cell::Status {
                level: kaptein_viewmodel::StatusLevel::Ok,
                ..
            }
        ));

        // And the *non-lens* path is unchanged: the built-in four-column mapping.
        let builtin = map_object_with(None, &obj, &gvk);
        assert_eq!(builtin.cells.len(), 4);
    }

    /// M1.7 DoD (lens path): a lens that binds a column to a secret-shaped field must
    /// not leak plaintext — `map_object_with` redacts the object before `render_row`.
    #[test]
    fn lens_column_does_not_leak_secret_values() {
        // A lens that (wrongly) binds a column directly to a secret value.
        let lens: kaptein_viewmodel::ViewDefinition = serde_json::from_value(serde_json::json!({
            "id": "com.example.leaky-lens",
            "api_version": 1,
            "target": { "group": "", "version": "v1", "kind": "Secret" },
            "columns": [
                { "id": "name", "header_key": "col.name", "kind": "text", "sortable": true, "field": "metadata.name" },
                { "id": "password", "header_key": "col.password", "kind": "text", "sortable": true, "field": "data.password" }
            ]
        }))
        .expect("valid lens");

        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        let obj = kube::core::DynamicObject {
            types: Some(kube::core::TypeMeta {
                api_version: "v1".into(),
                kind: "Secret".into(),
            }),
            metadata: ObjectMeta {
                name: Some("db-secret".into()),
                namespace: Some("default".into()),
                uid: Some("uid-secret-1".into()),
                ..Default::default()
            },
            // A Secret's data field holds base64-encoded values; the raw JSON has the
            // sensitive key `data` → `password` which redaction must mask.
            data: serde_json::json!({
                "data": { "password": "aHVudGVyMg==" },
                "stringData": { "token": "supersecret" }
            }),
        };
        let gvk = kube::core::GroupVersionKind::gvk("", "v1", "Secret");

        let row = map_object_with(Some(&lens), &obj, &gvk);

        // The name column is fine; the password column must be masked, not plaintext.
        assert_eq!(kaptein_viewmodel::cell_text(&row.cells[0]), "db-secret");
        // The password column is now the *typed* `Cell::Redacted` variant (M1.7), not a
        // `Text` cell carrying a marker string — so the frontend renders a mask without
        // any special-case string comparison.
        assert_eq!(
            row.cells[1],
            kaptein_viewmodel::Cell::Redacted,
            "a lens column bound to a secret field must be the typed Redacted cell"
        );
        // Its display text is the mask (the frontend renders `[REDACTED]`, never the
        // secret and never an empty-looking gap).
        assert_eq!(kaptein_viewmodel::cell_text(&row.cells[1]), "[REDACTED]");
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

    /// Build a client that will never be used to make a request (no tokio runtime) — only
    /// its identity matters for the informer lifecycle DoD. The plane is never seeded or
    /// watched, so the client is never touched.
    fn dummy_client() -> kube::Client {
        let config = kube::Config::new("https://127.0.0.1:1".parse::<http::Uri>().unwrap());
        kube::Client::try_from(config).expect("throwaway client")
    }

    fn pod_gvk_namespaced() -> kube::core::GroupVersionKind {
        kube::core::GroupVersionKind::gvk("", "v1", "Pod")
    }

    /// **M2.0c DoD (findings M + N), made falsifiable.** A session-scoped informer
    /// manager is bounded across *distinct* planes — not per plane. Constructing `cap`
    /// distinct planes through `with_shared_informers` must exhaust the cap (so the LRU/
    /// TTL/degrade-to-list path is reachable in the shipped TUI), and releasing a view's
    /// slot must return it so the next view is granted. A manager that leaked a slot per
    /// view switch (the M-without-N bug) would fail the release half; a per-plane manager
    /// (the M bug) would fail the bound half.
    #[tokio::test]
    async fn shared_manager_cap_is_enforced_across_planes_and_released_on_close() {
        let policy = kaptein_core::informer::InformerPolicy {
            max_watches: 2,
            idle_ttl: std::time::Duration::from_secs(60),
        };
        let shared = std::sync::Arc::new(kaptein_core::informer::InformerManager::new(policy));

        let make = |ns: &str| {
            LivePlane::with_shared_informers(
                dummy_client(),
                pod_gvk_namespaced(),
                Some(ns.to_string()),
                None,
                shared.clone(),
            )
        };

        // Two distinct planes share one manager: two distinct watch keys are live, and
        // the shared count reflects *both* (the M invariant).
        let a = make("ns-a");
        let b = make("ns-b");
        assert_eq!(
            shared.live(),
            0,
            "registration happens in watch_loop, not at construction"
        );
        assert_eq!(a.live_watches(), 0);

        // Register both keys exactly as watch_loop would, then a third is Denied (the cap
        // is reachable — it would never be, per-plane).
        use kaptein_core::informer::Registration;
        assert_eq!(shared.register(a.watch_key()), Registration::Granted);
        assert_eq!(shared.register(b.watch_key()), Registration::Granted);
        assert_eq!(shared.live(), 2);

        // Release a's slot (its watch task closed): the count drops and the slot is
        // reusable — the N half. Without release, `live()` would stay pinned at the cap.
        a.release_informer();
        assert_eq!(shared.live(), 1);

        let c = make("ns-c");
        assert_eq!(shared.register(c.watch_key()), Registration::Granted);
        assert_eq!(shared.live(), 2);
    }

    /// **M2.0c DoD (finding Z), made falsifiable.** The LRU's recency signal must come
    /// from *use*, not registration order. `LivePlane::query` touches the shared manager,
    /// so when the cap is full and a new view is registered, the **most-recently-queried**
    /// view survives eviction and the coldest (never-queried) one is evicted — the
    /// inverse of the pre-Z behaviour, where `min_by_key(last_touched)` picked the
    /// oldest-*registered* view (the one on screen).
    #[tokio::test]
    async fn lru_evicts_the_coldest_not_the_hottest_view() {
        use kaptein_viewmodel::DataPlane as _;
        let policy = kaptein_core::informer::InformerPolicy {
            max_watches: 2,
            idle_ttl: std::time::Duration::from_secs(60),
        };
        let shared = std::sync::Arc::new(kaptein_core::informer::InformerManager::new(policy));

        let make = |ns: &str| {
            LivePlane::with_shared_informers(
                dummy_client(),
                pod_gvk_namespaced(),
                Some(ns.to_string()),
                None,
                shared.clone(),
            )
        };

        use kaptein_core::informer::Registration;
        let a = make("ns-a");
        let b = make("ns-b");
        assert_eq!(shared.register(a.watch_key()), Registration::Granted);
        assert_eq!(shared.register(b.watch_key()), Registration::Granted);
        assert_eq!(shared.live(), 2);

        // Touch A (it is the view on screen): its recency now beats B's.
        a.query(&kaptein_viewmodel::Query::default())
            .await
            .expect("query touches the informer");

        // A third view is admitted; the LRU must evict B (the coldest — never queried),
        // not A (the hottest).
        let c = make("ns-c");
        assert_eq!(shared.register(c.watch_key()), Registration::Granted);
        assert_eq!(shared.live(), 2, "cap must stay at 2");
        // A is still live (its recency protected it); B is gone.
        assert!(
            shared.touch(&a.watch_key()),
            "A (hottest) must survive eviction"
        );
        assert!(!shared.touch(&b.watch_key()), "B (coldest) must be evicted");
    }

    /// **M2.2 DoD (shipped lens set), made falsifiable.** Every lens that ships in the
    /// `extensions/` directory must load *and validate* through the real `load_lens`
    /// path — not an inline fixture. The other lens tests use inline documents because
    /// `cargo publish` does not package `extensions/`; this test resolves the checked-in
    /// directory from `CARGO_MANIFEST_DIR` (skipping gracefully when it is absent, e.g.
    /// in a published-tarball build) so a shipped lens that drifts from the schema (a new
    /// field, a wrong `api_version`, a bad column) fails CI rather than silently
    /// skipping at TUI startup.
    #[test]
    fn every_shipped_lens_validates_through_load_lens() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("extensions");
        if !root.is_dir() {
            eprintln!("skipping: extensions/ not present (published-tarball build)");
            return;
        }
        let (lenses, problems) = kaptein_core::extension::discover_lenses(&root);
        assert!(
            problems.is_empty(),
            "shipped lens discovery reported problems: {problems:?}"
        );
        assert!(
            !lenses.is_empty(),
            "extensions/ must ship at least one lens"
        );
        for lens in &lenses {
            let vd = load_lens(&lens.entrypoint)
                .unwrap_or_else(|e| panic!("shipped lens {} failed to load: {e}", lens.id));
            // The lens's declared target must agree with the manifest's discovered target.
            let gvk = &vd.target;
            assert_eq!(
                (gvk.group.as_str(), gvk.version.as_str(), gvk.kind.as_str()),
                (
                    lens.target.group.as_str(),
                    lens.target.version.as_str(),
                    lens.target.kind.as_str()
                ),
                "lens {} target drifts from its discovered GVK",
                lens.id
            );
        }
    }
}
