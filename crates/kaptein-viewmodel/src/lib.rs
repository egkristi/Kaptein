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
pub mod render;
pub mod semantic;
pub mod surface;

pub use audit::AuditEvent;
pub use render::{DataPlane, Page, Query, Revision, RowPatch};
pub use semantic::{Action, ActionState, Status};
pub use surface::{Surface, SurfaceKind};
