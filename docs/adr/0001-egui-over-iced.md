# ADR-0001: Use `egui` over `iced` for the GUI frontend

- **Status:** Accepted
- **Date:** 2026-08-20
- **Deciders:** Kaptein maintainers

## Context

The `frontend-gui` crate needs a Rust GUI framework that renders the shared
`kaptein-viewmodel` and also compiles to WASM for the browser UI. The two leading options
are:

- **`iced`** — Elm-style (view tree rebuilt each `update`), strong accessibility story.
- **`egui`** — immediate-mode; mature ecosystem of data-dense widgets.

Both target the same platforms (native Win/macOS/Linux) and both support a WASM backend.

## Decision

Use **`egui`**.

## Rationale

The deciding factor is concrete and testable: **`egui_table`** (from Rerun) is a
virtualized table that handles **millions of rows** with resizable columns, sticky
headers, and heterogeneous row heights. Kaptein's primary surface is a virtualized table
over informer data (the ADR-0005 `Table` surface kind), and the performance budget
demands 50 000+ objects stay responsive. **`iced` has no virtualized table and no column
resizing out of the box.** That alone settles it.

Secondary points:

- Immediate mode maps cleanly onto "render the current view-model" — no retained widget
  state to synchronize with a live, delta-emitting data plane.
- `egui`'s single `update` pass keeps the frontend thin, which is the architectural goal.

## Consequences

- **Positive:** a battle-tested virtualized table path; identical code for native and
  WASM; fewer moving parts in `frontend-gui`.
- **Negative (with nuance):** accessibility requires explicit work. `egui` has
  **AccessKit** integration, so the gap versus `iced` is smaller than often assumed —
  but i18n and screen-reader support must still be verified, not assumed, in Phase 2's
  Definition of Done. Note also that Norwegian public-sector procurement (forskrift om
  universell utforming av IKT) can cover internal tooling for new acquisitions — treat
  this as a procurement risk, not merely an NFR.
- **Mitigation:** the "same keymap as the TUI" requirement is enforced in the
  view-model's action graph, so keyboard behavior is defined once and shared.

## Alternatives considered

- **`iced`** — rejected because it lacks the virtualized, resizable table that the
  primary surface requires, despite a strong built-in accessibility story.
- **Web frontend (e.g. `leptos`/`yew`)** — rejected; it would duplicate logic and break
  the "thin projection of one view-model" guarantee by introducing a second language.
