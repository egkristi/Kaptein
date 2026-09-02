//! Kaptein core benchmark — the Kubernetes-side hot paths.
//!
//! Companion to `kaptein-viewmodel/benches/query.rs`. Where the view-model bench gates
//! the *rendering* hot spot (`MemPlane::query`, fuzzy re-rank) over a synthetic
//! 50 000-row plane, this bench gates the **core** hot paths an operator actually feels
//! when the informer store is under watch churn:
//!
//! - **informer store apply** — a watch delta (Added/Modified/Deleted) → `InformerStore`
//!   upsert/remove + `summary_of` mapping. This is the per-event cost of the ADR-0006
//!   "informer-based, never polling" primitive; a busy cluster advances the revision
//!   per delta, so this runs on every change.
//! - **watchring reduce + push** — a `WatchEvent` → `ChangeRecord` reduction into the
//!   bounded M1.4 ring. The "what changed" landing view reads this on every frame.
//! - **redaction** — `redact_object` over a secret-shaped object, the M1.7 choke point
//!   every serialized resource passes through.
//!
//! Like the view-model bench, this is dependency-free (`harness = false`, no criterion),
//! runs in release mode, prints **machine-readable JSON** to `benchmarks/results/` (see
//! `scripts/bench-record.sh`), and exits non-zero on a budget regression so CI fails
//! loudly. The budgets are generous wall-clock bounds in the same "fails loudly, not
//! precisely" spirit as the `#[test]` guard: an accidental O(n²) or a per-event clone
//! blows orders of magnitude past them, while the linear path passes with wide margin.
//!
//! Run: `cargo bench -p kaptein-core --bench core_paths`.

use std::time::Instant;

use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use kube::api::{DynamicObject, ObjectList, WatchEvent};
use kube::core::GroupVersionKind;

use kaptein_core::store::InformerStore;
use kaptein_core::watchring::{WatchRing, reduce_event};

/// Budget: p99 per-event cost of applying a watch delta to the informer store,
/// in **nanoseconds**. A 50k-object store at a steady 100 deltas/sec must apply each
/// well under a millisecond or the watcher task falls behind. An O(n) clone-per-apply or
/// a quadratic rehash would blow past this; the linear path is a few hundred ns.
const APPLY_P99_BUDGET_NS: u128 = 100_000;

/// Budget: p99 reduce+push of a watch event into the ring, in **nanoseconds**. The
/// landing view reduces every delta; a pathological reduce (or a per-push clone) blows
/// past this while the compact `ChangeRecord` path stays a few hundred ns.
const RING_P99_BUDGET_NS: u128 = 100_000;

/// Budget: redact a secret-shaped object, in **microseconds** per object. Redaction runs
/// before every serialization; a per-key regex compile or a full-tree clone per object
/// would blow past this. (The regexes were already hoisted to `LazyLock` — finding P —
/// so this guards against that regression returning.)
const REDACT_P99_BUDGET_US: u128 = 500;

/// Number of watch deltas applied / ring events reduced / objects redacted.
const EVENTS: usize = 10_000;

/// Number of objects held in the store while deltas are applied (the steady-state size).
const STORE_ROWS: usize = 50_000;

fn pod_obj(name: &str, ns: &str, uid: &str) -> DynamicObject {
    DynamicObject {
        types: Some(kube::core::TypeMeta {
            api_version: "v1".into(),
            kind: "Pod".into(),
        }),
        metadata: ObjectMeta {
            name: Some(name.into()),
            namespace: if ns.is_empty() { None } else { Some(ns.into()) },
            uid: Some(uid.into()),
            ..Default::default()
        },
        data: serde_json::json!({"status": {"phase": "Running"}}),
    }
}

fn secret_obj() -> DynamicObject {
    DynamicObject {
        types: Some(kube::core::TypeMeta {
            api_version: "v1".into(),
            kind: "Secret".into(),
        }),
        metadata: ObjectMeta {
            name: Some("db-secret".into()),
            namespace: Some("default".into()),
            ..Default::default()
        },
        data: serde_json::json!({
            "data": {
                "username": "dXNlcg==",
                "password": "c2VjcmV0",
                "api_key": "plaintext-key",
                "tls.crt": "LS0tLS1CRUdJTiBDRVJUSUZJQ0FURS0tLS0t",
                "tls.key": "LS0tLS1CRUdJTiBQUklWQVRFIEtFWS0tLS0t"
            }
        }),
    }
}

/// p99 of a sorted slice of microsecond measurements.
fn p99_us(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[(samples.len() * 99) / 100]
}

/// p99 of a sorted slice of nanosecond measurements.
fn p99_ns(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[(samples.len() * 99) / 100]
}

fn main() {
    let gvk = GroupVersionKind::gvk("", "v1", "Pod");

    // ---- 1. Informer store apply (seed 50k, then apply 10k deltas). ----
    let store = InformerStore::new();
    let mut seed_items = Vec::with_capacity(STORE_ROWS);
    for i in 0..STORE_ROWS {
        seed_items.push(pod_obj(&format!("pod-{i}"), "ns", &format!("uid-{i}")));
    }
    let list = ObjectList {
        types: kube::core::TypeMeta::default(),
        metadata: Default::default(),
        items: seed_items,
    };
    store.seed(list, &gvk);

    let mut apply_ns = Vec::with_capacity(EVENTS);
    for i in 0..EVENTS {
        // Alternate Added (upsert) and Deleted (remove) to exercise both branches,
        // plus a Modified for the upsert branch's clone path.
        let obj = pod_obj(&format!("churn-{i}"), "ns", &format!("uid-churn-{i}"));
        let event = match i % 3 {
            0 => WatchEvent::Added(obj),
            1 => WatchEvent::Modified(obj),
            _ => WatchEvent::Deleted(obj),
        };
        let t0 = Instant::now();
        store.apply(&event, &gvk);
        apply_ns.push(t0.elapsed().as_nanos());
    }
    let apply_p99 = p99_ns(apply_ns);
    println!(
        "informer store apply over {EVENTS} deltas (store {STORE_ROWS} rows): p99 {apply_p99} ns"
    );

    // ---- 2. Watchring reduce + push (10k events). ----
    let ring = WatchRing::new(10_000);
    let mut ring_ns = Vec::with_capacity(EVENTS);
    for i in 0..EVENTS {
        let obj = pod_obj(&format!("pod-{i}"), "ns", &format!("uid-{i}"));
        let event = WatchEvent::Modified(obj);
        let t0 = Instant::now();
        let record = reduce_event(event, i as i64).expect("modified reduces to a record");
        ring.push(record);
        ring_ns.push(t0.elapsed().as_nanos());
    }
    let ring_p99 = p99_ns(ring_ns);
    println!("watchring reduce+push over {EVENTS} events: p99 {ring_p99} ns");

    // ---- 3. Redaction (10k secret-shaped objects). ----
    let mut redact_us = Vec::with_capacity(EVENTS);
    for _ in 0..EVENTS {
        let mut obj = secret_obj();
        let t0 = Instant::now();
        kaptein_core::redact::redact_object(&mut obj);
        redact_us.push(t0.elapsed().as_micros());
    }
    let redact_p99 = p99_us(redact_us);
    println!("redact secret-shaped object over {EVENTS} objects: p99 {redact_p99} µs");

    // ---- Emit machine-readable JSON for storage / comparison. ----
    let out = serde_json::json!({
        "schema": "kaptein-benchmark/v1",
        "suite": "kaptein-core",
        "git_sha": option_env!("KAPTEIN_BENCH_GIT_SHA").unwrap_or("unknown"),
        "metrics": {
            "store_apply_p99_ns": apply_p99,
            "watchring_p99_ns": ring_p99,
            "redact_p99_us": redact_p99,
        },
    });
    if let Ok(dir) = std::env::var("KAPTEIN_BENCH_OUT") {
        let path = std::path::Path::new(&dir);
        if let Err(e) = std::fs::create_dir_all(path) {
            eprintln!("warning: cannot create bench out dir {dir}: {e}");
        } else {
            let file = path.join("core.json");
            if let Err(e) = std::fs::write(&file, serde_json::to_string_pretty(&out).unwrap()) {
                eprintln!("warning: cannot write {}: {e}", file.display());
            }
        }
    }

    // ---- Budget gate (fails loudly, not precisely). ----
    let mut failed = false;
    if apply_p99 > APPLY_P99_BUDGET_NS {
        eprintln!(
            "REGRESSION: store apply p99 {apply_p99} ns exceeds budget {APPLY_P99_BUDGET_NS} ns"
        );
        failed = true;
    }
    if ring_p99 > RING_P99_BUDGET_NS {
        eprintln!("REGRESSION: watchring p99 {ring_p99} ns exceeds budget {RING_P99_BUDGET_NS} ns");
        failed = true;
    }
    if redact_p99 > REDACT_P99_BUDGET_US {
        eprintln!(
            "REGRESSION: redact p99 {redact_p99} µs exceeds budget {REDACT_P99_BUDGET_US} µs"
        );
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
}
