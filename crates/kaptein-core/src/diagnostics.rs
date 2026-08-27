//! Diagnostics rule engine — "why isn't this pod ready?"
//!
//! The minimal rule engine (M1.6): a set of rules over pod status that produce an
//! evidence chain, not just a verdict. This is the single engine that feeds the landing
//! view, the TUI diagnostics, and the MCP diagnostic moat (ADR-0013).

use k8s_openapi::api::core::v1::{ContainerStatus, Pod, PodCondition, PodStatus};

/// A single diagnostic finding with an evidence-based reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Stable machine-readable code (e.g. `image_pull_backoff`, `unschedulable`).
    pub code: String,
    /// Human-readable summary (localized by the frontend).
    pub summary: String,
}

/// Evaluate a pod and produce an ordered list of findings explaining why it is not ready
/// (or why it is pending). Returns an empty list if the pod is ready or has no status.
pub fn diagnose(pod: &Pod) -> Vec<Finding> {
    let Some(status) = &pod.status else {
        return vec![Finding {
            code: "no_status".into(),
            summary: "Pod has no status yet.".into(),
        }];
    };

    // Pending: check scheduling reasons first. PVC binding is the *most specific*
    // unschedulable signal (the scheduler tried and failed to bind a claim), so it is
    // checked before the generic `unschedulable` fallback.
    if status.phase.as_deref() == Some("Pending") {
        if let Some(f) = pvc_binding_finding(status) {
            return vec![f];
        }
        if let Some(f) = unschedulable_finding(status) {
            return vec![f];
        }
        if let Some(f) = image_pull_finding(status) {
            return vec![f];
        }
        return vec![Finding {
            code: "pending".into(),
            summary: "Pod is Pending but no specific reason is recorded.".into(),
        }];
    }

    // Running/other: check container readiness and restarts.
    let mut findings = Vec::new();
    for cs in status.container_statuses.as_ref().into_iter().flatten() {
        // CrashLoopBackOff (current waiting + last_state.terminated) is the strongest
        // signal — report it first and skip the weaker "not ready" fallback.
        if let Some(f) = crash_loop_backoff_finding(cs) {
            findings.push(f);
            continue;
        }
        if let Some(f) = container_crash_finding(cs) {
            findings.push(f);
        }
        if let Some(f) = container_not_ready_finding(cs) {
            findings.push(f);
        }
    }
    // A Running pod whose container enters ImagePullBackOff after an image update is
    // not Pending — surface the pull failure here too (the Pending branch already does).
    if let Some(f) = image_pull_finding(status) {
        findings.push(f);
    }
    // Readiness-probe failures surface on the Ready condition with a `last_probe_time`
    // and a reason like `Unhealthy`/`ReadinessProbeFailed` (the "probes" rule of M1.6).
    if let Some(f) = readiness_probe_finding(status) {
        findings.push(f);
    }
    // OOM forensics: a container killed by the kernel (exit 137 / reason OOMKilled) is
    // a distinct signal from a plain crash — even without CrashLoopBackOff, the operator
    // should see "out of memory", not "crashed". Covers both a currently-terminated
    // container and a restart whose last_state.terminated was OOM-killed.
    for cs in status.container_statuses.as_ref().into_iter().flatten() {
        if let Some(f) = oom_killed_finding(cs) {
            findings.push(f);
        }
    }
    if findings.is_empty() && !is_ready(status) {
        findings.push(Finding {
            code: "not_ready".into(),
            summary: "Pod is not ready, but no container-level reason is recorded.".into(),
        });
    }
    findings
}

/// Whether the pod's Ready condition is true.
pub fn is_ready(status: &PodStatus) -> bool {
    status
        .conditions
        .as_ref()
        .into_iter()
        .flatten()
        .any(|c| c.type_ == "Ready" && c.status == "True")
}

fn unschedulable_finding(status: &PodStatus) -> Option<Finding> {
    let c = condition(status, "PodScheduled")?;
    if c.status == "False" {
        let reason = c.reason.as_deref().unwrap_or("Unschedulable");
        let msg = c.message.as_deref().unwrap_or("No message");
        return Some(Finding {
            code: "unschedulable".into(),
            summary: format!("Pod is unschedulable: {reason} — {msg}"),
        });
    }
    None
}

fn image_pull_finding(status: &PodStatus) -> Option<Finding> {
    for cs in status.container_statuses.as_ref().into_iter().flatten() {
        let Some(reason) = cs
            .state
            .as_ref()
            .and_then(|w| w.waiting.as_ref())
            .and_then(|w| w.reason.as_deref())
        else {
            continue;
        };
        if reason.contains("ImagePull") || reason.contains("ErrImage") {
            return Some(Finding {
                code: "image_pull".into(),
                summary: format!("Container '{}' cannot pull its image: {reason}", cs.name),
            });
        }
    }
    None
}

fn pvc_binding_finding(status: &PodStatus) -> Option<Finding> {
    // A PVC-binding failure surfaces in the `PodScheduled` condition's message as
    // `persistentvolumeclaim "<name>" not found` (the scheduler tried and failed to bind
    // the claim). Extract the claim name from the message rather than fetching the PVC
    // resources themselves (a full PVC analysis — storage class, provisioner — is a
    // Phase 3a rule pack).
    let c = condition(status, "PodScheduled")?;
    let msg = c.message.as_deref().unwrap_or_default();
    let lower = msg.to_ascii_lowercase();
    if !lower.contains("persistentvolumeclaim") || !lower.contains("not found") {
        return None;
    }
    // Pull the claim name out of the message, falling back to a generic summary.
    let claim = msg
        .split("persistentvolumeclaim")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .map(|c| c.to_string());
    let summary = match claim {
        Some(c) => format!("Pod cannot bind PVC '{c}': not found."),
        None => "Pod cannot bind a persistentvolumeclaim: not found.".into(),
    };
    Some(Finding {
        code: "pvc_binding".into(),
        summary,
    })
}

/// The "probes" rule (M1.6): a `Ready=False` condition with a `last_probe_time` indicates
/// a readiness-probe failure. Surface the probe reason/message so the operator knows it
/// is a probe, not a crash.
fn readiness_probe_finding(status: &PodStatus) -> Option<Finding> {
    let ready = condition(status, "Ready")?;
    if ready.status == "False" && ready.last_probe_time.is_some() {
        let reason = ready.reason.as_deref().unwrap_or("ReadinessProbeFailed");
        let msg = ready.message.as_deref().unwrap_or("no message");
        return Some(Finding {
            code: "readiness_probe".into(),
            summary: format!("Readiness probe failing: {reason} — {msg}"),
        });
    }
    None
}

fn container_crash_finding(cs: &ContainerStatus) -> Option<Finding> {
    // A crash is a **terminated** container with a non-zero exit code, OR a restart that
    // was previously terminated non-zero (`last_state`). A Job that completed with exit 0
    // is *not* a crash, and a CrashLoopBackOff pod is caught via `last_state` when its
    // current `state` is `waiting`.
    let terminated = cs.state.as_ref()?.terminated.as_ref()?;
    if terminated.exit_code == 0 {
        return None;
    }
    let reason = terminated.reason.as_deref().unwrap_or("Error");
    Some(Finding {
        code: "crash_loop".into(),
        summary: format!(
            "Container '{}' terminated with exit code {} ({reason}) and restart count {}.",
            cs.name, terminated.exit_code, cs.restart_count
        ),
    })
}

/// A container in CrashLoopBackOff has a current `waiting` state (reason
/// `CrashLoopBackOff`) and a `last_state.terminated` with a non-zero exit — the actual
/// evidence of the crash. This is the "OOM forensics via lastTerminatedState" path the
/// README promises.
fn crash_loop_backoff_finding(cs: &ContainerStatus) -> Option<Finding> {
    let waiting = cs.state.as_ref()?.waiting.as_ref()?;
    let reason = waiting.reason.as_deref()?;
    // Only `CrashLoopBackOff` (or a bare `BackOff`) is the crash-loop signal. A naive
    // `contains("BackOff")` would also match `ImagePullBackOff`, which is an image-pull
    // failure, not a crash.
    if !reason.eq_ignore_ascii_case("CrashLoopBackOff") && !reason.eq_ignore_ascii_case("BackOff") {
        return None;
    }
    let last_terminated = cs.last_state.as_ref()?.terminated.as_ref()?;
    Some(Finding {
        code: "crash_loop_backoff".into(),
        summary: format!(
            "Container '{}' is in {} — last terminated with exit code {:?} ({}).",
            cs.name,
            reason,
            last_terminated.exit_code,
            last_terminated.reason.as_deref().unwrap_or("Error")
        ),
    })
}

fn container_not_ready_finding(cs: &ContainerStatus) -> Option<Finding> {
    if !cs.ready {
        let state_desc = match &cs.state {
            Some(s) if s.waiting.is_some() => {
                let reason = s
                    .waiting
                    .as_ref()
                    .and_then(|w| w.reason.clone())
                    .unwrap_or_else(|| "Waiting".into());
                format!("waiting ({reason})")
            }
            _ => "not ready".to_string(),
        };
        return Some(Finding {
            code: "container_not_ready".into(),
            summary: format!("Container '{}' is {state_desc}.", cs.name),
        });
    }
    None
}

/// An out-of-memory kill: the container's current `terminated` state (or its
/// `last_state.terminated`, when the container restarted) has reason `OOMKilled` or exit
/// code 137 (SIGKILL, the kernel OOM-killer's exit). This is the "OOM forensics" rule
/// (README): a memory kill is a capacity signal, not a code bug.
fn oom_killed_finding(cs: &ContainerStatus) -> Option<Finding> {
    // Prefer the current terminated state; fall back to the last state (a restart after
    // an OOM kill has `state.waiting`/`running` and the OOM evidence in `last_state`).
    let terminated = cs
        .state
        .as_ref()
        .and_then(|s| s.terminated.as_ref())
        .or_else(|| cs.last_state.as_ref().and_then(|s| s.terminated.as_ref()))?;
    let is_oom = terminated.reason.as_deref() == Some("OOMKilled") || terminated.exit_code == 137;
    if !is_oom {
        return None;
    }
    Some(Finding {
        code: "oom_killed".into(),
        summary: format!(
            "Container '{}' was OOM-killed (exit code {}).",
            cs.name, terminated.exit_code
        ),
    })
}

fn condition<'a>(status: &'a PodStatus, ty: &str) -> Option<&'a PodCondition> {
    status.conditions.as_ref()?.iter().find(|c| c.type_ == ty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateTerminated, ContainerStateWaiting,
    };

    fn pod_with(phase: &str, ready: bool) -> Pod {
        Pod {
            metadata: Default::default(),
            spec: None,
            status: Some(PodStatus {
                phase: Some(phase.into()),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".into(),
                    status: if ready { "True" } else { "False" }.into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn ready_pod_has_no_findings() {
        let pod = pod_with("Running", true);
        assert!(diagnose(&pod).is_empty());
    }

    #[test]
    fn pvc_binding_failure_is_detected_and_names_the_claim() {
        // A Pending pod whose PodScheduled condition carries the "persistentvolumeclaim
        // not found" message must surface as pvc_binding (before the generic
        // unschedulable fallback), with the claim name extracted.
        let mut pod = pod_with("Pending", false);
        if let Some(status) = &mut pod.status {
            status.conditions = Some(vec![
                PodCondition {
                    type_: "Ready".into(),
                    status: "False".into(),
                    ..Default::default()
                },
                PodCondition {
                    type_: "PodScheduled".into(),
                    status: "False".into(),
                    reason: Some("Unschedulable".into()),
                    message: Some(
                        "0/1 nodes are available: persistentvolumeclaim \"db-pvc\" not found."
                            .into(),
                    ),
                    ..Default::default()
                },
            ]);
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "pvc_binding"),
            "expected pvc_binding finding, got {findings:?}"
        );
        let f = findings.iter().find(|f| f.code == "pvc_binding").unwrap();
        assert!(
            f.summary.contains("db-pvc"),
            "summary should name the claim: {}",
            f.summary
        );
    }

    #[test]
    fn image_pull_backoff_is_detected() {
        let mut pod = pod_with("Pending", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                state: Some(ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("ImagePullBackOff".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "image_pull"),
            "expected image_pull finding, got {findings:?}"
        );
    }

    #[test]
    fn crash_loop_is_detected() {
        let mut pod = pod_with("Running", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                restart_count: 5,
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 1,
                        reason: Some("Error".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "crash_loop"),
            "expected crash_loop finding, got {findings:?}"
        );
    }

    #[test]
    fn oom_killed_is_detected_from_terminated_state() {
        let mut pod = pod_with("Running", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                restart_count: 0,
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 137,
                        reason: Some("OOMKilled".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "oom_killed"),
            "expected oom_killed finding, got {findings:?}"
        );
    }

    #[test]
    fn oom_killed_is_detected_from_last_state_after_restart() {
        // A restart after an OOM kill: current state is running/waiting, the OOM
        // evidence is in last_state.terminated. Must still surface as oom_killed.
        let mut pod = pod_with("Running", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                restart_count: 2,
                state: Some(ContainerState {
                    running: Some(Default::default()),
                    ..Default::default()
                }),
                last_state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 137,
                        reason: Some("OOMKilled".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "oom_killed"),
            "expected oom_killed from last_state, got {findings:?}"
        );
    }

    #[test]
    fn readiness_probe_failure_is_detected() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
        let mut pod = pod_with("Running", false);
        if let Some(status) = &mut pod.status {
            // A Ready=False condition with a last_probe_time signals a probe failure.
            if let Some(cond) = status.conditions.as_mut().and_then(|c| c.first_mut()) {
                cond.reason = Some("Unhealthy".into());
                cond.message = Some("readiness probe failed: connection refused".into());
                cond.last_probe_time = Some(Time(k8s_openapi::jiff::Timestamp::now()));
            }
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "readiness_probe"),
            "expected readiness_probe finding, got {findings:?}"
        );
    }

    #[test]
    fn exit_zero_job_is_not_a_crash() {
        let mut pod = pod_with("Succeeded", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                restart_count: 0,
                state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 0,
                        reason: Some("Completed".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            !findings.iter().any(|f| f.code == "crash_loop"),
            "exit-0 job must not be a crash, got {findings:?}"
        );
    }

    #[test]
    fn crash_loop_backoff_uses_last_state() {
        let mut pod = pod_with("Running", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                restart_count: 7,
                state: Some(ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("CrashLoopBackOff".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                last_state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 137,
                        reason: Some("OOMKilled".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            findings.iter().any(|f| f.code == "crash_loop_backoff"),
            "expected crash_loop_backoff finding, got {findings:?}"
        );
    }

    #[test]
    fn image_pull_backoff_is_not_a_crash_loop() {
        // ImagePullBackOff must NOT be misreported as crash_loop_backoff (it is an
        // image-pull failure, not a crash). It should surface as image_pull.
        let mut pod = pod_with("Running", false);
        if let Some(status) = &mut pod.status {
            status.container_statuses = Some(vec![ContainerStatus {
                name: "app".into(),
                ready: false,
                restart_count: 0,
                state: Some(ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("ImagePullBackOff".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                last_state: Some(ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        exit_code: 137,
                        reason: Some("OOMKilled".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }]);
        }
        let findings = diagnose(&pod);
        assert!(
            !findings.iter().any(|f| f.code == "crash_loop_backoff"),
            "ImagePullBackOff must not be crash_loop_backoff, got {findings:?}"
        );
        assert!(
            findings.iter().any(|f| f.code == "image_pull"),
            "expected image_pull finding for a Running pod, got {findings:?}"
        );
    }
}
