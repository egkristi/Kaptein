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
    never emit a plaintext secret value — a test proves it.
- **M1.8 kwok performance harness** *(elevated per review — the numbers must be measured,
  not aspirational)*
  - A kwok-based synthetic cluster (thousands of fake nodes/pods) drives the
    cross-cutting performance budget; CI runs the benches and fails on regression
  - Owns the p99 <16 ms, RSS <250 MB, cold-start <500 ms targets *in Phase 1*, while the
    design can still change to meet them
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
    as `Rejected` — with a test that proves it.
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
- **M2.0b Integration-test tier + platform CI matrix** *(elevated per review)*
  - A kind/envtest tier exercising the real kube client, the MCP protocol, the CLI, and
    every write path (scale/delete/restart/cordon/evict/apply/exec/portforward) — none
    are unit-tested today
  - CI runs on Windows and macOS (not just `ubuntu-latest`), plus a conformance check
    against the latest three Kubernetes minors
- **M2.1 Browser UI** — egui → wasm served by `serve`, same keymap; the native desktop
  packaging (code-signing, notarization, installers, auto-update) is deferred until
  after Phase 3a
- **M2.2 Workload lenses as data**: view-definition engine (YAML/CUE) binding CRDs to
  panels/columns/status/actions/health-checks; ship lenses for Strimzi, KubeVirt,
  cert-manager, Keycloak, Tekton, Velero, Karpenter, Knative
  - Ship a versioned JSON Schema (and/or CUE schema) for view definitions plus a
    `kaptein viewdef validate` command so lenses are reviewable in PRs
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
  review; SECURITY.md already promises it).
- **Redaction-aware error boundary** — `kaptein-viewmodel::Error` maps raw
  `kube::Error`/subprocess failures to user-facing messages; `kaptein-integration` is
  that boundary, not a `#[error("{0}")]` pass-through (elevated per review).
- **Config schema, precedence, and validation** — a single config file (XDG, TOML,
  schema-validated) with precedence config → CLI → env; a `kaptein config validate` /
  `explain-context` command so a typo in a prod regex is surfaced, not silently ignored.
- **Contract-version enforcement** — refuse to load a plugin, lens, or MCP client whose
  version is unsupported (per `docs/versioning.md`); the MCP surface advertises a
  version field and the compatibility gate is implemented (elevated per review).
- **Distribution & release sync** — a Homebrew tap, Krew plugin, container image,
  checksums, and install script, owned by a milestone; the site/README/docs stay in sync
  with a tag (the review found five releases of drift).
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

## Immediate next steps

*(These were the Phase 0 next steps; they are all long done. The live next steps are the
open blocking milestones — M1.7 (redaction), M1b.4 (MCP governance), M2.0 (data plane +
informers), M2.0b (integration tests), and the cross-cutting supply-chain items.)*

1. ~~Scaffold the Cargo workspace under `crates/`~~ — done (ADR-0014, five crates).
2. ~~Define the three-layer render contract and `AuditEvent`~~ — defined (ADR-0005); the
   **implementing** `DataPlane` is M2.0.
3. ~~Stand up the watcher/reflector store and CRD discovery~~ — CRD discovery done; the
   informer-backed bounded store is M2.0.
4. ~~Build the first ratatui `Table`~~ — done.
5. ~~Run `cargo deny check licenses`~~ — done and gated in CI.
