//! M2.0b — live integration-test tier (the write paths were unit-tested, not
//! exercised against a real API server).
//!
//! These tests exercise the **real kube client** against a live cluster, but only when
//! explicitly opted in: `KAPTEIN_LIVE_TESTS=1`. They are non-destructive and
//! self-cleaning: they create a throwaway namespace and a throwaway ConfigMap, exercise
//! the dry-run and real write/delete paths, and tear everything down — they never touch
//! existing namespaces or resources.
//!
//! Run locally against a cluster:
//!   KAPTEIN_LIVE_TESTS=1 KUBECONFIG=~/.kube/config cargo test -p kaptein-core --test live
//! In CI these are skipped (no KAPTEIN_LIVE_TESTS), so the default `cargo test` stays
//! hermetic.

use k8s_openapi::api::core::v1::{ConfigMap, Namespace};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::Client;
use kube::api::{Api, PostParams};
use kube::core::GroupVersionKind;

/// Are live tests enabled? Defaults to off so the default test run is hermetic and never
/// mutates a cluster by accident.
fn live_enabled() -> bool {
    std::env::var_os("KAPTEIN_LIVE_TESTS").is_some()
}

/// Connect to the cluster and create a throwaway namespace; returns `(client, ns_name)`.
/// The caller is responsible for deleting the namespace (see `teardown`).
async fn setup() -> Option<(Client, String)> {
    if !live_enabled() {
        eprintln!("skipping live test: KAPTEIN_LIVE_TESTS not set");
        return None;
    }
    let client = match kaptein_core::discovery::client().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("skipping live test: cluster unreachable ({e})");
            return None;
        }
    };
    // A unique namespace name so concurrent runs never collide.
    let ns_name = format!("kaptein-it-{}", std::process::id());
    let namespaces: Api<Namespace> = Api::all(client.clone());
    let ns = Namespace {
        metadata: ObjectMeta {
            name: Some(ns_name.clone()),
            ..Default::default()
        },
        spec: None,
        status: None,
    };
    if let Err(e) = namespaces.create(&PostParams::default(), &ns).await {
        eprintln!("skipping live test: cannot create namespace ({e})");
        return None;
    }
    Some((client, ns_name))
}

/// Delete the throwaway namespace (best-effort cleanup).
async fn teardown(client: &Client, ns_name: &str) {
    let namespaces: Api<Namespace> = Api::all(client.clone());
    use kube::api::DeleteParams;
    let _ = namespaces.delete(ns_name, &DeleteParams::default()).await;
}

/// A minimal Deployment fixture for the scale test.
fn deployment(name: &str, replicas: i32) -> k8s_openapi::api::apps::v1::Deployment {
    use k8s_openapi::api::apps::v1::DeploymentSpec;
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    k8s_openapi::api::apps::v1::Deployment {
        metadata: ObjectMeta {
            name: Some(name.into()),
            ..Default::default()
        },
        spec: Some(DeploymentSpec {
            replicas: Some(replicas),
            selector: k8s_openapi::apimachinery::pkg::apis::meta::v1::LabelSelector {
                match_labels: Some(
                    [("app".to_string(), name.to_string())]
                        .into_iter()
                        .collect(),
                ),
                ..Default::default()
            },
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(
                        [("app".to_string(), name.to_string())]
                            .into_iter()
                            .collect(),
                    ),
                    ..Default::default()
                }),
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "c".into(),
                        image: Some("busybox:1.36".into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        status: None,
    }
}

/// The read path: `discovery::list` lists the just-created ConfigMap, and
/// `describe_dynamic` round-trips its YAML with the secret-adjacent values intact.
#[tokio::test]
async fn list_and_describe_exercise_the_read_path() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    let cm_name = "test-cm";
    let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), &ns);
    let cm = ConfigMap {
        metadata: ObjectMeta {
            name: Some(cm_name.into()),
            ..Default::default()
        },
        data: Some(
            [("key".to_string(), "value".to_string())]
                .into_iter()
                .collect(),
        ),
        binary_data: None,
        immutable: None,
    };
    configmaps
        .create(&PostParams::default(), &cm)
        .await
        .expect("create ConfigMap");

    // discovery::list returns the ConfigMap as a ResourceSummary.
    let gvk = GroupVersionKind::gvk("", "v1", "ConfigMap");
    let summaries = kaptein_core::discovery::list(&client, &gvk, Some(&ns))
        .await
        .expect("list ConfigMaps");
    assert!(
        summaries.iter().any(|s| s.name == cm_name),
        "expected {cm_name} in {summaries:?}"
    );

    // describe_dynamic returns redacted YAML (a ConfigMap has no secrets, but the
    // redaction path is still exercised end-to-end).
    let yaml = kaptein_core::describe::describe_dynamic(&client, &gvk, Some(&ns), cm_name)
        .await
        .expect("describe ConfigMap");
    assert!(yaml.contains("test-cm"), "describe should name the object");
    assert!(yaml.contains("key: value"), "describe should include data");

    guard.cleanup().await;
}

/// The write path: `delete` dry-runs without removing, then removes for real.
#[tokio::test]
async fn delete_dry_run_then_real_delete() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    let cm_name = "delete-me";
    let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), &ns);
    configmaps
        .create(
            &PostParams::default(),
            &ConfigMap {
                metadata: ObjectMeta {
                    name: Some(cm_name.into()),
                    ..Default::default()
                },
                data: Some([("k".to_string(), "v".to_string())].into_iter().collect()),
                binary_data: None,
                immutable: None,
            },
        )
        .await
        .expect("create ConfigMap");

    let gvk = GroupVersionKind::gvk("", "v1", "ConfigMap");

    // 1. Dry-run: must not delete, and the report must say "would be deleted".
    let dry = kaptein_core::delete::delete(
        &client,
        &gvk,
        cm_name,
        Some(&ns),
        kube::api::PropagationPolicy::Background,
        false,
    )
    .await
    .expect("dry-run delete");
    assert!(!dry.deleted, "dry-run must not delete");
    assert!(dry.message.contains("would be deleted"), "{}", dry.message);
    assert!(
        configmaps.get_opt(cm_name).await.expect("get").is_some(),
        "dry-run must leave the ConfigMap in place"
    );

    // 2. Real delete: the object is gone.
    let real = kaptein_core::delete::delete(
        &client,
        &gvk,
        cm_name,
        Some(&ns),
        kube::api::PropagationPolicy::Background,
        true,
    )
    .await
    .expect("real delete");
    assert!(real.deleted, "real delete must report deleted=true");
    assert!(
        configmaps.get_opt(cm_name).await.expect("get").is_none(),
        "real delete must remove the ConfigMap"
    );

    guard.cleanup().await;
}

/// The scale write path: `scale` dry-runs without changing, then scales for real.
#[tokio::test]
async fn scale_dry_run_then_real_scale() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    // A Deployment has a `scale` subresource (ConfigMap does not), so it is the minimal
    // workload on which `scale` is a real, verifiable write.
    let deploy_name = "scale-me";
    let deployments: Api<k8s_openapi::api::apps::v1::Deployment> =
        Api::namespaced(client.clone(), &ns);
    deployments
        .create(&PostParams::default(), &deployment(deploy_name, 1))
        .await
        .expect("create Deployment");

    let gvk = GroupVersionKind::gvk("apps", "v1", "Deployment");

    // 1. Dry-run: must not change replicas.
    let dry = kaptein_core::workloads::scale(&client, &gvk, deploy_name, Some(&ns), 3, false)
        .await
        .expect("dry-run scale");
    assert!(!dry.scaled, "dry-run must not scale");
    assert!(dry.message.contains("would scale"), "{}", dry.message);

    // 2. Real scale to 3 replicas.
    let real = kaptein_core::workloads::scale(&client, &gvk, deploy_name, Some(&ns), 3, true)
        .await
        .expect("real scale");
    assert!(real.scaled, "real scale must report scaled=true");
    assert!(real.message.contains("scaled"), "{}", real.message);

    // The Deployment's observed replicas move to 3 (the controller reconciles async;
    // assert the spec, which the write path patches synchronously).
    let d = deployments.get(deploy_name).await.expect("get Deployment");
    let spec_replicas = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
    assert_eq!(spec_replicas, 3, "spec.replicas must be 3 after real scale");

    guard.cleanup().await;
}

/// The dry-run apply paths (M1.3): `dry_run_apply` (create) and `dry_run_apply_patch`
/// (apply) both validate against the server without persisting anything.
#[tokio::test]
async fn dry_run_apply_validates_without_persisting() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    let cm_name = "apply-me";
    // 1. dry_run_apply: a create dry-run must be accepted and must NOT persist.
    let manifest = format!(
        "apiVersion: v1\nkind: ConfigMap\nmetadata:\n  name: {cm_name}\n  namespace: {ns}\ndata:\n  k: v\n"
    );
    let created = kaptein_core::apply::dry_run_apply(&client, &manifest)
        .await
        .expect("dry-run create");
    assert!(created.accepted, "dry-run create must be accepted");
    let configmaps: Api<ConfigMap> = Api::namespaced(client.clone(), &ns);
    assert!(
        configmaps.get_opt(cm_name).await.expect("get").is_none(),
        "dry-run create must not persist"
    );

    // 2. Create for real so the patch dry-run has an existing object to apply against.
    configmaps
        .create(
            &PostParams::default(),
            &ConfigMap {
                metadata: ObjectMeta {
                    name: Some(cm_name.into()),
                    ..Default::default()
                },
                data: Some([("k".to_string(), "v".to_string())].into_iter().collect()),
                binary_data: None,
                immutable: None,
            },
        )
        .await
        .expect("create ConfigMap");

    // 3. dry_run_apply_patch: an apply dry-run against the existing object.
    let patched = kaptein_core::apply::dry_run_apply_patch(&client, &manifest)
        .await
        .expect("dry-run apply patch");
    assert!(patched.accepted, "dry-run apply patch must be accepted");
    // The patch dry-run must not change the persisted data (the value stays "v").
    let cm = configmaps.get(cm_name).await.expect("get ConfigMap");
    let value = cm
        .data
        .as_ref()
        .and_then(|d| d.get("k"))
        .cloned()
        .unwrap_or_default();
    assert_eq!(value, "v", "patch dry-run must not persist a change");

    guard.cleanup().await;
}

/// RAII cleanup: the throwaway namespace is deleted explicitly via `cleanup()` (awaited),
/// and also on drop as a best-effort backstop if an assertion panics before cleanup.
struct Cleanup<'a>(&'a Client, String);

impl<'a> Cleanup<'a> {
    /// Delete the throwaway namespace and await completion, so the test never leaves a
    /// namespace behind (a spawned-and-forgotten task could be dropped at process exit).
    async fn cleanup(&mut self) {
        teardown(self.0, &self.1).await;
    }
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        // Best-effort backstop only: if an assertion panicked before `cleanup()` ran,
        // issue a delete on the current runtime (a panic during teardown is ignored).
        let client = self.0.clone();
        let ns = self.1.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move { teardown(&client, &ns).await });
        }
    }
}
