# ADR-0003: Time-machine storage uses an append-only, log-structured layout

- **Status:** Accepted
- **Date:** 2026-08-20
- **Deciders:** Kaptein maintainers

## Context

The "time machine" (one of the four differentiators) persists the watch stream locally so
users can scrub backwards and diff between two timestamps. The store choice was left as
"redb or SQLite." The access pattern is write-heavy, append-mostly, and read by time
range — very different from a general-purpose SQL workload.

## Decision

Use an **append-only, log-structured layout** on top of a local embedded store (redb or
SQLite), keyed by `(resource identity, revision/time)`. Reads scan by time range;
compaction + a configurable retention TTL bound disk usage.

## Rationale

- **Write path is trivial and fast** (append), which matters because the informer emits a
  high volume of small updates.
- **Time-range reads map directly** onto the layout, matching "diff between 14:20 and
  14:35."
- **Compaction/retention is explicit**, preventing unbounded local disk growth.

## Consequences

- **Positive:** simple, fast writes; predictable storage; the layout is portable across
  redb/SQLite so the concrete engine can be chosen during implementation without changing
  the shape.
- **Negative:** point lookups by arbitrary field (e.g. "all pods with image X") require a
  separate index; the layout is optimized for time-travel, not analytics.
- **Out of scope:** the centralized/optional hub-mode history (M3.2) may use a different
  store; this ADR covers the local path only.

## Alternatives considered

- **General-purpose SQL tables** — rejected; adds per-row update overhead for an
  append-heavy stream and makes retention less natural.
- **Plain append-only files** — rejected; the embedded store gives transactions and crash
  safety for free.
