# Kaptein — *the console that knows what changed — and, next, lets you fix it in Git*

> Website: <https://kaptein.io> · Source: <https://github.com/egkristi/Kaptein>

[![CI](https://github.com/egkristi/Kaptein/actions/workflows/ci.yml/badge.svg)](https://github.com/egkristi/Kaptein/actions/workflows/ci.yml)
[![CodeQL](https://github.com/egkristi/Kaptein/actions/workflows/codeql.yml/badge.svg)](https://github.com/egkristi/Kaptein/actions/workflows/codeql.yml)
[![Release](https://img.shields.io/github/v/release/egkristi/Kaptein)](https://github.com/egkristi/Kaptein/releases)
[![License: BUSL-1.1](https://img.shields.io/badge/license-BUSL--1.1-orange.svg)](LICENSE)

**Kaptein** is a unified Kubernetes workbench: a fast terminal UI, a native GUI, and a
headless agent — all three thin projections of one renderer-agnostic domain layer. It
is built for operators, SREs, platform engineers, and security teams who live inside
`kubectl` all day and are tired of juggling a dozen single-purpose tools. *(Today the
CLI + TUI ship; the GUI and browser UI are on the Phase 2 roadmap — see
[Status at a glance](#status-at-a-glance).)*

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
| k9s — fast terminal nav, vim keymap | ✅ The same speed *plus* diagnostics and a governed MCP surface — today; 🛠️ GitOps and fleet on the roadmap |
| Lens / Headlamp — polished GUI, RBAC, multi-cluster | The *same* logic layer as the TUI — the GUI/browser UI is 🛠️ planned, the TUI ships ✅ |
| K8Studio / OpenShift Console — topology views | Keyboard-navigable topology, not mouse-first (🛠️ planned) |
| Popeye / kubescape / SRExpert — scans and reports | Integrated into a daily-driver with a remediation loop, not a one-off report (🛠️ planned) |
| kubecost — cost allocation | Cost plus capacity simulation and an on-prem TCO model (🛠️ planned) |

The unifying idea: **the domain layer is the product.** The TUI, GUI, and headless agent
are three thin projections of one view-model, so they cannot drift apart — none of them
owns any logic.

---

## Status at a glance

Kaptein is honest about what exists today versus what is on the roadmap. Everything
below is labelled one of three ways:

- ✅ **Available** — ships in the latest release, tested against a live cluster.
- 🧪 **Preview** — ships, but not yet complete or fully integrated across surfaces.
- 🛠️ **Planned** — on the roadmap (see [`ROADMAP.md`](./ROADMAP.md)); not yet shipped.

**Available now (Phase 1):** the `kaptein` CLI (one static binary — TUI, GUI, and
headless are subcommands of it), resource navigation
(list built-ins **and** CRDs, describe, logs, events, watch), diagnostics ("why isn't
this pod ready"), RBAC preflight, context guardrails (read-only default + break-glass),
secret masking, a dry-run-gated write path, the read-only **governed MCP server**, and
**lens-driven navigation** (data-first extensions for any CRD).

**Preview:** a cheap *"what changed in the last N minutes"* (in-memory watch ring + the
events API) — the precursor to the persistent Time Machine.

**Planned (Phase 2 → 3):** the **GitOps write path** (edit → PR, never the API server),
the persistent **Time Machine**, **fleet query + drift**, the browser/GUI surfaces,
topology & diff, observability, security/cost analytics, and the rest — all in
[`ROADMAP.md`](./ROADMAP.md).

---

## The differentiators

These are the capabilities that will hit where daily work actually hurts — with their
current status stated plainly:

1. ✅ **Governed MCP surface — available** — `kaptein mcp` lets AI agents drive Kaptein
   through the *same* guardrails as a human: RBAC preflight, context guardrails,
   read-only default, and break-glass, each agent running under its **own dedicated
   identity** (its own ServiceAccount and narrow RBAC) and landed in the same audit log
   with the *agent* as the actor. An agent never writes to the API server — it can only
   open a PR. This is the answer to "Shadow MCP": governed, auditable, scoped agent
   access (ADR-0010). *(Ships read-only today; the PR-only write path is M2.7.)*

2. 🛠️ **GitOps write path — planned (Phase 2)** — you edit in the UI *where you already
   stand during an incident*, with live cluster state beside you; the tool figures out
   *which file in which repo* owns the resource (via Flux/Argo metadata), makes the
   change in a branch, and opens a PR — with diff at both manifest level *and* rendered
   level (`kustomize build` / `helm template`). You write to Git, not to the API server.
   *(Today `kaptein edit`/`apply` are dry-run only — no write reaches the cluster or Git.
   The PR path is M2.3.)*

3. 🛠️ **Time machine — planned (Phase 3a)** — the watch stream is persisted locally;
   scrub backwards, see a resource as it was, diff between two timestamps, *"what changed
   between 14:20 and 14:35"* — with events and deploy markers from Git on the same
   timeline. *(🧪 A cheap preview — "what changed in the last N minutes" — ships now.)*

4. 🛠️ **Fleet query + drift — planned (Phase 3a)** — one query, all clusters: *"all
   Deployments without resource limits across 40 clusters"*. Cross-cluster diff and drift
   matrix. Saved queries in Git, scheduled reports, and **query-as-policy** (a query can
   fail CI) — the same data layer as drift detection (ADR-0011).

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
- **No CI/CD.** Devtron and Argo own it; Kaptein opens PRs, it does not run pipelines.
- **No service catalog.** Backstage has the catalog; Kaptein *integrates* (reads owner/
  runbook annotations), it does not compete.
- **No policy engine.** Kyverno is CNCF graduated; Kaptein *renders and preflights*
  policy, it does not enforce it.
- **No agent runtime.** `kagent` runs agents; Kaptein is the *governed tool surface*
  they call (ADR-0010).
- **No metrics/log store.** Kaptein queries Prometheus/Loki/etc., it does not store.

**Kaptein is the operator's console and the governed control point — not the platform.**
Everything above it arrives as lenses and integrations, not core code.

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
│   ✅ ships     │      │   🛠️ planned  │      │   🛠️ planned  │
└───────────────┘      └───────────────┘      └───────────────┘
```

**Consequence:** the TUI, GUI, and headless agent *cannot* drift apart, because none of
them owns any logic — they are projections of one view-model. *(The TUI ships today; the
GUI/browser and headless/serve surfaces are Phase 2.)*

Key architectural decisions are recorded as **ADRs** in [`docs/adr/`](docs/adr/) — the
`egui`-over-`iced` choice is [ADR-0001](docs/adr/0001-egui-over-iced.md).

### Extensibility: three tiers, one manifest

"Plugins", "modules", and "extensions" are one thing in Kaptein: an **extension** — any
declared, versioned way to add capability — and it always comes in one of three tiers,
chosen data-first:

1. ✅ **View definitions (lenses)** — declarative YAML/CUE binding a CRD to panels,
   columns, status inference, actions, and health checks. No code, PR-reviewable, checked
   into Git. **Ships today** (the engine + a lens set + lens-driven TUI navigation).
2. 🛠️ **WASM component-model plugins (WIT)** — sandboxed, language-agnostic code for when
   real logic is required. Behaves identically in every frontend. *(Planned, M2.6.)*
3. ✅ **Shell-out integrations** — external binaries (Krew plugins, `kustomize`, `helm`)
   invoked when present and degraded gracefully when absent. *(Ships today; Trivy/Grype/
   `istioctl` arrive with the scan features.)*

All three are declared by a shared **extension manifest** (`extension.yaml`) and
discovered from configurable, Git-backed extension paths — no central marketplace.
Lifecycle is managed with `kaptein extension {validate,list,enable,disable}`. Example
lenses ship under [`extensions/`](extensions/) — CNPG, Strimzi Kafka, KubeVirt,
cert-manager, Keycloak, Tekton, Velero, Karpenter, and Knative — all MIT/Apache-2.0.

**Sandbox by default.** 🛠️ WASM plugins will run with fuel metering, a memory cap, **no
network and no filesystem** unless a capability is declared in the WIT world *and* in the
manifest allowlist. See [ADR-0004](docs/adr/0004-extension-model.md).

---

## Authentication & context

Auth surface today:

- ✅ `kubeconfig` + exec credential plugins (kubelogin/Entra ID, aws, gcloud)
- ✅ Service-account tokens (the MCP agent identity, ADR-0007)
- 🛠️ OIDC device flow, client certificates, SPIFFE, `--as` impersonation *(deferred to
  1b / hub mode)*

Two things nobody does properly, which Kaptein treats as first-class:

- **RBAC preflight** — on context switch, run `SelfSubjectRulesReview` and *grey out*
  actions you're not allowed to perform **before** you try them — not a 403 afterwards.
- **Context guardrails** — prod contexts get a red frame, read-only by default, and an
  explicit *"break glass"* confirmation for writes. Configurable per regex on context
  name.

---

## Feature areas

> Legend: ✅ **Available** · 🧪 **Preview** · 🛠️ **Planned**. A section is marked by the
> state of its *bulk*; shipped sub-bullets are called out inline. The authoritative
> milestone-by-milestone state is [`ROADMAP.md`](./ROADMAP.md).

### 1. Resource navigation — ✅ available (core) / 🛠️ planned (some)
- ✅ Command palette + vim keymap + fuzzy jump
- ✅ All built-in resources **and all CRDs** auto-discovered
- ✅ Describe, scale, restart rollout, cordon/drain, evict, cascade selection on delete
  (all dry-run by default; `--confirm` + break-glass to write)
- ✅ **Logs**: multi-pod/multi-container streaming with regex filter, JSON parsing into
  columns, time windows
- ✅ Exec/attach, ephemeral containers (`kubectl debug`-style profiler), node debug pods
- ✅ Port-forward manager with named, persistent forwards and auto-reconnect
- ✅ Krew compatibility by shelling out to existing plugins
- 🛠️ YAML editor with schema validation from OpenAPI/CRD schema, server-side dry-run and
  diff before apply *(today: `kaptein edit`/`apply` are dry-run-only handoffs)*

### 2. Topology & diff — 🛠️ planned (Phase 2)
- Resource graph from ownerRefs, selectors, volumes, and RBAC bindings (K8Studio /
  OpenShift dev-perspective class), but **keyboard-navigable**
- Diff mode between two namespaces, two clusters, or two points in time

### 3. Observability — 🛠️ planned (Phase 3b)
- Built-in metrics-server reading + adapter for Prometheus/Thanos/VictoriaMetrics with a
  PromQL console
- Loki/OpenSearch for historical logs, correlated with the resource you're standing in
- Traces via Tempo/Jaeger with deep-links from a pod
- ✅ Events deduplicated onto a timeline *(ships today)*
- Alertmanager: active alerts → affected resource, and silences from the UI

### 4. Diagnostics & SRE (Popeye + SRExpert class) — ✅ available (pod diagnostics) / 🛠️ planned (the rest)
- ✅ *"Why isn't this pod ready?"* — a rule engine over events, scheduler reasons,
  node capacity, taints, imagePull, probe config, and PVC binding (`kaptein diagnose`
  and the MCP `diagnose`/`explain_pod_failure`/`why_is_job_pending` tools)
- ✅ **Governed MCP surface** (`kaptein mcp`): AI agents drive Kaptein through the same
  guardrails as a human — read-only today, PR-only writes later (ADR-0010)
- 🛠️ Continuous sanity scan with score and trend: missing limits/requests, no PDB, no
  probes, `:latest` tags, overly broad roles, orphaned PVC/ConfigMap/Secret
- 🛠️ OOM forensics with `lastTerminatedState` and the memory trend before the kill
- 🛠️ **Blast-radius preview** *(today: a read-only `blast_radius` MCP tool for
  Deployment→ReplicaSet→Pod)*
- 🛠️ LLM assistance: opt-in, local endpoint possible, secrets redacted — never on by
  default

### 5. Security & compliance (kubescape class) — 🛠️ planned (Phase 3b)
- Posture scan against CIS, NSA/CISA, and MITRE ATT&CK — plus NSM's *Grunnprinsipper*
- Image scanning (Trivy/Grype), SBOM viewing, cosign/sigstore verification, SLSA
- **SBOM reconciliation**, **VEX filtering** (CVE → reachable workloads)
- **Framework mapping**: CIS, NSA/CISA, MITRE ATT&CK, NSM, **CRA, NIS2, DORA**
- RBAC visualization, **policy preflight** (Kyverno/Gatekeeper/ValidatingAdmissionPolicy),
  NetworkPolicy editor with *"can A reach B?"*
- ✅ Secrets masked by default *(ships today)*; ESO/Vault/SOPS source display planned
- ✅ Signed releases + SBOM *(ships today — the *supply chain we scan for*)*

### 6. Cost & capacity (kubecost+) — 🛠️ planned (Phase 3b)
- Allocation per namespace/label/team/workload, cloud billing import + on-prem TCO,
  rightsizing, budgets, carbon estimate, capacity simulation

### 7. GitOps & lifecycle — 🛠️ planned (Phase 2/3)
- Flux and Argo CD as first-class citizens: sources, reconciliation status,
  suspend/resume, force reconcile
- **Write path goes to Git, not the API server.** Edit in the UI → the tool locates the
  owning file/repo (via Flux/Argo metadata) → changes in a branch → opens a PR, with
  diff at manifest *and* rendered level.
- Drift detector, Helm values diff + rollback, Crossplane XRD/claims, OLM subscriptions,
  deprecated-API scanning

### 8. Network & mesh — 🛠️ planned (Phase 2/3b)
- Gateway API + Ingress side by side; Istio mTLS/`istioctl analyze`; Cilium/Hubble
  flow-map; DNS and endpoint debugging

### 8b. AI & GPU workloads (DRA / Kueue / inference) — 🛠️ planned (Phase 3a)
- DRA-native views (`ResourceSlice`/`ResourceClaim`/`DeviceClass`), allocated-vs-actual
  GPU use, *"why isn't this job admitted?"*, Gateway API Inference Extension +
  `InferencePool`

### 9. Storage & data — 🛠️ planned (Phase 3a)
- PV/PVC/StorageClass/VolumeSnapshot, CSI driver status
- Velero/VolSync backup overview + **backup-gap report**, RPO/RTO per namespace
- A **CNPG lens**: primary/replica topology, replication lag, switchover/failover, PITR
- **KubeVirt as a first-class lens**: console, live migration, snapshots, MTV plans,
  instance types, hotplug

### 10. Workload lenses & extensions (data first, code second) — ✅ available
Declarative **view definitions** (YAML or CUE) that bind a CRD to panels, columns, status
inference, actions, and health checks — so Strimzi, KubeVirt, cert-manager, Keycloak,
Tekton, Velero, Karpenter, and Knative are supported *without hardcoding anything*, and
your teams can write their own for internal CRDs.

✅ Ships today: the schema, validator, status/condition rule evaluation, `render_row`,
the `extension.yaml` manifest + `list/validate/enable/disable` lifecycle, a lens set for
all eight targets under [`extensions/`](./extensions), and **lens-driven navigation** —
`kaptein lenses` discovers the set, `kaptein get --lens` renders one, and the **TUI
navigates discovered lenses** (drop a lens file into `KAPTEIN_EXTENSIONS_DIR`, default
`./extensions`, and its CRD becomes navigable with no recompile).

When a lens isn't enough, escalate to a **WASM plugin** (tier 2) or a **shell-out
integration** (tier 3). Data first, code second: **this is the only way "and more"
scales.** *(The WASM host + WIT worlds are 🛠️ planned, M2.6.)*

### 11. Fleet — 🛠️ planned (Phase 3a)
- **Fleet query**: one query, all clusters — Clusterpedia-class data layer, saved
  queries in Git, scheduled reports, **query-as-policy**; cross-cluster diff and drift
  matrix; optional hub mode with a per-cluster agent

### 12. Time machine — 🧪 preview / 🛠️ planned (Phase 3a)
- 🧪 *"What changed in the last N minutes"* ships today (`kaptein overview`/`events`, an
  in-memory watch ring + the events API)
- 🛠️ Persistent watch stream (redb/SQLite) with compaction and retention; scrub backwards,
  see a resource as it was, diff two timestamps — events + Git deploy markers on one
  timeline. **During an incident this alone is worth the whole tool.**

### 13. Incident & collaboration — 🛠️ planned (Phase 3b)
- Session recording → Markdown incident timeline; shared workspace configs in Git
- ✅ Full local audit log of every write operation *(ships today)*
- Operational memory (owner from labels/annotations, on-call, runbook from `runbook_url`)
- The incident timeline records what *you* did **and** what the *cluster* did

### 14. Cluster lifecycle, certificates & DR — 🛠️ planned (Phase 3b)
- Version matrix + EOL per cluster; control-plane health for on-prem (etcd size, defrag,
  leader elections, apiserver latency); certificate expiry across the fleet; backup-gap
  + RPO/RTO per namespace

---

## Non-functional requirements

- **One static binary** for the native and headless builds. No runtime dependencies.
  External tools that can't be embedded
  (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`, cloud billing CLIs) are
  invoked when present and degrade gracefully when absent — the core binary stays
  self-contained. The browser UI is a **wasm bundle** served by `serve`, not a binary.
- **No telemetry, no account, works in airgaps.**
- **Read-only default** for unknown contexts.
- **Signed releases with SBOM** — practice what we scan for. ✅ *Implemented: cosign
  keyless signatures, `SHA256SUMS`, a cosign-signed CycloneDX SBOM, and SLSA provenance
  on every release. The installer verifies the `SHA256SUMS` file against the OIDC
  identity and cosign-verifies the container image.*
- **Informer-based, not polling.** ✅ *Implemented for the TUI's live view (a bounded
  seed plus a reconnecting watch that relists-and-reconciles on reconnect feeds an
  in-memory data plane; no per-keystroke `api.list`). The ADR-0006 lifecycle policy —
  lazy per-view watches, LRU+TTL eviction, and a hard cap with degradation to on-demand
  list — is a validated, config-backed policy wired into `LivePlane`.*
- **Same keymap** in the TUI and GUI. 🛠️ *GUI not yet shipped.*
- **i18n** and a screen-reader-friendly GUI. 🛠️ *Planned.*
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

Only the crates that carry real structure exist now; the rest are split out when they
have code.

```
crates/
  kaptein-core/       # kube-rs client, watchers/reflectors, CRD discovery, stores
  kaptein-viewmodel/  # renderer-agnostic logic: columns, sort/filter, status, action graphs
  kaptein-tui/        # ratatui
  # future (split out when they have code): frontend-gui, serve, headless,
  # viewdef, plugins, ext-sdk
extensions/        # example extensions (lenses, plugins, integrations)
docs/
  adr/             # architecture decision records (see ADR-0001)
  architecture.md  # architecture overview
  INSTALL.md       # installation guide (cargo, install.sh, Krew, container, source)
  USAGE.md         # usage manual (CLI, TUI, MCP, config, lenses)
CONTRIBUTING.md    # contributing guide
SECURITY.md        # security policy & disclosure (the canonical threat model)
LICENSE            # BUSL-1.1 (source-available; converts to MIT on Change Date)
```

The full operator's manual — CLI reference, TUI keybindings, the governed MCP server,
configuration, and the lens/extension system — is in [`docs/USAGE.md`](docs/USAGE.md).
Installation instructions are in [`docs/INSTALL.md`](docs/INSTALL.md).

---

## Known limitations

**MVP — Phase 1, functional against a live cluster.** See [Status at a
glance](#status-at-a-glance) for the available/preview/planned breakdown, and
[`ROADMAP.md`](./ROADMAP.md) for the full phased plan and
[`ISSUES.md`](./ISSUES.md) for known issues.

Writes are **opt-in and gated**: `delete`, `scale`, `restart`, `cordon`, `uncordon`,
`evict`, and `debug` default to dry-run, require an explicit `--confirm`, and (for
prod/unknown contexts) a break-glass justification. Everything else is read-only.

**Limitations to read before relying on this** (all tracked):

- **Performance targets are not yet measured.** The budget in `ROADMAP.md` is a
  commitment, not a benchmark result; the kwok harness that will prove or disprove it is
  M1.8.

Fixed in the v0.27.0 re-audit (closed issues, kept here for context): log redaction
([#22](https://github.com/egkristi/Kaptein/issues/22)), MCP preflight pluralization
([#21](https://github.com/egkristi/Kaptein/issues/21)), watch reconnect relisting
([#20](https://github.com/egkristi/Kaptein/issues/20)), and the `force: true` write-path
guardrail ([#16](https://github.com/egkristi/Kaptein/issues/16)).

### Install

> **Looking for how to *use* Kaptein?** See [`docs/USAGE.md`](docs/USAGE.md) — the
> operator's manual with the full CLI reference, TUI keybindings, the governed MCP
> server, configuration, and the lens/extension system. (Installing? See
> [`docs/INSTALL.md`](docs/INSTALL.md).)

One static binary ships — the TUI, GUI, and headless agent are all projections of the
same view-model, invoked as subcommands (`kaptein tui`, `kaptein mcp`, …):

| Command | Purpose |
|---------|---------|
| `kaptein` | The CLI — scripting, one-shots, MCP server, extension lifecycle, **and** the TUI (`kaptein tui`). |

**Recommended — `cargo install` (CLI):** if you have a Rust toolchain (≥ 1.97), the
simplest way to get the CLI is the crate published on crates.io:

```bash
cargo install kaptein          # the CLI + TUI (one binary)
kaptein tui                    # launch the TUI
```

**Recommended — signed release (one binary, no Rust):** the install script downloads
the prebuilt, signed binary for your platform, verifies the SHA-256 checksum against
the release's `SHA256SUMS`, and cosign-verifies that file's signature against the GitHub
Actions OIDC identity, then installs to `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/egkristi/Kaptein/main/install.sh | bash
# pick a version / install dir:
KAPTEIN_VERSION=v0.30.0 KAPTEIN_INSTALL_DIR="$HOME/.local/bin" ./install.sh
```

Which to use: `cargo install` is the default for CLI-only users who already have Rust
(one standard command, version-pinned by crates.io, no signature verification to
configure). `install.sh` is the default when you want the verified signature chain
without a Rust toolchain — the path this project's security posture (`SECURITY.md`) is
built around.

Other install methods:

- **kubectl plugin (Krew)**: Kaptein is BUSL-1.1 (source-available), and Krew's central
  index is a CNCF project that requires plugins to be **open source under an OSI-approved
  license** — so it is not submitted to `kubernetes-sigs/krew-index`. Instead, Kaptein
  ships its own **custom index** and a standalone manifest on every release:

  ```bash
  # custom index (recommended):
  kubectl krew index add kaptein https://github.com/egkristi/krew-index.git
  kubectl krew install kaptein/kaptein

  # or, straight from the release asset (no index):
  kubectl krew install --manifest-url=https://github.com/egkristi/Kaptein/releases/latest/download/kaptein.yaml
  ```

  Both install a checksum-verified `kubectl kaptein` (see
  [#34](https://github.com/egkristi/Kaptein/issues/34) for the licensing rationale).
- **Container image**: `docker run ghcr.io/egkristi/kaptein get --gvk v1/Pod` — the
  release workflow builds a static image from the verified tarball, pushes it to GHCR,
  and cosign-signs the digest.
- **From source**: `cargo build --release` (see *Build & test* below).

Verify a downloaded artifact with cosign and checksums as described in
[`SECURITY.md`](./SECURITY.md#verifying-a-release). Every release ships cosign-signed
binaries, a `SHA256SUMS` file, a CycloneDX SBOM, and SLSA provenance.

### Build & test

```bash
cargo build --release

# Read path (read-only against your current kubeconfig context)
./target/release/kaptein get --gvk v1/Pod --namespace default
./target/release/kaptein get --gvk apps/v1/Deployment --namespace kube-system
./target/release/kaptein can --verb get --resource pods --namespace default
./target/release/kaptein context
./target/release/kaptein diagnose --name <pod> --namespace <ns>
./target/release/kaptein describe --gvk v1/Pod --name <pod> --namespace <ns>
./target/release/kaptein logs --name <pod> --namespace <ns> --tail 50
./target/release/kaptein events --namespace default --minutes 15
./target/release/kaptein overview --minutes 15
./target/release/kaptein config-validate
./target/release/kaptein config-explain-context --context <ctx>

# Governed MCP server (read-only; same guardrails as the CLI)
./target/release/kaptein mcp

# Gated write path (dry-run by default; --confirm required, break-glass on prod)
./target/release/kaptein scale --gvk apps/v1/Deployment --name <deploy> --replicas 3   # dry-run
./target/release/kaptein scale --gvk apps/v1/Deployment --name <deploy> --replicas 3 --confirm

# TUI (vim navigation; Tab switches kind, n cycles namespace, d describe, i diagnose)
./target/release/kaptein tui
```

Point `KUBECONFIG` at any cluster to test; read-only commands never mutate the cluster.

### Shell completions

`kaptein completions <shell>` emits tab-completion for the whole CLI (subcommands, flags,
and their arguments), generated from the same clap definitions the parser uses — so it
never drifts from the command surface:

```bash
# bash
kaptein completions bash > ~/.local/share/bash-completion/completions/kaptein
# zsh
kaptein completions zsh > "${fpath[1]}/_kaptein"
# fish
kaptein completions fish > ~/.config/fish/completions/kaptein.fish
# PowerShell
kaptein completions powershell > kaptein.ps1   # then dot-source it in $PROFILE
```

Supported shells: `bash`, `elvish`, `fish`, `powershell`, `zsh`.

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

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md) for the architecture rules and workflow, and
our [`CODE_OF_CONDUCT.md`](./CODE_OF_CONDUCT.md) for community standards.
