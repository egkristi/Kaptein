# Kaptein — *the domain layer is the product*

**Kaptein** is a unified Kubernetes workbench: a fast terminal UI, a native GUI, and a
headless agent — all three thin projections of one renderer-agnostic domain layer. It
is built for operators, SREs, platform engineers, and security teams who live inside
`kubectl` all day and are tired of juggling a dozen single-purpose tools.

> The mistake most Kubernetes tools make is treating the UI as the product. You either
> get a fast TUI without depth (k9s) or a heavy GUI you can't use over SSH (Lens,
> Headlamp). The right architecture is that **the domain layer is the product**, and the
> TUI/GUI/headless are three thin projections of the same view-model. That one decision
> decides whether a project survives year two.

---

## Why this exists

The Kubernetes tooling landscape is fragmented. Each tool owns a slice and locks you
into *its* UI:

| Tool | Strong at | Weak at |
|------|-----------|---------|
| **k9s** | Fast terminal nav, vim keymap | Shallow — no deep diagnostics, no GitOps, no fleet |
| **Lens / Headlamp** | Polished GUI, RBAC, multi-cluster | Heavy, web/Electron-centric, hard over SSH/bastion, logic lives in the UI |
| **Aptakube** | Clean desktop UX | Closed, opinionated, no automation path |
| **K8Studio** | Topology/relationship views | No write path, mouse-first |
| **Popeye** | Sanity scans | Read-only reports, no remediation loop |
| **kubescape** | Security posture scanning | Reports, not a daily-driver |
| **SRExpert** | SRE diagnostics | Point tool, not integrated |
| **OpenShift Console** | Dev perspective, topology | OpenShift-only, web-only |
| **Krust** | Rust-native k8s | Young, focused on primitives |
| **kubecost** | Cost allocation | Cost-only, commercial SaaS gravity |

**Kaptein** aims for *all of the above, and more* — with one architecture that keeps the
TUI, GUI, and headless agent from ever drifting apart, because none of them owns any
logic.

---

## The four things nobody else has

These are the differentiators that hit where daily work actually hurts:

1. **GitOps write path** — you edit in the UI; the tool figures out *which file in which
   repo* owns the resource (via Flux/Argo metadata), makes the change in a branch, and
   opens a PR — with diff at both manifest level *and* rendered level
   (`kustomize build` / `helm template`). You write to Git, not to the API server.

2. **Drift detection** — live state vs. rendered Git-state, continuously compared and
   surfaced, not a one-off report.

3. **Fleet query** — one query, all clusters: *"all Deployments without resource limits
   across 40 clusters"*. Cross-cluster diff and drift matrix.

4. **Time machine** — the watch stream is persisted locally; scrub backwards, see a
   resource as it was, diff between two timestamps, *"what changed between 14:20 and
   14:35"* — with events and deploy markers from Git on the same timeline.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│  kube-core                                                              │
│  kube-rs + tokio; watcher/reflector-based stores; CRD discovery;         │
│  DynamicObject; protobuf content-type; PartialObjectMetadata for         │
│  list-heavy views                                                        │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│  kube-viewmodel  (renderer-agnostic)                                     │
│  columns, sorting, filtering, status inference, "what am I allowed to    │
│  do here", action graphs. **All logic lives here.**                       │
└───────┬──────────────────────┬──────────────────────┬────────────────────┘
        │                      │                      │
┌───────▼───────┐      ┌───────▼───────┐      ┌───────▼───────┐
│ ratatui-      │      │ egui-         │      │ headless /    │
│ frontend      │      │ frontend      │      │ serve         │
│ terminal,     │      │ native +      │      │ agent, CI,    │
│ SSH, bastion  │      │ wasm browser  │      │ fleet-hub     │
└───────────────┘      └───────────────┘      └───────────────┘
```

**Consequence:** the TUI, GUI, and headless agent *cannot* drift apart, because none of
them owns any logic. That is the single decision that determines whether this project
survives.

### Extensibility: WASM Component Model, not plugins

Extension happens through the **WASM component model (WIT)** — not JS plugins
(Headlamp) or Go plugins that require recompilation. Sandboxed, language-agnostic,
cross-platform, and it behaves identically in every frontend.

But most "extensions" should not need code at all. See **Workload lenses** below.

---

## Authentication & context

Full auth surface:

- `kubeconfig`, exec credential plugins (kubelogin/Entra ID, aws, gcloud)
- OIDC device flow, client certificates, service account tokens, SPIFFE
- `--as` impersonation

Two things nobody does properly, which Kaptein treats as first-class:

- **RBAC preflight** — on context switch, run `SelfSubjectRulesReview` and *grey out*
  actions you're not allowed to perform **before** you try them — not a 403 afterwards.
- **Context guardrails** — prod contexts get a red frame, read-only by default, and an
  explicit *"break glass"* confirmation for writes. Configurable per regex on context
  name.

---

## Feature areas

### 1. Resource navigation (k9s / Lens / Aptakube parity)
- Command palette + vim keymap + fuzzy jump
- All built-in resources **and all CRDs** auto-discovered
- YAML editor with schema validation from OpenAPI/CRD schema, server-side dry-run and
  diff before apply
- Describe, scale, restart rollout, cordon/drain, evict, cascade selection on delete
- **Logs**: multi-pod/multi-container streaming with regex filter, JSON parsing into
  columns, time windows
- Exec/attach, ephemeral containers (`kubectl debug`-style profiler), node debug pods
- Port-forward manager with named, persistent forwards and auto-reconnect
- Krew compatibility by shelling out to existing plugins

### 2. Topology & diff
- Resource graph from ownerRefs, selectors, volumes, and RBAC bindings (K8Studio /
  OpenShift dev-perspective class), but **keyboard-navigable**
- Diff mode between two namespaces, two clusters, or two points in time

### 3. Observability
- Built-in metrics-server reading + adapter for Prometheus/Thanos/VictoriaMetrics with a
  PromQL console
- Loki/OpenSearch for historical logs, correlated with the resource you're standing in
- Traces via Tempo/Jaeger with deep-links from a pod
- Events deduplicated onto a timeline
- Alertmanager: active alerts → affected resource, and silences from the UI

### 4. Diagnostics & SRE (Popeye + SRExpert class)
- Continuous sanity scan with score and trend: missing limits/requests, no PDB, no
  probes, `:latest` tags, overly broad roles, orphaned PVC/ConfigMap/Secret
- *"Why isn't this pod ready?"* as a real decision tree over events, scheduler reasons,
  node capacity, taints, imagePull, probe config, and PVC binding
- OOM forensics with `lastTerminatedState` and the memory trend before the kill
- **Blast-radius preview**: before a change, which pods, PDBs, rollouts, and mesh routes
  are hit
- LLM assistance: opt-in, local endpoint possible, secrets redacted. **Never on by
  default.**

### 5. Security & compliance (kubescape class)
- Posture scan against CIS, NSA/CISA, and MITRE ATT&CK — plus NSM's *Grunnprinsipper*
  (Baseline Principles) as a first-class framework, since that's what you're actually
  audited on
- Image scanning (Trivy/Grype), SBOM viewing, cosign/sigstore verification, SLSA
  provenance
- RBAC visualization with effective permissions per ServiceAccount
- **Policy preflight**: run Kyverno/Gatekeeper/ValidatingAdmissionPolicy rulesets locally
  against a manifest and show *which policy would block it* before you send anything
- NetworkPolicy editor with simulation (*"can A reach B?"*)
- Secrets masked by default, with ESO/Vault/SOPS integration showing the *source*
  instead of the value
- CVE → affected workloads, not just affected images

### 6. Cost & capacity (kubecost+)
- Allocation per namespace/label/team/workload with showback and chargeback
- Cloud billing import for Azure/AWS/GCP **and** an on-prem TCO model — nobody handles
  on-prem OpenShift properly
- Rightsizing from actual usage, idle/waste report, budgets and alerting, carbon estimate
- Capacity simulation: *does the cluster survive losing a node or an AZ?*

### 7. GitOps & lifecycle — **the differentiator**
- Flux and Argo CD as first-class citizens: sources, reconciliation status,
  suspend/resume, force reconcile
- **Write path goes to Git, not the API server.** Edit in the UI → the tool locates the
  owning file/repo (via Flux/Argo metadata) → changes in a branch → opens a PR, with
  diff at manifest *and* rendered level. No tool in the list does this.
- Drift detector (live state vs. rendered Git state)
- Helm releases with values diff and rollback
- Crossplane XRD/claims with composition trace
- OLM subscriptions and upgrade channels
- Deprecated-API scanning before a cluster upgrade

### 8. Network & mesh
- Gateway API and Ingress side by side, cross-cluster route table
- Istio: mTLS status, ambient vs. sidecar, `istioctl analyze` parity, readable proxy
  config dump
- Cilium/Hubble flow-map with live traffic
- DNS and endpoint debugging

### 9. Storage & data
- PV/PVC/StorageClass/VolumeSnapshot, expansion, CSI driver status
- Velero/VolSync backup overview with restore-test status
- A proper **CNPG lens**: primary/replica topology, replication lag, switchover/failover,
  backup to object store, PITR window, WAL archive status, pending restart on parameter
  changes. *This does not exist today.*

### 10. Workload lenses as data, not code
Declarative **view definitions** (YAML or CUE) that bind a CRD to panels, columns, status
inference, actions, and health checks. This is how Strimzi, KubeVirt, cert-manager,
Keycloak, Tekton, Velero, Karpenter, and Knative are supported *without hardcoding
anything* — and your teams can write their own for internal CRDs and check them into Git.

WASM plugins only when real logic is needed. **This is the only way "and more" scales.**

### 11. Fleet
- **Fleet query**: one query, all clusters
- Cross-cluster diff and drift matrix
- Aggregated compliance, cost, and upgrade dashboards
- Optional hub mode with a small per-cluster agent, when you don't want N direct laptop
  connections

### 12. Time machine
- Watch stream persisted locally (redb/SQLite, optionally centralized)
- Scrub backwards, see a resource as it was, diff two timestamps
- *"What changed between 14:20 and 14:35"* — events and Git deploy markers on the same
  timeline. **During an incident this alone is worth the whole tool.**

### 13. Incident & collaboration
- Session recording (asciinema-like for TUI, event-log for GUI) exported to an incident
  timeline in Markdown
- Shared workspace configs in Git
- Full local audit log of every write operation

---

## Non-functional requirements

- **One static binary.** No runtime dependencies. External tools that can't be embedded
  (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`, cloud billing CLIs) are
  invoked when present and degrade gracefully when absent — the core binary stays
  self-contained.
- **No telemetry, no account, works in airgaps.**
- **Read-only default** for unknown contexts.
- **Signed releases with SBOM** — practice what we scan for.
- **Informer-based, not polling.**
- **Same keymap in both frontends.**
- **i18n** and a screen-reader-friendly GUI.

---

## Tech stack

| Concern | Choice |
|---------|--------|
| Language | Rust |
| Kubernetes client | `kube-rs` |
| Async runtime | `tokio` |
| TUI | `ratatui` |
| GUI | `egui` (+ wasm target for browser UI) |
| Headless / serve / agent | `axum` + `tonic` (gRPC) |
| Local persistence | `redb` or `sqlite` |
| Extensibility | WASM component model (WIT) |
| View definitions | YAML / CUE |
| CLI | `clap` |

### Repository layout (planned)

```
kube-core/         # kube-rs client, watchers/reflectors, CRD discovery, stores
kube-viewmodel/    # renderer-agnostic logic: columns, sort/filter, status, action graphs
crates/
  frontend-tui/    # ratatui
  frontend-gui/    # egui (+ wasm)
  headless/        # agent mode, CI, fleet-hub
  serve/           # serve backend
  plugins/         # WASM component model host + WIT interfaces
  viewdef/         # view definition schema + engine (YAML/CUE)
extensions/        # example view definitions & plugins
docs/              # architecture, contributing, security model
```

---

## Status

**Pre-alpha.** See [`ROADMAP.md`](./ROADMAP.md) for the phased plan. Nothing in this
repository is functional yet — this document and the roadmap define the target.

### Getting started (future)

```bash
# not yet available
cargo build --release
./target/release/kaptein
```

---

## License

TBD.
