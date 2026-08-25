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
    /// Extension lifecycle: ids of **disabled** extensions (ADR-0004). Discovery finds
    /// every manifest; only non-disabled extensions are loaded. Disabled-by-default for
    /// unknown ids (safe default).
    #[serde(default)]
    pub extensions: Extensions,
}

/// Extension enable/disable state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Extensions {
    /// Reverse-DNS ids of disabled extensions.
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl Extensions {
    /// Whether an extension id is enabled (i.e. not in the disabled set).
    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.iter().any(|d| d == id)
    }
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

/// Validate a config, returning a list of problems (empty = valid). A config that fails
/// to parse, or whose guardrail regexes are invalid, is surfaced here rather than
/// silently degrading to read-only (a typo in a prod regex must not silently vanish).
pub fn validate(config: &Config) -> Vec<String> {
    let mut problems = Vec::new();
    for (which, patterns) in [
        ("prod", &config.guardrails.prod),
        ("staging", &config.guardrails.staging),
    ] {
        for pat in patterns {
            if regex::Regex::new(pat).is_err() {
                problems.push(format!("guardrails.{which}: invalid regex '{pat}'"));
            }
        }
    }
    problems
}

/// Validate the config **file** at `path`: parse it and report parse errors and invalid
/// regexes. Returns `Ok(())` if the file is valid (or absent), and a list of problems
/// otherwise. This is the `kaptein config validate` primitive.
pub fn validate_file(path: &std::path::Path) -> Result<(), Vec<String>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(_) => return Ok(()), // absent config is valid (defaults)
    };
    let config: Config = toml::from_str(&text).map_err(|e| vec![format!("parse error: {e}")])?;
    let problems = validate(&config);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Explain the guardrail classification for a context name: the class, and why (which
/// regex matched). This is the `kaptein config explain-context` primitive — it turns a
/// "why is my context read-only" mystery into a concrete answer.
pub fn explain_context(config: &Config, context_name: &str) -> String {
    let class = config.guardrails.classify(context_name);
    let matched_prod = config.guardrails.prod.iter().find(|p| {
        regex::Regex::new(p)
            .map(|re| re.is_match(context_name))
            .unwrap_or(false)
    });
    let matched_staging = config.guardrails.staging.iter().find(|p| {
        regex::Regex::new(p)
            .map(|re| re.is_match(context_name))
            .unwrap_or(false)
    });
    match class {
        crate::guardrails::ContextClass::Prod => format!(
            "context '{context_name}' is classified PROD (matched prod regex {:?}); writes require --break-glass",
            matched_prod
        ),
        crate::guardrails::ContextClass::Staging => format!(
            "context '{context_name}' is classified STAGING (matched staging regex {:?}); writes allowed",
            matched_staging
        ),
        crate::guardrails::ContextClass::Unknown => format!(
            "context '{context_name}' is UNKNOWN (no prod/staging regex matched); read-only by default"
        ),
    }
}

/// Load the config, mutate it with `f`, and persist it back to the config file (TOML).
/// This is the primitive behind `kaptein extension enable/disable` — the only place the
/// tool writes its own config. Fails (returns `Err`) rather than silently not saving.
pub fn update_config(f: impl FnOnce(&mut Config)) -> Result<(), String> {
    let path = config_path();
    let mut config = load_from(&path);
    f(&mut config);
    let text = toml::to_string_pretty(&config).map_err(|e| format!("serialize config: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("write {}: {e}", path.display()))
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

    #[test]
    fn validate_flags_invalid_regex() {
        let c = Config {
            guardrails: Guardrails {
                prod: vec!["(".into()], // invalid regex
                staging: vec![],
            },
            extensions: Extensions::default(),
        };
        let problems = validate(&c);
        assert!(!problems.is_empty());
        assert!(problems[0].contains("invalid regex"));
    }

    #[test]
    fn validate_accepts_valid_config() {
        let c = Config {
            guardrails: Guardrails {
                prod: vec!["^prod-".into()],
                staging: vec![],
            },
            extensions: Extensions::default(),
        };
        assert!(validate(&c).is_empty());
    }

    #[test]
    fn explain_context_reports_classification() {
        let c = Config {
            guardrails: Guardrails {
                prod: vec!["^prod-".into()],
                staging: vec!["^stag-".into()],
            },
            extensions: Extensions::default(),
        };
        assert!(explain_context(&c, "prod-eu").contains("PROD"));
        assert!(explain_context(&c, "stag-eu").contains("STAGING"));
        assert!(explain_context(&c, "random").contains("UNKNOWN"));
    }

    #[test]
    fn extensions_disabled_set_gates_is_enabled() {
        let mut c = Config::default();
        assert!(c.extensions.is_enabled("com.example.a"));
        c.extensions.disabled.push("com.example.a".into());
        assert!(!c.extensions.is_enabled("com.example.a"));
        assert!(c.extensions.is_enabled("com.example.b"));
    }

    #[test]
    fn config_with_extensions_section_round_trips() {
        let text = r#"[guardrails]
prod = []
staging = []

[extensions]
disabled = ["com.example.a"]
"#;
        let c: Config = toml::from_str(text).expect("parse");
        assert_eq!(c.extensions.disabled, vec!["com.example.a"]);
        assert!(!c.extensions.is_enabled("com.example.a"));
    }
}
