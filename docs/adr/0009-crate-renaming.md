# ADR-0009: Rename `kube-*` crates to `kaptein-*` and consolidate layout

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

`kube-core` and `kube-viewmodel` occupy the `kube-*` namespace owned by the `kube-rs`
ecosystem (`kube`, `kube-client`, `kube-core`, `kube-derive`, `kube-runtime`, etc.).
Publishing crates under that prefix is confusing at best and colliding at worst. The
layout is also inconsistent: two crates live at the repo root while the rest live under
`crates/`.

## Decision

- Rename `kube-core` → `kaptein-core` and `kube-viewmodel` → `kaptein-viewmodel`.
- Consolidate **all** crates under `crates/`:
  `crates/kaptein-core`, `crates/kaptein-viewmodel`, `crates/frontend-tui`,
  `crates/frontend-gui`, `crates/headless`, `crates/serve`, `crates/plugins`,
  `crates/viewdef`, `crates/ext-sdk`.
- `extensions/` remains at the root as it holds *examples*, not workspace members.

## Consequences

- **Positive:** no namespace collision with `kube-rs`; a single, predictable crate layout.
- **Negative:** one-time rename churn before anything is published (cheap now, expensive
  later).

## Alternatives considered

- **Keep `kube-*`** — rejected: namespace collision and confusion with the `kube-rs`
  ecosystem.
