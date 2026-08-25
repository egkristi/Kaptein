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

/// Which projections implement which surface kind, and at what fidelity.
///
/// This is the **support matrix** that replaces the (unreachable) promise of universal
/// feature-parity. "TUI has no force-directed Graph layout — it has a keyboard-navigable
/// Tree projection of the same graph" is a documented design decision here, not a
/// contract breach. Contract tests assert against this matrix, not against parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupportLevel {
    /// Full surface: the kind renders natively.
    Full,
    /// Alternate projection: the same semantics, a different geometry (e.g. Graph → Tree).
    Alternate,
    /// Not supported by this projection.
    None,
}

/// A projection (which frontend or agent surface).
///
/// Only the **rendering** projections are here. Headless and MCP are consumers of the
/// *semantic layer* (they read the data plane and action graph, they never render a
/// surface), so they are a different axis — not members of this enum. Modelling them as
/// projections that are always `None` conflates two distinct concepts and implies that
/// e.g. MCP "cannot do graph things", when in fact MCP's `blast_radius` tool *is* graph
/// data (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    Tui,
    Gui,
    Browser,
}

/// Look up the support level for a `(projection, kind)` pair.
///
/// This is an **exhaustive** match, not a sparse lookup table. The compiler forces every
/// new `SurfaceKind` to be considered here — which is the whole point of a closed set. A
/// sparse table with a `None` default silently lied about `(Tui, Table)` (the primary
/// surface) and 19 of the other 26 pairs.
pub fn support_level(projection: Projection, kind: SurfaceKind) -> SupportLevel {
    use Projection::*;
    use SupportLevel::*;
    use SurfaceKind::*;

    match (projection, kind) {
        // These six render natively in every projection.
        (_, Table) => Full,
        (_, Tree) => Full,
        (_, Form) => Full,
        (_, Matrix) => Full,
        (_, Stream) => Full,
        (_, Chart) => Full,

        // Graph: force-directed + mouse in GUI/browser, keyboard-navigable tree
        // projection in the TUI (same data, different geometry).
        (Tui, Graph) => Alternate,
        (Gui, Graph) => Full,
        (Browser, Graph) => Full,

        // Editor: $EDITOR handoff in the TUI, a real editor in GUI/browser (no $EDITOR
        // exists in a browser).
        (Tui, Editor) => Alternate,
        (Gui, Editor) => Full,
        (Browser, Editor) => Full,

        // Terminal: PTY pass-through in the TUI, a full VT emulator in GUI/browser.
        (Tui, Terminal) => Full,
        (Gui, Terminal) => Full,
        (Browser, Terminal) => Full,
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
/// sortability, and the data-binding `field`); the frontend owns *geometry* (rendered
/// width in cells vs. font metrics). There is deliberately no `width` field here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Column {
    pub id: String,
    /// Message key resolved by the frontend for i18n (the view-model emits keys + args,
    /// not localized strings).
    pub header_key: String,
    /// The data kind — semantics (numeric vs. text) drives alignment and sort order.
    pub kind: ColumnKind,
    pub sortable: bool,
    /// For lens (view-definition) columns: the dotted JSON path that supplies this
    /// column's value (e.g. `metadata.name`, `spec.instances`). `None` for built-in
    /// surfaces and for the `Status` column, whose value is *inferred* by the lens's
    /// status rules rather than read from a single field (ADR-0012: the schema must
    /// express where every value comes from).
    #[serde(default)]
    pub field: Option<String>,
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
