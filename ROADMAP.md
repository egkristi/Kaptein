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

- Cargo workspace: `kube-core`, `kube-viewmodel`, `frontend-tui`, `frontend-gui`,
  `headless`, `serve`
- `kube-core` skeleton: `kube-rs` + `tokio` client bootstrap, watcher/reflector store
  trait, CRD discovery (`DynamicObject`), protobuf content-type flag,
  `PartialObjectMetadata` for list-heavy views
- `kube-viewmodel` skeleton: column model, sort/filter, action-graph types — *no*
  rendering
- Repo hygiene: `CONTRIBUTING.md`, `SECURITY.md`, and an ADR process (`docs/adr/`),
  starting with ADR-0001 (`egui` over `iced`)
- CI: `cargo fmt`, `clippy`, `test`, signed release + SBOM pipeline stub
- Definition of Done: `cargo build --workspace` is green; layer deps are one-directional
  (`frontend-*` → `viewmodel` → `core`)

## Phase 1 — Core + viewmodel + ratatui (k9s parity + RBAC preflight)

**Goal:** a TUI that is *already useful alone* as a k9s replacement, in ~3–4 months.

Milestones:

- **M1.1 Auth & context**
  - `kubeconfig` load, exec credential plugins, OIDC device flow, client certs, SA
    tokens, SPIFFE, `--as` impersonation
  - **RBAC preflight**: `SelfSubjectRulesReview` on context switch → grey out
    disallowed actions
  - **Context guardrails**: prod-context red frame, read-only default, "break glass"
    confirmation; configurable per regex on context name
- **M1.2 Resource navigation**
  - Command palette + vim keymap + fuzzy jump
  - Built-in resources + all CRDs auto-discovered
  - Describe, scale, restart rollout, cordon/drain, evict, cascade delete
  - Multi-pod/multi-container log streaming: regex filter, JSON → columns, time windows
  - Exec/attach, ephemeral containers, node debug pods
  - Port-forward manager (named, persistent, auto-reconnect)
  - Krew shell-out
- **M1.3 YAML editor**
  - OpenAPI/CRD schema validation, server-side dry-run, diff before apply
- **M1.4 Diagnostics (v1)**
  - *"Why isn't this pod ready?"* decision tree over events, scheduler reasons, node
    capacity, taints, imagePull, probes, PVC binding
  - Sanity scan v1 (missing limits/requests, no probes, `:latest`, orphaned resources)
  - Events deduplicated onto a timeline (feeds the decision tree)
- **M1.5 Observability (v1) — *stretch, not required for k9s parity***
  - Metrics-server reading; Prometheus/Thanos/VictoriaMetrics adapter + PromQL console
- Definition of Done: a daily-driver TUI over SSH with k9s parity, RBAC preflight, and
  guardrails. Read-only default for unknown contexts.

## Phase 2 — GUI + view definitions + GitOps + dry-run diff

**Goal:** the same view-model drives a native GUI, and the GitOps write path becomes the
differentiator.

Milestones:

- **M2.1 egui GUI** on the *same* view-model, same keymap, wasm backend for
  browser UI
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
- **M2.5 Network & storage v1**
  - Gateway API + Ingress side by side; DNS/endpoint debugging
  - PV/PVC/StorageClass/Snapshot, CSI status, Velero/VolSync overview
  - CNPG lens: primary/replica topology, lag, switchover/failover, PITR window
- **M2.6 WASM component model** host + WIT interfaces; plugin sandbox; first example
  plugins; a versioning + deprecation policy for WIT interfaces so plugins don't break
  across releases
- Definition of Done: GUI and TUI are feature-identical projections of one view-model —
  proven by contract tests asserting the TUI, GUI, and headless all consume the same
  render intent; a GitOps change can be authored and opened as a PR entirely from the UI.

## Phase 3 — Time machine, fleet, cost, security

**Goal:** the four differentiators are complete, plus the scanning/analytics surface.

Milestones:

- **M3.1 Time machine**
  - Persist watch stream locally (redb/SQLite, optionally centralized) with compaction
    and a configurable retention TTL
  - Scrub backwards, diff two timestamps, "what changed between 14:20 and 14:35"
  - Events + Git deploy markers on one timeline
- **M3.2 Fleet**
  - Fleet query (one query, all clusters); cross-cluster diff + drift matrix
  - Aggregated compliance/cost/upgrade dashboards
  - Optional hub mode with per-cluster agent (headless/serve)
- **M3.3 Cost & capacity**
  - Allocation per namespace/label/team/workload (showback + chargeback)
  - Cloud billing import (Azure/AWS/GCP) + on-prem TCO model
  - Rightsizing, idle/waste, budgets + alerting, carbon estimate
  - Capacity simulation (lose a node / an AZ)
- **M3.4 Security & compliance (kubescape class)**
  - Posture: CIS, NSA/CISA, MITRE ATT&CK, NSM *Grunnprinsipper*
  - Image scan (Trivy/Grype), SBOM, cosign/sigstore, SLSA
  - RBAC visualization (effective permissions per SA)
  - **Policy preflight**: Kyverno/Gatekeeper/ValidatingAdmissionPolicy locally
  - NetworkPolicy editor + "can A reach B" simulation
  - Secrets masked; ESO/Vault/SOPS source display; CVE → workloads
- **M3.5 Deep diagnostics & mesh**
  - Continuous sanity scan with score/trend; OOM forensics; blast-radius preview
  - Istio mTLS/ambient/`istioctl analyze`/config-dump; Cilium/Hubble flow-map
  - Loki/OpenSearch historical logs; Tempo/Jaeger traces; Alertmanager + silences
- **M3.6 Incident & collaboration**
  - Session recording (asciinema-like TUI / event-log GUI) → Markdown incident timeline
  - Shared workspace configs in Git; full local audit log (stable format, reused by the
    incident-timeline export)
- Definition of Done: the complete capability set from `README.md`, with the four
  differentiators (GitOps write path, drift detection, fleet query, time machine) fully
  functional and cross-frontend.

---

## Cross-cutting commitments (all phases)

- **One static binary**, no telemetry, no account, airgap-safe. External tools that can't
  be embedded (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`) are invoked
  when present and degrade gracefully when absent.
- **Read-only default** for unknown contexts.
- **Informer-based**, never polling.
- **Same keymap** in TUI and GUI.
- i18n + screen-reader-friendly GUI.
- Signed releases with SBOM.
- **Performance budget**: informer-driven views stay responsive against a synthetic
  cluster with thousands of CRDs; k9s is the baseline to beat (benchmarked in CI).
- **Kubernetes version support**: latest three minors; older API versions handled via
  discovery.

## Immediate next steps

1. Scaffold the Cargo workspace and the three core crates (Phase 0).
2. Stand up the `kube-core` watcher/reflector store and CRD discovery.
3. Define the `kube-viewmodel` column/action-graph types.
4. Build the first ratatui table view on top of it.
