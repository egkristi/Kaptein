//! View-definition (lens) schema — the declarative, **data-first** extension tier
//! (ADR-0004 tier 1, ADR-0012).
//!
//! A lens binds a CRD (or built-in resource) to columns, status inference, and actions
//! with **no code** — it is a YAML/JSON document checked into Git and PR-reviewed. This
//! module is the renderer-agnostic *semantics* of a lens: the data model and the
//! validation that decide whether a lens is well-formed. The frontends render it; the
//! core evaluates it. This module is wasm-pure (serde only, no `kube`/`tokio`), so the
//! browser UI and the headless agent share the same validation as the CLI.
//!
//! The **schema** (the JSON Schema document + example lenses) is MIT/Apache-2.0 per
//! ADR-0004's licensing split; this Rust implementation lives in the BUSL core.

use serde::{Deserialize, Serialize};

use crate::surface::{Column, ColumnKind};

/// The lens schema version this release validates. Bumped on a breaking change to the
/// lens schema (see `docs/versioning.md`); a lens declaring a different `api_version` is
/// refused with a migration error.
pub const LENS_SCHEMA_VERSION: u32 = 1;

/// The resource a lens describes: `group` (empty for core), `version`, `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupVersionKind {
    /// API group, e.g. `""` (core), `"apps"`, `"postgresql.cnpg.io"`.
    #[serde(default)]
    pub group: String,
    /// API version, e.g. `"v1"`.
    pub version: String,
    /// Resource kind, e.g. `"Pod"`, `"Cluster"` (CNPG), `"VirtualMachine"` (KubeVirt).
    pub kind: String,
}

impl GroupVersionKind {
    /// A compact `group/version/kind` string for diagnostics (group omitted for core).
    pub fn display(&self) -> String {
        if self.group.is_empty() {
            format!("{}/{}", self.version, self.kind)
        } else {
            format!("{}/{}/{}", self.group, self.version, self.kind)
        }
    }
}

/// A comparison operator in a status-inference rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleOp {
    /// `field == value` (string/number/bool equality).
    Eq,
    /// `field != value`.
    Ne,
    /// `field > value` (numeric).
    Gt,
    /// `field >= value` (numeric).
    Gte,
    /// `field < value` (numeric).
    Lt,
    /// `field <= value` (numeric).
    Lte,
    /// `field contains value` (substring).
    Contains,
}

/// A status-inference rule: when `field` `op` `value` holds, assign `level`.
///
/// The `field` is a dotted JSON path (e.g. `status.phase`, `spec.replicas`); the core
/// resolves it against the live object. This is declarative, so the schema is
/// structural — no free-form expression language (that lands with the full lens engine
/// in later M2.2 work).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatusRule {
    /// Dotted JSON path to the field, e.g. `"status.phase"`.
    pub field: String,
    /// The comparison.
    pub op: RuleOp,
    /// The value to compare against (string, number, or bool).
    pub value: serde_json::Value,
    /// The level to assign when the rule matches.
    pub level: crate::render::StatusLevel,
}

/// An action a lens declares. This is the lens-native form (snake_case `state`) — it
/// maps to the render contract's `semantic::Action` at evaluation time. It is a separate
/// type so the lens schema stays its own clean, user-authored contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LensAction {
    /// Stable action id, e.g. `"describe"`.
    pub id: String,
    /// Message key resolved by the frontend for i18n.
    pub label_key: String,
    /// Initial RBAC-preflight state (`allowed` — the core will grey it out if preflight
    /// denies it).
    #[serde(rename = "state", default = "default_action_state")]
    pub state: String,
}

fn default_action_state() -> String {
    "allowed".into()
}

/// The view-definition (lens) document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewDefinition {
    /// Unique reverse-DNS id, e.g. `"com.example.cnpg-lens"`.
    pub id: String,
    /// The lens schema version this document was written against. Must equal
    /// [`LENS_SCHEMA_VERSION`] (this release refuses anything else).
    pub api_version: u32,
    /// The resource kind this lens describes.
    pub target: GroupVersionKind,
    /// Columns to render (each is a view-model `Column`; semantics, not geometry).
    #[serde(default)]
    pub columns: Vec<Column>,
    /// Optional status-inference rules, evaluated in order (first match wins).
    #[serde(default)]
    pub status: Vec<StatusRule>,
    /// Actions this lens makes available, with their RBAC-preflight state.
    #[serde(default)]
    pub actions: Vec<LensAction>,
}

/// Validate a view definition, returning a list of problems (empty = valid).
///
/// This is the "reviewable in PRs" gate: a lens that fails validation is refused, never
/// silently ignored — exactly like a prod-regex typo in the config.
pub fn validate_viewdef(vd: &ViewDefinition) -> Vec<String> {
    let mut problems = Vec::new();

    if vd.id.trim().is_empty() {
        problems.push("id: must not be empty".into());
    } else if !vd.id.contains('.') {
        problems.push(format!(
            "id {:?}: must be reverse-DNS (e.g. \"com.example.cnpg-lens\")",
            vd.id
        ));
    }

    if vd.api_version != LENS_SCHEMA_VERSION {
        problems.push(format!(
            "api_version: this release supports lens schema v{LENS_SCHEMA_VERSION}, but the \
             lens declares v{} — a migration is required (docs/versioning.md)",
            vd.api_version
        ));
    }

    if vd.target.version.trim().is_empty() {
        problems.push("target.version: must not be empty".into());
    }
    if vd.target.kind.trim().is_empty() {
        problems.push("target.kind: must not be empty".into());
    }

    // Column ids must be unique and non-empty.
    let mut seen = std::collections::HashSet::new();
    for col in &vd.columns {
        if col.id.trim().is_empty() {
            problems.push("columns: a column has an empty id".into());
        } else if !seen.insert(col.id.as_str()) {
            problems.push(format!("columns: duplicate column id {:?}", col.id));
        }
        if !valid_header_key(&col.header_key) {
            problems.push(format!(
                "columns.{:?}: header_key must be a dotted i18n key (e.g. \"col.name\")",
                col.id
            ));
        }
    }

    // Status rules: field must be a dotted path; numeric ops need a numeric value.
    for (i, rule) in vd.status.iter().enumerate() {
        if !valid_field_path(&rule.field) {
            problems.push(format!(
                "status[{i}].field {:?}: not a dotted JSON path",
                rule.field
            ));
        }
        match rule.op {
            RuleOp::Gt | RuleOp::Gte | RuleOp::Lt | RuleOp::Lte => {
                if !rule.value.is_number() {
                    problems.push(format!(
                        "status[{i}].value: a numeric operator ({:?}) needs a numeric value",
                        rule.op
                    ));
                }
            }
            RuleOp::Contains => {
                if !rule.value.is_string() {
                    problems.push(format!(
                        "status[{i}].value: `contains` needs a string value"
                    ));
                }
            }
            RuleOp::Eq | RuleOp::Ne => {}
        }
    }

    // Action ids must be unique.
    let mut seen_actions = std::collections::HashSet::new();
    for action in &vd.actions {
        if action.id.trim().is_empty() || !seen_actions.insert(action.id.as_str()) {
            problems.push(format!(
                "actions: duplicate or empty action id {:?}",
                action.id
            ));
        }
    }

    problems
}

/// A dotted JSON field path: leading identifier, then `.identifier` segments (or
/// `[0]`-style numeric indexes).
fn valid_field_path(field: &str) -> bool {
    let mut parts = field.split('.');
    let Some(first) = parts.next() else {
        return false;
    };
    if !is_identifier(first) {
        return false;
    }
    parts.all(is_segment)
}

fn is_segment(seg: &str) -> bool {
    // A segment is an identifier, optionally followed by one or more `[index]`
    // subscripts: `containers`, `containers[0]`, `containers[0][1]`.
    let mut idx = seg.len();
    while idx > 0 && seg[..idx].ends_with(']') {
        let Some(open) = seg[..idx].rfind('[') else {
            return false;
        };
        let inside = &seg[open + 1..idx - 1];
        if inside.is_empty() || !inside.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        idx = open;
    }
    is_identifier(&seg[..idx])
}

fn is_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .enumerate()
            .all(|(i, c)| c.is_alphanumeric() || c == '_' || (i > 0 && c == '-'))
}

/// A dotted i18n header key (e.g. `"col.name"`): leading identifier, then `.identifier`
/// segments. Reuses the identifier rules of the view-model's message keys.
fn valid_header_key(key: &str) -> bool {
    key.split('.').all(is_identifier) && key.contains('.')
}

/// A concrete example: a built-in `Column` set for a CNPG `Cluster` (the hardest-lens
/// acceptance test from ADR-0012). This is data, not code — it is what a lens file
/// declares, and it validates cleanly.
pub fn example_cnpg_columns() -> Vec<Column> {
    vec![
        Column {
            id: "name".into(),
            header_key: "col.name".into(),
            kind: ColumnKind::Text,
            sortable: true,
        },
        Column {
            id: "instances".into(),
            header_key: "col.instances".into(),
            kind: ColumnKind::Number,
            sortable: true,
        },
        Column {
            id: "status".into(),
            header_key: "col.status".into(),
            kind: ColumnKind::Status,
            sortable: true,
        },
    ]
}

/// A concrete example status rule (CNPG): "phase == ClusterIsReady" → Ok.
pub fn example_status_rule() -> StatusRule {
    StatusRule {
        field: "status.phase".into(),
        op: RuleOp::Eq,
        value: serde_json::json!("ClusterIsReady"),
        level: crate::render::StatusLevel::Ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(id: &str) -> Column {
        Column {
            id: id.into(),
            header_key: format!("col.{id}"),
            kind: ColumnKind::Text,
            sortable: true,
        }
    }

    fn action(id: &str) -> LensAction {
        LensAction {
            id: id.into(),
            label_key: format!("action.{id}"),
            state: "allowed".into(),
        }
    }

    fn valid() -> ViewDefinition {
        ViewDefinition {
            id: "com.example.cnpg-lens".into(),
            api_version: LENS_SCHEMA_VERSION,
            target: GroupVersionKind {
                group: "postgresql.cnpg.io".into(),
                version: "v1".into(),
                kind: "Cluster".into(),
            },
            columns: vec![col("name"), col("status")],
            status: vec![example_status_rule()],
            actions: vec![action("describe")],
        }
    }

    #[test]
    fn valid_lens_has_no_problems() {
        assert!(validate_viewdef(&valid()).is_empty());
    }

    #[test]
    fn missing_reverse_dns_id_is_flagged() {
        let mut vd = valid();
        vd.id = "no-dot-here".into();
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("reverse-DNS")));
    }

    #[test]
    fn wrong_api_version_is_flagged() {
        let mut vd = valid();
        vd.api_version = 999;
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("api_version")));
    }

    #[test]
    fn duplicate_column_id_is_flagged() {
        let mut vd = valid();
        vd.columns = vec![col("name"), col("name")];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("duplicate column")));
    }

    #[test]
    fn numeric_op_with_string_value_is_flagged() {
        let mut vd = valid();
        vd.status = vec![StatusRule {
            field: "spec.replicas".into(),
            op: RuleOp::Gt,
            value: serde_json::json!("many"),
            level: crate::render::StatusLevel::Warning,
        }];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("numeric")));
    }

    #[test]
    fn contains_op_with_numeric_value_is_flagged() {
        let mut vd = valid();
        vd.status = vec![StatusRule {
            field: "status.phase".into(),
            op: RuleOp::Contains,
            value: serde_json::json!(3),
            level: crate::render::StatusLevel::Warning,
        }];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("contains")));
    }

    #[test]
    fn malformed_field_path_is_flagged() {
        let mut vd = valid();
        vd.status = vec![StatusRule {
            field: ".bad.path".into(),
            op: RuleOp::Eq,
            value: serde_json::json!("x"),
            level: crate::render::StatusLevel::Ok,
        }];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("field")));
    }

    #[test]
    fn duplicate_action_id_is_flagged() {
        let mut vd = valid();
        vd.actions = vec![action("x"), action("x")];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("action")));
    }

    #[test]
    fn field_path_validator_accepts_indexes() {
        assert!(valid_field_path("status.phase"));
        assert!(valid_field_path("spec.containers[0].name"));
        assert!(valid_field_path("metadata.labels.app"));
        assert!(!valid_field_path(""));
        assert!(!valid_field_path(".phase"));
        assert!(!valid_field_path("status..phase"));
    }
}
