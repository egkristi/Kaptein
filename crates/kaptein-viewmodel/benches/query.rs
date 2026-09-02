//! M1.8 performance benchmark — the budget is *measured*, not aspirational.
//!
//! This is the view-model half of the kwok harness's performance budget: it drives the
//! `MemPlane::query` hot spot (sort + filter + window) over a synthetic 50 000-row
//! plane and reports the p99 latency **and** the steady-state memory footprint. The full
//! kwok harness owns the end-to-end keystroke-to-frame, RSS, and cold-start numbers
//! (frontend-level, needs a cluster synthetic); this bench owns the two numbers the
//! view-model can measure portably and gate on its own: **p99 query latency** and
//! **process RSS** at 50 000 objects.
//!
//! Run with `cargo bench -p kaptein-viewmodel --bench query` (release mode). A non-zero
//! exit is a regression — CI treats it as a failure. The budgets are deliberately
//! generous wall-clock/memory bounds (the same "fails loudly, not precisely" philosophy
//! as the `#[test]` guard): an accidental O(n²) sort or an unbounded per-row allocation
//! blows orders of magnitude past them, while the linear path passes with a wide margin.

use std::time::Instant;

use kaptein_viewmodel::{Cell, DataPlane, MemPlane, Query, Row, RowId, Schema, SortSpec};

/// p99 query latency budget at 50 000 rows (milliseconds). The roadmap's
/// keystroke-to-frame budget is 16 ms; query is its dominant part, so the query-only
/// budget is set at half that. Generous enough to be noise-immune on CI, tight enough
/// that a quadratic regression (hundreds of ms) fails loudly.
const P99_QUERY_BUDGET_MS: u128 = 8;

/// Steady-state process RSS budget at 50 000 rows (megabytes). The roadmap's target is
/// 250 MB for the whole TUI (frontend + plane); the plane alone must stay well under it.
/// A per-row leak (e.g. a `Vec` growth bug or a clone per row) would blow past this while
/// the linear path sits far below.
const RSS_BUDGET_MB: u64 = 250;

/// Cold-start budget: seed 50 000 rows into a fresh `MemPlane` **and** answer the first
/// query, in milliseconds. The roadmap's target is "cold start to first usable frame
/// < 500 ms"; the `LivePlane::seed` → `MemPlane::upsert` path plus the first sort/window
/// query is the view-model-ownable half of that (the kube `list` that fills it is
/// network-bound and measured by the kwok harness, not this offline bench). An O(n²) seed
/// or an eager per-row allocation blow past this; the linear path sits far below.
const COLD_START_BUDGET_MS: u128 = 500;

/// Number of rows in the synthetic store (the roadmap's "50 000 objects in store").
const ROWS: usize = 50_000;

/// Number of query iterations over which p99 is computed (more = stabler percentile).
const ITERATIONS: usize = 200;

fn text(v: &str) -> Cell {
    Cell::Text {
        value: v.to_string(),
    }
}

fn row(i: usize) -> Row {
    Row {
        id: RowId(format!("row-{i}")),
        // Two cells: a name (Text) and a count (Number), mirroring the built-in table's
        // shape so the sort exercises both the allocation-free Text path and the
        // heterogeneous Number fallback.
        cells: vec![text(&format!("name-{i}")), Cell::Number { value: i as i64 }],
    }
}

/// The process's resident set size in megabytes, read from `/proc/self/status` (Linux).
/// Returns `None` on non-Linux (macOS/Windows), where the RSS budget is not gated — the
/// p99 latency budget still applies everywhere.
fn resident_mb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let vm_rss_kb = status.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key == "VmRSS" {
            value.split_whitespace().next()?.parse::<u64>().ok()
        } else {
            None
        }
    })?;
    Some(vm_rss_kb / 1024)
}

fn main() {
    let plane = MemPlane::new(Schema {
        column_ids: vec!["name".to_string(), "count".to_string()],
    });
    for i in 0..ROWS {
        plane.upsert(row(i));
    }
    assert_eq!(plane.revision().0 as usize, ROWS);

    // Warm up once so the first iteration's cold page-faults don't skew the percentile.
    let warmup = Query {
        start: 0,
        end: 60,
        sort: Some(SortSpec {
            column: "count".into(),
            descending: true,
        }),
        filter: None,
    };
    block_on(plane.query(&warmup)).expect("warmup query");

    // The measured query: a full sort over the whole set + a windowed result. This is
    // exactly what `query_plane` issues per keystroke (sort + window; `total` carried
    // separately), so p99 of this is the number the budget names.
    let query = Query {
        start: 0,
        end: 60,
        sort: Some(SortSpec {
            column: "name".into(),
            descending: false,
        }),
        filter: None,
    };
    let mut latencies = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let t0 = Instant::now();
        let page = block_on(plane.query(&query)).expect("query");
        let elapsed = t0.elapsed().as_millis();
        // Sanity: the window is honored, not silently ignored.
        assert_eq!(page.total, ROWS);
        assert_eq!(page.rows.len(), 60);
        latencies.push(elapsed);
    }
    latencies.sort_unstable();

    let p50 = latencies[ITERATIONS / 2];
    let p99 = latencies[(ITERATIONS * 99) / 100];
    let max = latencies[ITERATIONS - 1];
    println!(
        "query over {ROWS} rows × {ITERATIONS} iters — p50 {p50} ms, p99 {p99} ms, max {max} ms"
    );

    if p99 > P99_QUERY_BUDGET_MS {
        eprintln!("REGRESSION: p99 query latency {p99} ms exceeds budget {P99_QUERY_BUDGET_MS} ms");
        std::process::exit(1);
    }

    // Steady-state memory: report the RSS while the 50k-row plane is held. Gated on
    // Linux only (the RSS budget is the frontend-level target; this guards the plane's
    // own footprint, which must stay well under it).
    let rss_mb: Option<u64> = match resident_mb() {
        Some(rss) => {
            println!("steady-state RSS with {ROWS} rows held: {rss} MB");
            if rss > RSS_BUDGET_MB {
                eprintln!("REGRESSION: RSS {rss} MB exceeds budget {RSS_BUDGET_MB} MB");
                std::process::exit(1);
            }
            Some(rss)
        }
        None => {
            println!("steady-state RSS: not measured on this platform");
            None
        }
    };

    // Cold start: build a *fresh* plane, seed all 50k rows, and answer the first query.
    // This measures the view-model-ownable half of "cold start to first usable frame"
    // (seed + first sort/window); the kube list that fills it is network-bound and the
    // kwok harness's job.
    let cold_start = Instant::now();
    let fresh = MemPlane::new(Schema {
        column_ids: vec!["name".to_string(), "count".to_string()],
    });
    for i in 0..ROWS {
        fresh.upsert(row(i));
    }
    let first = block_on(fresh.query(&query)).expect("first query");
    assert_eq!(first.total, ROWS);
    let cold_ms = cold_start.elapsed().as_millis();
    println!("cold start (seed {ROWS} rows + first query): {cold_ms} ms");
    if cold_ms > COLD_START_BUDGET_MS {
        eprintln!("REGRESSION: cold start {cold_ms} ms exceeds budget {COLD_START_BUDGET_MS} ms");
        std::process::exit(1);
    }

    // Fuzzy re-rank (finding AA): the TUI's per-keystroke jump path ranks the whole store
    // via `fuzzy_rank_indices` (no per-row allocation). This gates the *search* path's
    // keystroke-to-frame latency, which the table-path query gate above did not see — the
    // blind spot that let finding AA's clone-per-keystroke land unnoticed. Budget is the
    // same 16 ms frame target: ranking 50k names must complete within one frame.
    let names: Vec<String> = (0..ROWS).map(|i| format!("name-{i}")).collect();
    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let rerank = Instant::now();
    let ranked = kaptein_viewmodel::fuzzy_rank_indices(name_refs.iter().copied(), "ame-4");
    let rerank_ms = rerank.elapsed().as_millis();
    // Sanity: a subsequence query over "name-<n>" matches everything (all contain "ame-4"
    // only if the digit matches — "ame-4" is a substring of "name-4", "name-40", "name-400"
    // …), so assert a non-empty result to prove the path is exercised.
    assert!(
        !ranked.is_empty(),
        "fuzzy re-rank over 50k names must match"
    );
    println!("fuzzy re-rank over {ROWS} names: {rerank_ms} ms");
    if rerank_ms > P99_QUERY_BUDGET_MS {
        eprintln!(
            "REGRESSION: fuzzy re-rank {rerank_ms} ms exceeds budget {P99_QUERY_BUDGET_MS} ms"
        );
        std::process::exit(1);
    }

    // Emit machine-readable JSON for storage / comparison (scripts/bench-record.sh merges
    // this with the core bench's output into one per-commit result file).
    let out = serde_json::json!({
        "schema": "kaptein-benchmark/v1",
        "suite": "kaptein-viewmodel",
        "git_sha": option_env!("KAPTEIN_BENCH_GIT_SHA").unwrap_or("unknown"),
        "metrics": {
            "query_p50_ms": p50,
            "query_p99_ms": p99,
            "query_max_ms": max,
            "rss_mb": rss_mb,
            "cold_start_ms": cold_ms,
            "fuzzy_rerank_ms": rerank_ms,
        },
    });
    if let Ok(dir) = std::env::var("KAPTEIN_BENCH_OUT") {
        let path = std::path::Path::new(&dir);
        if let Err(e) = std::fs::create_dir_all(path) {
            eprintln!("warning: cannot create bench out dir {dir}: {e}");
        } else {
            let file = path.join("viewmodel.json");
            if let Err(e) = std::fs::write(&file, serde_json::to_string_pretty(&out).unwrap()) {
                eprintln!("warning: cannot write {}: {e}", file.display());
            }
        }
    }
}

/// Minimal block-on for a `MemPlane` query (the view-model is wasm-pure — no tokio here;
/// the bench runs on the host, so a noop-waker spin is sufficient and dependency-free).
fn block_on<F: std::future::Future>(f: F) -> F::Output {
    use std::task::{Context, Poll, Waker};
    // `MemPlane::query` is `async` but completes synchronously (no awaits that yield), so
    // a single poll is always `Ready`. Use a noop waker — we never actually yield.
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut f = Box::pin(f);
    match f.as_mut().poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => unreachable!("MemPlane::query completes without yielding"),
    }
}
