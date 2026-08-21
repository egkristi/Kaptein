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
    // 1. Fetch the current object as YAML.
    let current = kaptein_core::describe::describe_dynamic(client, gvk, namespace, name).await?;

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

    let dry_run = kaptein_core::apply::dry_run_apply_patch(client, &edited).await?;
    let verdict = if dry_run.accepted {
        "dry-run accepted (no changes applied)"
    } else {
        "dry-run REJECTED (no changes applied)"
    };
    Ok(format!("{verdict}:\n{}", dry_run.response_yaml))
}

/// A stable temp-file name for the edit session.
fn temp_file_for(name: &str) -> Result<PathBuf, kaptein_core::Error> {
    let mut p = std::env::temp_dir();
    p.push(format!("kaptein-edit-{name}.yaml"));
    Ok(p)
}
