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

The Kubernetes tooling landscape is fragmented — each tool owns a slice and locks you
into *its* UI. Rather than grade individual projects, here is **what Kaptein does
differently**:

| What existing tools do well | What Kaptein adds |
|-----------------------------|-------------------|
| k9s — fast terminal nav, vim keymap | The same speed *plus* deep diagnostics, GitOps, and fleet — without a second tool |
| Lens / Headlamp — polished GUI, RBAC, multi-cluster | The same UI *plus* a TUI over SSH, with logic that lives in a shared layer, not the UI |
| K8Studio / OpenShift Console — topology views | Keyboard-navigable topology, not mouse-first |
| Popeye / kubescape / SRExpert — scans and reports | Integrated into a daily-driver with a remediation loop, not a one-off report |
| kubecost — cost allocation | Cost plus capacity simulation and an on-prem TCO model |

The unifying idea: **the domain layer is the product.** The TUI, GUI, and headless agent
are three thin projections of one view-model, so they cannot drift apart — none of them
owns any logic.

---

## The four things that are hard to find elsewhere

These are the differentiators that hit where daily work actually hurts:

1. **GitOps write path, from the operator console** — you edit in the UI *where you
   already stand during an incident*, with live cluster state beside you; the tool
   figures out *which file in which repo* owns the resource (via Flux/Argo metadata),
   makes the change in a branch, and opens a PR — with diff at both manifest level *and*
   rendered level (`kustomize build` / `helm template`). You write to Git, not to the
   API server. (IDP portals like Backstage/Port offer self-service actions, but two
   clicks away from reality; Kaptein does it from the live operator console.)

2. **Drift detection** — live state vs. rendered Git-state, continuously compared and
   surfaced, not a one-off report.

3. **Fleet query** — one query, all clusters: *"all Deployments without resource limits
   across 40 clusters"*. Cross-cluster diff and drift matrix.

4. **Time machine** — the watch stream is persisted locally; scrub backwards, see a
   resource as it was, diff between two timestamps, *"what changed between 14:20 and
   14:35"* — with events and deploy markers from Git on the same timeline.

---

## Non-goals

Kaptein deliberately does **not** reimplement tools that already do one job well — it
renders, orchestrates, and cross-references them:

- **No secret storage.** Secrets stay in your existing systems (Vault, ESO, SOPS);
  Kaptein shows the *source*, never the value, and masks by default.
- **No reimplemented scanners.** Trivy/Grype, Kyverno/Gatekeeper, `istioctl`,
  `kustomize`, `helm`, and Krew plugins are shelled out to, never vendored.
- **No hosted service or telemetry.** No account, no cloud dependency, no analytics.
- **No polling.** State comes from informers/watch streams, never periodic scrapes of the
  API server.
- **No hardcoded per-CRD UIs.** Operator-specific lenses are data (view definitions), not
  code — WASM only when real logic is required.

These exclusions are what keep the surface tractable and the single binary honest.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│  kaptein-core                                                           │
│  kube-rs + tokio; watcher/reflector-based stores; CRD discovery;         │
│  DynamicObject; protobuf content-type; PartialObjectMetadata for         │
│  list-heavy views                                                        │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
┌───────────────────────────────▼─────────────────────────────────────────┐
│  kaptein-viewmodel  (renderer-agnostic)                                  │
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

Key architectural decisions are recorded as **ADRs** in [`docs/adr/`](docs/adr/) — the
`egui`-over-`iced` choice is [ADR-0001](docs/adr/0001-egui-over-iced.md).

### Extensibility: three tiers, one manifest

"Plugins", "modules", and "extensions" are one thing in Kaptein: an **extension** — any
declared, versioned way to add capability — and it always comes in one of three tiers,
chosen data-first:

1. **View definitions (lenses)** — declarative YAML/CUE binding a CRD to panels, columns,
   status inference, actions, and health checks. No code, PR-reviewable, checked into
   Git. This is the default and covers the "and more" long tail.
2. **WASM component-model plugins (WIT)** — sandboxed, language-agnostic code for when
   real logic is required. Behaves identically in every frontend; no JS plugins
   (Headlamp) or Go plugins that require recompilation.
3. **Shell-out integrations** — external binaries (Krew plugins, `kustomize`, `helm`,
   Trivy/Grype, `istioctl`) invoked when present and degraded gracefully when absent.

All three are declared by a shared **extension manifest** (`extension.yaml`) and
discovered from configurable, Git-backed extension paths — no central marketplace.
Lifecycle is managed with `kaptein extension {validate,list,enable,disable}`.

**Sandbox by default.** WASM plugins run with fuel metering, a memory cap, **no network
and no filesystem** unless a capability is declared in the WIT world *and* in the
manifest allowlist. See [ADR-0004](docs/adr/0004-extension-model.md).

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
  default** — disabled per context until explicitly enabled. Redaction is structural
  (driven by the CRD schema and well-known secret keys like `env`, `data`, and
  annotations), not regex-only, and the input to any model is always shown for review
  before it leaves the machine.

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

### 10. Workload lenses & extensions (data first, code second)
Declarative **view definitions** (YAML or CUE) that bind a CRD to panels, columns, status
inference, actions, and health checks. This is how Strimzi, KubeVirt, cert-manager,
Keycloak, Tekton, Velero, Karpenter, and Knative are supported *without hardcoding
anything* — and your teams can write their own for internal CRDs and check them into Git.

When a lens isn't enough, escalate to a **WASM plugin** (tier 2) or a **shell-out
integration** (tier 3) — see *Extensibility* above. Data first, code second: **this is
the only way "and more" scales.**

### 11. Fleet
- **Fleet query**: one query, all clusters
- Cross-cluster diff and drift matrix
- Aggregated compliance, cost, and upgrade dashboards
- Optional hub mode with a small per-cluster agent, when you don't want N direct laptop
  connections

### 12. Time machine
- Watch stream persisted locally (redb/SQLite, optionally centralized) with compaction
  and a configurable retention TTL so local disk stays bounded
- Scrub backwards, see a resource as it was, diff two timestamps
- *"What changed between 14:20 and 14:35"* — events and Git deploy markers on the same
  timeline. **During an incident this alone is worth the whole tool.**

### 13. Incident & collaboration
- Session recording (asciinema-like for TUI, event-log for GUI) exported to an incident
  timeline in Markdown
- Shared workspace configs in Git
- Full local audit log of every write operation, in a single stable format that is also
  the source for incident-timeline exports (one format, two consumers)

---

## Non-functional requirements

- **One static binary** for the native and headless builds. No runtime dependencies.
  External tools that can't be embedded
  (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`, cloud billing CLIs) are
  invoked when present and degrade gracefully when absent — the core binary stays
  self-contained. The browser UI is a **wasm bundle** served by `serve`, not a binary.
- **No telemetry, no account, works in airgaps.**
- **Read-only default** for unknown contexts.
- **Signed releases with SBOM** — practice what we scan for.
- **Informer-based, not polling.**
- **Same keymap** in the TUI and GUI.
- **i18n** and a screen-reader-friendly GUI.
- **Kubernetes version support**: target the latest three minors; older API versions
  handled via the discovery API's served versions.

---

## Tech stack

| Concern | Choice |
|---------|--------|
| Language | Rust |
| Kubernetes client | `kube-rs` + `k8s-openapi` (typed built-ins) |
| Async runtime | `tokio` |
| TUI | `ratatui` |
| GUI | `egui` + `egui_table` (+ wasm target for browser UI) |
| Headless / serve / agent | `axum` (HTTP/REST + gRPC-Web) + `tonic` (gRPC) |
| Local persistence | `redb` or `sqlite` |
| Extensibility | WASM component model (WIT) |
| View definitions | YAML / CUE |
| CLI | `clap` |

### Repository layout (planned)

```
crates/
  kaptein-core/    # kube-rs client, watchers/reflectors, CRD discovery, stores
  kaptein-viewmodel/ # renderer-agnostic logic: columns, sort/filter, status, action graphs
  frontend-tui/    # ratatui
  frontend-gui/    # egui + egui_table (+ wasm)
  headless/        # agent mode: drives the view-model directly (no listener)
  serve/           # network server: axum HTTP/gRPC-Web + tonic gRPC; browser + hub
  plugins/         # WASM component-model host + WIT interfaces + manifest loader
  viewdef/         # view definition schema + engine (YAML/CUE)
  ext-sdk/         # extension authoring SDK (MIT/Apache-2.0)
extensions/        # example extensions (lenses, plugins, integrations)
docs/
  adr/             # architecture decision records (see ADR-0001)
  architecture.md  # architecture overview
CONTRIBUTING.md    # contributing guide
SECURITY.md        # security policy & disclosure (the canonical threat model)
LICENSE            # BUSL-1.1 (source-available; converts to MIT on Change Date)
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

Kaptein is **source-available** under the [Business Source License 1.1](./LICENSE) —
**not** an OSI open-source license.

- **Free for individuals, home use, and small/medium businesses**: production use is
  granted while you and your affiliates have less than **USD 5,000,000** in annual
  revenue **and** fewer than **25 employees**.
- **Larger commercial entities** require a commercial license (or must wait for the
  Change Date).
- On the **Change Date** (rolling — four years after each version's first public
  release), each version automatically converts to **MIT**.
- The **extension surface** (`ext-sdk/`, WIT worlds, view-definition schema, example
  extensions) is **MIT/Apache-2.0**, so third parties can write lenses and plugins
  without taking BUSL terms on their own work.

The exact thresholds and terms are in the [Additional Use Grant](./LICENSE) and are
easy to adjust as the project evolves.
