//! Layer 3 — surface kinds.
//!
//! A small, **closed** set of surfaces. Each frontend implements the set once; new
//! views are combinations of these, never new variants. The set is complete by design —
//! `Form` and `Matrix` are included because both appear in the roadmap (see ADR-0005).

/// A single surface, carrying the kind and any kind-specific data.
#[derive(Debug, Clone, PartialEq)]
pub enum Surface {
    Table {
        columns: Vec<Column>,
    },
    Tree {
        columns: Vec<Column>,
    },
    Graph,
    Form {
        fields: Vec<Field>,
    },
    Matrix {
        rows: Vec<String>,
        cols: Vec<String>,
    },
    Stream,
    Editor {
        buffers: u8,
    },
    Chart,
    Terminal,
}

impl Surface {
    pub fn kind(&self) -> SurfaceKind {
        match self {
            Surface::Table { .. } => SurfaceKind::Table,
            Surface::Tree { .. } => SurfaceKind::Tree,
            Surface::Graph => SurfaceKind::Graph,
            Surface::Form { .. } => SurfaceKind::Form,
            Surface::Matrix { .. } => SurfaceKind::Matrix,
            Surface::Stream => SurfaceKind::Stream,
            Surface::Editor { .. } => SurfaceKind::Editor,
            Surface::Chart => SurfaceKind::Chart,
            Surface::Terminal => SurfaceKind::Terminal,
        }
    }
}

/// The closed set of surface kinds. Adding a new kind is a breaking contract change
/// (see `docs/versioning.md`), so the set is expanded only deliberately, never ad hoc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceKind {
    Table,
    Tree,
    Graph,
    /// Schema-driven structured input (NetworkPolicy "can A reach B", VM creation from
    /// instance types, break-glass confirmation, extension config, fleet-query builder,
    /// Kueue quota editing). Neither free-text (`Editor`) nor tabular (`Table`).
    Form,
    /// Two-dimensional data (clusters × resources, drift matrix, cross-cluster diff)
    /// with per-cell status; virtualizes in both axes.
    Matrix,
    Stream,
    /// Diff is a *mode* over `Editor` (free-text/YAML, two buffers) or over
    /// `Table`/`Matrix` (row/cell-level diff decoration), not a separate kind.
    Editor,
    /// Time-series including the time-machine scrubber (x-axis interaction).
    Chart,
    Terminal,
}

/// A column definition. The view-model owns *meaning* (id, header key); the frontend
/// owns *geometry* (rendered width in cells vs. font metrics). There is deliberately no
/// `width` field here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub id: String,
    /// Message key resolved by the frontend for i18n (the view-model emits keys + args,
    /// not localized strings).
    pub header_key: String,
}

/// A schema-driven form field. The semantic layer defines the field; the frontend
/// renders the widget. Diff and validation live in the semantic layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub id: String,
    pub label_key: String,
    pub kind: FieldKind,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    Text,
    Number,
    Bool,
    /// One-of selection (e.g. instance type, size class).
    Choice {
        options: Vec<String>,
    },
}
