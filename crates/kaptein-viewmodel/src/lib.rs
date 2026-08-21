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
pub mod error;
pub mod render;
pub mod semantic;
pub mod sink;
pub mod surface;

pub use audit::{Actor, ActorKind, AuditEvent, Operation, Outcome, ResourceRef, Source};
pub use error::Error;
pub use render::{
    Cell, DataPlane, Filter, Page, Query, Revision, Row, RowId, RowPatch, SortSpec, StatusLevel,
};
pub use semantic::{Action, ActionState, Status};
pub use sink::{AuditConfig, AuditSink};
pub use surface::{
    Column, ColumnKind, EditorMode, Field, FieldKind, Projection, SupportLevel, Surface,
    SurfaceKind, support_level,
};

#[cfg(test)]
mod contract_tests;
