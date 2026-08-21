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
3. **Surface kinds** — a small, **closed** set: `Table`, `Tree`, `Graph`, `Stream`,
   `Editor`, `Chart`, `Terminal`. Each frontend implements the set **once**. New views are
   *combinations* of these, never new variants.

## Consequences

- **Positive:** a contract that can express the whole product; virtualized rows that meet
  the performance budget; deltas that make informer-driven updates cheap over the
  `serve`/gRPC-Web path.
- **Contract tests become meaningful:** the same query yields the same rows, actions, and
  enabled state across projections — not just "the same enum variant was passed."
- **Negative:** more design up front; the closed surface-kind set is a real API surface
  that must stay stable.
- **Renaming:** the `kube-*` crates are renamed to `kaptein-*` (see ADR-0009); the
  three-layer contract lives in `kaptein-viewmodel`.

## Alternatives considered

- **Single enum of `RenderIntent` variants** — rejected: frontends would each implement
  every variant, and contract tests would only verify that the same variant was passed,
  not that behavior is equivalent.
- **One `RenderIntent` per feature** — rejected: unbounded variant growth and no shared
  virtualized data path.
