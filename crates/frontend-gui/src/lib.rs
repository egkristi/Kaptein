//! Kaptein egui desktop + wasm frontend.
//!
//! Renders the view-model's render-intent; owns **geometry**, never semantics.
//! Uses `egui_table` for the virtualized `Table` surface (ADR-0001).

#![forbid(unsafe_code)]
