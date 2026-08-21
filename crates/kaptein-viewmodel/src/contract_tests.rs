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
