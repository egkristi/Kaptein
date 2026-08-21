//! Configuration loading — the single config file (XDG path + TOML).
//!
//! The config file carries the guardrail policy and, later, the keymap, lens paths,
//! saved queries, and view layouts (ADR: "one workspace repo"). This module loads the
//! guardrails section only for now; the full schema grows with later milestones.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::guardrails::Guardrails;

/// The top-level config schema.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Context guardrails (context-name patterns → prod/staging).
    #[serde(default)]
    pub guardrails: Guardrails,
}

/// Resolve the config file path: `$KAPTEIN_CONFIG`, else `$XDG_CONFIG_HOME/kaptein/config.toml`,
/// else `~/.config/kaptein/config.toml`.
pub fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("KAPTEIN_CONFIG") {
        return PathBuf::from(p);
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("kaptein").join("config.toml");
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("kaptein")
            .join("config.toml");
    }
    PathBuf::from("config.toml")
}

/// Load the config, returning `Config::default()` if the file is absent or unparseable
/// (a bad config must never block startup, and must never weaken guardrails — the
/// default is `Unknown`, i.e. read-only for unmatched contexts).
pub fn load() -> Config {
    load_from(&config_path())
}

/// Load from an explicit path (used by tests to avoid mutating process env).
fn load_from(path: &std::path::Path) -> Config {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Config::default();
    };
    toml::from_str(&text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_loads_default() {
        let c = load_from(std::path::Path::new("/nonexistent/kaptein-config.toml"));
        assert!(c.guardrails.prod.is_empty());
    }

    #[test]
    fn parses_guardrails_toml() {
        let tmp = std::env::temp_dir().join("kaptein-config-test.toml");
        std::fs::write(
            &tmp,
            r#"[guardrails]
prod = ["^prod-"]
staging = ["^stag-"]
"#,
        )
        .unwrap();
        let c = load_from(&tmp);
        assert_eq!(c.guardrails.prod, vec!["^prod-"]);
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn partial_toml_with_only_prod_parses() {
        let tmp = std::env::temp_dir().join("kaptein-config-partial.toml");
        std::fs::write(
            &tmp,
            r#"[guardrails]
prod = ["webtop"]
"#,
        )
        .unwrap();
        let c = load_from(&tmp);
        assert_eq!(c.guardrails.prod, vec!["webtop"]);
        assert!(c.guardrails.staging.is_empty());
        std::fs::remove_file(&tmp).ok();
    }
}
