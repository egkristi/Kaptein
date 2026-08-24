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

    // Pending: check scheduling reasons first.
    if status.phase.as_deref() == Some("Pending") {
        if let Some(f) = unschedulable_finding(status) {
            return vec![f];
        }
        if let Some(f) = image_pull_finding(status) {
            return vec![f];
        }
        if let Some(f) = pvc_binding_finding(status) {
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
        if let Some(f) = container_crash_finding(cs) {
            findings.push(f);
        }
        if let Some(f) = container_not_ready_finding(cs) {
            findings.push(f);
        }
    }
    // Readiness-probe failures surface on the Ready condition with a `last_probe_time`
    // and a reason like `Unhealthy`/`ReadinessProbeFailed` (the "probes" rule of M1.6).
    if let Some(f) = readiness_probe_finding(status) {
        findings.push(f);
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

fn pvc_binding_finding(_status: &PodStatus) -> Option<Finding> {
    // PVC binding failures surface as unschedulable ("persistentvolumeclaim ... not
    // found") in the PodScheduled condition message, so this is a fallback for when the
    // scheduler hasn't recorded a reason. We return None here; full PVC analysis needs
    // the PVC resources themselves (a later rule pack).
    None
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
    let terminated = cs.state.as_ref()?.terminated.as_ref()?;
    let reason = terminated.reason.as_deref().unwrap_or("Error");
    Some(Finding {
        code: "crash_loop".into(),
        summary: format!(
            "Container '{}' terminated with exit code {:?} ({reason}) and restart count {}.",
            cs.name, terminated.exit_code, cs.restart_count
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
}
