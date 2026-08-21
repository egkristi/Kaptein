//! Layer 3 — surface kinds.
//!
//! A small, **closed** set of surfaces. Each frontend implements the set once; new
//! views are combinations of these, never new variants. The set is complete by design —
//! `Form` and `Matrix` are included because both appear in the roadmap (see ADR-0005).
//!
//! `SurfaceKind` is derived from `Surface` with `strum::EnumDiscriminants`, so the two
//! lists can never drift apart.

use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

/// A single surface, carrying the kind and any kind-specific data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, EnumDiscriminants)]
#[strum_discriminants(derive(Serialize, Deserialize))]
#[strum_discriminants(name(SurfaceKind))]
pub enum Surface {
    Table {
        columns: Vec<Column>,
    },
    Tree {
        columns: Vec<Column>,
    },
    /// Known incomplete: `Graph` carries no data yet (topology layout is still to be
    /// designed — see ADR-0005's "known incomplete" note).
    Graph,
    Form {
        fields: Vec<Field>,
    },
    /// Two-dimensional virtualized data. Axis labels are the *identity* of each axis;
    /// the cells themselves are queried through the data plane (like `Table`), never
    /// materialized here.
    Matrix {
        row_axis: Vec<String>,
        col_axis: Vec<String>,
    },
    /// Known incomplete: stream contents are described by the data plane, not this unit.
    Stream,
    Editor {
        mode: EditorMode,
    },
    /// Known incomplete: chart configuration (series, axes) is still to be designed.
    Chart,
    /// Known incomplete: terminal I/O framing is still to be designed.
    Terminal,
}

impl Surface {
    pub fn kind(&self) -> SurfaceKind {
        SurfaceKind::from(self)
    }
}

/// The closed set of surface kinds, derived from `Surface` via
/// `#[strum_discriminants(name(SurfaceKind))]`, so the variant lists cannot drift apart.
/// Adding a new kind is a breaking contract change (see `docs/versioning.md`).
///
/// The mode of an `Editor` surface. Diff is a *mode* (two buffers), not a separate kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EditorMode {
    Single,
    Diff { left: String, right: String },
}

/// A column definition. The view-model owns *meaning* (id, header key, data kind,
/// sortability); the frontend owns *geometry* (rendered width in cells vs. font
/// metrics). There is deliberately no `width` field here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub id: String,
    /// Message key resolved by the frontend for i18n (the view-model emits keys + args,
    /// not localized strings).
    pub header_key: String,
    /// The data kind — semantics (numeric vs. text) drives alignment and sort order.
    pub kind: ColumnKind,
    pub sortable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Text,
    Number,
    Timestamp,
    Status,
}

/// A schema-driven form field. The semantic layer defines the field; the frontend
/// renders the widget. Diff and validation live in the semantic layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub id: String,
    pub label_key: String,
    pub kind: FieldKind,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldKind {
    Text,
    Number,
    Bool,
    /// One-of selection (e.g. instance type, size class). Options are identity values;
    /// display labels are message keys resolved by the frontend.
    Choice {
        options: Vec<String>,
    },
}
