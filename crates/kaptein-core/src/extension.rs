//! Extension manifest + discovery — the shared `extension.yaml` declaration (ADR-0004).
//!
//! Every extension (lens, WASM plugin, or shell-out integration) is declared by an
//! `extension.yaml` manifest and discovered from configurable, Git-backed paths — no
//! central marketplace. This module parses and validates that manifest. The lifecycle
//! (`kaptein extension {list,enable,disable}`) builds on it; tier-2/3 *loading* (wasmtime,
//! shell-out) lands with M2.6.
//!
//! The manifest *schema* is MIT/Apache-2.0 (extension surface, ADR-0004); this Rust
//! implementation lives in the BUSL core.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The extension-manifest schema version this release accepts.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// The three extension tiers (ADR-0004), chosen data-first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionKind {
    /// A declarative view definition (no code).
    Lens,
    /// A WASM component-model plugin (sandboxed code).
    Plugin,
    /// A shell-out integration to an external binary.
    Integration,
}

/// The shared extension manifest (`extension.yaml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionManifest {
    /// Unique reverse-DNS id, e.g. `"com.example.cnpg-lens"`.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// SemVer string, e.g. `"1.2.0"`.
    pub version: String,
    /// The manifest schema version. Refused if it differs from
    /// [`MANIFEST_SCHEMA_VERSION`].
    pub api_version: u32,
    /// Exactly one tier.
    pub kind: ExtensionKind,
    /// The lens file (`.yaml`), `.wasm`, or command spec — relative to the manifest dir.
    pub entrypoint: String,
    /// Capabilities for plugin/integration tiers (empty = default-deny).
    #[serde(default)]
    pub permissions: Vec<String>,
}

/// Validate a manifest, returning a list of problems (empty = valid).
pub fn validate_manifest(m: &ExtensionManifest) -> Vec<String> {
    let mut problems = Vec::new();
    if m.id.trim().is_empty() || !m.id.contains('.') {
        problems.push(format!("id {:?}: must be reverse-DNS", m.id));
    }
    if m.name.trim().is_empty() {
        problems.push("name: must not be empty".into());
    }
    if m.version.trim().is_empty() {
        problems.push("version: must be a SemVer string".into());
    }
    if m.api_version != MANIFEST_SCHEMA_VERSION {
        problems.push(format!(
            "api_version: this release supports manifest schema v{MANIFEST_SCHEMA_VERSION}, \
             but the manifest declares v{}",
            m.api_version
        ));
    }
    if m.entrypoint.trim().is_empty() {
        problems.push("entrypoint: must not be empty".into());
    }
    problems
}

/// A discovered extension: the parsed manifest plus its source directory (so the
/// entrypoint can be resolved relative to the manifest).
#[derive(Debug, Clone)]
pub struct DiscoveredExtension {
    pub manifest: ExtensionManifest,
    /// The directory containing the `extension.yaml` (the Git-backed path).
    pub dir: PathBuf,
}

/// A lens' declared target, resolved from its `entrypoint` file at discovery time — the
/// `group/version/kind` the lens binds, so a frontend can decide whether a CRD is
/// lens-navigable without loading the full lens (M2.2 "lens-driven kind list").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredLens {
    /// The extension id (reverse-DNS), e.g. `"com.example.cnpg-lens"`.
    pub id: String,
    /// The human-readable extension name.
    pub name: String,
    /// The `group/version/kind` the lens targets (group empty for core).
    pub target: kube::core::GroupVersionKind,
}

/// Discover enabled **lens** extensions in a directory tree: walk `extension.yaml`
/// manifests, keep `kind: lens`, and resolve each lens' declared target from its
/// `entrypoint` file. A lens whose entrypoint is missing or unparseable is reported as a
/// problem (never silently dropped — same contract as `validate_manifest`). The caller
/// applies the `enable`/`disable` set via [`Config::extensions`](crate::config::Config).
pub fn discover_lenses(root: &Path) -> (Vec<DiscoveredLens>, Vec<String>) {
    let (exts, mut problems) = discover(root);
    let mut lenses = Vec::new();
    for ext in exts {
        if ext.manifest.kind != ExtensionKind::Lens {
            continue;
        }
        let entry = ext.dir.join(&ext.manifest.entrypoint);
        match read_lens_target(&entry) {
            Ok(gvk) => lenses.push(DiscoveredLens {
                id: ext.manifest.id,
                name: ext.manifest.name,
                target: gvk,
            }),
            Err(e) => problems.push(format!("{}: {e}", entry.display())),
        }
    }
    lenses.sort_by(|a, b| {
        let ak = format!("{}/{}/{}", a.target.group, a.target.version, a.target.kind);
        let bk = format!("{}/{}/{}", b.target.group, b.target.version, b.target.kind);
        ak.cmp(&bk).then(a.id.cmp(&b.id))
    });
    (lenses, problems)
}

/// Read a lens file and return its declared `target` `GroupVersionKind`, tolerating the
/// two field spellings a lens may use (`target: {group, version, kind}`).
fn read_lens_target(path: &Path) -> Result<kube::core::GroupVersionKind, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read lens: {e}"))?;
    let value: serde_json::Value =
        serde_yaml::from_str(&text).map_err(|e| format!("cannot parse lens: {e}"))?;
    let target = value
        .get("target")
        .ok_or_else(|| "lens has no `target`".to_string())?;
    let group = target
        .get("group")
        .and_then(|g| g.as_str())
        .unwrap_or_default()
        .to_string();
    let version = target
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "lens target has no `version`".to_string())?
        .to_string();
    let kind = target
        .get("kind")
        .and_then(|k| k.as_str())
        .ok_or_else(|| "lens target has no `kind`".to_string())?
        .to_string();
    Ok(kube::core::GroupVersionKind::gvk(&group, &version, &kind))
}

/// Discover extensions by walking a directory tree for `extension.yaml` manifests.
/// Returns `(discovered, problems)` — a malformed manifest is reported as a problem,
/// not silently skipped.
pub fn discover(root: &Path) -> (Vec<DiscoveredExtension>, Vec<String>) {
    let mut found = Vec::new();
    let mut problems = Vec::new();
    walk(root, &mut found, &mut problems);
    (found, problems)
}

fn walk(dir: &Path, found: &mut Vec<DiscoveredExtension>, problems: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, found, problems);
        } else if entry.file_name() == "extension.yaml" {
            match load_manifest(&path) {
                Ok(manifest) => found.push(DiscoveredExtension {
                    manifest,
                    dir: path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
                }),
                Err(problems_in_file) => problems.extend(problems_in_file),
            }
        }
    }
}

fn load_manifest(path: &Path) -> Result<ExtensionManifest, Vec<String>> {
    let text =
        std::fs::read_to_string(path).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    let value: serde_json::Value = serde_yaml::from_str(&text)
        .map_err(|e| vec![format!("{}: parse error: {e}", path.display())])?;
    let manifest: ExtensionManifest =
        serde_json::from_value(value).map_err(|e| vec![format!("{}: {e}", path.display())])?;
    let problems = validate_manifest(&manifest)
        .into_iter()
        .map(|p| format!("{}: {p}", path.display()))
        .collect::<Vec<_>>();
    if problems.is_empty() {
        Ok(manifest)
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_manifest() -> ExtensionManifest {
        ExtensionManifest {
            id: "com.example.cnpg-lens".into(),
            name: "CNPG lens".into(),
            version: "1.2.0".into(),
            api_version: MANIFEST_SCHEMA_VERSION,
            kind: ExtensionKind::Lens,
            entrypoint: "lens.cnpg.yaml".into(),
            permissions: vec![],
        }
    }

    #[test]
    fn valid_manifest_has_no_problems() {
        assert!(validate_manifest(&valid_manifest()).is_empty());
    }

    #[test]
    fn wrong_api_version_is_flagged() {
        let mut m = valid_manifest();
        m.api_version = 999;
        assert!(
            validate_manifest(&m)
                .iter()
                .any(|p| p.contains("api_version"))
        );
    }

    #[test]
    fn missing_reverse_dns_id_is_flagged() {
        let mut m = valid_manifest();
        m.id = "nodot".into();
        assert!(
            validate_manifest(&m)
                .iter()
                .any(|p| p.contains("reverse-DNS"))
        );
    }

    #[test]
    fn discover_finds_manifests_recursively() {
        let tmp = std::env::temp_dir().join("kaptein-ext-test");
        let sub = tmp.join("a").join("b");
        std::fs::create_dir_all(&sub).unwrap();
        let manifest = "id: com.example.t\nname: T\nversion: 1.0.0\napi_version: 1\nkind: lens\nentrypoint: t.yaml\n";
        std::fs::write(sub.join("extension.yaml"), manifest).unwrap();
        let (found, problems) = discover(&tmp);
        assert!(problems.is_empty(), "unexpected problems: {problems:?}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].manifest.id, "com.example.t");
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn discover_reports_malformed_manifest() {
        let tmp = std::env::temp_dir().join("kaptein-ext-bad");
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("extension.yaml"), "id: nodot\n").unwrap();
        let (found, problems) = discover(&tmp);
        assert!(found.is_empty());
        assert!(!problems.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn discover_lenses_resolves_target_gvk_and_skips_non_lens() {
        let tmp = std::env::temp_dir().join("kaptein-lens-test");
        std::fs::create_dir_all(&tmp).unwrap();

        // A lens extension with a real entrypoint.
        let lens_dir = tmp.join("cnpg");
        std::fs::create_dir_all(&lens_dir).unwrap();
        std::fs::write(
            lens_dir.join("extension.yaml"),
            "id: com.example.cnpg-lens\nname: CNPG lens\nversion: 1.0.0\napi_version: 1\nkind: lens\nentrypoint: lens.yaml\n",
        )
        .unwrap();
        std::fs::write(
            lens_dir.join("lens.yaml"),
            "id: com.example.cnpg-lens\napi_version: 1\ntarget: {group: postgresql.cnpg.io, version: v1, kind: Cluster}\n",
        )
        .unwrap();

        // A non-lens extension (plugin) must be skipped.
        let plugin_dir = tmp.join("plugin");
        std::fs::create_dir_all(&plugin_dir).unwrap();
        std::fs::write(
            plugin_dir.join("extension.yaml"),
            "id: com.example.p\nname: P\nversion: 1.0.0\napi_version: 1\nkind: plugin\nentrypoint: p.wasm\n",
        )
        .unwrap();

        let (lenses, problems) = discover_lenses(&tmp);
        assert!(problems.is_empty(), "unexpected problems: {problems:?}");
        assert_eq!(lenses.len(), 1);
        assert_eq!(lenses[0].id, "com.example.cnpg-lens");
        assert_eq!(lenses[0].target.group, "postgresql.cnpg.io");
        assert_eq!(lenses[0].target.version, "v1");
        assert_eq!(lenses[0].target.kind, "Cluster");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
