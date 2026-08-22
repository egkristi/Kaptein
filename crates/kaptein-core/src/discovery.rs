//! Generic resource listing over the discovery API.
//!
//! Lists any namespaced or cluster-scoped resource kind as `DynamicObject`, so built-in
//! resources and CRDs are handled uniformly ("built-in resources + all CRDs
//! auto-discovered"). `PartialObjectMetadata` is reserved for list-heavy views in a
//! later milestone.

use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::api::{Api, DynamicObject, ObjectList};
use kube::config::KubeConfigOptions;
use kube::core::{ApiResource, GroupVersionKind};
use kube::{Client, Config, ResourceExt};

use crate::Error;

/// A display-neutral summary of a single dynamic resource.
#[derive(Debug, Clone)]
pub struct ResourceSummary {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    /// The resource's creation time (Rust-native), for the frontend to format.
    pub created: Option<Time>,
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

    Ok(list
        .into_iter()
        .map(|obj| ResourceSummary {
            name: obj.name_any(),
            namespace: obj.namespace().unwrap_or_default(),
            kind: obj
                .types
                .as_ref()
                .map(|t| t.kind.clone())
                .unwrap_or_else(|| gvk.kind.clone()),
            created: obj.metadata.creation_timestamp,
        })
        .collect())
}

/// A sort key for a `ResourceSummary` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    Namespace,
    Kind,
    /// Creation timestamp (oldest first when ascending).
    Created,
}

impl SortKey {
    /// Parse a column name (case-insensitive). Returns `None` for unknown columns.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "name" => Some(SortKey::Name),
            "namespace" | "ns" => Some(SortKey::Namespace),
            "kind" => Some(SortKey::Kind),
            "created" | "age" => Some(SortKey::Created),
            _ => None,
        }
    }
}

/// Sort a list of summaries by `key` (ascending; pass `descending = true` to reverse).
///
/// Sorting is stable and locale-independent (byte order), so it matches across
/// frontends and in headless/CI. Empty namespaces/names sort first.
pub fn sort_summaries(items: &mut [ResourceSummary], key: SortKey, descending: bool) {
    items.sort_by(|a, b| {
        let ord = match key {
            SortKey::Name => a.name.cmp(&b.name),
            SortKey::Namespace => a.namespace.cmp(&b.namespace),
            SortKey::Kind => a.kind.cmp(&b.kind),
            SortKey::Created => match (a.created.as_ref(), b.created.as_ref()) {
                (Some(ta), Some(tb)) => ta.0.cmp(&tb.0),
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            },
        };
        if descending { ord.reverse() } else { ord }
    });
}

/// Filter summaries by a case-insensitive substring match on name, namespace, or kind.
///
/// A `None` or empty filter keeps everything. This is the cheap, predictable form of
/// the `Filter` contract (the full expression language lands with the lens engine in
/// Phase 2).
pub fn filter_summaries(
    items: Vec<ResourceSummary>,
    substring: Option<&str>,
) -> Vec<ResourceSummary> {
    let needle = substring.unwrap_or("").trim().to_ascii_lowercase();
    if needle.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|r| {
            r.name.to_ascii_lowercase().contains(&needle)
                || r.namespace.to_ascii_lowercase().contains(&needle)
                || r.kind.to_ascii_lowercase().contains(&needle)
        })
        .collect()
}

/// List, then sort and filter, in one call — the data-plane "query" for the CLI/TUI.
pub async fn list_with(
    client: &Client,
    gvk: &GroupVersionKind,
    namespace: Option<&str>,
    sort_key: Option<SortKey>,
    descending: bool,
    filter: Option<&str>,
) -> Result<Vec<ResourceSummary>, Error> {
    let mut items = list(client, gvk, namespace).await?;
    if let Some(key) = sort_key {
        sort_summaries(&mut items, key, descending);
    }
    Ok(filter_summaries(items, filter))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(name: &str, ns: &str, kind: &str) -> ResourceSummary {
        ResourceSummary {
            name: name.into(),
            namespace: ns.into(),
            kind: kind.into(),
            created: None,
        }
    }

    #[test]
    fn sort_by_name_ascending() {
        let mut v = vec![r("zebra", "", "Pod"), r("apple", "", "Pod")];
        sort_summaries(&mut v, SortKey::Name, false);
        assert_eq!(v[0].name, "apple");
        assert_eq!(v[1].name, "zebra");
    }

    #[test]
    fn sort_by_name_descending() {
        let mut v = vec![r("apple", "", "Pod"), r("zebra", "", "Pod")];
        sort_summaries(&mut v, SortKey::Name, true);
        assert_eq!(v[0].name, "zebra");
        assert_eq!(v[1].name, "apple");
    }

    #[test]
    fn sort_by_namespace() {
        let mut v = vec![r("a", "z", "Pod"), r("b", "a", "Pod")];
        sort_summaries(&mut v, SortKey::Namespace, false);
        assert_eq!(v[0].namespace, "a");
        assert_eq!(v[1].namespace, "z");
    }

    #[test]
    fn sort_key_parse() {
        assert_eq!(SortKey::parse("NAME"), Some(SortKey::Name));
        assert_eq!(SortKey::parse("ns"), Some(SortKey::Namespace));
        assert_eq!(SortKey::parse("created"), Some(SortKey::Created));
        assert_eq!(SortKey::parse("bogus"), None);
    }

    #[test]
    fn filter_is_case_insensitive_substring() {
        let v = vec![
            r("frontend", "prod", "Deployment"),
            r("worker", "staging", "Pod"),
            r("db", "prod", "StatefulSet"),
        ];
        let out = filter_summaries(v, Some("PROD"));
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| r.namespace == "prod"));
    }

    #[test]
    fn filter_empty_keeps_all() {
        let v = vec![r("a", "ns", "Pod")];
        assert_eq!(filter_summaries(v.clone(), None).len(), 1);
        assert_eq!(filter_summaries(v, Some("  ")).len(), 1);
    }
}
