//! Generic resource listing over the discovery API.
//!
//! Lists any namespaced or cluster-scoped resource kind as `DynamicObject`, so built-in
//! resources and CRDs are handled uniformly ("built-in resources + all CRDs
//! auto-discovered"). `PartialObjectMetadata` is reserved for list-heavy views in a
//! later milestone.

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Api, DynamicObject, ListParams, ObjectList};
use kube::config::KubeConfigOptions;
use kube::core::{ApiResource, GroupVersionKind, PartialObjectMeta};
use kube::{Client, Config, ResourceExt};

use crate::Error;

/// A display-neutral summary of a single dynamic resource.
#[derive(Debug, Clone)]
pub struct ResourceSummary {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    /// The object's `metadata.uid` — the stable identity the render contract keys rows by
    /// (ADR-0005), not a positional index.
    pub uid: Option<String>,
    /// The resource's creation time (Rust-native), for the frontend to format.
    pub created: Option<Time>,
    /// A best-effort human-readable status (pod phase, etc.), or empty when the kind has
    /// no well-known status.
    pub status: String,
}

/// Extract a display status from a `DynamicObject`'s `status` sub-object, falling back to
/// the kind-appropriate default. Pods use `status.phase`; everything else is left for a
/// lens (Phase 2).
fn status_of(kind: &str, obj: &DynamicObject) -> String {
    // Pod phase is the one status every operator looks at first.
    if kind == "Pod" {
        if let Some(phase) = obj
            .data
            .get("status")
            .and_then(|s| s.get("phase"))
            .and_then(|p| p.as_str())
        {
            return phase.to_string();
        }
        return "Unknown".to_string();
    }
    String::new()
}

/// Build a Kubernetes client from the default kubeconfig.
pub async fn client() -> Result<Client, Error> {
    let config = Config::from_kubeconfig(&KubeConfigOptions::default()).await?;
    Ok(Client::try_from(config)?)
}

/// Build a Kubernetes client for a specific named context (k9s-parity context
/// switching). Falls back to the default context when `context` is `None`.
pub async fn client_for_context(context: Option<&str>) -> Result<Client, Error> {
    let options = KubeConfigOptions {
        context: context.map(|c| c.to_string()),
        cluster: None,
        user: None,
    };
    let config = Config::from_kubeconfig(&options).await?;
    Ok(Client::try_from(config)?)
}

/// Build a Kubernetes client for a **dedicated agent identity** (ADR-0007 mode 3):
/// prefer an in-cluster ServiceAccount (the pod's mounted token, the agent's own narrow
/// RBAC), then fall back to a `KAPTEIN_SA_TOKEN` bearer token, then the default
/// kubeconfig. This gives each MCP agent its own identity in the cluster rather than a
/// shared human credential.
pub async fn agent_client() -> Result<Client, Error> {
    // 1. In-cluster ServiceAccount (default for MCP running as a pod).
    if let Ok(config) = Config::incluster() {
        return Ok(Client::try_from(config)?);
    }
    // 2. Explicit bearer token (`KAPTEIN_SA_TOKEN`) as the agent's identity.
    if let Ok(token) = std::env::var("KAPTEIN_SA_TOKEN")
        && !token.trim().is_empty()
        && let Ok(mut config) = Config::from_kubeconfig(&KubeConfigOptions::default()).await
    {
        config.auth_info.token = Some(secrecy::SecretString::new(token.into_boxed_str()));
        return Ok(Client::try_from(config)?);
    }
    // 3. Fall back to the default kubeconfig (best-effort; still audited as an agent).
    client().await
}

/// Resolve the agent identity name for the audit log: `$KAPTEIN_AGENT`, else the
/// current ServiceAccount (from `KAPTEIN_SA_TOKEN` is not available; in-cluster pods
/// expose the SA via the token), else a stable `mcp-client` default.
pub fn agent_identity_name() -> String {
    if let Ok(name) = std::env::var("KAPTEIN_AGENT")
        && !name.trim().is_empty()
    {
        return name;
    }
    "mcp-client".to_string()
}

/// How the agent identity was resolved — so callers can distinguish a real dedicated
/// identity from a silent fallback to the operator's own kubeconfig (which would record
/// an *agent* actor for actions taken with the *human's* credentials, exactly what
/// SECURITY.md promises against).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentIdentityResolution {
    /// In-cluster ServiceAccount (the pod's own narrow identity).
    InClusterServiceAccount,
    /// Explicit `KAPTEIN_SA_TOKEN` bearer token.
    ExplicitToken,
    /// `KAPTEIN_AGENT` was set (audit name only; the client identity is separate).
    NamedAgent,
    /// Fallback to the default kubeconfig — the operator's own credentials.
    OperatorKubeconfig,
}

/// Resolve the agent identity *and report how it was resolved*. The MCP server uses this
/// to refuse (or warn) on the operator-kubeconfig fallback, so the audit log never
/// silently records an agent actor for actions taken with human credentials.
pub fn agent_identity_resolution() -> AgentIdentityResolution {
    // 1. In-cluster ServiceAccount.
    if std::env::var("KUBERNETES_SERVICE_HOST").is_ok()
        && std::env::var("KUBERNETES_SERVICE_PORT").is_ok()
    {
        return AgentIdentityResolution::InClusterServiceAccount;
    }
    // 2. Explicit bearer token.
    if std::env::var("KAPTEIN_SA_TOKEN")
        .map(|t| !t.trim().is_empty())
        .unwrap_or(false)
    {
        return AgentIdentityResolution::ExplicitToken;
    }
    // 3. A named agent (audit-name only) — still falls back to kubeconfig for the client.
    if std::env::var("KAPTEIN_AGENT")
        .map(|n| !n.trim().is_empty())
        .unwrap_or(false)
    {
        return AgentIdentityResolution::NamedAgent;
    }
    AgentIdentityResolution::OperatorKubeconfig
}

/// Read the current context name from the kubeconfig (for guardrail classification).
pub fn current_context_name() -> Result<String, Error> {
    use kube::config::Kubeconfig;
    let kc = Kubeconfig::read()?;
    Ok(kc.current_context.unwrap_or_default())
}

/// List all contexts defined in the kubeconfig (name + cluster + user), for a
/// context-switching picker (k9s-parity).
pub fn list_contexts() -> Result<Vec<ContextSummary>, Error> {
    use kube::config::Kubeconfig;
    let kc = Kubeconfig::read()?;
    let current = kc.current_context.unwrap_or_default();
    let mut out = Vec::new();
    for named in kc.contexts {
        let cluster = named
            .context
            .as_ref()
            .map(|c| c.cluster.clone())
            .unwrap_or_default();
        let user = named
            .context
            .as_ref()
            .and_then(|c| c.user.clone())
            .unwrap_or_default();
        out.push(ContextSummary {
            name: named.name.clone(),
            cluster,
            user,
            current: named.name == current,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// A single kubeconfig context.
#[derive(Debug, Clone)]
pub struct ContextSummary {
    pub name: String,
    pub cluster: String,
    pub user: String,
    /// `true` if this is the active context.
    pub current: bool,
}

/// List resources of a given `group/version/kind`, namespaced or cluster-scoped.
///
/// Uses `DynamicObject` so any built-in resource or CRD works without a typed binding.
pub async fn list(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
) -> Result<Vec<ResourceSummary>, Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let list: ObjectList<DynamicObject> =
        api.list(&Default::default()).await.map_err(Error::Api)?;

    Ok(list.into_iter().map(|obj| summary_of(&obj, gvk)).collect())
}

/// List the **full** `DynamicObject`s of a given `group/version/kind`, without reducing
/// them to summaries. This is the data source for lens-driven rendering (M2.2): a lens
/// reads `spec`/`status` fields directly, so the full object body must be available — not
/// the metadata-only summary. Namespaced or cluster-scoped, via `DynamicObject` so any
/// built-in resource or CRD works uniformly.
pub async fn list_objects(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
) -> Result<Vec<DynamicObject>, Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };
    let list: ObjectList<DynamicObject> =
        api.list(&Default::default()).await.map_err(Error::Api)?;
    Ok(list.items)
}

/// Convert a `DynamicObject` into a display-neutral `ResourceSummary`.
pub fn summary_of(obj: &DynamicObject, gvk: &GroupVersionKind) -> ResourceSummary {
    let kind = obj
        .types
        .as_ref()
        .map(|t| t.kind.clone())
        .unwrap_or_else(|| gvk.kind.clone());
    let status = status_of(&kind, obj);
    ResourceSummary {
        name: obj.name_any(),
        namespace: obj.namespace().unwrap_or_default(),
        kind,
        uid: obj.metadata.uid.clone(),
        created: obj.metadata.creation_timestamp.clone(),
        status,
    }
}

/// A bounded page of summaries plus the next `continue` token, or `None` when the page
/// is the last. This is the server-side-paginated form of `list` (ADR-0006): the caller
/// pages through `limit` objects at a time instead of one unbounded `api.list`, so
/// list-heavy views are bounded and the API server is not asked to materialize the world.
pub async fn list_bounded(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    limit: u32,
    continue_token: Option<&str>,
) -> Result<(Vec<ResourceSummary>, Option<String>), Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let mut lp = ListParams::default().limit(limit.max(1));
    if let Some(token) = continue_token {
        lp = lp.continue_token(token);
    }

    let list: ObjectList<DynamicObject> = api.list(&lp).await.map_err(Error::Api)?;
    let next = list.metadata.continue_.clone();
    Ok((
        list.into_iter().map(|obj| summary_of(&obj, gvk)).collect(),
        next,
    ))
}

/// A bounded, **metadata-only** page of summaries (ADR-0006: `PartialObjectMetadata`
/// for list-heavy views). Uses the `application/json;as=PartialObjectMetadataList`
/// accept header so the API server returns only `metadata` — no full object bodies — for
/// views that need name/namespace/kind/age/status but not the whole spec. This is what
/// keeps a 50 000-object view bounded without materializing every object.
pub async fn list_metadata_bounded(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    limit: u32,
    continue_token: Option<&str>,
) -> Result<(Vec<ResourceSummary>, Option<String>), Error> {
    let ar = ApiResource::from_gvk(gvk);
    let api: Api<DynamicObject> = match namespace {
        Some(ns) => Api::namespaced_with(client.clone(), ns, &ar),
        None => Api::all_with(client.clone(), &ar),
    };

    let mut lp = ListParams::default().limit(limit.max(1));
    if let Some(token) = continue_token {
        lp = lp.continue_token(token);
    }

    // `PartialObjectMeta<DynamicObject>` carries `types` + `metadata` only.
    let list: ObjectList<PartialObjectMeta<DynamicObject>> =
        api.list_metadata(&lp).await.map_err(Error::Api)?;
    let next = list.metadata.continue_.clone();
    let summaries = list
        .into_iter()
        .map(|meta| {
            // A metadata-only object has no `.status`/`.data`; status is empty (the
            // frontend falls back to a kind-appropriate placeholder). `uid`/`created`
            // come from the metadata itself. The API server erases the kind to
            // `PartialObjectMetadata`, so restore the *requested* kind for display.
            ResourceSummary {
                name: meta.name_any(),
                namespace: meta.namespace().unwrap_or_default(),
                kind: gvk.kind.clone(),
                uid: meta.metadata.uid.clone(),
                created: meta.metadata.creation_timestamp.clone(),
                status: String::new(),
            }
        })
        .collect();
    Ok((summaries, next))
}
