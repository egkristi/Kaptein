# ADR-0011: Fleet query is a data layer + policy, not a search box

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

The fleet query was specified as "one query, all clusters," but the market already has a
project that solves the multi-cluster data layer: **Clusterpedia**, which syncs resources
from multiple clusters and offers OpenAPI-compatible search with low memory use. k9s's
own issue tracker documents operators running one terminal per cluster and wanting a
shared filter/namespace across `k9s --context a,b`.

## Decision

- Consider **Clusterpedia** as the backend for hub mode rather than reimplementing
  multi-cluster sync; Kaptein adds the operator UX on top.
- Make fleet query a **product**, not a feature, with three capabilities:
  1. **Saved queries** checked into Git.
  2. **Scheduled queries** that generate reports.
  3. **Query-as-policy** — a fleet query can fail a CI job if it returns rows, making
     fleet query a compliance tool, not a search field.

## Consequences

- **Positive:** fleet query + drift become one coherent differentiator (they are the same
  data layer viewed differently); reuses a proven backend.
- **Negative:** depending on Clusterpedia adds an integration surface; if its shape
  doesn't fit, the fallback is a Kaptein-managed sync (more scope).

## Alternatives considered

- **Build multi-cluster sync from scratch** — rejected as first choice: reinvents
  Clusterpedia; keep as fallback.
