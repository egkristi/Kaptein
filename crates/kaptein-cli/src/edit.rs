//! `$EDITOR` handoff (M1.3) — edit a resource's YAML locally, then dry-run the result.
//!
//! The safe form of edit-and-apply: fetch the live object as YAML, open it in the
//! user's `$EDITOR` (or `vi` as a fallback), then submit the edited document as a
//! **server-side dry-run** — validating it without ever mutating the cluster. A real
//! apply requires an explicit, separate `kaptein apply --confirm`-style write path.

use std::path::PathBuf;
use std::process::Command;

use kube::Client;

/// Fetch, edit, and dry-run a resource.
pub async fn edit_in_editor(
    client: &Client,
    gvk: &kube::core::GroupVersionKind,
    namespace: Option<&str>,
    name: &str,
) -> Result<String, kaptein_core::Error> {
    // 1. Fetch the current object as YAML — **unredacted**, because the operator edits
    //    real values. A redacted round-trip would submit the literal `[REDACTED]` marker
    //    over every secret value (issue #17). The unmask is audited below.
    let current = kaptein_core::describe::describe_dynamic_policy(
        client,
        gvk,
        namespace,
        name,
        kaptein_core::describe::RedactPolicy::Unredacted,
    )
    .await?;

    // Audit the secret view before showing the values (M1.7 DoD: `Operation::SecretViewed`
    // is emitted when an operator unmasks a secret).
    if kaptein_core::redact::is_secret_kind(&gvk.kind) {
        audit_secret_viewed(&gvk.kind, namespace.unwrap_or_default(), name);
    }

    // 2. Write it to a temp file so the editor has a stable path.
    let tmp = temp_file_for(name)?;
    std::fs::write(&tmp, &current).map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;

    // 3. Open it in $EDITOR (fall back to vi). Block until the editor exits.
    //    $EDITOR may carry arguments (e.g. "code --wait"), so split on whitespace.
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let mut parts = editor.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| kaptein_core::Error::Internal("$EDITOR is empty".to_string()))?;
    let status = Command::new(program)
        .args(parts)
        .arg(&tmp)
        .status()
        .map_err(|e| {
            kaptein_core::Error::Internal(format!(
                "failed to launch editor '{editor}': {e} (set $EDITOR to your editor of choice)"
            ))
        })?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(kaptein_core::Error::Internal(format!(
            "editor '{editor}' exited with {status}"
        )));
    }

    // 4. Read the edited manifest and dry-run it (never applies).
    let edited =
        std::fs::read_to_string(&tmp).map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
    let _ = std::fs::remove_file(&tmp);

    if edited.trim() == current.trim() {
        return Ok("no changes made (edited document matches the live object)".into());
    }

    // 4a. Show the diff (before/after) — the M1.3 "diff before apply" step.
    let diff = kaptein_viewmodel::unified_diff(&current, &edited, 3);
    let diff_text = kaptein_viewmodel::render_unified(&diff, "live", "edited");

    let dry_run = kaptein_core::apply::dry_run_apply_patch(client, &edited).await?;
    let verdict = if dry_run.accepted {
        "dry-run accepted (no changes applied)"
    } else {
        "dry-run REJECTED (no changes applied)"
    };
    Ok(format!(
        "diff ({}+/{}-):\n{diff_text}\n\n{verdict}:\n{}",
        diff.added, diff.removed, dry_run.response_yaml
    ))
}

/// A stable temp-file name for the edit session.
fn temp_file_for(name: &str) -> Result<PathBuf, kaptein_core::Error> {
    let mut p = std::env::temp_dir();
    p.push(format!("kaptein-edit-{name}.yaml"));
    Ok(p)
}

/// Emit a best-effort `SecretViewed` audit event (M1.7 / ADR-0010) when the `edit` path
/// unmasks a secret for editing. Audit-write failure must not block the edit.
fn audit_secret_viewed(kind: &str, namespace: &str, name: &str) {
    use kaptein_viewmodel::audit::{
        Actor, ActorKind, AuditEvent, Operation, Outcome, ResourceRef, Source,
    };
    let context = kaptein_core::discovery::current_context_name().unwrap_or_default();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let event = AuditEvent {
        timestamp: now_ms,
        actor: Actor {
            kind: ActorKind::Human,
            name: std::env::var("USER").unwrap_or_else(|_| "human".into()),
        },
        context,
        operation: Operation::SecretViewed,
        target: ResourceRef {
            group: "".into(),
            kind: kind.into(),
            namespace: namespace.into(),
            name: name.into(),
        },
        outcome: Outcome::Applied,
        source: Source::Tui,
        session_id: "cli".into(),
        reason: None,
        on_behalf_of: None,
    };
    let _ = crate::audit::append(&event);
}
