# Architecture

This document is the canonical synthesis of Kaptein's architecture. Individual decisions
are recorded as numbered ADRs under `docs/adr/`; this file ties them together.

## The one rule

**The domain layer is the product.** Layer dependencies are strictly one-directional:

```
kaptein-core ──► kaptein-viewmodel ──► frontend-tui
                                 ──► frontend-gui
                                 ──► headless / serve
```

- `kaptein-core` owns the Kubernetes client (`kube-rs` + `tokio`), watchers/reflectors,
  CRD discovery, and stores. It must not depend on the view-model or any frontend.
- `kaptein-viewmodel` owns all logic: columns, sorting, filtering, status inference,
  permission decisions, and action graphs.
- The frontends (`frontend-tui`, `frontend-gui`, `headless`, `serve`) render, never
  compute — they consume a **render-intent** produced by the view-model.

### Semantics vs. geometry

"Frontends render, never compute" is about **semantics**, not geometry. The view-model
owns *meaning*; the frontend owns *layout*. Concretely:

| Owned by view-model (semantics) | Owned by frontend (geometry) |
|---|---|
| Which columns exist, their ids, sort/filter, status | Column *width* (terminal cells vs. font metrics) |
| Actions and their enabled/greyed state (RBAC) | Text truncation (grapheme width vs. glyph advance) |
| Row content, typed and redaction-aware | Scroll position, focus, hover |
| Overall status and selection *identity* | Modal overlay z-order |

The view-model must **never** know which frontend is rendering (no circular dependency);
the frontend must never recompute meaning (no drift).

## The render contract: three layers

The render-intent is **not** one materialized snapshot. It is three layers (see
ADR-0005):

1. **Data plane** — a virtualized, queryable source emitting deltas:
   `query(range, sort, filter) -> Page` and `Stream<RowPatch>` with a revision number.
2. **Semantic layer** — the renderer-agnostic part: actions, RBAC state, status
   inference, blast radius.
3. **Surface kinds** — a small, closed set: `Table`, `Tree`, `Graph`, `Form`, `Matrix`,
   `Stream`, `Editor`, `Chart`, `Terminal`. Each frontend implements the set once; new
   views are combinations, never new variants. `Form` (schema-driven structured input)
   and `Matrix` (two-axis virtualized data) are included up front because both appear in
   the roadmap — see ADR-0005. Diff is a *mode* over `Editor`/`Table`/`Matrix`, not a
   separate kind.

Contract tests assert that the same query yields the same rows, actions, and enabled
state across projections — not merely that the same variant was passed.

### `AuditEvent`

The single write-audit record, serialized with `serde`, used by both the local audit log
and the incident-timeline export (one format, two consumers):

```rust
struct AuditEvent {
    timestamp: SystemTime,
    actor: String,             // the real user OR the dedicated agent identity
    context: String,           // cluster/context id — never a secret
    operation: Operation,      // e.g. Delete, Scale, GitPrOpened
    target: ResourceRef,       // group/kind/name/namespace
    outcome: Outcome,          // applied / dry-run / rejected
}
```

Audit records **operations, not values** — secrets are never persisted. An agent has its
**own** actor identity (never the operator's), so agent actions are distinguishable in
the log (ADR-0010, ADR-0007).

## The projections

- **`frontend-tui`** (ratatui) — terminal, SSH/bastion. The first daily-driver surface.
- **`frontend-gui`** (egui + wasm) — native desktop, and a browser bundle that relays
  through `serve` (see ADR-0002). Uses `egui_table` for the virtualized `Table` surface.
- **`headless`** — agent mode that drives the view-model directly, **no network
  listener**; used for CI and scripting.
- **`serve`** — the network server (`axum` HTTP/REST + gRPC-Web, `tonic` gRPC for native
  peers); the target for the browser UI and the hub mode (M3.2). Uses one of three
  identity modes (token forwarding, impersonation, dedicated agent identity) per ADR-0007.

### Transport note

Browsers cannot speak raw gRPC. The browser surface is **gRPC-Web (and HTTP/REST)** on
`axum`; `tonic` gRPC is reserved for the native headless↔serve path. Exec/attach and
port-forward are relayed as streams through `serve` (SPDY/WebSocket); this is clarified
in ADR-0002.

## Typed vs. dynamic resources

Two code paths, deliberately separated:

- **Built-in resources** use `k8s-openapi` typed structs (fast, strongly-typed columns).
- **CRDs and list-heavy views** use `DynamicObject` and `PartialObjectMetadata`.

Do not force built-ins through `DynamicObject` (unnecessary `serde_json` thrash), and do
not try to type CRDs at compile time.

## Informer management

Informers are lazy per view, evicted LRU + TTL, default to `PartialObjectMetadata`, use
label/field selectors where scoped, and are subject to a **hard cap** on concurrent
watches with degradation to on-demand list (see ADR-0006).

## Extensibility

Three tiers, one `extension.yaml` manifest, data-first (see ADR-0004):

1. View definitions (lenses) — declarative YAML/CUE, no code.
2. WASM component-model plugins (WIT) — sandboxed (fuel metering, memory cap,
   default-deny network/FS).
3. Shell-out integrations — external binaries, graceful when absent.

The extension *surface* (`ext-sdk`, WIT worlds, view-definition schema, example
extensions) is **MIT/Apache-2.0**, not BUSL — so third parties can build lenses without
taking BUSL terms on their own work.

## GitOps source discovery

The write path locates the owning file by **re-render + match** (GVK + name + namespace),
caches re-render results by source revision, and **degrades honestly** when the source is
ambiguous (generated by Helm/Crossplane/ConfigMap-generator/webhook): it does not open a
PR, it shows *why*. See ADR-0008.

## Storage & the time machine

The watch stream persists to a local embedded store using an append-only, log-structured
layout with compaction + retention TTL (see ADR-0003).

## The governed MCP surface

`kaptein mcp` exposes the view-model as a governed MCP server. Every tool call passes
through the same guardrails as a human (RBAC preflight, context guardrails, read-only
default, break-glass), is impersonated as a real identity via `--as` (ADR-0007), and
lands in the same `AuditEvent` log. An agent **never writes to the API server** — it can
only open a PR (ADR-0010). Kaptein does **not** run agents; it is the governed tool
surface they call.

## Fleet query as a data layer

Fleet query and drift detection share one data layer (ADR-0011). Hub mode considers
**Clusterpedia** as the sync backend rather than reimplementing multi-cluster sync, and
fleet query is a product: saved queries in Git, scheduled reports, and query-as-policy
(a query can fail CI).

## Lens schema acceptance tests

The view-definition schema is designed against the three hardest lenses — **DRA/Kueue/
inference**, **KubeVirt**, and **CNPG** — as acceptance tests, not frozen in Phase 0
(ADR-0012). If one of those cannot be expressed in the schema, the schema is too weak.

## Related ADRs

- ADR-0001 — `egui` over `iced` (GUI framework)
- ADR-0002 — browser relays through `serve` (transport)
- ADR-0003 — time-machine storage layout
- ADR-0004 — three-tier extension model
- ADR-0005 — `RenderIntent` is three layers
- ADR-0006 — informer management
- ADR-0007 — `serve` authentication & impersonation
- ADR-0008 — GitOps source discovery
- ADR-0009 — crate renaming
- ADR-0010 — governed MCP server
- ADR-0011 — fleet query data layer
- ADR-0012 — lens schema acceptance tests
