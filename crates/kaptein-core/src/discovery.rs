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
