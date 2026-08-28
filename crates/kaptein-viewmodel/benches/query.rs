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
    match resident_mb() {
        Some(rss) => {
            println!("steady-state RSS with {ROWS} rows held: {rss} MB");
            if rss > RSS_BUDGET_MB {
                eprintln!("REGRESSION: RSS {rss} MB exceeds budget {RSS_BUDGET_MB} MB");
                std::process::exit(1);
            }
        }
        None => println!("steady-state RSS: not measured on this platform"),
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
