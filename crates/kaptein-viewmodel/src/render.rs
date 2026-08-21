//! Layer 1 — the data plane.
//!
//! A virtualized, queryable source that emits deltas. It never materializes the world:
//! the frontend asks for a `Page` and receives `RowPatch` deltas keyed by stable `RowId`.
//!
//! The trait is deliberately **object-safe, async, fallible, and streaming** so it can
//! cross the `serve`/gRPC-Web boundary (ADR-0002): the time machine replays historical
//! state, test fixtures feed recordings, `serve` proxies over the network, and the fleet
//! layer aggregates several clusters — four runtime-swappable implementations.

use std::ops::Range;
use std::pin::Pin;

use futures_util::Stream;
use serde::{Deserialize, Serialize};

use crate::error::Error;

/// A monotonically increasing revision of the underlying store.
///
/// Consumers use it to detect staleness: if a held revision is older than the latest,
/// they re-query rather than assuming their snapshot is current.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Revision(pub u64);

/// A stable row identity — a Kubernetes `uid`, or a `group/kind/namespace/name` tuple
/// when a `uid` is unavailable. Never a positional index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RowId(pub String);

/// A lazy, virtualized query against the data plane.
///
/// `start`/`end`, `sort`, and `filter` describe what the frontend wants *now* — the
/// store returns a bounded page, never a full materialization of every object. The
/// window is a plain `start`/`end` pair (not `std::ops::Range<usize>`) so it serializes
/// stably over the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Query {
    /// Inclusive window start (e.g. 400 for a virtualized table window).
    pub start: usize,
    /// Exclusive window end (e.g. 460).
    pub end: usize,
    /// Sort key and direction (column id + descending flag).
    pub sort: Option<SortSpec>,
    /// Filter predicate in a stable, serializable form (not a closure — it must cross
    /// the `serve`/gRPC-Web boundary).
    pub filter: Option<Filter>,
}

impl Query {
    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortSpec {
    pub column: String,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Filter {
    /// A string in a documented query language (e.g. `metadata.labels.app = "foo"`).
    pub expression: String,
}

/// A single cell value. Typed and redaction-aware (secrets never reach a cell as a
/// plaintext value — the semantic layer already replaced them with a marker).
///
/// Variants use `#[serde(tag = "type")]` for a stable wire representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Cell {
    Text {
        value: String,
    },
    Number {
        value: i64,
    },
    /// A typed instant (unix epoch millis) — the frontend formats and localizes, so
    /// sorting and localization stay possible.
    Timestamp {
        millis: i64,
    },
    /// A redacted value — the frontend renders a mask, never the secret.
    Redacted,
    /// A typed status chip; `level` drives color, `label_key` is localized by the
    /// frontend. Status inference lives in the semantic layer, not in string matching.
    Status {
        level: StatusLevel,
        label_key: String,
    },
}

/// The severity of a status cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusLevel {
    Ok,
    Info,
    Warning,
    Error,
    Pending,
}

/// One row of the virtualized table, keyed by a stable identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub id: RowId,
    pub cells: Vec<Cell>,
}

/// A page of rows plus enough metadata to render it correctly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub rows: Vec<Row>,
    /// Total number of matching rows (for scrollbar sizing), not just this page.
    pub total: usize,
    /// The revision this page reflects.
    pub revision: Revision,
}

/// A delta to a specific row, keyed by `RowId` — never by position. Position is
/// geometry, which the frontend owns; keying by identity keeps the patch stream
/// idempotent and reorder-safe (a reconnect can re-apply the same patch without harm).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RowPatch {
    Upsert { id: RowId, row: Row },
    Remove { id: RowId },
}

/// The data plane exposed to every frontend.
///
/// Object-safe (`Box<dyn DataPlane>`), async, fallible, and streaming — matching ADR-0002
/// (browser → `serve` → `kaptein-core`) and the unified error enum.
#[async_trait::async_trait]
pub trait DataPlane: Send + Sync {
    /// Execute a lazy query and return a bounded page.
    async fn query(&self, query: &Query) -> Result<Page, Error>;

    /// Subscribe to deltas from the given revision onward.
    fn subscribe(&self, from: Revision) -> Pin<Box<dyn Stream<Item = RowPatch> + Send>>;
}
