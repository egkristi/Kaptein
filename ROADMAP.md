# Kaptein Roadmap

The phasing follows one rule: **ship something useful alone every phase.** No phase
depends on a later phase's UI, because every phase lands on the shared view-model.

```
Phase 0 ──► Phase 1 ──────────────► Phase 2 ──────────────► Phase 3 ──────────►
scaffold   core + viewmodel       GUI, view defs,          time machine, fleet,
           + ratatui (k9s parity) GitOps, dry-run diff     cost, security scan
           + RBAC preflight
```

---

## Phase 0 — Foundations (scaffolding)

**Goal:** a compiling workspace with the layer boundaries enforced from day one.

**Status: done.** ADR-0014 collapsed the Phase 0 crate list from nine to four crates
(`kaptein-core`, `kaptein-viewmodel`, `kaptein-tui`, `kaptein-cli`), later joined by
`kaptein-integration` as the native integration layer. The remaining items below are
kept for history; the crates that "have code" now are exactly those five.

- Cargo workspace under `crates/`: `kaptein-core`, `kaptein-viewmodel`, `kaptein-tui`,
  `kaptein-cli`, `kaptein-integration` (the `frontend-gui`, `headless`, `serve`,
  `plugins`, `viewdef`, `ext-sdk` crates are split out only when they carry real code —
  see ADR-0014)
- `kaptein-core` skeleton: `kube-rs` + `tokio` client bootstrap, watcher/reflector store
  trait, CRD discovery (`DynamicObject`), protobuf content-type flag,
  `PartialObjectMetadata` for list-heavy views
- `kaptein-viewmodel` skeleton: three-layer render contract (data plane, semantic layer,
  surface kinds) + `AuditEvent` type — *no* rendering (see ADR-0005)
- Config & error foundation: define the config-file schema (XDG path + TOML) and the
  unified `Error` enum in `kaptein-viewmodel`, so every later milestone builds on them
- Repo hygiene: `CONTRIBUTING.md`, `SECURITY.md`, CLA, and an ADR process (`docs/adr/`)
- CI: `cargo fmt`, `clippy`, `test`, `cargo deny check licenses`, signed release + SBOM
  pipeline stub
- Definition of Done: `cargo build --workspace` is green; layer deps are one-directional
  (`kaptein-tui` → `kaptein-integration` → `kaptein-core`, with no frontend depending on
  `kaptein-core` directly); `cargo deny check licenses` passes on the skeleton

## Phase 1 — Core + viewmodel + ratatui (k9s parity + RBAC preflight)

**Goal:** a TUI that is *already useful alone* as a k9s replacement, in ~3–4 months.

Milestones:

- **M1.1 Auth & context**
  - `kubeconfig` load + exec credential plugins (AKS/Entra, EKS, GKE)
  - **RBAC preflight**: `SelfSubjectRulesReview` on context switch → grey out
    disallowed actions
  - **Context guardrails**: prod-context red frame, read-only default, "break glass"
    confirmation; configurable per regex on context name
  - *Deferred to 1b:* SPIFFE, OIDC device flow, SA tokens
  - **Fixed (v0.30.0, second pass) — the guardrail hole at the highest-privilege point is
    closed.** `kaptein exec` now takes `--confirm` (required — exec has no dry-run) and
    `--break-glass`, gates through `gate_write`, and emits an `Operation::Exec` audit
    event, matching `debug`. `debug` now emits a distinct `Operation::EphemeralAttach`
    (finding V), and opening/removing a `port-forward` writes `Operation::PortForward`
    (finding W). `ISSUES.md` findings U, V, W are resolved.
  - **DoD (falsifiable):** a *coverage test* asserts that **every** CLI subcommand that can
    mutate the cluster or open a channel into it takes `--confirm` + `--break-glass`, calls
    `gate_write`, and emits a distinct `Operation`. Enumerating that set by hand is what let
    `exec` slip; the test must derive it rather than restate it — otherwise the next
    mutating command added will slip the same way. *Landed: `every_confirming_subcommand_also_declares_break_glass`
    derives the governed set from the clap command tree, and a second test asserts
    `Exec`/`EphemeralAttach`/`PortForward` are distinct governed operations.*
- **M1.2 Resource navigation**
  - Command palette + vim keymap + fuzzy jump
  - Built-in resources + all CRDs auto-discovered
  - Describe, scale, restart rollout, cordon/drain, evict, cascade delete
  - Multi-pod/multi-container log streaming: regex filter, JSON → columns, time windows
  - Exec/attach, ephemeral containers, node debug pods
  - Port-forward manager (named, persistent, auto-reconnect)
  - Krew shell-out
  - **Fixed (v0.30.0 →) — dynamic shell completion can hang the shell.**
    `completion.rs` states its own contract as "completion degrades to 'no candidates' —
    never a panic or a **hang**", but each completer called `rt.block_on(...)` around a
    kube query with no `tokio::time::timeout`. Against an endpoint that *drops* rather
    than refuses (a firewalled or stale kubeconfig cluster — common on a laptop that
    moved networks), tab-completion blocked for the client's full timeout. Each
    cluster-querying completer is now wrapped in a 300 ms `tokio::time::timeout`, returning
    `[]` on elapse (`ISSUES.md` finding R).
- **M1.3 Edit & apply**
  - `$EDITOR` handoff for edits; server-side dry-run + diff before apply
  - *OpenAPI/CRD schema validation lands in Phase 2 with the lens engine*
- **M1.4 Recent activity (cheap "what changed")**
  - In-memory ring buffer of the watch stream + the events API — **no persistence**
  - "What changed in the last 15 minutes" without the time-machine storage subsystem
  - Validates the differentiator's behavior a year before the redb layer exists
- **M1.5 Landing view**
  - One screen answering **two** of the three operator questions: is anything broken,
    and what changed recently (from the M1.4 ring buffer + events)
  - The k9s Pulses / OpenShift cluster-overview equivalent — the screen that decides
    whether anyone leaves the tool open
  - *The third question — "what is about to break" (certificates, quota, capacity) —
    needs subsystems from M3a/M3b and lands there, not in Phase 1.*
  - *Extended (v0.30.1 →): the overview now also surfaces **misconfigured** pods —
    `diagnostics::missing_resources` flags containers with no CPU/memory requests or
    limits (ADR-0015), the shipped consumer of the M1.6 `no_requests`/`no_limits` rule
    (read-only, no metrics).*
- **M1.6 Minimal diagnostics rule engine**
  - One rule engine, **one rule pack**: "why isn't this pod ready" over events,
    scheduler reasons, probes, and PVC binding
  - This single pack feeds **three consumers at once**: the landing view (M1.5), the
    TUI diagnostics, and the MCP moat tool in 1b — validating the engine's shape before
    3a builds out the rest (resolves the ADR-0013 vs Phase 1b contradiction)
  - **A canned pod-status fixture corpus** — JSON fixtures for the common failure shapes
    (CrashLoopBackOff, exit-0 Job, ImagePullBackOff, unschedulable, probe failure) with
    expected findings, so the engine is regression-tested as packs grow (see the review).
  - *Landed (v0.30.0 →): **init-container diagnostics.** `diagnose` now inspects
    `init_container_statuses` (previously only `container_statuses`) and surfaces
    `init_container_error` (terminated non-zero) / `init_container_waiting` (waiting with a
    reason like `ImagePullBackOff`/`CrashLoopBackOff`) — checked **before** the scheduling
    reasons in the `Pending` branch, so an `Init:Error`/`PodInitializing` pod reads as
    "init container X failed", not "pending" or "not ready". A `init_container_error.json`
    fixture + unit tests pin it.*
  - **Added: *detecting* missing requests/limits is a Phase 1 rule** (ADR-0015). `README.md`
    §4 reads as one feature, but it is two with very different costs: **detecting that a
    container declares no requests/limits needs no metrics at all** — it is a pure
    `PodSpec` predicate, the same shape as every rule already in the pack, and it ships
    here. **Recommending a *value*** needs VPA or Prometheus and is M3b.1. Splitting them
    lets the cheap half — the one that catches the actual common misconfiguration — ship
    two phases earlier. Rule codes: `no_requests` / `no_limits`, with a fixture each.
  - *Landed (v0.30.1 →): `diagnostics::missing_resources` is the detection half of
    ADR-0015 — a pure `PodSpec` predicate over app + init containers that emits
    `no_requests` (no `resources.requests.cpu`/`memory`) and `no_limits` (no
    `resources.limits.cpu`/`memory`). It is a separate entry point from `diagnose`
    (which answers "why isn't this pod ready", not "is this pod misconfigured"), with
    four unit tests and two fixture-corpus tests (`no_requests.json`/`no_limits.json`)
    pinning it. **Recommending a value** remains M3b.1 (ADR-0015: render, don't
    compute).* *(v0.30.1 →: the **shipped path** is also pinned by a live test —
    `overview_flags_pods_missing_requests_and_limits` runs `overview_with_health`
    against a real API server and asserts a bare pod is flagged while a provisioned one
    is not.)*
- **M1.7 Secret masking & redaction — *blocking*** *(elevated from M3b.2 per review)*
  - A single `kaptein-core` redaction choke point through which **every** serialized
    resource passes before reaching a frontend, the MCP `describe` tool, or an audit log
  - Kubernetes `Secret` `data`/`stringData` are masked; sensitive-named fields
    (password/token/key/credential/…) are masked anywhere they appear
  - `Cell::Redacted` is actually constructed (not just defined), and `Operation::SecretViewed`
    is emitted when an operator unmasks a secret
  - **DoD (falsifiable):** `kaptein describe --gvk v1/Secret` and the MCP `describe` tool
    never emit a plaintext secret value — a test proves it. **Redaction covers
    `metadata.annotations` (incl. `last-applied-configuration`) and log streams** — a test
    proves a `kubectl apply`-created Secret does not leak via its annotation, and the
    `logs` path is redaction-aware. The `Cell::Redacted` / `SecretViewed` bullets stay
    **open** until they have a real unmask path, not an implicit "done".
  - *Landed: the resource path. `redact::redact_object` masks `Secret` `data`/`stringData`,
    sensitive-named fields anywhere in the object, and a Secret's `metadata.annotations`
    (closing the `last-applied-configuration` leak); `describe::RedactionPolicy` gives
    `kaptein edit` an explicit `Unredacted` path with a `SecretViewed` audit event.
    **`Cell::Redacted` is now actually constructed** (v0.30.0 →): `render_row` recognizes
    the `[REDACTED]` marker `kaptein-core::redact` produces and emits the *typed*
    `Cell::Redacted` variant, and `cell_text` renders it as the mask — the frontend renders
    a mask with no special-case string comparison. Only the unmask-in-place affordance
    remains open (deliberately).*
  - **Resolved (v0.27.0 re-audit):** log redaction landed — `redact::redact_line` masks
    `key=value`/`key: value`/JSON/`Authorization: Bearer` shapes for sensitive keys and is
    applied in `pod_logs`/`multi_pod_logs`/`follow_logs` (the MCP `logs` tool routes through
    `pod_logs`) — issue #22. Secret annotation redaction was narrowed to
    `last-applied-configuration`, Helm release-values/`.values`, and sensitive-named keys,
    preserving `meta.helm.sh/*`/Argo CD metadata — issue #29.
  - **Fixed (v0.30.0 →) — `redact_line` recompiled its regexes on every line.**
    `regex::Regex::new` was called *inside* the per-line function, including the ~20-branch
    `SENSITIVE_KEY_ALT` alternation, and `redact_line` runs per line in `pod_logs`,
    `multi_pod_logs`, and `follow_logs` — the streaming path. Both patterns are now
    hoisted into `static … : LazyLock<Regex>` (MSRV 1.97), so a follow stream no longer
    spends its time compiling (`ISSUES.md` finding P).
  - **Open (re-audit v0.32.0) — *blocking*: on the lens path the "single choke point" is a
    convention, and it has now leaked twice** (`ISSUES.md` finding AC). `render_row`,
    `evaluate_status`, and `evaluate_health` each take a bare `&serde_json::Value` and
    **trust the caller** to have redacted it. Nothing in the type system and no test
    enforces that. Two plaintext-Secret leaks have shipped on exactly this pattern —
    **#35** (the TUI's `map_object_with`) and **commit 6692b10** (the CLI `get --lens`
    path, independently, months later) — both found by audit rather than by tests or
    types, and both fixed **pointwise**. The three cluster-facing paths now each redact by
    a *different* mechanism (`map_object_with`, an inline `redact_object` call, and
    `get_dynamic_redacted`), which is three chances to forget rather than one place that
    cannot be forgotten.
    - `kaptein-core::describe` is the counter-example that proves this is solvable:
      `describe_dynamic` redacts by default and the opt-out is a separate, explicitly-named
      `describe_dynamic_policy` used only by the audited `edit` path. Nothing outside
      `describe.rs` reaches the unredacted variant. That shape works; the lens path never
      got it.
    - **Fix: make the guarantee unrepresentable to violate.** Introduce a `Redacted`
      newtype constructible only by the redactor and take it in `render_row` /
      `evaluate_status` / `evaluate_health`; a bare `Value` then does not compile. The
      lens-*authoring* path (`viewdef render`, which renders a user-supplied file and has
      no cluster secret to leak) opts out through a visibly-named constructor, so the
      exemption is greppable rather than implicit.
    - **DoD (falsifiable):** it is a **compile error** to render a lens against an
      unredacted object. That is the assertion; a third pointwise fix is not.
    - This is the "derive, don't restate" lesson that fixed the exec guardrail coverage
      test, applied to types instead of tests — the guarantee should come from the
      signature, not from every future caller remembering.
    - *Fixed (v0.32.0 →): `render_row`/`evaluate_status`/`evaluate_health` now take
      `&Redacted` — a newtype wrapping `serde_json::Value`, constructible only via
      `Redacted::from_redacted` (the cluster paths, after `redact_object`) or the
      deliberately-greppable `Redacted::from_unredacted_for_lens_authoring` (used only by
      `viewdef render`). A bare `Value` no longer compiles, so the DoD — "it is a compile
      error to render against an unredacted object" — holds by construction. The three
      cluster-facing paths now each *produce* a `Redacted` and are indistinguishable in
      the type system; the opt-out is a single greppable constructor, not three chances to
      forget.*
- **M1.8 kwok performance harness** *(elevated per review — the numbers must be measured,
  not aspirational)*
  - A kwok-based synthetic cluster (thousands of fake nodes/pods) drives the
    cross-cutting performance budget; CI runs the benches and fails on regression
  - Owns the p99 <16 ms, RSS <250 MB, cold-start <500 ms targets *in Phase 1*, while the
    design can still change to meet them
  - **Known hot spot to fix before the harness can pass (re-audit v0.27.0):** the TUI's
    per-frame *rendering* is now windowed, but `kaptein-tui::query_plane` still issues
    `Query { start: 0, end: 50_000 }` on every loop iteration (~10 Hz) and
    `MemPlane::query` deep-clones the entire row `Vec` and sorts it before windowing. The
    clone-and-sort, not the allocation, is what the p99 budget will trip over. The TUI
    needs `page.total` for `rows.len()`/`G` navigation, so the fix is to query the visible
    window and carry `total` separately (`ISSUES.md` finding I, issue #28).
    - *Landed (v0.28.2 →): the sort's per-comparison cost was the dominant allocator —
      `cmp_cells` cloned two `String`s per comparison via its `cell_text` fallback
      (~1.7M allocations per 50k-row sort). Text-vs-Text and Status-vs-Status now compare
      `&str` (identical ordering, zero allocation), with a test pinning the ordering.
      **`MemPlane::query` no longer deep-clones the whole row set:** it sorts/filters an
      index permutation (`sort_indices`/`filter_indices`, same semantics as
      `sort_rows`/`filter_rows`) and clones only the windowed rows — a 50k-row view now
      clones the visible window, not 50k `Row`s per frame. **The TUI now carries `total`
      separately** (`query_plane` returns `(rows, total)`), so the "N rows" status line and
      the `G`/`j` navigation use the true post-filter count, decoupled from the
      materialized window. Still open: querying *only* the visible window (a deeper nav
      refactor), and the kwok harness that measures the p99 budget.*
    - *Landed (v0.30.1 →): the **visible-window query** (finding Q's remaining half).
      `query_plane` now takes `start`/`end` and materializes only `[scroll,
      scroll+page_height)` — a busy cluster advancing the revision per watch delta
      re-materializes a few dozen rows, not 50 000. `selected`/`scroll` are kept valid by
      a pure, unit-tested `clamp_viewport`; navigation (`j`/`k`/`g`/`G`/sort) re-queries
      the window; fuzzy-jump snapshots the full set once on `/` and re-windows to the
      chosen row on `Enter`. The kwok synthetic-cluster harness remains the frontend-level
      Phase 1 tail.*
    - *Landed (v0.29.0 →): the **view-model half of the budget is now measured, not
      aspirational**. `crates/kaptein-viewmodel/benches/query.rs` is a dependency-free,
      release-mode benchmark that drives `MemPlane::query` (sort + filter + window) over a
      50 000-row synthetic plane and reports p50/p99/max over 200 iterations, exiting
      non-zero if p99 exceeds an 8 ms budget (half the 16 ms keystroke-to-frame target,
      of which query is the dominant part). A `bench` job in `ci.yml` runs it and fails on
      regression. *(v0.30.0 →: the bench also measures **steady-state RSS** — it holds the
      50k-row plane and reads `VmRSS` from `/proc/self/status`, gating it against the
      250 MB target (measured ~16 MB on Linux; skipped on non-Linux where the p99 latency
      gate still applies).)* *(v0.30.1 →: the bench now also measures **cold start** — it
      builds a fresh plane, seeds all 50k rows, and answers the first query, gating the
      whole at the roadmap's 500 ms (measured ~24 ms; the kube `list` that fills a real
      plane is network-bound and remains the kwok harness's job). The **kwok**
      synthetic-cluster harness (thousands of fake nodes/pods) and the end-to-end
      frontend keystroke-to-frame number remain the frontend-level Phase 1 tail — this
      bench gates the three numbers the view-model owns in isolation.*
    - *Landed (v0.32.0 →): the benchmark is now a **recorded, comparable** suite, not a
      one-shot gate. A second, dependency-free `crates/kaptein-core/benches/core_paths.rs`
      gates the **Kubernetes-side hot paths** (informer-store watch-delta apply, watchring
      reduce+push, and `redact_object`) over 10 000 synthetic events with their own budgets
      (p99 ns/µs); both benches emit machine-readable JSON (`schema: kaptein-benchmark/v1`)
      to `$KAPTEIN_BENCH_OUT`; a `benchmarks/` directory (README + `schema.json`) documents
      the result contract; and `scripts/bench-record.sh` runs both suites, stores the merged
      result under `benchmarks/results/<sha>-<ts>.json` (git-ignored), and prints a
      line-by-line diff against the previous run — so performance is comparable across
      commits/releases, not just gated. The CI `bench` job now runs both suites and fails
      on regression. The kwok harness + end-to-end keystroke-to-frame number remain the
      frontend-level tail.*
    - **Open (external strategy review, 2026-09) — the benchmark is absolute; the *claim*
      is comparative.** `README.md`'s comparison table promises "the same speed [as k9s]
      *plus* deep diagnostics", and that is the load-bearing claim for terminal users —
      but every number the suite records is Kaptein-against-itself. The reviewer is right
      that this is the one place a skeptic can dismiss the project wholesale, and the fix
      is cheap now that the kwok harness is the remaining piece anyway: run **k9s and
      Kaptein against the same kwok cluster** at 1 k / 10 k / 50 k pods and record startup,
      keystroke-to-render, RSS, and API request count for both, in-repo and reproducible.
      Either it substantiates the table's first row, or it tells us to change the row —
      both outcomes are worth more than the current silence. Fold this into the kwok
      harness rather than tracking it separately.
  - **Open (re-audit v0.31.0) — the allocation pattern this milestone removed came back on
    the search path** (finding AA). Windowing closed finding Q for the steady-state table,
    but fuzzy-jump did not follow. Entering `/` correctly snapshots the full set (search
    must span the store, not the window), and then **every keystroke and every backspace**
    runs `fuzzy_rerank(jump_master.clone(), q)`. `TableRow` is `{ String, String,
    Vec<String> }`, so on a 50 000-row view that clone alone is ~150 k `String`
    allocations; `fuzzy_jump` then returns `FuzzyMatch { candidate: String }` per match —
    and an empty query matches everything — for another ~50 k, plus a 50 k-entry `HashMap`
    and a sort. A ten-character query costs roughly two million allocations.
    **The bench does not see any of this:** it gates `MemPlane::query` only, so the guard
    is blind exactly where interactive latency now lives. Take `&[TableRow]` and return
    indices (or `Vec<&TableRow>`), and **add a fuzzy-rerank case to `benches/query.rs`** so
    keystroke-to-frame is gated on the search path too — otherwise the same regression can
    land again without the gate noticing, which is what just happened.
    *Fixed (v0.31.0 →): `fuzzy_rerank` takes `&[TableRow]` and returns `Vec<usize>`; jump
    mode renders from `jump_master`+`jump_order` (no per-keystroke clone);
    `fuzzy_rank_indices` + an allocation-free `fuzzy_score` removed the per-candidate
    `String`/`Vec<char>`; and `benches/query.rs` gates a fuzzy re-rank (4 ms vs 11 ms).*
- Definition of Done: a daily-driver TUI over SSH with k9s parity, RBAC preflight,
  guardrails, and **masked secrets**. Read-only default for unknown contexts.

  **k9s-parity checklist (all must be true):**
  - list pods / deployments / services / nodes; column sort and filter
  - context switching and namespace switching
  - logs with follow + regex filter; describe
  - exec into a pod; scale a deployment; delete with cascade selection
  - port-forward; YAML view of any resource
  - RBAC-preflight-greyed actions and prod-context "break glass" gate

## Phase 1b — Governed MCP surface (read-only)

**Goal:** a distributable `kaptein mcp` artifact in month 4–5, before k9s parity is
complete. It is the one differentiator with a limited shelf life (agent-governance is
hot now, commoditized by ~2028) and it is cheap: it needs `kaptein-core`, the semantic
layer, RBAC preflight, context guardrails, and `AuditEvent` — all of which land in Phase
0 and Phase 1 — and needs **none** of the GUI, lens engine, viewdef schema, WIT/WASM,
GitOps write path, time machine, or fleet.

- **M1b.1** `kaptein mcp` as a read-only MCP server: every tool call through the same
  guardrails (RBAC preflight, context guardrails, read-only default, break-glass), each
  agent under a **dedicated identity** (ADR-0007, ADR-0010), landed in the audit log.
- **M1b.2** Auth: OIDC token forwarding and dedicated agent ServiceAccounts (no
  `impersonate` dependency).
- **M1b.3** Tool taxonomy (ADR-0013): primitives (`list_resources`, `describe`,
  `get_logs`, `get_events`) plus the diagnostic moat (`explain_pod_failure`,
  `what_changed_between`, `blast_radius`, `why_is_job_pending`), backed by the M1.6
  rule engine (the one pack ships in Phase 1; more packs in 3a).
  *Landed (v0.30.0 →): the moat is **not MCP-only** — `kaptein blast-radius` and
  `kaptein why-job-pending` expose two of the four moat tools as first-class CLI
  commands, reusing `kaptein-core::moat` (the same engine the MCP tools call), so the
  moat is one implementation surfaced two ways.*
  - **Open (external strategy review, 2026-09) — *the tool list is hand-maintained, so the
    agent surface can drift from the human one*** (`ISSUES.md` finding AF).
    `mcp.rs::tools()` is a literal `vec![Tool::new(…), …]` with nine hand-written entries
    and **zero** references to `actions_as_semantic`, `semantic::Action`, or any action
    graph — while `surface.rs`'s own doc comment states the contract: *"Headless and MCP
    are consumers of the **semantic layer** (they read the data plane and action graph…)"*.
    Today, adding a view-model action does not expose it to agents and removing one does
    not retract it; the two surfaces are kept in sync by memory.
    - This is the **third** hand-maintained set this project has kept in prose: the
      guarded-command set (finding U — drifted, `exec` was missing), lens redaction
      (finding AC — drifted twice), and now the agent tool surface. The first two both
      drifted before anyone noticed. There is no reason to expect the third to behave
      differently.
    - **Fix:** derive the tool list from the action graph, or assert equivalence in a test
      that fails when a view-model action has no corresponding tool (and vice versa).
    - The payoff is a feature, not just hygiene: **a new view-model action becomes
      agent-callable for free** — which is the "domain layer is the product" thesis
      actually cashing out, and a better demo of the architecture than any prose.
    - **DoD (falsifiable):** adding an action to the view-model's action graph makes it
      appear in `tools()` with no edit to `mcp.rs`, or a test fails.
- **M1b.4 Governance conformance — *blocking*** *(elevated per review)*
  - Every tool call actually runs **RBAC preflight + context classification + read-only
    guardrail** *before* reaching the API server — not merely documented
  - The audit record is governance-grade: `Outcome::Rejected` is emitted on refusal, the
    `target` carries the real resource (not the tool name), `session_id` is a real
    per-session value (not a constant), and the outcome is recorded **after** execution
    (failed calls are not logged as `Applied`)
  - **DoD (falsifiable):** a tool call the agent's ServiceAccount is not permitted to make
    is refused before it reaches the API server, and the refusal appears in the audit log
    as `Rejected` — with a test that proves it. **The preflight resource and namespace
    are derived from the tool call's own arguments** — a test asserts
    `describe(gvk=v1/Secret, ns=kube-system)` is refused for an agent scoped to pods in
    `default`, and that `describe` of a pod in `default` is allowed.
  - *Landed: `preflight_target` now derives `(verb, resource, group, namespace)` from the
    call's own arguments instead of a hardcoded `pods`/`default`; refusals audit as
    `Rejected` with a real target and per-session id.*
  - **Resolved (v0.27.0 re-audit):** `resource_from_kind` now resolves the plural via
    `ApiResource::from_gvk(&gvk).plural` (kube's own pluralizer) — the same plural the
    request uses — so the gate and the call can no longer disagree, with tests asserting
    `NetworkPolicy` → `networkpolicies`, `PriorityClass` → `priorityclasses`, `GatewayClass`
    → `gatewayclasses` (issue #21).
- **Definition of Done:** someone can add Kaptein as an MCP server and get governed,
  read-only Kubernetes access without opening the TUI — a distribution channel the TUI
  does not have. The M1b.4 DoD holds, not just the happy path.

## Phase 2 — Browser UI + view definitions + GitOps + dry-run diff

**Goal:** the same view-model drives the **browser UI** (via `serve` + wasm), and the
GitOps write path becomes the differentiator. The native desktop GUI is **not** on the
critical path — it is the same egui code packaged later, after 3a validates the product.

> **Sequencing note (external strategy review, 2026-09): ship M2.3 before M2.1 and M2.6.**
> The phase is titled "Browser UI + … + GitOps", and the ordering has so far implied the
> UI comes first. Three arguments say otherwise, and they are independent:
>
> 1. **M2.3 completes the product's own sentence.** The tagline is *"the console that knows
>    what changed — and lets you fix it in Git"*; the second half **is** M2.3. Until it
>    ships, the headline claim is unshipped, and `apply_patch_real` remains a
>    pre-positioned function with no caller (finding Y).
> 2. **M2.3 is what makes the agent surface writable safely.** ADR-0010's whole position is
>    that an agent never writes to the API server — it opens a PR. Without M2.3 the governed
>    MCP surface is permanently read-only, which caps the differentiator's value at
>    "diagnosis" when the demand is for "safe remediation".
> 3. **Both differentiators now have live competition.** The reviewer identified
>    GitOps-diagnosis tooling (`skyhook-io/radar`) and governed-agent tooling (Kubernetes
>    MCP Guard, with human-in-the-loop plan approval and JSONL audit) already shipping.
>    Neither has the shared-guardrail architecture, but shipping order decides who is seen
>    to have solved it. Spending the next phase on a GUI cedes that.
>
> The GUI argument compounds it: the Kubernetes Dashboard is archived and **Headlamp** is
> the SIG-UI successor — CNCF Sandbox, Microsoft-backed, Apache-2.0, with an established
> plugin system. Entering that category late and solo is the worst-odds bet on this
> roadmap, and there is a cheaper play: publishing `kaptein-viewmodel` as a stable,
> semver'd boundary (already published to crates.io) and letting a Headlamp **plugin** be
> the browser surface, rather than building `serve` + wasm from scratch. Worth deciding
> explicitly before M2.1 starts, not during it.
>
> This note does not renumber the milestones — M2.1/M2.6 keep their identifiers. It records
> the recommended execution order and why.

Milestones:

- **M2.0 Wire the render contract + informer store — *blocking*** *(elevated per review:
  two fully-specified ADRs have no implementing code)*
  - `impl DataPlane` (ADR-0005): the `Surface`/`Column`/`Action`/`ActionState`/`Status`
    types are actually constructed outside tests; sorting/filtering move out of
    `discovery::list_with` into the view-model's data plane (the CLI/TUI consume the
    same `Page`/`Row`/`Query`)
  - An informer-backed store (ADR-0006): `discovery::list` becomes bounded
    (`limit` + `continue` token + `PartialObjectMetadata` for list-heavy views); the TUI
    stops re-listing the whole cluster per keystroke
  - **DoD (falsifiable):** the TUI renders from a `DataPlane` subscription, not per-key
    `api.list` calls — and there is at least one `#[tokio::test]` exercising the store.
    **The shipped frontend path uses the bounded/`PartialObjectMetadata` store;
    `run_informer` has a caller outside tests** — close only when both hold.
  - **Status: done.** The render contract + informer store are fully wired.
    `kaptein-viewmodel::mem_plane::MemPlane` is a concrete `DataPlane`;
    `kaptein-viewmodel::table` owns sorting/filtering; `kaptein-core::store::InformerStore`
    + `run_informer` do bounded list-then-watch; `kaptein-core::discovery::list_metadata_bounded`
    is the `PartialObjectMetadata` path; `kaptein-integration::LivePlane` is an
    informer-backed `DataPlane` with a real `subscribe`, and the TUI renders from it
    (a background watch task applies deltas; no per-key `api.list`). A live `#[tokio::test]`
    exercises the store/client against a cluster when `KUBECONFIG` is present.
  - **Resolved (v0.27.0 re-audit):** `LivePlane::seed` now pages through
    `discovery::list_bounded` (full objects, limit 500) instead of the unbounded
    `discovery::list`, so the frontend path is bounded while preserving the status column
    (verified live against the cluster) — issue #27.
  - **Cleanup (re-audit v0.30.0, second pass):** delete `KubernetesPlane`. It has zero
    callers and is a `pub` second `DataPlane` implementation whose semantics *diverge*
    from the supported one — it uses the unbounded `discovery::list` that #27 moved off,
    always returns `Revision(0)` (so staleness detection silently no-ops), and its
    `subscribe` returns `stream::empty()` (so no consumer ever gets a delta). *Fixed:
    deleted* (`ISSUES.md` finding X).
- **M2.0c Watch resilience & informer lifecycle** *(new per re-audit — ADR-0006 is ~30 %
  implemented)*
  - Relist-on-410, reconnect with backoff, `WatchEvent::Error` handling, and bookmark
    handling (partly done: `LivePlane::watch_loop` reconnects; the bounded
    `store::watch_from` does not).
  - The ADR-0006 subjects that have no code: lazy-per-view informers, LRU + TTL eviction,
    and a hard cap on concurrent watches with degradation to on-demand list.
  - *Landed (2026-08-25): `kaptein-core::informer::InformerManager` implements the
    lifecycle policy — lazy per-view `register` (idempotent `touch`), `evict_idle`
    with TTL, and a hard cap that returns `Denied` (degrade-to-on-demand-list) instead of
    exceeding the cap. The policy (`max_watches`, `idle_ttl_secs`) is exposed in the
    config file under `[informer]` (ADR-0006 requires the cap to be a configurable
    policy) and validated by `kaptein config validate`.*
  - **Resolved (v0.27.0 re-audit):** all three gaps are closed — `watch_loop` relists and
    reconciles on every reconnect (no ghost rows, issue #20); `LivePlane` holds a shared
    `Arc<InformerManager>` and registers its watch key, degrading to a one-shot list on
    `Denied` (issue #25); and `register` evicts the least-recently-used entry to admit a
    hot view (issue #26). The TUI builds planes from the `[informer]` config policy.
       Either implement LRU admission (and give `watches` an ordering — it is a `HashMap`
       today, despite the "insertion order" comment), or amend ADR-0006 and the module docs
       to say TTL-only and explain why that is sufficient.
  - **DoD (falsifiable):** a watch that expires mid-session leaves the store *converged* —
    a test kills a watch, deletes an object out-of-band, and asserts the row disappears
    after reconnect; and `InformerManager::live()` is observably bounded by `max_watches`
    while driving the TUI through more distinct views than the cap allows.
  - *Landed (v0.28.x → v0.29.0): all three v0.27.0 gaps closed — `watch_loop` relists and
    reconciles on every reconnect (#20), `LivePlane` registers with an `InformerManager`
    and degrades to a bounded list on `Denied` (#25), and `register` performs LRU
    admission, evicting the least-recently-used entry when the cap is full (#26).*
  - **Re-opened (re-audit v0.30.0) — the second half of the DoD still does not hold.**
    The mechanism is now correct; the *wiring* still never exercises it. See `ISSUES.md`
    findings M, N, O.
    1. **The manager is per plane, so the cap is unreachable.** The `informers` field is
       documented as "shared across clones so the cap is enforced across *all* live planes
       in a process, not per plane (issue #25)" — but `new_with_policy` and
       `new_lens_with_policy` each build `Arc::new(InformerManager::new(policy))`, and the
       TUI's `rebuild_plane` calls `new_plane(...)`, not `clone`. Every kind/namespace
       switch gets a fresh manager holding exactly one watch, so `max_watches` (16) is
       never reached and LRU/TTL/degrade-to-list are dead paths. The DoD's second clause —
       *"`InformerManager::live()` is observably bounded while driving the TUI through more
       distinct views than the cap allows"* — is precisely the assertion that would have
       caught this, and it was never written. **Write that test first.**
       *Fixed: the TUI holds one session-scoped `InformerManager` and passes it to every
       plane via `LivePlane::with_shared_informers`; the DoD test
       `shared_manager_cap_is_enforced_across_planes_and_released_on_close` drives distinct
       planes through a shared manager and asserts the cap is reached.*
    2. **`release` and `touch` have no callers.** `rebuild_plane` aborts the watch task
       without releasing its slot, and nothing refreshes recency, so `last_touched` is
       always registration time and the LRU has no usage signal. Fixing (1) alone converts
       this into a slot leak — after `max_watches` view switches every new view degrades to
       a one-shot list and the TUI silently stops being live. **(1) and (2) are one change**:
       hoist the manager to session scope, release on rebuild (or hold a Drop guard), and
       touch on query.
       *Partly fixed: `watch_loop` holds a `WatchSlotGuard` that releases the slot on exit
       (view close or task abort), so a session-scoped manager no longer leaks a slot per
       view switch — verified by the DoD test.* **Still open — the `touch` half
       (finding Z).** `grep -rn '\.touch('` outside `informer.rs` still returns nothing;
       `last_touched` is only ever written by `register`. **This became live when (1)
       landed:** while the cap was unreachable a missing recency signal was inert, but now
       that eviction actually runs, `register`'s `min_by_key(last_touched)` selects the
       **oldest-registered** view — which for an operator who opens one view and then
       cycles through others is *the view on screen*. The LRU inverts and evicts the
       hottest entry. Hook `informers.touch(&watch_key)` into `LivePlane::query` (the TUI
       already re-queries per revision change, so the hook point costs nothing).
       *Fixed (v0.31.0 →): `LivePlane::query` now calls `informers.touch(&watch_key)`, and
       `lru_evicts_the_coldest_not_the_hottest_view` asserts the hottest view survives.*
    3. **Reconcile removes but never adds.** `relist_and_reconcile` drops rows absent from
       the relist but never upserts rows present in the relist and missing from the plane,
       so objects **created during an outage stay invisible** until they next change — the
       mirror of the ghost rows #20 removed. The relist is metadata-only and therefore
       cannot carry `status`, so this needs a decision (full-object relist vs. metadata
       upsert plus a deferred status fetch), not a patch.
       *Fixed: `relist_and_reconcile` relists **full objects** (not metadata summaries) and
       upserts rows missing from the plane with a correct `status`, so objects created
       during an outage appear immediately (finding O).*
  - **Added to the DoD:** a test drives the TUI through more distinct (kind, namespace)
    views than `max_watches` and asserts `live() <= max_watches` **and** that the
    most-recently-used view still holds a live watch; and a reconnect test asserts an
    object *created* during the outage appears after reconnect, not only that a deleted
    one disappears.
  - **The second DoD clause is still not a test** (finding Z). The shipped
    `shared_manager_cap_is_enforced_across_planes_and_released_on_close` asserts the cap is
    reached and that `release` frees a slot — the *first* clause. It makes no recency
    assertion, so *"the most-recently-used view still holds a live watch"* remains prose,
    and the test passes with the LRU evicting the wrong entry. **This is the third
    consecutive cycle in which a written DoD clause was only partially turned into a
    test.** Close it by asserting the survivor: fill the cap, query view A, register a new
    view, and assert **A** is still live and the coldest one was evicted.
    *Closed (v0.31.0 →): `lru_evicts_the_coldest_not_the_hottest_view` does exactly this —
    it fills the cap, queries view A (touching it via `LivePlane::query`), registers a
    third view, and asserts A survives and the coldest view is evicted.*
  - **Open (external strategy review, 2026-09) — property-test this subsystem**
    (`ISSUES.md` finding AG). The reviewer flagged the ADR-0006 lifecycle as "a subtle
    correctness surface worth fuzzing", and the audit record makes that unusually
    concrete: findings **C** (LRU missing), **M** (cap unreachable), **N**/**Z**
    (`release`/`touch` uncalled) were **all** in this one module, and **every one was found
    by reading code, not by a failing test**. Four bugs, one subsystem, zero caught by the
    nine example-based unit tests that cover it. That is the strongest empirical argument
    for randomised testing anywhere in this codebase.
    - The shape is ideal for it: a small state machine (`register`/`touch`/`release`/
      `evict_idle` over a bounded map) with crisp invariants — `live() <= max_watches`;
      a `release` frees exactly one slot; the most-recently-touched key is never the
      eviction victim; no operation sequence leaks a slot; `register` is idempotent.
    - No new dependency is strictly required (a deterministic seeded sequence generator in
      a `#[test]` would do), though `proptest` as a dev-dependency is the conventional
      choice. `cargo deny` already gates the licence check.
    - **DoD (falsifiable):** a randomised sequence test over ≥10 000 operation sequences
      asserts the invariants above and is wired into `cargo test`.
- **M2.0b Integration-test tier + platform CI matrix** *(elevated per review)*
  - A kind/envtest tier exercising the real kube client, the MCP protocol, the CLI, and
    every write path (scale/delete/restart/cordon/evict/apply/exec/portforward) — none
    are unit-tested today
  - CI runs on Windows and macOS (not just `ubuntu-latest`), plus a conformance check
    against the latest three Kubernetes minors
  - *Landed (2026-08-25): the Windows/macOS test matrix was already in CI; the first
    **live integration-test tier** now exists — `crates/kaptein-core/tests/live.rs`
    exercises the real kube client (list, describe, and the delete dry-run vs. real
    write path) against a cluster, self-cleaning in a throwaway namespace and gated on
    `KAPTEIN_LIVE_TESTS=1` so the default run stays hermetic.*
  - *Extended (v0.30.1 →): the live tier now exercises **eight** paths, up from five —
    adding `restart` (asserts the `restartedAt` annotation lands), `evict` (dry-run +
    real evict against a throwaway standalone pod), and `exec` (runs `echo` in a running
    pod and asserts the output). This closes the "exec/restart/evict not unit-tested
    today" gap the milestone names.*
  - *Extended (v0.31.0 →): `cordon`/`uncordon` are now live-tested too —
    `cordon_marks_node_unschedulable_then_uncordon_restores_it` cordons the throwaway
    kind node (dry-run + real, asserting `unschedulable`) and uncordons to restore it,
    self-cleaning. The earlier "mutates a *real* node" exclusion no longer holds on a
    throwaway kind cluster. This closes the last "every write path" clause — the live
    tier now covers list/describe, delete, scale, apply-dry-run, blast_radius, restart,
    evict, exec, cordon/uncordon, port-forward, and the missing-resources overview.*
  - *CI-wired (v0.30.1 →): a `live` job now runs the tier against a throwaway `kind`
    cluster on every push, so `KAPTEIN_LIVE_TESTS=1` is no longer a locally-only
    opt-in — the shipped-path test actually runs in CI. The job is a **latest-three-
    minors conformance matrix** (v1.37 / v1.36 / v1.35, node images pinned by digest),
    closing the milestone's "conformance check against the latest three Kubernetes
    minors" clause.*
  - **Open (re-audit v0.31.0) — three DoD clauses are still uncovered** (finding AB). The
    tier is now genuinely good and covers the hard part; what remains is the tail the
    milestone text explicitly names:
    - **port-forward** — zero references in `live.rs`, though the DoD lists it among "every
      write path" and it is the operation that most often outlives its session.
    - **the MCP protocol** — zero references. `kaptein mcp` is the Phase 1b differentiator
      and its governance gate (`preflight_target` → `governance_check`) has never been
      exercised against a real API server; a stdio round-trip asserting one allowed call
      and one RBAC-refused call would cover it.
    - **the CLI** — every test drives the *library*. Nothing execs the `kaptein` binary, so
      argument parsing, the `--confirm`/`--break-glass` wiring, and the audit-file write
      are untested end to end — and those are exactly the layers finding U lived in.
    - **cordon/uncordon** are excluded on the grounds that they "mutate a real node". That
      was true when the tier ran against whatever cluster `KUBECONFIG` pointed at; it is no
      longer true now the tier runs on a **throwaway kind cluster**, where cordoning the
      single node is safe and disposable. Either cover them or update the rationale.
    Either close these or narrow the DoD text — marking M2.0b done against a subset is the
    failure mode this milestone exists to prevent.
  - *Resolved (v0.31.0 →) — **port-forward covered; the DoD is narrowed for the other two.**
    `port_forward_binds_and_bridges` now live-tests `core::portforward::forward` against a
    throwaway `nc -l` pod (bound local listener on an ephemeral port). The **MCP protocol**
    and the **CLI binary** are *deliberately* out of scope for `crates/kaptein-core/tests/
    live.rs`: both live in the `kaptein-cli` binary crate (the MCP server is `mcp.rs`, the
    audit-write and `--confirm`/`--break-glass` wiring are `main.rs`), which
    `kaptein-core` cannot depend on without violating the one-directional layer rule. A
    **CLI-level integration tier** (which can `assert_cmd` the built `kaptein` binary and
    drive `mcp` over stdio) is the correct home for those two clauses and is tracked as a
    follow-on, not claimed as done here. The M2.0b DoD text is amended to scope the
    core live tier to the library write paths and name the CLI tier separately.
    *(v0.31.0 →: the **CLI binary** clause is now covered too — `delete_confirm_round_trips_through_the_cli`
    drives the real `run(cli)` dispatch (`--confirm` + `--break-glass` → `gate_write` →
    core delete → audit) against a live cluster and asserts the object is removed; it runs
    in the `live` CI job alongside the core tier. The **MCP protocol** clause is also
    covered — `governance_check_runs_real_preflight_against_a_live_server` drives the MCP
    governance gate (`preflight_target` → `governance_check` → `SelfSubjectRulesReview`)
    against a live cluster, in CI. All three AB gaps are now closed.)*
- **M2.1 Browser UI** — egui → wasm served by `serve`, same keymap; the native desktop
  packaging (code-signing, notarization, installers, auto-update) is deferred until
  after Phase 3a
- **M2.2 Workload lenses as data**: view-definition engine (YAML/CUE) binding CRDs to
  panels/columns/status/actions/health-checks; ship lenses for Strimzi, KubeVirt,
  cert-manager, Keycloak, Tekton, Velero, Karpenter, Knative
  - Ship a versioned JSON Schema (and/or CUE schema) for view definitions plus a
    `kaptein viewdef validate` command so lenses are reviewable in PRs
  - **Status: schema + validation + lifecycle landed, lens set shipped, rendering done.**
    `kaptein-viewmodel::lens` defines the versioned lens data model (`ViewDefinition`,
    `GroupVersionKind`, `StatusRule`/`RuleOp`, `ConditionRule`, `HealthCheck`/`HealthFinding`,
    `LensAction`), `validate_viewdef`, `evaluate_status` (field-path resolution + scalar
    **and Kubernetes-condition** rule evaluation), `evaluate_health` (per-check findings,
    many-at-once — a resource can fail several checks and all surface), and `render_row`
    (maps a lens + a resource into the render contract's `Row` — the status-rule
    *rendering* half, with a data-bound `Column.field` so a column's value source is
    explicit, not implicit, per ADR-0012); `kaptein viewdef validate -f` parses a lens and
    reports problems; `kaptein viewdef schema` emits the JSON Schema; `kaptein viewdef
    render` renders a lens against a live/fixture resource *and* prints its health
    findings; the `extension.yaml` manifest + `kaptein extension
    {list,validate,enable,disable}` lifecycle (ADR-0004) are implemented; the example lens
    set ships under `extensions/` — CNPG, Strimzi Kafka, KubeVirt, cert-manager, Keycloak,
    Tekton, Velero, Karpenter, Knative (all MIT/Apache-2.0). *(v0.31.0 →: per-lens
    **health checks** landed in the data model + both JSON Schemas — the `health` array
    declares predicates (`field`/`op`/`value`) that must hold, each with its own severity,
    and `evaluate_health` returns a `HealthFinding` per failing check; the schema's
    `additionalProperties: false` now also covers `health`, closing the earlier
    "health-checks" doc-vs-code gap.)*
  - **Resolved (v0.27.0 re-audit → lens navigation):** the CLI consumes a lens —
    `kaptein get --gvk <gvk> --lens <file>` lists full objects
    (`core::discovery::list_objects`) and renders each through `render_row` (lens columns +
    lens-inferred status), verified live. **Lens discovery landed:** `kaptein lenses`
    (`core::extension::discover_lenses`) walks configured extension paths, resolves each
    lens entrypoint's `target` GVK, and honours the `enable`/`disable` set. **Lens-driven
    TUI navigation landed:** the TUI discovers lens kinds at startup
    (`KAPTEIN_EXTENSIONS_DIR`, defaulting to `./extensions`) and renders each through a
    `LivePlane::new_lens` — the lens's declared columns become the table schema and its
    status rules drive the status chip, so dropping a lens file into the path makes its
    CRD navigable with **no recompile**. `render_row` is on the TUI's `DataPlane` path
    (`map_object_with` is the single seed/watch mapping). **Lens action graph landed:**
    `ViewDefinition::actions_as_semantic` maps a lens's declared `actions` into the render
    contract's `semantic::Action` (`allowed`/`gated`/`forbidden` → `ActionState`), and the
    TUI surfaces the selected resource's action graph (the lens's action ids, or
    `describe, diagnose` for built-ins) — the action *id* maps to the existing bindings
    (`d` = describe, `i` = diagnose). **Landed (v0.29.0 →): per-action RBAC grey-out.**
    `semantic::action_verb` maps an action id to its RBAC verb (describe/logs/exec/diagnose
    → `get`, scale/restart/delete → `update`/`delete`, unknown → `get`); `downgrade_forbidden`
    turns an action `Forbidden` when preflight denies that verb (carrying the structured
    verb/resource/namespace); `kaptein-integration::preflight_actions` runs one
    `SelfSubjectRulesReview` for the target GVK (pluralized with kube's own pluralizer) and
    downgrades in place — the **shipped path** — and the TUI renders the forbidden marker
    and refuses the `d`/`i` bindings for a greyed-out action. **Still open (Phase 2+):**
    a dedicated per-lens health *panel* (the TUI now surfaces findings via the `h` key,
    M2.2 data model + evaluation; a richer panel is M2.4+), and the browser UI's lens
    navigation (M2.1).
  - **Open (re-audit v0.32.0) — health checks shipped ahead of their documentation and
    ahead of their proof.** Two small gaps, both cheap:
    1. **The user manual never mentions them** (`ISSUES.md` finding AD). `README.md`
       documents health checks and `ROADMAP.md` tracks them, but `docs/USAGE.md` — the
       actual manual — has **zero** references: its TUI keymap table lists `n`, `/`, `:`,
       `d`, `i` and not **`h`**, and §5 (lenses) and §7.4 (*Make this CRD navigable*) are
       silent. A user reading the manual cannot discover the feature. Note the CI
       `version-sync` gate catches *version* drift between docs but cannot catch a
       *feature* landing undocumented — worth considering whether the keymap table should
       be generated from the TUI's own key match, the same derive-don't-restate shape used
       for the guardrail coverage test.
    2. **Only 1 of the 9 shipped lenses declares `health:`** (`lens.cnpg.yaml`) —
       deliberate as a demonstration, but ADR-0012's argument is that the schema is proven
       by the *hardest* lenses, and health predicates are exactly where a schema gets
       stressed (`ISSUES.md` finding AE). Strimzi Kafka readiness across broker/zookeeper
       conditions, cert-manager `Certificate` expiry windows, and KubeVirt
       `VirtualMachine` run/migration state each want shapes CNPG's checks do not
       exercise. Adding health to two or three more of the shipped set is the cheapest
       available test of whether the health schema is expressive enough **before** it is
       versioned as stable.
    *Fixed (v0.32.0 →): `docs/USAGE.md` now documents `h` in the keymap, the `health:`
    block in §5.2, health findings in `viewdef-render` (§5.4), and the health step in §7.4
    (AD); and Strimzi Kafka, cert-manager Certificate, and KubeVirt VirtualMachine now
    declare `health:` blocks — four of the nine shipped lenses (AE).*
  - **DoD (falsifiable):** dropping a new lens file into an extension path makes its CRD
    navigable in the TUI with its declared columns and status, **with no recompile** — and
    a test asserts a lens-declared column reaches a `Row` through the data plane, not only
    through `viewdef render`. *(Satisfied: `lens_column_reaches_row_through_data_plane` in
    `kaptein-integration` asserts a lens column reaches a `Row` via `map_object_with`, the
    live seed/watch path.)*
- **M2.3 GitOps (the differentiator)**
  - Flux + Argo CD first-class: sources, reconciliation status, suspend/resume, force
    reconcile
  - **Git write path**: edit in UI → locate owning file/repo → branch → PR, with diff at
    manifest *and* rendered level (`kustomize build` / `helm template`)
  - Drift detector (live vs. rendered Git state)
  - Helm releases: values diff + rollback
  - Deprecated-API scanning before upgrade
  - Crossplane XRD/claims + composition trace; OLM subscriptions + upgrade channels
- **M2.4 Topology & diff**
  - ownerRef/selector/volume/RBAC resource graph, keyboard-navigable
  - Diff between two namespaces / clusters / points in time
  - **Application-centric view** (the "app as the unit" navigation): a Deployment and
    its ReplicaSets, Pods, Service, Ingress/HTTPRoute, ConfigMaps, Secrets, PVCs, HPA,
    PDB, ServiceAccount, NetworkPolicy — one screen. Uses the `Tree`/`Graph` surface
    kinds as the *default* navigation, not a feature you go to. *(Decide now whether the
    app view, not the resource list, is the primary navigation — it is expensive to
    change in Phase 3.)*
- **M2.5 Network & storage v1**
  - Gateway API + Ingress side by side; DNS/endpoint debugging
  - PV/PVC/StorageClass/Snapshot, CSI status, Velero/VolSync overview
  - CNPG lens: primary/replica topology, lag, switchover/failover, PITR window
- **M2.6 Extension system**
  - WASM component-model host + versioned WIT interfaces; plugin sandbox (fuel metering,
    memory cap, default-deny network/FS)
  - Extension manifest loader + discovery from Git-backed paths; `kaptein extension`
    lifecycle subcommands (validate, list, enable, disable)
    *Manifest loader + discovery + full lifecycle (`list`/`validate`/`enable`/`disable`)
    are implemented (`kaptein-core::extension` + `kaptein-core::config::Extensions`).
    The WASM host + WIT worlds (tier 2) remain, gated on real lenses existing first.*
  - `ext-sdk/` authoring crate (manifest types, WIT worlds, host imports) with a
    versioning + deprecation policy so plugins don't break across releases
  - First example extensions: a lens, a WASM plugin, and a shell-out integration

  *The WIT worlds are defined here, late in Phase 2, after real lenses exist — not in
  Phase 0 (see ADR-0004).*
- **M2.7 Governed MCP surface (`kaptein mcp`)** — *extends the read-only Phase 1b
  surface with the PR write path*: every tool call passes through the same guardrails,
  each agent under a dedicated identity (ADR-0007), and landed in the same audit log;
  an agent only opens PRs (ADR-0010).
- Definition of Done: the browser UI and TUI are **semantically equivalent where the
  surface kind allows it** — proven by contract tests asserting against the
  `SurfaceSupport` matrix, not universal feature-parity (`Terminal`, `Editor`, `Graph`,
  and `Stream` differ structurally per projection); a GitOps change can be authored and
  opened as a PR entirely from the browser UI; `kaptein mcp` exposes the governed agent
  surface with the PR write path.

## Phase 3a — Differentiators & relevance

**Goal:** ship the differentiators and the lenses that decide whether Kaptein is
relevant. This is the phase that "ships something useful alone"; Phase 3b is gated on it.

Milestones:

- **M3a.1 Time machine**
  - Persist watch stream locally (redb/SQLite, optionally centralized) with compaction
    and a configurable retention TTL
  - Scrub backwards, diff two timestamps, "what changed between 14:20 and 14:35"
  - Events + Git deploy markers on one timeline
  - **Boundary: this store holds resource *state*, never usage *metrics*** (ADR-0015).
    Once redb is in the codebase the reasoning *"we already have a store — just add usage
    samples"* becomes available, and it is wrong. The time machine writes are
    low-frequency and event-driven (one per object change, bounded by the watch stream);
    per-container usage samples arrive at metrics cadence across the whole fleet, which is
    a different write volume and retention profile by orders of magnitude. Taking that
    step turns Kaptein into a time-series database and breaches the "no metrics/log store"
    non-goal. **Guard it:** the store's key space is `(resource identity, revision/time)`
    per ADR-0003 — a schema that cannot express a metric sample is the cheapest possible
    enforcement, so keep it that way rather than generalizing the key.
  - *The one legitimate use of this store for rightsizing is the opposite direction:
    M3b.1 asks the time machine "was this workload redeployed since the recommendation's
    samples were taken?" — a state query, and the differentiator no other rightsizing tool
    can answer.*
- **M3a.2 Fleet**
  - Fleet query (one query, all clusters); cross-cluster diff + drift matrix
  - Saved queries in Git; scheduled reports; **query-as-policy** (fail CI on rows)
  - Clusterpedia-class data layer for hub mode (ADR-0011)
  - Optional hub mode with per-cluster agent (headless/serve)
- **M3a.3 Workload lenses (DRA / KubeVirt / CNPG)**
  - DRA-native views: `ResourceSlice`/`ResourceClaim`/`DeviceClass`; "why isn't this job
    admitted?" over ClusterQueue quota, gang scheduling, preemption; InferencePool
  - KubeVirt lens: console, live migration, snapshots, MTV plans, instance types, hotplug
  - CNPG lens: topology, replication lag, switchover/failover, PITR, WAL status
  - These three are the **acceptance tests** for the view-definition schema (ADR-0012)
- **M3a.4 Diagnostics subsystem (`kaptein-diagnostics`)** — *extends the M1.6 engine*
  - One rule engine over live state, events, and history — not four separate features
  - Rule packs **are lenses**: every lens contributes diagnostics, not just views
  - Backs the MCP diagnostic tools (ADR-0013), the landing view, and the TUI
- **M3a.5 Native desktop packaging (post-validation)**
  - Same egui code as the browser UI; code-signing, notarization, installers,
    auto-update — done only after 3a proves the product is worth packaging
- Definition of Done: the five differentiators (governed MCP, GitOps write path, time
  machine, fleet query + drift) are functional and cross-frontend, and the three lenses
  are usable on real clusters.

## Phase 3b — Analytics surface (conditional)

**Goal:** the scanning/analytics and lifecycle surface. **Gated on Phase 3a finding
users** — an explicit stopping point, not a failure.

Milestones:

- **M3b.1 Cost & capacity**
  - Allocation per namespace/label/team/workload (showback + chargeback)
  - Cloud billing import (Azure/AWS/GCP) + on-prem TCO model
  - Idle/waste, budgets + alerting, carbon estimate
  - Capacity simulation (lose a node / an AZ)
  - **Rightsizing — three tiers, and Kaptein never computes the recommendation**
    (ADR-0015). The mechanism was previously unspecified ("rightsizing from actual
    usage" implies Prometheus; Goldilocks implies VPA); it is now decided:
    1. **Read VPA recommendations** when the `VerticalPodAutoscaler` CRD is present — a
       cross-resource join between the workload's `resources.requests` and the VPA's
       `status.recommendation.containerRecommendations[]`.
    2. **Query a coarse estimate from Prometheus** when VPA is absent — live query,
       **nothing stored**, labelled as the cruder estimate it is.
    3. **No recommender of our own.** No usage-history store, no histogram/decay model,
       no checkpointing — that is VPA's job (`kubernetes/autoscaler`, SIG-Autoscaling),
       and building it would breach the "no metrics/log store" non-goal.
  - **The moat is adjudication, not the number** (ADR-0015) — the same split as ADR-0013's
    MCP taxonomy. Every recommendation names its **source** (VPA / PromQL / none) and
    carries a confidence signal, and these rules live in the **diagnostics engine**
    (M1.6 → M3a.4), not in cost-surface code:
    - provenance — how much history backs it (VPA checkpoints carry sample counts)
    - **HPA conflict** — an HPA scaling on CPU alongside a VPA recommending CPU on the
      same workload is a documented upstream footgun; Kaptein sees both objects
    - **staleness from deploys** — the time machine knows the workload was redeployed
      since the samples were taken, so the recommendation describes the *old* image
      (nothing else in this space can say this)
    - **pod-level `resources` incompatibility** — VPA does not support workloads defining
      pod-level resource stanzas; flag it rather than show a number admission will reject
    - **blast radius** — applying it may make the pod unschedulable, breach a
      ResourceQuota, or change QoS class
    - **remediation** — open a PR against the owning manifest (M2.3 / ADR-0008).
      Goldilocks stops at the number; the loop is the differentiator.
  - **Blocked on a lens-schema gap:** the tier-1 join is not expressible today —
    `ViewDefinition` has a single `target` GVK and all field paths resolve against that
    one object. Cross-resource joins must land in M2.2 first; this is a better acceptance
    test for the schema than the three single-object lenses ADR-0012 originally chose.
  - **DoD (falsifiable):** on a cluster with VPA, a rightsizing row shows current vs.
    recommended with the source labelled `vpa` and a sample-count-backed confidence; on a
    cluster with Prometheus only, the same row reads `promql (estimate)`; on a cluster with
    neither it reads "no recommendation available" — **never a fabricated number** — and a
    workload with a CPU-scaling HPA carries the conflict warning.
- **M3b.2 Security & compliance (kubescape class)**
  - Posture: CIS, NSA/CISA, MITRE ATT&CK, NSM *Grunnprinsipper*, **CRA, NIS2, DORA**
  - Image scan (Trivy/Grype), SBOM, cosign/sigstore, SLSA
  - **SBOM reconciliation** (two generators, diff, trust decision)
  - **VEX filtering** (CVE → actually reachable workloads)
  - RBAC visualization (effective permissions per SA)
  - **Policy preflight**: Kyverno/Gatekeeper/ValidatingAdmissionPolicy locally
  - NetworkPolicy editor + "can A reach B" simulation
  - Secrets masked; ESO/Vault/SOPS source display; CVE → workloads
- **M3b.3 Deep diagnostics & mesh**
  - Continuous sanity scan with score/trend; OOM forensics; blast-radius preview
  - Istio mTLS/ambient/`istioctl analyze`/config-dump; Cilium/Hubble flow-map
  - Loki/OpenSearch historical logs; Tempo/Jaeger traces; Alertmanager + silences
- **M3b.4 Incident & collaboration**
  - Session recording (asciinema-like TUI / event-log GUI) → Markdown incident timeline
  - Shared workspace configs in Git; full local audit log (stable format, reused by the
    incident-timeline export)
  - Operational memory: owner from labels/annotations, on-call (PagerDuty/Opsgenie/
    Grafana OnCall), runbook from `runbook_url` or Git-backed markdown
  - Incident timeline records cluster actions (deploys, scaling, node events, alerts) —
    a postmortem, not a command log
  - **Optional audit sink** (syslog / OTLP / webhook) with local buffering — the hook
    for enterprise adoption and the CRA/NIS2/DORA mapping
- **M3b.5 Cluster lifecycle, certificates & DR**
  - Version matrix + EOL per cluster; operator compatibility; PDB blockers
  - Control-plane health for on-prem: etcd size, defrag, leader elections, apiserver
    latency
  - Certificate expiry across the fleet (kubelet, cert-manager, webhook CA, mesh CA)
  - Backup-gap report + RPO/RTO per namespace
- Definition of Done: the complete capability set from `README.md`, with the five
  differentiators (governed MCP, GitOps write path, time machine, fleet query + drift)
  fully functional and cross-frontend.

---

## Open strategic decisions (not engineering — the maintainer's call)

*Raised by an external strategy review (2026-09). Recorded here because each has concrete
engineering consequences already tracked in this file, but **none is decided**, and none
should be settled by an audit. Each needs an explicit answer; several of the others resolve
once the first two are answered.*

1. **Licence: keep BUSL-1.1 wholesale, or split open-core?** The review argues for
   Apache-2.0 on core/viewmodel/TUI/CLI/MCP with BUSL retained on hub, fleet query,
   multi-cluster `serve`, and SSO — Grafana's model. The evidence for a cost is already in
   this repo rather than hypothetical: the *central* Krew index is closed to us (#34,
   worked around with a custom index that most casual installs will not add), Homebrew
   core / nixpkgs / distro packaging are effectively closed, and the CNCF landscape is a
   primary discovery surface for platform engineers. The sharpest form of the argument:
   the differentiated GTM niche this roadmap keeps pointing at — European regulated and
   public-sector on-prem (airgap, no telemetry, NSM *Grunnprinsipper*, CRA/NIS2/DORA in
   M3b.2) — is exactly the buyer whose procurement checks the OSI-approved list. If that
   is the target, the licence currently works against it.
   - Counter-considerations the review does not weigh: the extension surface is *already*
     MIT/Apache-2.0 (ADR-0004), which is the part third parties actually build on; and a
     licence change is effectively one-way once outside contributors exist.
   - If the answer is "split", it is an **ADR**, not a roadmap bullet — it changes the CLA,
     the commercial thresholds, and the crate metadata.
   - If the answer is "keep", the review's fallback is worth taking: drop the 25-employee
     test (it excludes tiny consultancies while a 20-person, well-funded startup passes)
     and publish a one-page plain-English FAQ, since ambiguity — not the terms — is what
     damaged BUSL adoption elsewhere.
2. **Is this a product/company or an open project?** The repo currently signals both: BUSL
   + CLA + commercial thresholds say company; "no telemetry, no account, no hosted service,
   no marketplace" says open project. They imply different licences, different roadmaps,
   and a different README. Answering this resolves (1), the scope question in (3), and the
   contribution-friction question below.
3. **Scope: is Phase 3b advertised as "planned" or as "integration targets"?** The review
   reads the 14 README feature areas as a five-year roadmap for one person. That
   overstates the position — Phase 3b is already explicitly *conditional* ("gated on Phase
   3a finding users — an explicit stopping point, not a failure") and M3b.2 says "Image
   scan (Trivy/Grype)", i.e. shell-out, consistent with the "no reimplemented scanners"
   non-goal. The legitimate residue is **presentational**: a README reader cannot tell
   integrate-from-implement. Making that distinction explicit in the feature list is a
   docs fix, not a scope cut. *(Recorded so this specific critique does not recur.)*
4. **Bus factor.** 247 commits, one contributor, CLA + DCO + BUSL. Nothing here survives
   the maintainer losing interest. If it is meant to, contribution friction is the first
   thing (1) and (2) should optimise for.

**Presentation gaps** with engineering-adjacent cost, tracked as `ISSUES.md` findings AH
and AI: no screenshot/GIF/asciinema anywhere in the README (for a *terminal* UI, the demo
is the product), two install options both labelled "Recommended", and an MSRV policy
(`docs/versioning.md`: "lags to accommodate airgapped and distro toolchains") that the
actual `1.97.1` pin does not deliver.

**Signals worth watching** (proposed by the review, adopted here because each is
falsifiable and none is a vanity metric): time-to-first-useful-command for someone who has
never seen Kaptein; whether anyone *outside* the repo writes a lens — the single best
evidence the ADR-0005 architecture bet paid off; and the first user who lets an agent touch
a non-toy cluster, whose objection list is a better Phase 2 backlog than this document.

---

## Cross-cutting commitments (all phases)

- **One static binary**, no telemetry, no account, airgap-safe. External tools that can't
  be embedded (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`) are invoked
  when present and degrade gracefully when absent.
- **Read-only default** for unknown contexts.
- **Informer-based**, never polling.
- **Same keymap** in TUI and GUI.
- i18n + screen-reader-friendly GUI.
- **Secrets are masked by default** (M1.7) — redaction runs in `kaptein-core` before any
  serialization, not at the frontend.
- **Signed releases + SBOM** — cosign signature, SLSA provenance, SBOM, and a
  `SHA256SUMS` file on every release (elevated from the Phase 0 "stub" to a real DoD per
  review; SECURITY.md already promises it). *Done: cosign keyless signing + CycloneDX
  SBOM + `SHA256SUMS` (release.yml), the SBOM is cosign-signed against the same OIDC
  identity as the binaries, and SLSA provenance is generated per release via
  `slsa-framework/slsa-github-generator` (the `provenance` job).*
- **Release-gate hygiene** — neither `release.yml` nor `publish.yml` runs `cargo test`
  before shipping; `publish.yml` pushes five crates on a raw tag with no `needs:` on
  anything. Add `needs: test` to each. *Done: `release.yml` has a `test` gate job and
  `publish.yml` now has a `needs: test` gate and is idempotent (skips crates already
  at the tagged version).*
- **Redaction-aware error boundary** — `kaptein-viewmodel::Error` maps raw
  `kube::Error`/subprocess failures to user-facing messages; `kaptein-integration` is
  that boundary, not a `#[error("{0}")]` pass-through (elevated per review).
- **Config schema, precedence, and validation** — a single config file (XDG, TOML,
  schema-validated) with precedence config → CLI → env; a `kaptein config validate` /
  `explain-context` command so a typo in a prod regex is surfaced, not silently ignored.
- **Contract-version enforcement** — refuse to load a plugin, lens, or MCP client whose
  version is unsupported (per `docs/versioning.md`); the MCP surface advertises a
  version field and the compatibility gate is implemented (elevated per review). *MCP
  gate: done — `kaptein-viewmodel::versioned` + `mcp.rs` refuse a client with an
  incompatible major. Lens (M2.2) and WIT (M2.6) gates land with their engines.*
- **Distribution & release sync** — a Homebrew tap, Krew plugin, container image,
  checksums, and install script, owned by a milestone; the site/README/docs stay in sync
  with a tag (the review found five releases of drift). **The milestone must name the
  artifact: a release-triggered site/README version bump** (kaptein.io currently shows
  v0.17.0 against the repo, and offers build-from-source only despite signed binaries
  existing). *Landed (2026-08-25): `install.sh` (checksum-verified install from the
  signed release binaries), `krew/kaptein.yaml` (Krew plugin manifest), and a `Dockerfile`
  (distroless static image built from the verified release tarball). Remaining: a
  Homebrew tap, a release-triggered site/README version bump, and wiring the Krew
  manifest into a CI publish step.*
  - **Resolved (v0.27.0 re-audit):** all three advertised install paths now work as
    shipped — `release.yml` renders the Krew manifest at release time (real tag + per-target
    sha256s, failing on any leftover `PLACEHOLDER_*`, issue #23); `install.sh` cosign-verifies
    `SHA256SUMS` against the OIDC identity (degrading with a loud warning when cosign is
    absent, issue #24); and a `container` release job builds, pushes, and cosign-signs
    `ghcr.io/egkristi/kaptein` (issue #31). The unused `VERSION_TAG` was removed (issue #30).
    `README.md` now documents all three as real channels. *Remaining: a Homebrew tap and a
    release-triggered site/README version bump.*
  - *Guarded (v0.30.1 →): a `version-sync` CI job derives the workspace version from
    `Cargo.toml` and fails if `README.md`/`install.sh`/`Dockerfile`/`docs/INSTALL.md`
    do not all reference `v<version>` — the derive-don't-restate guard against the
    "five releases of drift" the milestone names. The Homebrew tap and an automated
    release-triggered bump remain (the latter would fight the manual release commit on a
    PR-protected branch, so the consistency gate is the shipped form).*
  - **Resolved (v0.29.0):** `kubectl krew install kaptein` now works end to end — not via
    the central `kubernetes-sigs/krew-index` (a CNCF repo that requires OSI-approved open
    source, which BUSL-1.1 is not), but via a **custom index**
    (`https://github.com/egkristi/krew-index`, `plugins/kaptein.yaml` with real version +
    per-platform sha256s) and the release-published manifest
    (`kubectl krew install --manifest-url=.../releases/latest/download/kaptein.yaml`). Both
    verified against a real `krew` install (issue #34).
- **Performance budget**: a synthetic cluster via **kwok** (thousands of fake nodes and
  pods, no kubelets) drives CI benchmarks (owned by M1.8). Falsifiable targets:
  - p99 keystroke-to-frame < 16 ms at 50 000 objects in store
  - steady-state RSS < 250 MB at 50 000 objects
  - cold start to first usable frame < 500 ms
  - concurrent watches ≤ N for a given view set (see ADR-0006)
  - idle bytes/sec over the `serve` path ≈ 0 (no polling)
- **Kubernetes version support**: latest three minors; older API versions handled via
  discovery.
- **Config & errors**: a single config file (XDG path, TOML, schema-validated) with
  precedence config → CLI → env; a unified error enum in `kaptein-viewmodel` that maps raw
  `kube::Error` and subprocess failures to redaction-aware, user-facing messages.

- **How a milestone closes** *(added after the v0.27.0 re-audit)* — the recurring failure
  across three audit cycles has not been missing code; it has been **DoDs that a partial
  implementation satisfies literally**. Bounded-list code that exists but is not on the
  frontend path (M2.0). A policy manager with no caller (M2.0c). A signed release whose
  own installer skips verification. A lens engine no surface consumes (M2.2). Before
  marking anything done, both must hold:
  1. **The shipped path takes it** — not a test, not a CLI subcommand added to prove
     reachability, but the code path a user actually exercises.
  2. **A test fails if someone removes it** — the DoD names an assertion, not a state.

  When a DoD cannot be written that way, that is a signal the milestone is really two
  milestones.

- **Immediate next steps** — *(Phase 0 is long done, and the entire v0.27.0 re-audit batch
  (#20–#31) is closed: bounded frontend seed, relist-on-reconnect, LRU admission, preflight
  pluralization, log redaction, all three distribution channels, and the query benchmark.
  Findings P, Q, R, S, U, V, W, X, Y, M, N, O, T, Z, AA, and AB are all fixed, and the
  v0.31.0 re-audit batch is closed too: `LivePlane::query` now calls `touch` so the LRU
  stops evicting the hottest view (Z), fuzzy rerank is allocation-free with a bench case
  covering it (AA), and M2.0b's remaining DoD clauses — port-forward, the MCP protocol,
  the CLI binary end to end, and cordon/uncordon — are all live-tested in CI (AB). The
  v0.32.0 batch — AC (redaction as a type), AD (health in the manual), AE (health on more
  lenses) — is also closed. The live next steps, in order: **1.** the **M1.8 kwok
  synthetic-cluster harness** (the last aspirational number); **2. M2.1 browser UI**;
  **3. M2.2** per-lens action/health *panel*; and the distribution tail (Homebrew tap,
  release-triggered site version bump).)*

- **What the v0.30.0 re-audit says about the process** — the previous cycle's lesson
  ("the shipped path must take it") worked: every v0.27.0 finding is genuinely closed, and
  the mechanisms added are correct on their own terms. The new failure mode is one level
  up. Three of this cycle's findings (M, N, O) are *second-order consequences of the
  fixes themselves*: a manager that is shared-by-`Clone` but constructed per caller, a
  lifecycle API whose release half was never called, a reconcile that handles one
  direction of drift. Each passes its own unit tests. What would have caught all three is
  the DoD clause that was written and then not implemented — *"`InformerManager::live()`
  is observably bounded while driving the TUI through more distinct views than the cap
  allows"*. **A DoD assertion that is written but never turned into a test is worth
  nothing.** Before closing M2.0c this time, write that test first and watch it fail.

- **What the second v0.30.0 pass adds** — finding U is a different species from everything
  above it, and worth separating. M through T were *drift*: a doc that outran its code, a
  mechanism wired one level short. U is an **omission in the model itself** — `exec` was
  never gated because the guardrail set was enumerated by hand, command by command, and
  the most dangerous command was simply not on the list. Every audit so far has asked
  "does the shipped path take it?", which finds drift. It does not find something that was
  never enumerated. The countermeasure is different in kind: **derive the guarded set
  rather than restate it.** A coverage test that walks the CLI's own command enumeration
  and asserts every mutating variant carries `--confirm`, `--break-glass`, `gate_write`,
  and a distinct `Operation` would have failed the day `exec` was added — and will fail
  the day the next one is. Prefer that shape wherever a rule applies "to all X".

- **What the v0.31.0 pass adds — partial fixes are the durable failure mode.** The
  governance batch worked: the derive-don't-restate coverage test is exactly the right
  shape and it will keep working. But two of this cycle's three findings are the *same*
  shape as each other, and it is a shape worth naming.
  - **Finding Z:** N covered `release` **and** `touch`; the fix landed `release`, the row
    was marked *Fixed*, and `touch` was quietly dropped. Worse, the fix to M made the
    missing half **live** — the LRU now runs, and without a recency signal it evicts the
    hottest view.
  - **Finding AA:** M1.8 removed a clone-per-frame from the table path; the identical
    clone-per-keystroke on the search path was never in scope, and the benchmark that
    guards the budget does not cover that path, so nothing noticed.

  Both are cases where **a fix closed the instance it was written against and left the
  class open**. The countermeasures are specific and cheap: when a finding names two
  symptoms, the closing note must address both or explicitly defer one; and when a
  performance fix lands, the *gate* must move with it — a budget guard that covers one
  path while the hot path migrates elsewhere is worse than none, because it reports green.

  Three cycles running, a written DoD clause has been only partially turned into a test
  (`InformerManager::live()` boundedness twice, the fuzzy path now). The pattern is stable
  enough to act on: **treat an unimplemented DoD clause as a failing test, not as prose.**

- **What the v0.32.0 pass adds — the previous lessons worked; the remaining one is about
  *types*, not tests.** Z, AA, and AB were closed properly, and closed the *class* rather
  than the instance: the LRU test now asserts recency (not just the cap), the bench moved
  with the hot path, and M2.0b's DoD is fully covered. That is the "partial fixes" lesson
  landing. What is left is a different axis.

  Finding AC is the **second** plaintext-Secret leak on the lens render path (#35, then
  6692b10 months later, in a different crate, found the same way — by audit). Both fixes
  were correct and both were pointwise, because `render_row(&ViewDefinition, &Value)`
  *cannot* enforce anything: the guarantee lives in reviewer memory, re-verified at every
  new call site forever. The v0.32.0 health feature added a fourth such call site and got
  it right — but nothing would have caught it if it hadn't.

  The three countermeasures this project has adopted have all been the same move at
  different levels, and they have all worked:
  - **the shipped path must take it** (behaviour) — killed the "code exists but nothing
    calls it" class;
  - **derive the set, don't restate it** (tests) — the guardrail coverage test killed the
    "we enumerated the dangerous commands by hand and missed one" class;
  - **make it a type** (compilation) — not yet applied, and it is what AC needs.

  The rule generalises: **when a guarantee must hold at every call site, encode it in the
  signature.** A convention that has been violated twice is not a convention, it is a
  latent defect with a review step in front of it. `kaptein-core::describe` already
  demonstrates the shape in this codebase — safe by default, opt-out explicitly named and
  greppable. The lens path should look like that.

1. ~~Scaffold the Cargo workspace under `crates/`~~ — done (ADR-0014, five crates).
2. ~~Define the three-layer render contract and `AuditEvent`~~ — defined (ADR-0005); the
   **implementing** `DataPlane` is M2.0.
3. ~~Stand up the watcher/reflector store and CRD discovery~~ — CRD discovery done; the
   informer-backed bounded store is M2.0.
4. ~~Build the first ratatui `Table`~~ — done.
5. ~~Run `cargo deny check licenses`~~ — done and gated in CI.
