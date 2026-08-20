# ADR-0001: Use `egui` over `iced` for the GUI frontend

- **Status:** Accepted
- **Date:** 2026-08-20
- **Deciders:** Kaptein maintainers

## Context

The `frontend-gui` crate needs a Rust GUI framework that renders the shared
`kube-viewmodel` and also compiles to WASM for the browser UI. The two leading options
are:

- **`iced`** — retained-mode, Elm-style architecture; strong accessibility story.
- **`egui`** — immediate-mode; extremely fast to iterate on data-heavy, custom widgets.

Both target the same platforms (native Win/macOS/Linux) and both support a WASM backend.

## Decision

Use **`egui`**.

## Rationale

- **Immediate mode suits the workload.** The GUI is a projection of an informer-driven
  view-model that changes frequently (tables, graphs, timeline scrubbers, topology). An
  immediate-mode UI avoids the retained widget state that must be manually kept in sync
  with a live data source.
- **Fast iteration on custom, data-dense widgets.** Many of Kaptein's views (topology
  graph, blast-radius preview, fleet matrices) are not standard widget-library shapes and
  benefit from drawing directly.
- **Lower conceptual overhead for a small frontend team** that must also maintain the TUI
  and headless projections — `egui`'s single `update` pass maps cleanly onto "render the
  current view-model."

## Consequences

- **Positive:** fewer moving parts in `frontend-gui`; identical code path for native and
  WASM.
- **Negative:** accessibility and i18n require explicit work (e.g. `egui`'s screen-reader
  and keyboard-focus features are less mature than `iced`'s). These are tracked as
  explicit non-functional requirements and must be verified, not assumed, in Phase 2's
  Definition of Done.
- **Mitigation:** the "same keymap as the TUI" requirement is enforced in the
  view-model's action graph, so keyboard behavior is defined once and shared.

## Alternatives considered

- **`iced`** — rejected primarily for its retained-mode overhead on rapidly mutating,
  data-dense views, despite a better built-in accessibility story.
- **Web frontend (e.g. `leptos`/`yew`)** — rejected; it would duplicate logic and break
  the "thin projection of one view-model" guarantee by introducing a second language.
