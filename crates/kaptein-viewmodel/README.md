# kaptein-viewmodel

The renderer-agnostic domain layer — the product. It owns all *semantics*: columns,
sorting, filtering, status inference (including view-definition/lens evaluation),
permission decisions, and action graphs.

Frontends (TUI, GUI, headless, serve) consume the render contract defined here and own
only *geometry*. This crate is wasm-pure.

See the [Kaptein repository](https://github.com/egkristi/Kaptein) for the full
architecture, roadmap, and license terms (BUSL-1.1, converting to MIT on the Change
Date).
