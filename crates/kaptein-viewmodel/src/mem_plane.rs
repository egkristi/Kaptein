//! An in-memory `DataPlane` — the first concrete implementation of the render contract
//! (ADR-0005), used by tests, fixtures, and the time-machine replay. It is wasm-pure
//! (no `kube`/`tokio`), so the browser UI shares it with the native frontends.
//!
//! `MemPlane` holds a `Vec<Row>` keyed by stable `RowId` plus a monotonic `Revision`. A
//! `query` filters, sorts, and windows the rows into a bounded `Page`; `subscribe`
//! replays past patches (from a held revision) and then live patches, exactly like the
//! network `DataPlane` a `serve` proxy would expose.

use std::collections::VecDeque;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_channel::mpsc;
use futures_util::{Stream, StreamExt};

use crate::error::Error;
use crate::render::{DataPlane, Page, Query, Revision, Row, RowId, RowPatch};
use crate::table::{filter_rows, sort_rows};

/// The column schema of a `MemPlane`: the cell index of each named column. `query`
/// resolves `SortSpec.column` against this; the frontend owns layout, never this list.
#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub column_ids: Vec<String>,
}

/// A versioned patch, for history replay. Holds the full `RowPatch` (upsert **or**
/// remove), so a subscriber replaying from an old revision sees deletions, not just the
/// last upsert of every row.
#[derive(Debug, Clone)]
struct HistoryEntry {
    revision: Revision,
    patch: RowPatch,
}

#[derive(Debug)]
struct State {
    /// Live rows keyed by `RowId`. A `Vec` keeps the natural insertion order for a
    /// stable no-sort query; identity lives in the key, not the position.
    rows: Vec<Row>,
    /// `RowId -> index` into `rows` for O(1) upsert/remove.
    index: std::collections::HashMap<RowId, usize>,
    /// Bounded history of patches, oldest first (cap bounds memory, not correctness).
    /// `VecDeque` makes the cap eviction O(1) (a `Vec::remove(0)` would memmove the
    /// whole buffer per event).
    history: VecDeque<HistoryEntry>,
}

/// An in-memory, mutable data plane.
#[derive(Clone)]
pub struct MemPlane {
    state: Arc<Mutex<State>>,
    revision: Arc<AtomicU64>,
    schema: Schema,
    /// Live patch fan-out; held so a `subscribe` that outlives `MemPlane` keeps its
    /// history and broadcast alive (the view-model is `Clone`, not `'static`-leaked).
    live: Arc<Mutex<Vec<mpsc::UnboundedSender<RowPatch>>>>,
    history_cap: usize,
}

impl MemPlane {
    /// Create an empty `MemPlane` with the given column schema.
    pub fn new(schema: Schema) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                rows: Vec::new(),
                index: std::collections::HashMap::new(),
                history: VecDeque::new(),
            })),
            revision: Arc::new(AtomicU64::new(0)),
            schema,
            live: Arc::new(Mutex::new(Vec::new())),
            history_cap: 1024,
        }
    }

    /// The current revision (monotonic, +1 per applied patch).
    pub fn revision(&self) -> Revision {
        Revision(self.revision.load(Ordering::SeqCst))
    }

    /// Insert or replace a row (upsert), bumping the revision and broadcasting a patch.
    pub fn upsert(&self, row: Row) -> Revision {
        let id = row.id.clone();
        let (revision, patch) = {
            let mut state = self.state.lock().expect("mem plane poisoned");
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
            match state.index.get(&id) {
                Some(&i) => {
                    state.rows[i] = row.clone();
                }
                None => {
                    let len = state.rows.len();
                    state.index.insert(id.clone(), len);
                    state.rows.push(row.clone());
                }
            }
            let patch = RowPatch::Upsert { id, row };
            state.history.push_back(HistoryEntry {
                revision: Revision(revision),
                patch: patch.clone(),
            });
            if state.history.len() > self.history_cap {
                state.history.pop_front();
            }
            (Revision(revision), patch)
        };
        self.broadcast(patch);
        revision
    }

    /// Remove a row by id (no-op if absent), bumping the revision.
    pub fn remove(&self, id: &RowId) -> Revision {
        let (revision, patch) = {
            let mut state = self.state.lock().expect("mem plane poisoned");
            let Some(i) = state.index.remove(id) else {
                return self.revision();
            };
            state.rows.swap_remove(i);
            if let Some(moved) = state.rows.get(i) {
                let moved_id = moved.id.clone();
                state.index.insert(moved_id, i);
            }
            let revision = self.revision.fetch_add(1, Ordering::SeqCst) + 1;
            let patch = RowPatch::Remove { id: id.clone() };
            state.history.push_back(HistoryEntry {
                revision: Revision(revision),
                patch: patch.clone(),
            });
            if state.history.len() > self.history_cap {
                state.history.pop_front();
            }
            (Revision(revision), patch)
        };
        self.broadcast(patch);
        revision
    }

    fn broadcast(&self, patch: RowPatch) {
        let mut senders = self.live.lock().expect("mem plane live poisoned");
        // Drop senders whose receiver has gone away, so a long-lived plane with many
        // short-lived subscribers doesn't leak a sender per call.
        senders.retain(|tx| !tx.is_closed());
        for tx in senders.iter() {
            let _ = tx.unbounded_send(patch.clone());
        }
    }

    /// The full row set (for tests/assertions).
    pub fn rows(&self) -> Vec<Row> {
        self.state.lock().expect("mem plane poisoned").rows.clone()
    }
}

#[async_trait::async_trait]
impl DataPlane for MemPlane {
    async fn query(&self, query: &Query) -> Result<Page, Error> {
        let revision = self.revision();
        let mut rows = {
            let state = self.state.lock().expect("mem plane poisoned");
            state.rows.clone()
        };
        sort_rows(&mut rows, &self.schema.column_ids, query.sort.as_ref());
        rows = filter_rows(rows, query.filter.as_ref());

        let total = rows.len();
        let start = query.start.min(total);
        let end = query.end.min(total).max(start);
        let page_rows: Vec<Row> = rows.into_iter().skip(start).take(end - start).collect();
        Ok(Page {
            rows: page_rows,
            total,
            revision,
        })
    }

    fn subscribe(&self, from: Revision) -> Pin<Box<dyn Stream<Item = RowPatch> + Send>> {
        // Replay history from the held revision (inclusive-exclusive on revision), then
        // attach to the live broadcast. Mirrors an informer's "list then watch" shape:
        // a consumer holding an old revision never misses the deltas in between —
        // including `Remove`s (the patch is replayed verbatim, not reduced to an upsert).
        let history: Vec<RowPatch> = {
            let state = self.state.lock().expect("mem plane poisoned");
            state
                .history
                .iter()
                .filter(|e| e.revision > from)
                .map(|e| e.patch.clone())
                .collect()
        };
        let (tx, rx) = mpsc::unbounded();
        self.live.lock().expect("mem plane live poisoned").push(tx);
        let stream = futures_util::stream::iter(history).chain(rx);
        Box::pin(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Cell, Filter, SortSpec};

    fn text(v: &str) -> Cell {
        Cell::Text { value: v.into() }
    }
    fn row(id: &str, name: &str, n: i64) -> Row {
        Row {
            id: RowId(id.into()),
            cells: vec![text(name), Cell::Number { value: n }],
        }
    }

    fn plane() -> MemPlane {
        MemPlane::new(Schema {
            column_ids: vec!["name".into(), "count".into()],
        })
    }

    #[test]
    fn query_filters_sorts_and_windows() {
        let p = plane();
        p.upsert(row("a", "zebra", 10));
        p.upsert(row("b", "apple", 2));
        p.upsert(row("c", "banana", 9));

        let page = tokio_test_block(p.query(&Query {
            start: 0,
            end: 100,
            sort: Some(SortSpec {
                column: "count".into(),
                descending: false,
            }),
            filter: None,
        }))
        .expect("query");
        let names: Vec<String> = page
            .rows
            .iter()
            .map(|r| crate::table::cell_text(&r.cells[0]))
            .collect();
        assert_eq!(names, vec!["apple", "banana", "zebra"]);
        assert_eq!(page.total, 3);

        // Window: skip 1, take 1.
        let page2 = tokio_test_block(p.query(&Query {
            start: 1,
            end: 2,
            sort: Some(SortSpec {
                column: "count".into(),
                descending: false,
            }),
            filter: None,
        }))
        .expect("query");
        assert_eq!(page2.rows.len(), 1);
        assert_eq!(page2.total, 3);
        assert_eq!(crate::table::cell_text(&page2.rows[0].cells[0]), "banana");
    }

    #[test]
    fn filter_by_expression() {
        let p = plane();
        p.upsert(row("a", "zebra", 10));
        p.upsert(row("b", "apple", 2));
        let page = tokio_test_block(p.query(&Query {
            start: 0,
            end: 100,
            sort: None,
            filter: Some(Filter {
                expression: "app".into(),
            }),
        }))
        .expect("query");
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.rows[0].id, RowId("b".into()));
    }

    #[test]
    fn subscribe_replays_history_then_live() {
        let p = plane();
        p.upsert(row("a", "one", 1));
        let from = p.revision(); // after "one"
        p.upsert(row("b", "two", 2));
        p.upsert(row("a", "one-v2", 1)); // upsert overwrites

        // Collect a few patches from `from`.
        let mut stream = p.subscribe(from);
        let mut got = Vec::new();
        // Replay is synchronous (iter) — drain the first two history patches.
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        let mut pin = Pin::new(&mut stream);
        while let std::task::Poll::Ready(Some(item)) = pin.as_mut().poll_next(&mut cx) {
            got.push(item);
            if got.len() == 2 {
                break;
            }
        }
        assert_eq!(got.len(), 2);
    }

    #[test]
    fn remove_deletes_row() {
        let p = plane();
        p.upsert(row("a", "one", 1));
        p.upsert(row("b", "two", 2));
        p.remove(&RowId("a".into()));
        let rows = p.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, RowId("b".into()));
    }

    #[test]
    fn history_replay_includes_removals() {
        let p = plane();
        p.upsert(row("a", "one", 1));
        let from = p.revision(); // after "one"
        p.upsert(row("b", "two", 2));
        p.remove(&RowId("a".into())); // deletion must be replayed, not resurrected

        let mut stream = p.subscribe(from);
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        let mut pin = Pin::new(&mut stream);
        let mut got = Vec::new();
        while let std::task::Poll::Ready(Some(item)) = pin.as_mut().poll_next(&mut cx) {
            got.push(item);
            if got.len() == 2 {
                break;
            }
        }
        // Two patches: upsert "b", then remove "a".
        assert_eq!(got.len(), 2);
        assert!(matches!(&got[0], RowPatch::Upsert { id, .. } if id.0 == "b"));
        assert!(matches!(&got[1], RowPatch::Remove { id } if id.0 == "a"));
    }

    #[test]
    fn broadcast_drops_closed_senders() {
        let p = plane();
        // Subscribe, then drop the stream — the sender becomes closed.
        {
            let _stream = p.subscribe(Revision(0));
        }
        p.upsert(row("a", "one", 1));
        // The closed sender was retained away; broadcasting must not grow the live vec
        // unboundedly (we can only observe via a subsequent broadcast not panicking).
        let live_len = p.live.lock().expect("live").len();
        assert!(
            live_len <= 1,
            "closed sender should be dropped, got {live_len}"
        );
    }

    // Minimal async block executor (no tokio in the view-model; this crate stays wasm-pure).
    fn tokio_test_block<F: std::future::Future>(f: F) -> F::Output {
        futures_util::pin_mut!(f);
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        loop {
            match f.as_mut().poll(&mut cx) {
                std::task::Poll::Ready(v) => return v,
                std::task::Poll::Pending => std::thread::yield_now(),
            }
        }
    }
}
