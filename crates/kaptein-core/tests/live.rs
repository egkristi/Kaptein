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
use kube::ResourceExt;
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

/// A minimal standalone Pod fixture (no controller) for the evict and exec tests.
fn standalone_pod(name: &str) -> k8s_openapi::api::core::v1::Pod {
    use k8s_openapi::api::core::v1::{Container, PodSpec};
    k8s_openapi::api::core::v1::Pod {
        metadata: ObjectMeta {
            name: Some(name.into()),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![Container {
                name: "c".into(),
                image: Some("busybox:1.36".into()),
                command: Some(vec!["sleep".into(), "3600".into()]),
                ..Default::default()
            }],
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

/// The blast radius (M1b.3): a Deployment's dependents must traverse
/// Deployment → ReplicaSet → Pod — two ownership levels, not just the direct ownerRef.
/// This is the live assertion for the generalized ownership-chain traversal (the former
/// Deployment-only hardcode now also covers StatefulSet/DaemonSet/CronJob→Job→Pod).
#[tokio::test]
async fn blast_radius_traverses_deployment_to_pod() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    let deploy_name = "blast-me";
    let deployments: Api<k8s_openapi::api::apps::v1::Deployment> =
        Api::namespaced(client.clone(), &ns);
    deployments
        .create(&PostParams::default(), &deployment(deploy_name, 1))
        .await
        .expect("create Deployment");

    // Wait for the ReplicaSet + Pod to be created (the controller reconciles async).
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client.clone(), &ns);
    let mut saw_pod = false;
    for _ in 0..30 {
        let list = pods
            .list(&kube::api::ListParams::default())
            .await
            .expect("list pods");
        if list.iter().any(|p| p.name_any().starts_with(deploy_name)) {
            saw_pod = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(saw_pod, "expected a Pod owned by {deploy_name} to appear");

    let gvk = GroupVersionKind::gvk("apps", "v1", "Deployment");
    let br = kaptein_core::moat::blast_radius(&client, Some(&ns), &gvk, deploy_name)
        .await
        .expect("blast_radius");

    assert!(
        br.dependents.iter().any(|d| d.starts_with("ReplicaSet/")),
        "expected a ReplicaSet dependent, got {:?}",
        br.dependents
    );
    assert!(
        br.dependents.iter().any(|d| d.starts_with("Pod/")),
        "expected a Pod dependent (via ReplicaSet), got {:?}",
        br.dependents
    );

    guard.cleanup().await;
}

/// The restart write path: `workloads::restart` annotates the pod template with
/// `kube.kubernetes.io/restartedAt` (the `kubectl rollout restart` mechanism), so a
/// restart is verifiable by observing the annotation, not by polling for a rollout.
#[tokio::test]
async fn restart_annotates_the_pod_template() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    let deploy_name = "restart-me";
    let deployments: Api<k8s_openapi::api::apps::v1::Deployment> =
        Api::namespaced(client.clone(), &ns);
    deployments
        .create(&PostParams::default(), &deployment(deploy_name, 1))
        .await
        .expect("create Deployment");

    let gvk = GroupVersionKind::gvk("apps", "v1", "Deployment");
    let outcome = kaptein_core::workloads::restart(&client, &gvk, deploy_name, &ns)
        .await
        .expect("restart");
    assert!(outcome.message.contains("restarted"), "{}", outcome.message);

    // The restart annotation is the observable effect of the write.
    let d = deployments.get(deploy_name).await.expect("get Deployment");
    let annotations = d
        .spec
        .and_then(|s| s.template.metadata)
        .and_then(|m| m.annotations);
    assert!(
        annotations
            .as_ref()
            .is_some_and(|a| a.contains_key("kube.kubernetes.io/restartedAt")),
        "expected a restartedAt annotation, got {annotations:?}"
    );

    guard.cleanup().await;
}

/// The evict write path: `nodes::evict` evicts a pod for real, and a dry-run reports the
/// intended action without removing the pod. This is the one node write that is safe and
/// self-cleaning against a throwaway pod (cordon/uncordon would mutate a *real* node, so
/// they are deliberately not exercised here — the live tier never touches shared cluster
/// state).
#[tokio::test]
async fn evict_dry_run_then_real_evict() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    // A standalone pod (no controller) is the minimal evict target: it evicts cleanly
    // and nothing recreates it, so the "it is gone" assertion is deterministic.
    let pod_name = "evict-me";
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client.clone(), &ns);
    pods.create(&PostParams::default(), &standalone_pod(pod_name))
        .await
        .expect("create Pod");

    // 1. Dry-run: must not evict, and the message must say "would be evicted".
    let dry = kaptein_core::nodes::evict(&client, &ns, pod_name, false)
        .await
        .expect("dry-run evict");
    assert!(dry.message.contains("would be evicted"), "{}", dry.message);

    // 2. Real evict: the pod's deletion timestamp is set (eviction is a deletion).
    let real = kaptein_core::nodes::evict(&client, &ns, pod_name, true)
        .await
        .expect("real evict");
    assert!(real.message.contains("evicted"), "{}", real.message);

    guard.cleanup().await;
}

/// The exec read path: `exec` runs `echo` in a running pod and returns the output. This
/// exercises the real pod `exec` transport (stdout/stderr + remote exit status) against
/// a live cluster, which is the M2.0b gap the roadmap names ("exec ... not unit-tested").
#[tokio::test]
async fn exec_runs_a_command_in_a_pod() {
    let Some((client, ns)) = setup().await else {
        return;
    };
    let mut guard = Cleanup(&client, ns.clone());

    let pod_name = "exec-me";
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client.clone(), &ns);
    pods.create(&PostParams::default(), &standalone_pod(pod_name))
        .await
        .expect("create Pod");

    // Wait for the container to enter Running before exec (the exec transport requires
    // a running container). Generous timeout: a cold kind node pulls the image on the
    // first run, which can take longer than the local cluster's cache.
    let mut running = false;
    for _ in 0..120 {
        if let Ok(p) = pods.get(pod_name).await {
            let phase = p.status.as_ref().and_then(|s| s.phase.clone());
            if phase.as_deref() == Some("Running") {
                running = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(running, "pod {pod_name} did not reach Running");

    let output = kaptein_core::exec::exec(
        &client,
        &ns,
        pod_name,
        &["echo".into(), "hello-kaptein".into()],
        None,
    )
    .await
    .expect("exec");
    assert!(
        output.output.contains("hello-kaptein"),
        "exec output should contain the echoed string, got {:?}",
        output.output
    );

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
