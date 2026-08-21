//! Local audit log — append JSONL `AuditEvent` records to disk.
//!
//! The audit log records **operations, not values** (secrets never reach it). This is
//! the first consumer of the `AuditEvent` contract; the incident-timeline export is the
//! second (one format, two consumers — see `docs/architecture.md`).

use std::path::PathBuf;

use kaptein_viewmodel::audit::AuditEvent;

/// Resolve the audit log path: `$KAPTEIN_AUDIT`, else `$XDG_STATE_HOME/kaptein/audit.jsonl`,
/// else `~/.local/state/kaptein/audit.jsonl`.
pub fn audit_path() -> PathBuf {
    if let Ok(p) = std::env::var("KAPTEIN_AUDIT") {
        return PathBuf::from(p);
    }
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("kaptein").join("audit.jsonl");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("kaptein")
            .join("audit.jsonl");
    }
    PathBuf::from("audit.jsonl")
}

/// Append a single audit event as one JSON line. Failures to open/write are returned
/// rather than panicking, but the caller (the MCP/CLI tool path) should not let an audit
/// failure block the actual operation — audit is best-effort from the tool's perspective.
pub fn append(event: &AuditEvent) -> std::io::Result<()> {
    append_to(&audit_path(), event)
}

/// Append to an explicit path (used by tests to avoid mutating process env).
fn append_to(path: &std::path::Path, event: &AuditEvent) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaptein_viewmodel::audit::{Actor, ActorKind, Operation, Outcome, ResourceRef, Source};

    #[test]
    fn append_and_read_back() {
        let tmp = std::env::temp_dir().join("kaptein-audit-test.jsonl");
        let event = AuditEvent {
            timestamp: 1_700_000_000_000,
            actor: Actor {
                kind: ActorKind::Agent,
                name: "agent-x".into(),
            },
            context: "prod".into(),
            operation: Operation::List,
            target: ResourceRef {
                group: "".into(),
                kind: "Pod".into(),
                namespace: "default".into(),
                name: "".into(),
            },
            outcome: Outcome::Applied,
            source: Source::Mcp,
            session_id: "sess-1".into(),
            reason: None,
            on_behalf_of: Some("erling".into()),
        };
        append_to(&tmp, &event).unwrap();
        let text = std::fs::read_to_string(&tmp).unwrap();
        assert!(text.contains("\"operation\":\"List\""));
        std::fs::remove_file(&tmp).ok();
    }
}
