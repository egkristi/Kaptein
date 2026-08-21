# Architecture

This document is the canonical synthesis of Kaptein's architecture. Individual decisions
are recorded as numbered ADRs under `docs/adr/`; this file ties them together.

## The one rule

**The domain layer is the product.** Layer dependencies are strictly one-directional:

```
kaptein-core ──► kaptein-viewmodel ──► frontend-tui (and future frontends)
```

- `kaptein-core` owns the Kubernetes client (`kube-rs` + `tokio`), watchers/reflectors,
  CRD discovery, and stores. It must not depend on the view-model or any frontend.
- `kaptein-viewmodel` owns all logic: columns, sorting, filtering, status inference,
  permission decisions, action graphs, the render contract, diagnostics, and the audit
  record.
- Frontends render, never compute — they consume a **render-intent** produced by the
  view-model.

### Crate layout (current, four crates)

Only the crates that carry real structure exist now; the rest are split out when they
have code (splitting a module into a crate is an afternoon; holding nine synchronized
crates through Phase 1 is weekly friction):

```
crates/
  kaptein-core/       # kube-rs client, watchers/reflectors, CRD discovery, stores
  kaptein-viewmodel/  # renderer-agnostic logic (the product)
  frontend-tui/       # ratatui
  # future (split out when they have code): frontend-gui, serve, headless,
  # viewdef, plugins, ext-sdk
```

### Error placement

Errors are split deliberately:

- **`kaptein-core::Error`** — the raw type: network, auth, watch interruption, discovery.
- **`kaptein-viewmodel::Error`** — the user-facing, redaction-aware type. It is **wasm-pure**
  and does **not** depend on `kaptein-core` (the browser UI consumes the view-model, and
  `kaptein-core` pulls `kube` → `hyper` → `mio`, all non-wasm).
- The **core → view-model error mapping** lives at the *integration layer* — the native
  frontend or binary that owns both crates — not inside the view-model. The core reports
  *what failed*; the integration layer decides *how to say it* without leaking secrets.

### The one workspace repo

Team configuration lives in **one Git repo**, not four loose surfaces: keymap, lenses,
saved fleet queries, view layouts, and guardrail policy in a single folder with one
schema, reviewed in PRs like everything else. This is both a simplification and a
feature — *"your team's Kaptein setup is a Git repo"* — which gives onboarding for free
and fits the GitOps thesis.

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
default, break-glass), runs under a **dedicated agent identity** (ADR-0007), and lands
in the same `AuditEvent` log. An agent **never writes to the API server** — it can only
open a PR (ADR-0010). Kaptein does **not** run agents; it is the governed tool surface
they call.

The tool taxonomy (ADR-0013) splits into **primitives** (`list_resources`, `describe`,
`get_logs`, `get_events` — commodity) and **diagnostics** (`explain_pod_failure`,
`what_changed_between`, `blast_radius`, `why_is_job_pending` — the moat). Diagnostics are
backed by the rule engine below.

## Semantic equivalence & the support matrix

Not all surface kinds project identically across frontends, for structural reasons:
`Terminal` is a PTY pass-through in the TUI but a full VT emulator in a GUI; `Editor` is
`$EDITOR` handoff in the TUI but a real editor in the browser (no `$EDITOR` exists);
`Graph` is force-directed + mouse in a GUI but a keyboard-navigated `Tree` projection in
the TUI; `Stream` needs backpressure handling over gRPC-Web. The DoD is therefore
**semantic equivalence where the surface kind allows it**, encoded in the
`SurfaceSupport` matrix (`Projection` × `SurfaceKind` → `SupportLevel::Full | Alternate |
None`), not universal parity. Contract tests assert against the matrix.

## Diagnostics subsystem

"why isn't this pod ready", sanity scan, blast radius, and "why isn't this job admitted"
are one engine, not four features: rules over live state, events, and history that
produce an evidence chain. It lives in `kaptein-diagnostics` (a module in the view-model,
split out when it grows), and its **rule packs are lenses** — so every new lens
contributes diagnostics, and the engine is exactly what the MCP diagnostic tools call
(ADR-0013).

## Audit sink

The local audit log is not an audit trail for a reviewer. An optional **audit sink**
(syslog, OTLP, or webhook), configured per context with local buffering during downtime,
forwards events (ADR: `AuditSink`/`AuditConfig`). This is the hook for enterprise
adoption and the CRA/NIS2/DORA mapping in M3.4.

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
- ADR-0013 — MCP tool taxonomy
- ADR-0014 — collapse to four crates
