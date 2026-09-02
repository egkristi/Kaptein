//! Kaptein view-model: the renderer-agnostic domain layer (the product).
//!
//! This crate owns all *semantics* — columns, sorting, filtering, status inference,
//! permission decisions, and action graphs. The frontends (TUI, GUI, headless, serve)
//! consume the render contract defined here and own only *geometry*.
//!
//! See ADR-0005 (`docs/adr/0005-render-intent-three-layers.md`) for the three-layer
//! model this crate implements.

#![forbid(unsafe_code)]

pub mod audit;
pub mod diff;
pub mod error;
pub mod fuzzy;
pub mod lens;
pub mod logparse;
pub mod mem_plane;
pub mod render;
pub mod semantic;
pub mod sink;
pub mod surface;
pub mod table;
pub mod versioned;

pub use audit::{Actor, ActorKind, AuditEvent, Operation, Outcome, ResourceRef, Source};
pub use diff::{DiffLine, UnifiedDiff, render_unified, unified_diff};
pub use error::Error;
pub use fuzzy::{FuzzyMatch, FuzzyRanked, fuzzy_jump, fuzzy_rank_indices};
pub use lens::{
    ConditionRule, GroupVersionKind, HealthCheck, HealthFinding, LENS_SCHEMA_VERSION, LensAction,
    REDACTED_MARKER, Redacted, RuleOp, StatusRule, ViewDefinition, evaluate_health,
    evaluate_status, render_row, validate_viewdef,
};
pub use logparse::{LogCell, ParsedLogLine, infer_columns, parse_json_line, parse_log_stream};
pub use mem_plane::{MemPlane, Schema};
pub use render::{
    Cell, DataPlane, Filter, Page, Query, Revision, Row, RowId, RowPatch, SortSpec, StatusLevel,
};
pub use semantic::{Action, ActionState, Status, action_verb, downgrade_forbidden};
pub use sink::{AuditConfig, AuditSink};
pub use surface::{
    Column, ColumnKind, EditorMode, Field, FieldKind, Projection, SupportLevel, Surface,
    SurfaceKind, support_level,
};
pub use table::{cell_text, cmp_cells, filter_rows, sort_rows};
pub use versioned::{
    ApiVersion, MCP_API_VERSION, MCP_VERSION_META_KEY, is_compatible, parse_api_version,
};

#[cfg(test)]
mod contract_tests;
