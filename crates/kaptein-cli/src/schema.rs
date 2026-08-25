//! Embedded view-definition (lens) schema.
//!
//! The canonical `extensions/viewdef.schema.json` lives outside this crate, but
//! `cargo publish` packages only the crate directory — an `include_str!` that reaches
//! `../../../extensions/...` would fail to build the published tarball. The schema is
//! therefore embedded here as a `const` (MIT/Apache-2.0 extension surface, ADR-0004), so
//! `kaptein viewdef schema` works both in-repo and from the crates.io package.

/// The versioned JSON Schema for view definitions (lens schema v1).
pub const VIEWDEF_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$id": "https://kaptein.io/schemas/viewdef/v1.json",
  "title": "Kaptein view definition (lens) — schema v1",
  "description": "A declarative view definition that binds a CRD (or built-in resource) to columns, status inference, and actions — no code. This is the MIT/Apache-2.0 extension surface (ADR-0004 tier 1, ADR-0012). `api_version` must be 1; a future breaking change bumps it and this schema $id.",
  "type": "object",
  "required": ["id", "api_version", "target"],
  "additionalProperties": false,
  "properties": {
    "id": {
      "type": "string",
      "description": "Unique reverse-DNS id, e.g. \"com.example.cnpg-lens\".",
      "pattern": "^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$"
    },
    "api_version": {
      "type": "integer",
      "const": 1,
      "description": "The lens schema version this document targets. This release supports v1."
    },
    "target": {
      "type": "object",
      "required": ["version", "kind"],
      "additionalProperties": false,
      "properties": {
        "group": { "type": "string", "default": "", "description": "API group (empty for core)." },
        "version": { "type": "string", "description": "API version, e.g. \"v1\"." },
        "kind": { "type": "string", "description": "Resource kind, e.g. \"Pod\", \"Cluster\" (CNPG), \"VirtualMachine\" (KubeVirt)." }
      }
    },
    "columns": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["id", "header_key", "kind"],
        "additionalProperties": false,
        "properties": {
          "id": { "type": "string", "description": "Column id (semantics, not geometry)." },
          "header_key": { "type": "string", "description": "Dotted i18n header key, e.g. \"col.name\"." },
          "kind": { "type": "string", "enum": ["text", "number", "timestamp", "status"] },
          "sortable": { "type": "boolean", "default": true },
          "field": { "type": "string", "description": "Dotted JSON path supplying this column's value (e.g. \"metadata.name\", \"spec.instances\"). Required for non-status columns; omitted for the Status column whose value is inferred." }
        }
      }
    },
    "status": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["field", "op", "value", "level"],
        "additionalProperties": false,
        "properties": {
          "field": { "type": "string", "description": "Dotted JSON path, e.g. \"status.phase\" or \"spec.containers[0].name\"." },
          "op": { "type": "string", "enum": ["eq", "ne", "gt", "gte", "lt", "lte", "contains"] },
          "value": { "description": "Value to compare against (string/number/bool)." },
          "level": { "type": "string", "enum": ["ok", "info", "warning", "error", "pending"] }
        }
      }
    },
    "conditions": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["condition_type", "status", "level"],
        "additionalProperties": false,
        "properties": {
          "condition_type": { "type": "string", "description": "The Kubernetes condition type, e.g. \"Ready\" or \"ReconciliationSucceeded\"." },
          "status": { "type": "string", "enum": ["True", "False", "Unknown"], "description": "The condition status to match." },
          "level": { "type": "string", "enum": ["ok", "info", "warning", "error", "pending"] }
        }
      }
    },
    "actions": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["id", "label_key"],
        "additionalProperties": false,
        "properties": {
          "id": { "type": "string", "description": "Stable action id, e.g. \"describe\"." },
          "label_key": { "type": "string", "description": "Dotted i18n key, e.g. \"action.describe\"." },
          "state": { "type": "string", "enum": ["allowed", "forbidden", "gated"], "default": "allowed" }
        }
      }
    }
  }
}
"#;
