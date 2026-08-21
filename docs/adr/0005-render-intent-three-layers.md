# ADR-0005: `RenderIntent` is three layers, not one snapshot

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

The original `RenderIntent` sketch was a single materialized table
(`columns`/`rows`/`actions`/`status`/`selection`). It cannot express most of the product
(topology graph, timeline scrubber, log stream, YAML editor + diff, PromQL plot, fleet
matrix, Hubble flow-map, exec terminal), would force full materialization of potentially
hundreds of thousands of rows, and has no model for the deltas that informers already
produce.

## Decision

Split the render contract into **three layers**:

1. **Data plane** — a virtualized, queryable source that emits deltas.
   - `query(range, sort, filter) -> Page` (lazy, never materializes the world)
   - `Stream<RowPatch>` carrying a **revision number**, so a consumer can detect
     staleness and re-request.
2. **Semantic layer** — the genuinely renderer-agnostic part: actions, RBAC state, status
   inference, blast radius. Same for every frontend.
3. **Surface kinds** — a small, **closed** set: `Table`, `Tree`, `Graph`, `Form`,
   `Matrix`, `Stream`, `Editor`, `Chart`, `Terminal`. Each frontend implements the set
   **once**. New views are *combinations* of these, never new variants.

The set is closed, so it must be complete. Three additions were needed beyond the first
draft, each justified by an existing roadmap feature:

- **`Form`** — schema-driven structured input, neither free-text (`Editor`) nor tabular
  (`Table`): the NetworkPolicy "can A reach B" simulation, VM creation from instance
  types, break-glass confirmation with structured input, extension configuration, the
  fleet-query builder, and Kueue quota editing. Without it, every one of these becomes an
  ad-hoc frontend widget — logic in the frontend, which the architecture exists to
  prevent.
- **`Matrix`** — two-dimensional data (clusters × resources, drift matrix, cross-cluster
  diff) with per-cell status. A `Table` has one header row; `Matrix` virtualizes in *both*
  axes and has its own sort semantics (gpui-component makes the same distinction between
  virtualized rows and columns).
- **Diff** is a *mode*, not a kind. It appears in at least five places (dry-run, Helm
  values, time machine, cross-cluster, rendered-vs-live GitOps). It is expressed as an
  `Editor` with two buffers for free-text/YAML, or a `Table`/`Matrix` variant with
  row/cell-level diff decoration for structured data. The semantic layer provides the
  diff; the surface kind renders it.

The **timeline scrubber** (time machine) is a `Chart` with x-axis interaction — a
scrubbable time series, not a distinct kind.

## Consequences

- **Positive:** a contract that can express the whole product; virtualized rows that meet
  the performance budget; deltas that make informer-driven updates cheap over the
  `serve`/gRPC-Web path.
- **Contract tests become meaningful:** the same query yields the same rows, actions, and
  enabled state across projections — not just "the same enum variant was passed."
- **Negative:** more design up front; the closed surface-kind set is a real API surface
  that must stay stable — which is why `Form` and `Matrix` are included now, not bolted
  on later.
- **Renaming:** the `kube-*` crates are renamed to `kaptein-*` (see ADR-0009); the
  three-layer contract lives in `kaptein-viewmodel`.

## Alternatives considered

- **Single enum of `RenderIntent` variants** — rejected: frontends would each implement
  every variant, and contract tests would only verify that the same variant was passed,
  not that behavior is equivalent.
- **One `RenderIntent` per feature** — rejected: unbounded variant growth and no shared
  virtualized data path.
- **`Form`/`Matrix` as ad-hoc frontend widgets** — rejected: that is logic in the
  frontend, the exact drift the architecture prevents.
