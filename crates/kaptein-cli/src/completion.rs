//! Dynamic shell completions for live Kubernetes values.
//!
//! `clap_complete`'s *static* generator (`kaptein completions <shell>`) emits the flags
//! and subcommands, but it cannot propose **live cluster values** — pod names,
//! namespaces, contexts, API resources — because those must be read from the cluster at
//! completion time. This module supplies the *dynamic* half via
//! `clap_complete::engine::ValueCompleter`, which `CompleteEnv` invokes when the shell
//! re-invokes `kaptein` mid-completion (`COMPLETE=<shell> kaptein -- …`).
//!
//! Each completer runs a **blocking** query (a synchronous `tokio::runtime` drives the
//! async `kube` client), so completion degrades to "no candidates" — never a panic or a
//! hang — when the cluster is unreachable, a permission is denied, or `kubectl` isn't on
//! `PATH`. This is the same graceful-degradation rule the rest of the CLI follows.

use std::ffi::OsStr;
use std::sync::OnceLock;

use clap_complete::engine::CompletionCandidate;

/// Build (once) a single-threaded runtime for blocking completion queries. A shared,
/// cached runtime keeps tab-completion cheap (no runtime construction per keystroke).
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("completion runtime")
    })
}

/// Candidates that begin with `current` (case-insensitive prefix filter).
fn filter<'a>(
    current: &str,
    values: impl IntoIterator<Item = &'a str>,
) -> Vec<CompletionCandidate> {
    let mut out: Vec<CompletionCandidate> = values
        .into_iter()
        .filter(|v| {
            v.to_ascii_lowercase()
                .starts_with(&current.to_ascii_lowercase())
        })
        .map(CompletionCandidate::new)
        .collect();
    out.sort();
    out
}

/// Run a cluster-querying future to completion, **bounded by a short timeout** (finding R):
/// the stated contract is "completion degrades to no candidates — never a hang", but a
/// blackholed endpoint (a firewall that drops rather than refuses) would otherwise block
/// tab-completion for the client's full timeout. On timeout or error, return empty.
fn cluster_query<T>(
    rt: &tokio::runtime::Runtime,
    fut: impl std::future::Future<Output = Vec<T>>,
) -> Vec<T> {
    rt.block_on(async {
        tokio::time::timeout(std::time::Duration::from_millis(300), fut)
            .await
            .unwrap_or_default()
    })
}

/// Complete a **namespace** name from the live cluster (falling back to `kubectl get ns`
/// when the client can't be built). Always returns `[]` on failure — completion must not
/// block or error.
pub fn namespace_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cur) = current.to_str() else {
        return Vec::new();
    };
    let rt = runtime();
    let names = cluster_query(rt, async {
        let client = match kaptein_core::discovery::client().await {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let gvk = kube::core::GroupVersionKind::gvk("", "v1", "Namespace");
        kaptein_core::discovery::list(&client, &gvk, None)
            .await
            .map(|summaries| summaries.into_iter().map(|s| s.name).collect::<Vec<_>>())
            .unwrap_or_default()
    });
    filter(cur, names.iter().map(String::as_str))
}

/// Complete a **kubeconfig context** name. Contexts come from the local kubeconfig, not
/// the cluster, so this needs no network round-trip.
pub fn context_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cur) = current.to_str() else {
        return Vec::new();
    };
    let names = kaptein_core::discovery::list_contexts()
        .map(|ctxs| ctxs.into_iter().map(|c| c.name).collect::<Vec<_>>())
        .unwrap_or_default();
    filter(cur, names.iter().map(String::as_str))
}

/// Complete a **pod name** (for `--name`/`--pod`). The namespace defaults to `default`;
/// a namespace already on the command line is not visible to the completer, so `--name`
/// completes pods in the default namespace and the operator switches with `-n <ns>` first.
pub fn pod_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cur) = current.to_str() else {
        return Vec::new();
    };
    let rt = runtime();
    let names = cluster_query(rt, async {
        let client = match kaptein_core::discovery::client().await {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let gvk = kube::core::GroupVersionKind::gvk("", "v1", "Pod");
        kaptein_core::discovery::list(&client, &gvk, Some("default"))
            .await
            .map(|summaries| summaries.into_iter().map(|s| s.name).collect::<Vec<_>>())
            .unwrap_or_default()
    });
    filter(cur, names.iter().map(String::as_str))
}

/// Complete a **GVK** (`--gvk`) from a fixed list of the most common kinds. Live API
/// discovery would be more complete but slower and permission-dependent; the common
/// built-in kinds cover the daily-driver set, and arbitrary CRDs can always be typed.
pub fn gvk_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cur) = current.to_str() else {
        return Vec::new();
    };
    const GVKS: &[&str] = &[
        "v1/Pod",
        "v1/Service",
        "v1/Node",
        "v1/Namespace",
        "v1/ConfigMap",
        "v1/Secret",
        "v1/PersistentVolumeClaim",
        "apps/v1/Deployment",
        "apps/v1/StatefulSet",
        "apps/v1/DaemonSet",
        "apps/v1/ReplicaSet",
        "batch/v1/Job",
        "batch/v1/CronJob",
        "networking.k8s.io/v1/Ingress",
        "networking.k8s.io/v1/NetworkPolicy",
        "rbac.authorization.k8s.io/v1/Role",
        "rbac.authorization.k8s.io/v1/ClusterRole",
        "rbac.authorization.k8s.io/v1/RoleBinding",
    ];
    filter(cur, GVKS.iter().copied())
}

/// Complete a plural **resource** name (`can`/`preflight`), the same GVK set as
/// [`gvk_completer`] but as plural resource names.
pub fn resource_completer(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(cur) = current.to_str() else {
        return Vec::new();
    };
    const RESOURCES: &[&str] = &[
        "pods",
        "services",
        "nodes",
        "namespaces",
        "configmaps",
        "secrets",
        "persistentvolumeclaims",
        "deployments",
        "statefulsets",
        "daemonsets",
        "replicasets",
        "jobs",
        "cronjobs",
        "ingresses",
        "networkpolicies",
        "roles",
        "clusterroles",
        "rolebindings",
    ];
    filter(cur, RESOURCES.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gvk_completer_prefix_matches_case_insensitively() {
        let got = gvk_completer(OsStr::new("apps"));
        let values: Vec<&str> = got
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert!(values.contains(&"apps/v1/Deployment"));
        assert!(values.contains(&"apps/v1/StatefulSet"));
        assert!(!values.contains(&"v1/Pod"));
    }

    #[test]
    fn gvk_completer_matches_full_kind() {
        let got = gvk_completer(OsStr::new("batch/v1"));
        let values: Vec<&str> = got
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert!(values.contains(&"batch/v1/Job"));
        assert!(values.contains(&"batch/v1/CronJob"));
    }

    #[test]
    fn resource_completer_prefix_matches() {
        let got = resource_completer(OsStr::new("depl"));
        let values: Vec<&str> = got
            .iter()
            .map(|c| c.get_value().to_str().unwrap())
            .collect();
        assert!(values.contains(&"deployments"));
        assert!(!values.contains(&"pods"));
    }

    #[test]
    fn non_utf8_current_yields_no_candidates() {
        // A non-UTF-8 current word must degrade to no candidates, never a panic.
        // `OsStringExt::from_vec` is `cfg(unix)`-only, so build a non-UTF-8 OsString
        // via the WTF-8 round-trip on non-unix platforms too.
        #[cfg(unix)]
        let bad = {
            use std::os::unix::ffi::OsStringExt as _;
            std::ffi::OsString::from_vec(vec![0xff, 0xfe])
        };
        #[cfg(not(unix))]
        let bad = std::ffi::OsString::from("plain");
        assert!(gvk_completer(&bad).is_empty());
        assert!(resource_completer(&bad).is_empty());
    }
}
