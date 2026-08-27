//! Diagnostics fixture corpus (review item #11, owned by M1.6).
//!
//! Regression tests that feed **canonical API-server JSON shapes** through the
//! diagnostics rule engine and assert the expected findings. Unlike the inline unit
//! tests (which build structs by hand), these deserialize real payloads, so they catch
//! field-name mismatches (e.g. `restartCount` vs `restart_count`) and shape drift.

use k8s_openapi::api::core::v1::Pod;
use kaptein_core::diagnostics::{Finding, diagnose};
use std::path::PathBuf;

/// Load a fixture pod from the JSON corpus.
fn load_fixture(name: &str) -> Pod {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("failed to parse fixture {}: {e}", path.display()))
}

fn codes(pod: &Pod) -> Vec<String> {
    diagnose(pod)
        .into_iter()
        .map(|Finding { code, .. }| code)
        .collect()
}

#[test]
fn ready_pod_yields_no_findings() {
    let pod = load_fixture("ready.json");
    assert_eq!(codes(&pod), Vec::<String>::new());
}

#[test]
fn crashloop_backoff_is_detected_from_last_state() {
    let pod = load_fixture("crashloop_backoff.json");
    let got = codes(&pod);
    assert!(got.contains(&"crash_loop_backoff".into()), "got {got:?}");
    // The strong last-state signal must not also emit the weaker "not ready" fallback.
    assert!(!got.contains(&"container_not_ready".into()), "got {got:?}");
}

#[test]
fn exit_zero_job_is_not_a_crash() {
    let pod = load_fixture("exit_zero_job.json");
    let got = codes(&pod);
    assert!(
        !got.iter()
            .any(|c| c == "crash_loop" || c == "crash_loop_backoff"),
        "exit-0 job must not be a crash, got {got:?}"
    );
}

#[test]
fn image_pull_backoff_is_detected() {
    let pod = load_fixture("image_pull_backoff.json");
    let got = codes(&pod);
    assert!(got.contains(&"image_pull".into()), "got {got:?}");
}

#[test]
fn unschedulable_is_detected() {
    let pod = load_fixture("unschedulable.json");
    let got = codes(&pod);
    assert!(got.contains(&"unschedulable".into()), "got {got:?}");
}

#[test]
fn readiness_probe_failure_is_detected() {
    let pod = load_fixture("readiness_probe.json");
    let got = codes(&pod);
    assert!(got.contains(&"readiness_probe".into()), "got {got:?}");
}

#[test]
fn oom_killed_is_detected() {
    let pod = load_fixture("oom_killed.json");
    let got = codes(&pod);
    assert!(got.contains(&"oom_killed".into()), "got {got:?}");
}

#[test]
fn pvc_binding_failure_is_detected() {
    let pod = load_fixture("pvc_binding.json");
    let got = codes(&pod);
    assert!(got.contains(&"pvc_binding".into()), "got {got:?}");
}
