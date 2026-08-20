# Architecture

This document is the canonical synthesis of Kaptein's architecture. Individual decisions
are recorded as numbered ADRs under `docs/adr/`; this file ties them together.

## The one rule

**The domain layer is the product.** Layer dependencies are strictly one-directional:

```
kube-core ──► kube-viewmodel ──► frontend-tui
                             ──► frontend-gui
                             ──► headless / serve
```

- `kube-core` owns the Kubernetes client (`kube-rs` + `tokio`), watchers/reflectors, CRD
  discovery, and stores. It must not depend on the view-model or any frontend.
- `kube-viewmodel` owns all logic: columns, sorting, filtering, status inference,
  permission decisions, and action graphs.
- The frontends (`frontend-tui`, `frontend-gui`, `headless`, `serve`) render, never
  compute — they consume a **render-intent** produced by the view-model.

## The two stable interfaces

Two types are the product's load-bearing contract and are defined **first**, in
`kube-viewmodel`, before any frontend:

### `RenderIntent`

The single output every projection consumes. Minimal sketch:

```rust
struct RenderIntent {
    columns: Vec<Column>,      // id, header key, width, alignment
    rows: Vec<Row>,            // cell values (typed, redaction-aware)
    actions: Vec<Action>,      // available actions + enabled/greyed (RBAC preflight)
    status: Option<Status>,    // overall view status
    selection: Selection,      // focus/selection state
}
```

Frontends may *style* this but never recompute it. Contract tests assert the TUI, GUI,
and headless all consume the **same** `RenderIntent` for the same input.

### `AuditEvent`

The single write-audit record, serialized with `serde`, used by both the local audit log
and the incident-timeline export (one format, two consumers):

```rust
struct AuditEvent {
    timestamp: SystemTime,
    context: String,           // cluster/context id — never a secret
    operation: Operation,      // e.g. Delete, Scale, GitPrOpened
    target: ResourceRef,       // group/kind/name/namespace
    actor: String,
    outcome: Outcome,          // applied / dry-run / rejected
}
```

Audit records **operations, not values** — secrets are never persisted.

## The projections

- **`frontend-tui`** (ratatui) — terminal, SSH/bastion. The first daily-driver surface.
- **`frontend-gui`** (egui + wasm) — native desktop, and a browser bundle that relays
  through `serve` (see ADR-0002).
- **`headless`** — agent mode that drives the view-model directly, **no network
  listener**; used for CI and scripting.
- **`serve`** — the network server (`axum` HTTP/REST + gRPC-Web, `tonic` gRPC for native
  peers); the target for the browser UI and the hub mode (M3.2).

### Transport note

Browsers cannot speak raw gRPC. The browser surface is **gRPC-Web (and HTTP/REST)** on
`axum`; `tonic` gRPC is reserved for the native headless↔serve path. See ADR-0002.

## Typed vs. dynamic resources

Two code paths, deliberately separated:

- **Built-in resources** use `k8s-openapi` typed structs (fast, strongly-typed columns).
- **CRDs and list-heavy views** use `DynamicObject` and `PartialObjectMetadata`.

Do not force built-ins through `DynamicObject` (unnecessary `serde_json` thrash), and do
not try to type CRDs at compile time.

## Extensibility

Three tiers, one `extension.yaml` manifest, data-first (see ADR-0004):

1. View definitions (lenses) — declarative YAML/CUE, no code.
2. WASM component-model plugins (WIT) — sandboxed (fuel metering, memory cap,
   default-deny network/FS).
3. Shell-out integrations — external binaries, graceful when absent.

## Storage & the time machine

The watch stream persists to a local embedded store using an append-only, log-structured
layout with compaction + retention TTL (see ADR-0003).

## Related ADRs

- ADR-0001 — `egui` over `iced` (GUI framework)
- ADR-0002 — browser relays through `serve` (transport)
- ADR-0003 — time-machine storage layout
- ADR-0004 — three-tier extension model
