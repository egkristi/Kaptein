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
(`kaptein-core`, `kaptein-viewmodel`, `frontend-tui`, `kaptein-cli`), later joined by
`kaptein-integration` as the native integration layer. The remaining items below are
kept for history; the crates that "have code" now are exactly those five.

- Cargo workspace under `crates/`: `kaptein-core`, `kaptein-viewmodel`, `frontend-tui`,
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
  (`frontend-tui` → `kaptein-integration` → `kaptein-core`, with no frontend depending on
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
- **M1.2 Resource navigation**
  - Command palette + vim keymap + fuzzy jump
  - Built-in resources + all CRDs auto-discovered
  - Describe, scale, restart rollout, cordon/drain, evict, cascade delete
  - Multi-pod/multi-container log streaming: regex filter, JSON → columns, time windows
  - Exec/attach, ephemeral containers, node debug pods
  - Port-forward manager (named, persistent, auto-reconnect)
  - Krew shell-out
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
- **M1.6 Minimal diagnostics rule engine**
  - One rule engine, **one rule pack**: "why isn't this pod ready" over events,
    scheduler reasons, probes, and PVC binding
  - This single pack feeds **three consumers at once**: the landing view (M1.5), the
    TUI diagnostics, and the MCP moat tool in 1b — validating the engine's shape before
    3a builds out the rest (resolves the ADR-0013 vs Phase 1b contradiction)
  - **A canned pod-status fixture corpus** — JSON fixtures for the common failure shapes
    (CrashLoopBackOff, exit-0 Job, ImagePullBackOff, unschedulable, probe failure) with
    expected findings, so the engine is regression-tested as packs grow (see the review).
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
    `kaptein edit` an explicit `Unredacted` path with a `SecretViewed` audit event.*
  - **Resolved (v0.27.0 re-audit):** log redaction landed — `redact::redact_line` masks
    `key=value`/`key: value`/JSON/`Authorization: Bearer` shapes for sensitive keys and is
    applied in `pod_logs`/`multi_pod_logs`/`follow_logs` (the MCP `logs` tool routes through
    `pod_logs`) — issue #22. Secret annotation redaction was narrowed to
    `last-applied-configuration`, Helm release-values/`.values`, and sensitive-named keys,
    preserving `meta.helm.sh/*`/Argo CD metadata — issue #29.
- **M1.8 kwok performance harness** *(elevated per review — the numbers must be measured,
  not aspirational)*
  - A kwok-based synthetic cluster (thousands of fake nodes/pods) drives the
    cross-cutting performance budget; CI runs the benches and fails on regression
  - Owns the p99 <16 ms, RSS <250 MB, cold-start <500 ms targets *in Phase 1*, while the
    design can still change to meet them
  - **Known hot spot to fix before the harness can pass (re-audit v0.27.0):** the TUI's
    per-frame *rendering* is now windowed, but `frontend-tui::query_plane` still issues
    `Query { start: 0, end: 50_000 }` on every loop iteration (~10 Hz) and
    `MemPlane::query` deep-clones the entire row `Vec` and sorts it before windowing. The
    clone-and-sort, not the allocation, is what the p99 budget will trip over. The TUI
    needs `page.total` for `rows.len()`/`G` navigation, so the fix is to query the visible
    window and carry `total` separately (`ISSUES.md` finding I, issue #28).
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
    `KAPTEIN_LIVE_TESTS=1` so the default run stays hermetic. Remaining: kind/envtest in
    CI (a cluster is not guaranteed on ubuntu runners) and the latest-three-minors
    conformance matrix.*
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
    `GroupVersionKind`, `StatusRule`/`RuleOp`, `ConditionRule`, `LensAction`),
    `validate_viewdef`, `evaluate_status` (field-path resolution + scalar **and
    Kubernetes-condition** rule evaluation), and `render_row` (maps a lens + a resource
    into the render contract's `Row` — the status-rule *rendering* half, with a
    data-bound `Column.field` so a column's value source is explicit, not implicit, per
    ADR-0012); `kaptein viewdef validate -f` parses a lens and reports problems;
    `kaptein viewdef schema` emits the JSON Schema; `kaptein viewdef render` renders a
    lens against a live/fixture resource; the `extension.yaml` manifest +
    `kaptein extension {list,validate,enable,disable}` lifecycle (ADR-0004) are
    implemented; the example lens set ships under `extensions/` — CNPG, Strimzi Kafka,
    KubeVirt, cert-manager, Keycloak, Tekton, Velero, Karpenter, Knative (all
    MIT/Apache-2.0).
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
    (`map_object_with` is the single seed/watch mapping). **Still open (Phase 2+):**
    per-lens action/health surfaces (M2.4+), the browser UI's lens navigation (M2.1), and
    the `kaptein` TUI's lens action graph.
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
  - Rightsizing, idle/waste, budgets + alerting, carbon estimate
  - Capacity simulation (lose a node / an AZ)
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

- **Immediate next steps** — *(Phase 0 is long done. The live next steps are the
  re-audit re-opens — **M2.0** (bounded seed on the frontend path), **M2.0c** (relist on
  reconnect, wire `InformerManager`, LRU-or-amend-the-ADR), **M1.7** (log redaction),
  **M1b.4** (preflight plural must match the request) — then the remaining **M2.0b**
  (kind/envtest + latest-three-minors conformance), **M1.8 kwok harness**, **M2.1 browser
  UI**, **M2.2 lens-driven navigation**, and the **distribution** fixes (Krew placeholders,
  installer signature verification). The SLSA-provenance and release-gate-hygiene pieces
  are done.)*

1. ~~Scaffold the Cargo workspace under `crates/`~~ — done (ADR-0014, five crates).
2. ~~Define the three-layer render contract and `AuditEvent`~~ — defined (ADR-0005); the
   **implementing** `DataPlane` is M2.0.
3. ~~Stand up the watcher/reflector store and CRD discovery~~ — CRD discovery done; the
   informer-backed bounded store is M2.0.
4. ~~Build the first ratatui `Table`~~ — done.
5. ~~Run `cargo deny check licenses`~~ — done and gated in CI.
