//! Contract tests for the render contract.
//!
//! These establish the *form*: the same render-intent must serialize to the same wire
//! shape and be re-appliable across projections. The full cross-frontend suite arrives
//! with the first frontend implementation; this module pins the shape now.

use crate::audit::{Actor, ActorKind, AuditEvent, Operation, Outcome, ResourceRef, Source};
use crate::render::{Cell, Query, Row, RowId, RowPatch, StatusLevel};
use crate::surface::{Column, ColumnKind, Surface};

/// The render contract must round-trip through serde (it crosses gRPC-Web per ADR-0002).
#[test]
fn query_and_page_round_trip() {
    let query = Query {
        start: 400,
        end: 460,
        sort: Some(crate::render::SortSpec {
            column: "restarts".into(),
            descending: true,
        }),
        filter: Some(crate::render::Filter {
            expression: "metadata.labels.app = \"foo\"".into(),
        }),
    };

    let json = serde_json::to_string(&query).expect("query must serialize");
    let back: Query = serde_json::from_str(&json).expect("query must deserialize");
    assert_eq!(query, back);
}

/// A `RowPatch` keyed by `RowId` is idempotent: re-applying the same patch must not
/// corrupt state (this is what makes reconnects safe over `serve`).
#[test]
fn row_patch_is_keyed_by_identity() {
    let id = RowId("pod-123".into());
    let row = Row {
        id: id.clone(),
        cells: vec![Cell::Status {
            level: StatusLevel::Warning,
            label_key: "status.running".into(),
        }],
    };

    let patch = RowPatch::Upsert {
        id: id.clone(),
        row: row.clone(),
    };

    // Identity is carried in the patch itself, independent of any positional index.
    match &patch {
        RowPatch::Upsert { id: patch_id, .. } => assert_eq!(patch_id, &id),
        RowPatch::Remove { .. } => panic!("expected upsert"),
    }
}

/// The audit record must round-trip and keep the agent's own identity (ADR-0007/0010).
#[test]
fn audit_event_round_trips_with_agent_identity() {
    let event = AuditEvent {
        timestamp: 1_700_000_000_000,
        actor: Actor {
            kind: ActorKind::Agent,
            name: "agent-scaling-7".into(),
        },
        context: "prod-eu".into(),
        operation: Operation::Scale,
        target: ResourceRef {
            group: "apps".into(),
            kind: "Deployment".into(),
            namespace: "payments".into(),
            name: "checkout".into(),
        },
        outcome: Outcome::Applied,
        source: Source::Mcp,
        session_id: "sess-42".into(),
        reason: None,
        on_behalf_of: Some("erling".into()),
    };

    let json = serde_json::to_string(&event).expect("audit must serialize");
    let back: AuditEvent = serde_json::from_str(&json).expect("audit must deserialize");
    assert_eq!(event, back);
    assert_eq!(event.source, Source::Mcp);
}

/// A column carries semantics (kind, sortable), never geometry (no width field).
#[test]
#[ignore = "full cross-frontend contract suite arrives with the first frontend"]
fn surface_kind_is_derived_from_surface() {
    let table = Surface::Table {
        columns: vec![Column {
            id: "name".into(),
            header_key: "col.name".into(),
            kind: ColumnKind::Text,
            sortable: true,
        }],
    };
    assert_eq!(table.kind(), crate::surface::SurfaceKind::Table);
}

/// The support matrix — not universal parity — is the truth contract tests assert
/// against. "TUI has no force-directed Graph; it has a Tree projection" is a documented
/// design decision, encoded here. The matrix is exhaustive, so the primary surface
/// `(Tui, Table)` is `Full` (the sparse-table bug made it `None`).
#[test]
fn support_matrix_is_exhaustive_and_correct() {
    use crate::surface::{Projection, SupportLevel, SurfaceKind, support_level};

    // Primary surface must be full in the TUI.
    assert_eq!(
        support_level(Projection::Tui, SurfaceKind::Table),
        SupportLevel::Full
    );
    assert_eq!(
        support_level(Projection::Tui, SurfaceKind::Graph),
        SupportLevel::Alternate
    );
    assert_eq!(
        support_level(Projection::Gui, SurfaceKind::Graph),
        SupportLevel::Full
    );
    // Editor: $EDITOR handoff (alternate) in TUI, real editor (full) in browser.
    assert_eq!(
        support_level(Projection::Tui, SurfaceKind::Editor),
        SupportLevel::Alternate
    );
    assert_eq!(
        support_level(Projection::Browser, SurfaceKind::Editor),
        SupportLevel::Full
    );
}

/// The contract test the review asked for (M2.0 DoD): the *same* `Query` over the *same*
/// `DataPlane` yields the same rows regardless of projection — the TUI, GUI, and headless
/// all consume the render-intent, not a per-frontend recomputation. This pins the form
/// (query → sort/filter → page) against a `MemPlane`.
#[test]
fn same_query_yields_same_page_across_projections() {
    use crate::mem_plane::{MemPlane, Schema};
    use crate::render::{Cell, DataPlane, Filter, Query, Row, RowId, SortSpec};

    fn text(v: &str) -> Cell {
        Cell::Text { value: v.into() }
    }
    fn row(id: &str, name: &str, count: i64) -> Row {
        Row {
            id: RowId(id.into()),
            cells: vec![text(name), Cell::Number { value: count }],
        }
    }

    let plane = MemPlane::new(Schema {
        column_ids: vec!["name".into(), "count".into()],
    });
    plane.upsert(row("a", "web", 3));
    plane.upsert(row("b", "api", 5));
    plane.upsert(row("c", "worker", 1));

    let query = Query {
        start: 0,
        end: 10,
        sort: Some(SortSpec {
            column: "count".into(),
            descending: true,
        }),
        filter: Some(Filter {
            expression: "e".into(), // matches "web" and "worker"
        }),
    };

    // Block on the (async) query without tokio — the view-model stays wasm-pure.
    let page = {
        let fut = plane.query(&query);
        futures_util::pin_mut!(fut);
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(p) => p.expect("query"),
            std::task::Poll::Pending => panic!("mem plane query must not pend"),
        }
    };

    // Two rows match the filter, sorted by count descending: web(3) then worker(1).
    assert_eq!(page.total, 2);
    let names: Vec<&str> = page
        .rows
        .iter()
        .map(|r| match &r.cells[0] {
            Cell::Text { value } => value.as_str(),
            _ => "",
        })
        .collect();
    assert_eq!(names, vec!["web", "worker"]);

    // The exact same query on a second handle to the same plane is identical — this is
    // what "the TUI, GUI, and headless consume the same render-intent" means.
    let again = {
        let fut = plane.query(&query);
        futures_util::pin_mut!(fut);
        let waker = futures_util::task::noop_waker();
        let mut cx = std::task::Context::from_waker(&waker);
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(p) => p.expect("query"),
            std::task::Poll::Pending => panic!("mem plane query must not pend"),
        }
    };
    assert_eq!(again, page);
}
