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

use crate::render::{Cell, Row, RowId};
use crate::semantic::{Action, ActionState};
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

/// A status-inference rule over Kubernetes conditions (`status.conditions[]`).
///
/// The scalar [`StatusRule`] cannot express how the majority of modern CRDs signal
/// readiness — via a typed condition (`type` + `status`) rather than a bare phase. This
/// rule matches the first condition whose `type` equals [`Self::condition_type`]; if
/// that condition's `status` equals [`Self::status`], the rule fires at [`Self::level`].
/// This is what lets Strimzi Kafka, KubeVirt VirtualMachine, cert-manager Certificate,
/// Keycloak, Tekton PipelineRun, Karpenter NodePool, and Knative Service all be declared
/// as data (ADR-0012's "prove the schema against the hardest lenses" test).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConditionRule {
    /// The condition `type` to match, e.g. `"Ready"`, `"ReconciliationSucceeded"`.
    pub condition_type: String,
    /// The condition `status` to match: `"True"`, `"False"`, or `"Unknown"` (the three
    /// canonical Kubernetes condition statuses).
    pub status: String,
    /// The level to assign when the condition matches.
    pub level: crate::render::StatusLevel,
}

/// A health check a lens declares: a predicate (`field` `op` `value`) that must hold for
/// the resource to be considered healthy, plus the severity to surface when it does
/// **not** hold. Unlike [`StatusRule`] (which picks a *single* status level per resource,
/// first match wins), a lens can declare **many** health checks, each evaluated
/// independently — so a CNPG Cluster can be simultaneously "not enough ready instances"
/// (error) *and* "replication lag over threshold" (warning), and both surface.
///
/// The predicate is the *healthy* condition: when it holds, the check emits nothing;
/// when it fails (or the field is absent — a resource that cannot be verified is not
/// healthy), the check emits a [`HealthFinding`] at [`Self::level`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HealthCheck {
    /// Stable check id, e.g. `"ready-instances"`.
    pub id: String,
    /// Message key resolved by the frontend for i18n, e.g. `"health.ready-instances"`.
    pub label_key: String,
    /// Dotted JSON path the check asserts on, e.g. `"status.readyInstances"`.
    pub field: String,
    /// The comparison that must hold for the resource to be healthy.
    pub op: RuleOp,
    /// The value the field is compared against (the healthy threshold).
    pub value: serde_json::Value,
    /// The severity surfaced when the check fails.
    pub level: crate::render::StatusLevel,
}

/// A failing health check: the check's id, its i18n label key, and the severity it
/// declares. Emitted by [`evaluate_health`] for each check whose predicate does not hold.
/// Passing checks emit nothing, so a healthy resource yields an empty list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthFinding {
    /// The check's stable id (e.g. `"ready-instances"`).
    pub id: String,
    /// The check's i18n label key.
    pub label_key: String,
    /// The severity the check declares for a failure.
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
    /// Optional condition-based status rules (`status.conditions[]`), evaluated after
    /// `status` (first match wins within the whole sequence).
    #[serde(default)]
    pub conditions: Vec<ConditionRule>,
    /// Optional health checks, each evaluated independently (a resource can fail many at
    /// once — see [`HealthCheck`]).
    #[serde(default)]
    pub health: Vec<HealthCheck>,
    /// Actions this lens makes available, with their RBAC-preflight state.
    #[serde(default)]
    pub actions: Vec<LensAction>,
}

impl ViewDefinition {
    /// Map a lens's declared actions into the render contract's `semantic::Action`s,
    /// resolving each lens-native `state` (`allowed`/`gated`/`forbidden`) to an
    /// `ActionState`. This is the "action graph" half of M2.2: the lens declares the
    /// action *id* and *label key*; the frontend renders it and the core grey-out/p
    /// reflight logic acts on the `ActionState` — renderer-agnostic, so the TUI, GUI,
    /// and MCP surface share it.
    pub fn actions_as_semantic(&self) -> Vec<Action> {
        self.actions
            .iter()
            .map(|a| Action {
                id: a.id.clone(),
                label_key: a.label_key.clone(),
                state: match a.state.as_str() {
                    "gated" => ActionState::Gated {
                        reason_key: "action.gated".into(),
                    },
                    "forbidden" => ActionState::Forbidden {
                        verb: String::new(),
                        resource: String::new(),
                        namespace: None,
                    },
                    _ => ActionState::Allowed,
                },
            })
            .collect()
    }
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
        // A data column's value must come from somewhere: either a `field` path, or a
        // `Status` kind (whose value is *inferred* by the status/condition rules).
        if col.kind != ColumnKind::Status && col.field.as_deref().is_none_or(str::is_empty) {
            problems.push(format!(
                "columns.{:?}: a non-status column needs a `field` (dotted JSON path) so \
                 its value is data-bound, not implicit (ADR-0012)",
                col.id
            ));
        } else if let Some(field) = col.field.as_deref()
            && !field.is_empty()
            && !valid_field_path(field)
        {
            problems.push(format!(
                "columns.{:?}.field {:?}: not a dotted JSON path",
                col.id, field
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

    // Condition rules: the type must be non-empty and the status must be one of the
    // canonical Kubernetes condition statuses (True/False/Unknown).
    for (i, rule) in vd.conditions.iter().enumerate() {
        if rule.condition_type.trim().is_empty() {
            problems.push(format!("conditions[{i}].condition_type: must not be empty"));
        }
        if !is_condition_status(&rule.status) {
            problems.push(format!(
                "conditions[{i}].status {:?}: must be one of \"True\", \"False\", \"Unknown\"",
                rule.status
            ));
        }
    }

    // Health checks: ids must be unique and non-empty; label keys must be dotted i18n
    // keys; the field must be a dotted path; numeric ops need a numeric value and
    // `contains` needs a string (the same predicate rules as `status`).
    let mut seen_health = std::collections::HashSet::new();
    for (i, check) in vd.health.iter().enumerate() {
        if check.id.trim().is_empty() || !seen_health.insert(check.id.as_str()) {
            problems.push(format!(
                "health[{i}]: duplicate or empty check id {:?}",
                check.id
            ));
        }
        if !valid_header_key(&check.label_key) {
            problems.push(format!(
                "health[{i}].label_key {:?}: must be a dotted i18n key (e.g. \"health.ready\")",
                check.label_key
            ));
        }
        if !valid_field_path(&check.field) {
            problems.push(format!(
                "health[{i}].field {:?}: not a dotted JSON path",
                check.field
            ));
        }
        match check.op {
            RuleOp::Gt | RuleOp::Gte | RuleOp::Lt | RuleOp::Lte => {
                if !check.value.is_number() {
                    problems.push(format!(
                        "health[{i}].value: a numeric operator ({:?}) needs a numeric value",
                        check.op
                    ));
                }
            }
            RuleOp::Contains => {
                if !check.value.is_string() {
                    problems.push(format!(
                        "health[{i}].value: `contains` needs a string value"
                    ));
                }
            }
            RuleOp::Eq | RuleOp::Ne => {}
        }
    }

    problems
}

/// A canonical Kubernetes condition status: `True`, `False`, or `Unknown`.
fn is_condition_status(status: &str) -> bool {
    matches!(status, "True" | "False" | "Unknown")
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

/// Resolve a dotted field path (with `[i]` subscripts) against a JSON object. Returns
/// `None` when the path is absent or a segment does not exist. Used by status-rule
/// evaluation so a lens can read `status.phase` or `spec.containers[0].name`.
fn resolve_field<'a>(root: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for segment in field.split('.') {
        // Split a `name[0][1]` segment into the identifier and its subscripts.
        let (ident, subscripts) = split_subscripts(segment);
        cur = cur.get(ident)?;
        for sub in subscripts {
            cur = cur.get(sub)?;
        }
    }
    Some(cur)
}

/// Split `"containers[0][1]"` into `("containers", [0, 1])`. A segment with no
/// subscripts yields an empty index list.
fn split_subscripts(segment: &str) -> (&str, Vec<usize>) {
    let mut idx = segment.len();
    let mut subs = Vec::new();
    while idx > 0 && segment[..idx].ends_with(']') {
        if let Some(open) = segment[..idx].rfind('[') {
            let inside = &segment[open + 1..idx - 1];
            if let Ok(n) = inside.parse::<usize>() {
                subs.push(n);
            }
            idx = open;
        } else {
            break;
        }
    }
    subs.reverse();
    (&segment[..idx], subs)
}

/// Evaluate a lens's status rules against a live resource's JSON representation,
/// returning the first matching level, or `None` when no rule matches. This is the
/// "status inference" half of the lens engine (ADR-0012): the frontend colors the status
/// chip from the level; the lens declares the meaning.
///
/// Scalar `status` rules are evaluated first, then `conditions` rules (first match wins
/// across the whole sequence).
pub fn evaluate_status(
    vd: &ViewDefinition,
    resource: &serde_json::Value,
) -> Option<crate::render::StatusLevel> {
    for rule in &vd.status {
        if rule_matches(rule, resource) {
            return Some(rule.level);
        }
    }
    for rule in &vd.conditions {
        if condition_matches(rule, resource) {
            return Some(rule.level);
        }
    }
    None
}

/// Render a lens + a live resource into the render contract's `Row` (ADR-0005).
///
/// This is the "status-rule *rendering*" half of M2.2: it maps a `ViewDefinition`'s
/// columns onto a resource's JSON, so a frontend (TUI/GUI/browser/headless) consumes the
/// *same* `Row`/`Cell` for the same input. Column semantics:
///
/// - A column whose `field` is set resolves that dotted path against the resource and
///   emits a typed cell (numbers → `Number`, strings/bools/null → `Text`).
/// - A `Status`-kind column's value is **inferred** via [`evaluate_status`]: the lens's
///   status/condition rules decide the `StatusLevel` and the chip label.
/// - A `field` that is absent/`None` on a `Text` column renders an empty cell; a
///   missing field on a `Status` column renders an `Info` chip (no rule matched).
///
/// The stable `RowId` is the resource `metadata.uid` when present, else
/// `namespace/name` (the same identity contract as `kaptein-integration`).
pub fn render_row(vd: &ViewDefinition, resource: &serde_json::Value) -> Row {
    let id = resource
        .get("metadata")
        .and_then(|m| m.get("uid"))
        .and_then(|u| u.as_str())
        .map(|uid| RowId(uid.to_string()))
        .unwrap_or_else(|| {
            let name = resource
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            let ns = resource
                .get("metadata")
                .and_then(|m| m.get("namespace"))
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            RowId(if ns.is_empty() {
                name.to_string()
            } else {
                format!("{ns}/{name}")
            })
        });

    let cells = vd
        .columns
        .iter()
        .map(|col| cell_for_column(col, resource, vd))
        .collect();

    Row { id, cells }
}

/// The marker string the redaction choke point (`kaptein-core::redact`) substitutes for a
/// masked secret value. The view-model owns the *meaning* of a redacted cell (a mask, not
/// text), so it recognizes this exact string and constructs the typed [`Cell::Redacted`]
/// variant rather than a plain `Text` cell — giving every frontend a uniform signal to
/// render a mask, never the secret. Core owns producing the marker; this constant must
/// stay in sync with `kaptein-core::redact::REDACTED`.
pub const REDACTED_MARKER: &str = "[REDACTED]";

/// Build the `Cell` for a single lens column against a resource.
fn cell_for_column(col: &Column, resource: &serde_json::Value, vd: &ViewDefinition) -> Cell {
    if col.kind == ColumnKind::Status {
        // The status chip is *inferred* (not read from a single field): the lens's
        // rules decide the level and label.
        let (level, label) = match evaluate_status(vd, resource) {
            Some(level) => (level, level_label(level)),
            None => (crate::render::StatusLevel::Info, "unknown".to_string()),
        };
        return Cell::Status {
            level,
            label_key: label,
        };
    }

    // Data columns read a dotted field path; a missing/unset field is an empty cell.
    let Some(field) = col.field.as_deref() else {
        return empty_cell_for_kind(col.kind);
    };
    match resolve_field(resource, field) {
        Some(serde_json::Value::Number(n)) if n.is_i64() => Cell::Number {
            value: n.as_i64().unwrap_or(0),
        },
        Some(serde_json::Value::Number(n)) => Cell::Text {
            value: n.to_string(),
        },
        // A string that is the redaction marker is a *typed* redacted cell, not text — so
        // a frontend renders a mask, never the (already-masked) value. This is the M1.7
        // "Cell::Redacted is actually constructed" path: the marker string produced by
        // `kaptein-core::redact` becomes the render contract's redacted variant.
        Some(serde_json::Value::String(s)) if s == REDACTED_MARKER => Cell::Redacted,
        Some(serde_json::Value::String(s)) => Cell::Text { value: s.clone() },
        Some(serde_json::Value::Bool(b)) => Cell::Text {
            value: b.to_string(),
        },
        Some(serde_json::Value::Null) | None => empty_cell_for_kind(col.kind),
        Some(other) => Cell::Text {
            value: other.to_string(),
        },
    }
}

/// An empty cell matching a column's kind (empty text, or `0` for numbers).
fn empty_cell_for_kind(kind: ColumnKind) -> Cell {
    match kind {
        ColumnKind::Number => Cell::Number { value: 0 },
        _ => Cell::Text {
            value: String::new(),
        },
    }
}

/// A stable, i18n-facing label for a status level (the frontend resolves the key).
fn level_label(level: crate::render::StatusLevel) -> String {
    match level {
        crate::render::StatusLevel::Ok => "status.ok".into(),
        crate::render::StatusLevel::Info => "status.info".into(),
        crate::render::StatusLevel::Warning => "status.warning".into(),
        crate::render::StatusLevel::Error => "status.error".into(),
        crate::render::StatusLevel::Pending => "status.pending".into(),
    }
}

/// Match a condition rule: find `status.conditions[]` and look for a condition whose
/// `type` equals the rule's type and whose `status` equals the rule's status.
fn condition_matches(rule: &ConditionRule, resource: &serde_json::Value) -> bool {
    let Some(conditions) = resource.get("status").and_then(|s| s.get("conditions")) else {
        return false;
    };
    let Some(list) = conditions.as_array() else {
        return false;
    };
    list.iter().any(|cond| {
        cond.get("type").and_then(|t| t.as_str()) == Some(rule.condition_type.as_str())
            && cond.get("status").and_then(|s| s.as_str()) == Some(rule.status.as_str())
    })
}

fn rule_matches(rule: &StatusRule, resource: &serde_json::Value) -> bool {
    predicate_holds(&rule.field, rule.op, &rule.value, resource)
}

/// Evaluate a lens's health checks against a resource, returning a finding for every
/// check whose predicate does **not** hold (a failed check is a finding; an absent field
/// is also a failure — a resource that cannot be verified is not healthy). Passing checks
/// emit nothing, so a healthy resource yields an empty list.
pub fn evaluate_health(vd: &ViewDefinition, resource: &serde_json::Value) -> Vec<HealthFinding> {
    vd.health
        .iter()
        .filter(|check| !predicate_holds(&check.field, check.op, &check.value, resource))
        .map(|check| HealthFinding {
            id: check.id.clone(),
            label_key: check.label_key.clone(),
            level: check.level,
        })
        .collect()
}

/// The shared predicate shared by [`StatusRule`] and [`HealthCheck`]: does `field` `op`
/// `value` hold against `resource`? An absent field never holds. Numeric comparisons use
/// `i64` (non-numeric operands never match); `contains` is substring on strings.
fn predicate_holds(
    field: &str,
    op: RuleOp,
    value: &serde_json::Value,
    resource: &serde_json::Value,
) -> bool {
    let Some(actual) = resolve_field(resource, field) else {
        return false;
    };
    match op {
        RuleOp::Eq => actual == value,
        RuleOp::Ne => actual != value,
        RuleOp::Gt | RuleOp::Gte | RuleOp::Lt | RuleOp::Lte => {
            // Numeric comparison; non-numeric operands never match.
            let (Some(a), Some(b)) = (actual.as_i64(), value.as_i64()) else {
                return false;
            };
            match op {
                RuleOp::Gt => a > b,
                RuleOp::Gte => a >= b,
                RuleOp::Lt => a < b,
                RuleOp::Lte => a <= b,
                _ => unreachable!(),
            }
        }
        RuleOp::Contains => match (actual.as_str(), value.as_str()) {
            (Some(a), Some(b)) => a.contains(b),
            _ => false,
        },
    }
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
            field: Some("metadata.name".into()),
        },
        Column {
            id: "instances".into(),
            header_key: "col.instances".into(),
            kind: ColumnKind::Number,
            sortable: true,
            field: Some("spec.instances".into()),
        },
        Column {
            id: "status".into(),
            header_key: "col.status".into(),
            kind: ColumnKind::Status,
            sortable: true,
            field: None,
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
            field: Some(format!("metadata.{id}")),
        }
    }

    fn status_col(id: &str) -> Column {
        Column {
            id: id.into(),
            header_key: format!("col.{id}"),
            kind: ColumnKind::Status,
            sortable: true,
            field: None,
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
            columns: vec![col("name"), status_col("status")],
            status: vec![example_status_rule()],
            conditions: vec![],
            health: vec![],
            actions: vec![action("describe")],
        }
    }

    #[test]
    fn valid_lens_has_no_problems() {
        assert!(validate_viewdef(&valid()).is_empty());
    }

    #[test]
    fn actions_as_semantic_maps_state_and_label() {
        let mut vd = valid();
        vd.actions = vec![
            LensAction {
                id: "describe".into(),
                label_key: "action.describe".into(),
                state: "allowed".into(),
            },
            LensAction {
                id: "restart".into(),
                label_key: "action.restart".into(),
                state: "gated".into(),
            },
        ];
        let actions = vd.actions_as_semantic();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].id, "describe");
        assert_eq!(actions[0].label_key, "action.describe");
        assert!(matches!(actions[0].state, ActionState::Allowed));
        assert!(matches!(actions[1].state, ActionState::Gated { .. }));
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

    #[test]
    fn resolve_field_reads_nested_and_indexed_paths() {
        let v = serde_json::json!({
            "status": {"phase": "Running"},
            "spec": {"containers": [{"name": "app"}]}
        });
        assert_eq!(
            resolve_field(&v, "status.phase"),
            Some(&serde_json::json!("Running"))
        );
        assert_eq!(
            resolve_field(&v, "spec.containers[0].name"),
            Some(&serde_json::json!("app"))
        );
        assert_eq!(resolve_field(&v, "status.nope"), None);
    }

    #[test]
    fn evaluate_status_first_match_wins() {
        let mut vd = valid();
        vd.status = vec![
            StatusRule {
                field: "status.phase".into(),
                op: RuleOp::Eq,
                value: serde_json::json!("Running"),
                level: crate::render::StatusLevel::Ok,
            },
            StatusRule {
                field: "status.phase".into(),
                op: RuleOp::Ne,
                value: serde_json::json!("Running"),
                level: crate::render::StatusLevel::Warning,
            },
        ];
        let running = serde_json::json!({"status": {"phase": "Running"}});
        assert_eq!(
            evaluate_status(&vd, &running),
            Some(crate::render::StatusLevel::Ok)
        );
        let pending = serde_json::json!({"status": {"phase": "Pending"}});
        assert_eq!(
            evaluate_status(&vd, &pending),
            Some(crate::render::StatusLevel::Warning)
        );
        let empty = serde_json::json!({});
        assert_eq!(evaluate_status(&vd, &empty), None);
    }

    #[test]
    fn numeric_rule_compares_numerically() {
        let mut vd = valid();
        vd.status = vec![StatusRule {
            field: "spec.replicas".into(),
            op: RuleOp::Gt,
            value: serde_json::json!(1),
            level: crate::render::StatusLevel::Warning,
        }];
        let three = serde_json::json!({"spec": {"replicas": 3}});
        assert_eq!(
            evaluate_status(&vd, &three),
            Some(crate::render::StatusLevel::Warning)
        );
        let one = serde_json::json!({"spec": {"replicas": 1}});
        assert_eq!(evaluate_status(&vd, &one), None);
    }

    #[test]
    fn contains_rule_matches_substring() {
        let mut vd = valid();
        vd.status = vec![StatusRule {
            field: "status.message".into(),
            op: RuleOp::Contains,
            value: serde_json::json!("back-off"),
            level: crate::render::StatusLevel::Error,
        }];
        let msg = serde_json::json!({"status": {"message": "back-off pulling image"}});
        assert_eq!(
            evaluate_status(&vd, &msg),
            Some(crate::render::StatusLevel::Error)
        );
    }

    #[test]
    fn condition_rule_matches_ready_true() {
        let mut vd = valid();
        vd.status = vec![];
        vd.conditions = vec![
            ConditionRule {
                condition_type: "Ready".into(),
                status: "True".into(),
                level: crate::render::StatusLevel::Ok,
            },
            ConditionRule {
                condition_type: "Ready".into(),
                status: "False".into(),
                level: crate::render::StatusLevel::Error,
            },
        ];
        let ready = serde_json::json!({
            "status": {"conditions": [{"type": "Ready", "status": "True"}]}
        });
        assert_eq!(
            evaluate_status(&vd, &ready),
            Some(crate::render::StatusLevel::Ok)
        );
        let not_ready = serde_json::json!({
            "status": {"conditions": [{"type": "Ready", "status": "False"}]}
        });
        assert_eq!(
            evaluate_status(&vd, &not_ready),
            Some(crate::render::StatusLevel::Error)
        );
        // A condition of a different type must not match.
        let other = serde_json::json!({
            "status": {"conditions": [{"type": "Progressing", "status": "True"}]}
        });
        assert_eq!(evaluate_status(&vd, &other), None);
        // Missing conditions must not match.
        assert_eq!(
            evaluate_status(&vd, &serde_json::json!({"status": {}})),
            None
        );
    }

    #[test]
    fn evaluate_health_emits_a_finding_per_failing_check() {
        let mut vd = valid();
        vd.health = vec![
            HealthCheck {
                id: "ready-instances".into(),
                label_key: "health.ready-instances".into(),
                field: "status.readyInstances".into(),
                op: RuleOp::Gte,
                value: serde_json::json!(3),
                level: crate::render::StatusLevel::Error,
            },
            HealthCheck {
                id: "replication-lag".into(),
                label_key: "health.replication-lag".into(),
                field: "status.replicationLag".into(),
                op: RuleOp::Lt,
                value: serde_json::json!(10),
                level: crate::render::StatusLevel::Warning,
            },
        ];
        // readyInstances=2 (<3 → fails), replicationLag=4 (<10 → passes).
        let resource = serde_json::json!({
            "status": {"readyInstances": 2, "replicationLag": 4}
        });
        let findings = evaluate_health(&vd, &resource);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "ready-instances");
        assert_eq!(findings[0].label_key, "health.ready-instances");
        assert_eq!(findings[0].level, crate::render::StatusLevel::Error);
    }

    #[test]
    fn evaluate_health_treats_absent_field_as_failing() {
        let mut vd = valid();
        vd.health = vec![HealthCheck {
            id: "ready-instances".into(),
            label_key: "health.ready-instances".into(),
            field: "status.readyInstances".into(),
            op: RuleOp::Gte,
            value: serde_json::json!(3),
            level: crate::render::StatusLevel::Error,
        }];
        // No `status.readyInstances` → the check cannot be verified → not healthy.
        let findings = evaluate_health(&vd, &serde_json::json!({"status": {}}));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].id, "ready-instances");
    }

    #[test]
    fn evaluate_health_returns_empty_when_all_checks_pass() {
        let mut vd = valid();
        vd.health = vec![HealthCheck {
            id: "ready".into(),
            label_key: "health.ready".into(),
            field: "status.phase".into(),
            op: RuleOp::Eq,
            value: serde_json::json!("ClusterIsReady"),
            level: crate::render::StatusLevel::Error,
        }];
        let findings = evaluate_health(
            &vd,
            &serde_json::json!({"status": {"phase": "ClusterIsReady"}}),
        );
        assert!(findings.is_empty());
    }

    #[test]
    fn health_check_validation_flags_bad_shape() {
        let mut vd = valid();
        vd.health = vec![
            HealthCheck {
                id: "dup".into(),
                label_key: "health.dup".into(),
                field: "status.x".into(),
                op: RuleOp::Eq,
                value: serde_json::json!(1),
                level: crate::render::StatusLevel::Error,
            },
            HealthCheck {
                id: "dup".into(),
                label_key: "health.dup".into(),
                field: "status.x".into(),
                op: RuleOp::Eq,
                value: serde_json::json!(1),
                level: crate::render::StatusLevel::Error,
            },
        ];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("health[1]")));

        let mut vd2 = valid();
        vd2.health = vec![HealthCheck {
            id: "ready".into(),
            label_key: "no-dot".into(),
            field: "status.x".into(),
            op: RuleOp::Eq,
            value: serde_json::json!(1),
            level: crate::render::StatusLevel::Error,
        }];
        assert!(
            validate_viewdef(&vd2)
                .iter()
                .any(|p| p.contains("label_key"))
        );

        let mut vd3 = valid();
        vd3.health = vec![HealthCheck {
            id: "ready".into(),
            label_key: "health.ready".into(),
            field: ".bad.path".into(),
            op: RuleOp::Eq,
            value: serde_json::json!(1),
            level: crate::render::StatusLevel::Error,
        }];
        assert!(
            validate_viewdef(&vd3)
                .iter()
                .any(|p| p.contains("not a dotted JSON path"))
        );

        let mut vd4 = valid();
        vd4.health = vec![HealthCheck {
            id: "ready".into(),
            label_key: "health.ready".into(),
            field: "status.x".into(),
            op: RuleOp::Gt,
            value: serde_json::json!("many"),
            level: crate::render::StatusLevel::Error,
        }];
        assert!(validate_viewdef(&vd4).iter().any(|p| p.contains("numeric")));
    }

    #[test]
    fn invalid_condition_status_is_flagged() {
        let mut vd = valid();
        vd.conditions = vec![ConditionRule {
            condition_type: "Ready".into(),
            status: "Yes".into(),
            level: crate::render::StatusLevel::Ok,
        }];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("conditions[0].status")));
    }

    #[test]
    fn empty_condition_type_is_flagged() {
        let mut vd = valid();
        vd.conditions = vec![ConditionRule {
            condition_type: "".into(),
            status: "True".into(),
            level: crate::render::StatusLevel::Ok,
        }];
        let problems = validate_viewdef(&vd);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("conditions[0].condition_type"))
        );
    }

    #[test]
    fn non_status_column_without_field_is_flagged() {
        let mut vd = valid();
        // A text column with no `field` cannot be data-bound.
        vd.columns = vec![Column {
            id: "name".into(),
            header_key: "col.name".into(),
            kind: ColumnKind::Text,
            sortable: true,
            field: None,
        }];
        let problems = validate_viewdef(&vd);
        assert!(problems.iter().any(|p| p.contains("field")));
    }

    #[test]
    fn malformed_column_field_is_flagged() {
        let mut vd = valid();
        vd.columns = vec![Column {
            id: "name".into(),
            header_key: "col.name".into(),
            kind: ColumnKind::Text,
            sortable: true,
            field: Some(".bad.path".into()),
        }];
        let problems = validate_viewdef(&vd);
        assert!(
            problems
                .iter()
                .any(|p| p.contains("not a dotted JSON path"))
        );
    }

    #[test]
    fn render_row_maps_fields_and_infers_status() {
        // A lens with a name (data), instances (number), and status (inferred) column.
        let vd = ViewDefinition {
            id: "com.example.cnpg-lens".into(),
            api_version: LENS_SCHEMA_VERSION,
            target: GroupVersionKind {
                group: "postgresql.cnpg.io".into(),
                version: "v1".into(),
                kind: "Cluster".into(),
            },
            columns: vec![
                Column {
                    id: "name".into(),
                    header_key: "col.name".into(),
                    kind: ColumnKind::Text,
                    sortable: true,
                    field: Some("metadata.name".into()),
                },
                Column {
                    id: "instances".into(),
                    header_key: "col.instances".into(),
                    kind: ColumnKind::Number,
                    sortable: true,
                    field: Some("spec.instances".into()),
                },
                Column {
                    id: "status".into(),
                    header_key: "col.status".into(),
                    kind: ColumnKind::Status,
                    sortable: true,
                    field: None,
                },
            ],
            status: vec![StatusRule {
                field: "status.phase".into(),
                op: RuleOp::Eq,
                value: serde_json::json!("ClusterIsReady"),
                level: crate::render::StatusLevel::Ok,
            }],
            conditions: vec![],
            health: vec![],
            actions: vec![],
        };

        let resource = serde_json::json!({
            "metadata": {"uid": "abc-123", "name": "pg", "namespace": "db"},
            "spec": {"instances": 3},
            "status": {"phase": "ClusterIsReady"}
        });

        let row = render_row(&vd, &resource);
        // Stable identity is metadata.uid.
        assert_eq!(row.id, RowId("abc-123".into()));
        assert_eq!(row.cells.len(), 3);
        // name → Text("pg")
        assert_eq!(row.cells[0], Cell::Text { value: "pg".into() });
        // instances → Number(3)
        assert_eq!(row.cells[1], Cell::Number { value: 3 });
        // status → inferred Ok chip
        assert_eq!(
            row.cells[2],
            Cell::Status {
                level: crate::render::StatusLevel::Ok,
                label_key: "status.ok".into(),
            }
        );
    }

    #[test]
    fn render_row_falls_back_to_ns_name_identity_and_info_status() {
        let vd = ViewDefinition {
            id: "com.example.t".into(),
            api_version: LENS_SCHEMA_VERSION,
            target: GroupVersionKind {
                group: "example.io".into(),
                version: "v1".into(),
                kind: "Thing".into(),
            },
            columns: vec![Column {
                id: "status".into(),
                header_key: "col.status".into(),
                kind: ColumnKind::Status,
                sortable: true,
                field: None,
            }],
            status: vec![],
            conditions: vec![],
            health: vec![],
            actions: vec![],
        };
        // No uid → ns/name identity; no matching rule → Info "unknown" chip.
        let resource = serde_json::json!({
            "metadata": {"name": "x", "namespace": "n"}
        });
        let row = render_row(&vd, &resource);
        assert_eq!(row.id, RowId("n/x".into()));
        assert_eq!(
            row.cells[0],
            Cell::Status {
                level: crate::render::StatusLevel::Info,
                label_key: "unknown".into(),
            }
        );
    }

    #[test]
    fn render_row_constructs_typed_redacted_cell_for_the_marker() {
        // M1.7: a lens column bound to a field that the redaction choke point masked to
        // the `[REDACTED]` marker must produce the *typed* `Cell::Redacted` variant — not
        // a plain `Text` cell carrying the marker string — so a frontend renders a mask
        // with no special-case string comparison.
        let vd = ViewDefinition {
            id: "com.example.secret".into(),
            api_version: LENS_SCHEMA_VERSION,
            target: GroupVersionKind {
                group: "".into(),
                version: "v1".into(),
                kind: "Secret".into(),
            },
            columns: vec![Column {
                id: "password".into(),
                header_key: "col.password".into(),
                kind: ColumnKind::Text,
                sortable: true,
                field: Some("data.password".into()),
            }],
            status: vec![],
            conditions: vec![],
            health: vec![],
            actions: vec![],
        };
        // A Secret whose `data.password` was already masked by kaptein-core::redact.
        let resource = serde_json::json!({
            "metadata": {"name": "db", "namespace": "n"},
            "data": {"password": REDACTED_MARKER}
        });
        let row = render_row(&vd, &resource);
        assert_eq!(row.cells[0], Cell::Redacted);
    }

    #[test]
    fn render_row_keeps_plain_strings_as_text() {
        // A non-marker string is still a Text cell (the redaction special-case must not
        // over-match ordinary values that merely contain the word "redacted").
        let vd = ViewDefinition {
            id: "com.example.t".into(),
            api_version: LENS_SCHEMA_VERSION,
            target: GroupVersionKind {
                group: "example.io".into(),
                version: "v1".into(),
                kind: "Thing".into(),
            },
            columns: vec![Column {
                id: "note".into(),
                header_key: "col.note".into(),
                kind: ColumnKind::Text,
                sortable: true,
                field: Some("spec.note".into()),
            }],
            status: vec![],
            conditions: vec![],
            health: vec![],
            actions: vec![],
        };
        let resource = serde_json::json!({
            "metadata": {"name": "x"},
            "spec": {"note": "a redacted-looking but real value"}
        });
        let row = render_row(&vd, &resource);
        assert_eq!(
            row.cells[0],
            Cell::Text {
                value: "a redacted-looking but real value".into()
            }
        );
    }
}
