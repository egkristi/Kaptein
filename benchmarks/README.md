# Kaptein benchmark results

This directory is the durable, version-controlled home for the benchmark **results** that
every commit and every release must record (see `scripts/bench-record.sh` and the M1.8
milestone). The benchmark *definitions* live in the crates that own the code:

- `crates/kaptein-viewmodel/benches/query.rs` — the view-model render hot spots
  (`MemPlane::query` p50/p99/max, steady-state RSS, cold start, fuzzy re-rank) over a
  synthetic 50 000-row plane.
- `crates/kaptein-core/benches/core_paths.rs` — the Kubernetes-side hot paths
  (informer-store watch-delta apply, watchring reduce+push, `redact_object`) over
  10 000 synthetic events.

Both print **machine-readable JSON** (`schema: kaptein-benchmark/v1`) to
`$KAPTEIN_BENCH_OUT` and exit non-zero on a budget regression, so CI fails loudly.

## Layout

```
benchmarks/
  README.md                 # this file
  schema.json               # the result-file JSON Schema (documentation + linting)
  results/                  # one JSON file per recorded run (git-ignored)
    <git-sha>-<timestamp>.json
```

`results/` is **git-ignored** — individual runs are ephemeral and noisy. The comparable,
useful artifact is the **trend**: `scripts/bench-record.sh` writes the latest run and
prints a diff against the previous run, so a regression is visible at a glance.

## Recording a run

```bash
KAPTEIN_BENCH_GIT_SHA="$(git rev-parse --short HEAD)" \
  ./scripts/bench-record.sh
```

The script runs both benches, merges their JSON into one result file, and prints:

- the merged metrics, and
- a line-by-line comparison against the most recent previous result (if any).

## Result-file schema

`schema: "kaptein-benchmark/v1"`; top-level keys are `suite`, `git_sha`, `host` (optional),
and `metrics` (a flat object of `number` values). See `schema.json` for the canonical
description. The metric names are stable across releases so results remain comparable:

- **view-model**: `query_p50_ms`, `query_p99_ms`, `query_max_ms`, `rss_mb`,
  `cold_start_ms`, `fuzzy_rerank_ms`.
- **core**: `store_apply_p99_ns`, `watchring_p99_ns`, `redact_p99_us`.

## What this means for Kubernetes users

The numbers are chosen to answer the questions an operator actually has about a
daily-driver TUI over a large cluster:

- **Is the table still 16 ms/frame at 50 000 objects?** — `query_p99_ms` (budget 8 ms for
  the query half) and `fuzzy_rerank_ms` (the search path).
- **Will it fit in memory?** — `rss_mb` for a 50 000-row plane (budget 250 MB).
- **How fast does it start on a big cluster?** — `cold_start_ms` (budget 500 ms).
- **Does the informer keep up with watch churn?** — `store_apply_p99_ns` (the per-delta
  cost of the ADR-0006 store) and `watchring_p99_ns` (the M1.4 "what changed" ring).
- **Is secret redaction a per-object tax?** — `redact_p99_us` (the M1.7 choke point).

Together they make performance **measurable across releases** instead of aspirational,
which is the whole point of M1.8.
