//! Layer 1 — the data plane.
//!
//! A virtualized, queryable source that emits deltas. It never materializes the world:
//! the frontend asks for a `Page` and receives `RowPatch` deltas keyed by a `Revision`.

/// A monotonically increasing revision of the underlying store.
///
/// Consumers use it to detect staleness: if a held revision is older than the latest,
/// they re-query rather than assuming their snapshot is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(pub u64);

/// A lazy, virtualized query against the data plane.
///
/// `range`, `sort`, and `filter` describe what the frontend wants *now* — the store
/// returns a bounded page, never a full materialization of every object.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// Inclusive row range requested (e.g. 400..460 for a virtualized table window).
    pub range: std::ops::Range<usize>,
    /// Sort key and direction (column id + descending flag).
    pub sort: Option<SortSpec>,
    /// Filter predicate in a stable, serializable form (not a closure — it must cross
    /// the `serve`/gRPC-Web boundary).
    pub filter: Option<Filter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortSpec {
    pub column: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    /// A string in a documented query language (e.g. `metadata.labels.app = "foo"`).
    pub expression: String,
}

/// A single cell value. Typed and redaction-aware (secrets never reach a cell as a
/// plaintext value — the semantic layer already replaced them with a marker).
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(String),
    Number(i64),
    Timestamp(String),
    /// A redacted value — the frontend renders a mask, never the secret.
    Redacted,
    /// A status-colored chip (e.g. Running/Failed/Pending).
    Status(String),
}

/// One row of the virtualized table.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
}

/// A page of rows plus enough metadata to render it correctly.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub rows: Vec<Row>,
    /// Total number of matching rows (for scrollbar sizing), not just this page.
    pub total: usize,
    /// The revision this page reflects.
    pub revision: Revision,
}

/// A delta to a specific row (or the signal that it was removed).
#[derive(Debug, Clone, PartialEq)]
pub enum RowPatch {
    Upsert { index: usize, row: Row },
    Remove { index: usize },
}

/// The data plane the view-model exposes to every frontend.
pub trait DataPlane {
    /// Execute a lazy query and return a bounded page.
    fn query(&self, query: &Query) -> Page;

    /// Subscribe to deltas from the given revision onward.
    fn subscribe(&self, from: Revision) -> impl Iterator<Item = RowPatch>;
}
