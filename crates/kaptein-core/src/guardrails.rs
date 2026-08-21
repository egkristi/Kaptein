//! Context guardrails — "which contexts are dangerous?"
//!
//! Classifies a kubeconfig context name as `Prod`, `Staging`, or `Unknown`, so the
//! frontend can apply the guardrail policy: prod contexts get a red frame, a read-only
//! default, and require an explicit "break glass" confirmation for writes (M1.1).
//!
//! The policy is configurable per regex on context name. Unknown contexts are read-only
//! by default.

use serde::{Deserialize, Serialize};

/// The guardrail classification for a context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextClass {
    /// Production — red frame, read-only default, "break glass" for writes.
    Prod,
    /// Staging/development — writes allowed without break-glass.
    Staging,
    /// Unknown — read-only by default (the safe fallback).
    Unknown,
}

/// Guardrail configuration: which context-name patterns map to which class.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Guardrails {
    /// Regex patterns (matched against the context name) that classify as production.
    #[serde(default)]
    pub prod: Vec<String>,
    /// Regex patterns that classify as staging.
    #[serde(default)]
    pub staging: Vec<String>,
}

impl Guardrails {
    /// Classify a context name against the configured patterns.
    ///
    /// Matching is "first match wins": `prod` is checked before `staging`, and an
    /// unmatched name is `Unknown` (read-only by default). Regex compilation errors
    /// degrade to "no match" rather than failing the classification, so a bad pattern
    /// can never accidentally classify a context as *less* dangerous.
    pub fn classify(&self, context_name: &str) -> ContextClass {
        if self.matches(&self.prod, context_name) {
            ContextClass::Prod
        } else if self.matches(&self.staging, context_name) {
            ContextClass::Staging
        } else {
            ContextClass::Unknown
        }
    }

    fn matches(&self, patterns: &[String], name: &str) -> bool {
        patterns.iter().any(|p| {
            regex::Regex::new(p)
                .map(|re| re.is_match(name))
                .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prod_matches_before_staging() {
        let g = Guardrails {
            prod: vec!["^prod-".into(), ".*-prod$".into()],
            staging: vec!["^stag-".into()],
        };
        assert_eq!(g.classify("prod-eu-west"), ContextClass::Prod);
        assert_eq!(g.classify("my-app-prod"), ContextClass::Prod);
        assert_eq!(g.classify("stag-eu"), ContextClass::Staging);
        assert_eq!(g.classify("random-context"), ContextClass::Unknown);
    }

    #[test]
    fn bad_regex_degrades_to_no_match() {
        let g = Guardrails {
            prod: vec!["(".into()], // invalid regex
            staging: vec![],
        };
        // A broken prod pattern must not classify anything as prod.
        assert_eq!(g.classify("prod-anything"), ContextClass::Unknown);
    }

    #[test]
    fn default_is_unknown_for_everything() {
        let g = Guardrails::default();
        assert_eq!(g.classify("prod"), ContextClass::Unknown);
        assert_eq!(g.classify("anything"), ContextClass::Unknown);
    }
}
