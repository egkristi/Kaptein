# Kaptein — *the console that knows what changed — and lets you fix it in Git*

> Website: <https://kaptein.io> · Source: <https://github.com/egkristi/Kaptein>

[![CI](https://github.com/egkristi/Kaptein/actions/workflows/ci.yml/badge.svg)](https://github.com/egkristi/Kaptein/actions/workflows/ci.yml)
[![CodeQL](https://github.com/egkristi/Kaptein/actions/workflows/codeql.yml/badge.svg)](https://github.com/egkristi/Kaptein/actions/workflows/codeql.yml)
[![Release](https://img.shields.io/github/v/release/egkristi/Kaptein)](https://github.com/egkristi/Kaptein/releases)
[![License: BUSL-1.1](https://img.shields.io/badge/license-BUSL--1.1-orange.svg)](LICENSE)

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

## The five things that are hard to find elsewhere

These are the differentiators that hit where daily work actually hurts:

1. **Governed MCP surface** — `kaptein mcp` lets AI agents drive Kaptein through the
   *same* guardrails as a human: RBAC preflight, context guardrails, read-only default,
   and break-glass, each agent running under its **own dedicated identity** (its own
   ServiceAccount and narrow RBAC) and landed in the same audit log with the *agent* as
   the actor. An agent never writes to the API server — it can only open a PR. This is
   the answer to "Shadow MCP": governed, auditable, scoped agent access (ADR-0010).

2. **GitOps write path, from the operator console** — you edit in the UI *where you
   already stand during an incident*, with live cluster state beside you; the tool
   figures out *which file in which repo* owns the resource (via Flux/Argo metadata),
   makes the change in a branch, and opens a PR — with diff at both manifest level *and*
   rendered level (`kustomize build` / `helm template`). You write to Git, not to the
   API server. (IDP portals like Backstage/Port offer self-service actions, but two
   clicks away from reality; Kaptein does it from the live operator console. And an
   agent's write path is the *same* PR path.)

3. **Time machine** — the watch stream is persisted locally; scrub backwards, see a
   resource as it was, diff between two timestamps, *"what changed between 14:20 and
   14:35"* — with events and deploy markers from Git on the same timeline.

4. **Fleet query + drift** — one query, all clusters: *"all Deployments without
   resource limits across 40 clusters"*. Cross-cluster diff and drift matrix. Saved
   queries in Git, scheduled reports, and **query-as-policy** (a query can fail CI) —
   the same data layer as drift detection (ADR-0011).

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
Lifecycle is managed with `kaptein extension {validate,list,enable,disable}`. Example
lenses ship under [`extensions/`](extensions/) — CNPG, Strimzi Kafka, KubeVirt,
cert-manager, Keycloak, Tekton, Velero, Karpenter, and Knative — all MIT/Apache-2.0.

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
- **Governed MCP surface** (`kaptein mcp`): AI agents drive Kaptein through the same
  guardrails as a human — never writing to the API server, only opening a PR (see the
  fifth differentiator and ADR-0010).
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
- **SBOM reconciliation**: run two generators, diff them, and show which package list you
  trust and why — the mismatch is the signal
- **VEX filtering**: CVE → *actually reachable* workloads, not a CVE dump
- **Framework mapping**: CIS, NSA/CISA, MITRE ATT&CK, NSM *Grunnprinsipper*, **plus
  CRA, NIS2, and DORA** control mappings — a European procurement trigger American tools
  systematically miss
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

### 8b. AI & GPU workloads (DRA / Kueue / inference)
- **DRA-native views**: `ResourceSlice`, `ResourceClaim`, and `DeviceClass` as
  first-class resources (GA in 1.34) — a niche no console renders well yet.
- **Allocated vs. actual GPU use, with honesty about the measurement**: DCGM Exporter
  cannot attribute metrics to individual containers under time-slicing, so Kaptein says
  so — the same "show the source, never the value" honesty as secrets.
- **"Why isn't this job admitted?"** — the sibling to "why isn't this pod ready?", over
  ClusterQueue quota, gang scheduling, preemption, and ResourceClaim binding.
- **Tokens, not just GPU percentage** — Gateway API Inference Extension and `InferencePool`
  as real resources to render, with TTFT as the north star.

### 9. Storage & data
- PV/PVC/StorageClass/VolumeSnapshot, expansion, CSI driver status
- Velero/VolSync backup overview with restore-test status, plus a **backup-gap report**
  (which workloads have *no* backup) and RPO/RTO per namespace
- A proper **CNPG lens**: primary/replica topology, replication lag, switchover/failover,
  backup to object store, PITR window, WAL archive status, pending restart on parameter
  changes. *This does not exist today.*
- **KubeVirt as a first-class lens** (not one bullet): console (VNC/serial), live
  migration status, snapshot/restore, MTV migration plans with wave progression,
  VM templates + instance types, hotplug disk/NIC, node placement and evacuation — the
  VM vocabulary vSphere admins need when they land on Kubernetes.

### 10. Workload lenses & extensions (data first, code second)
Declarative **view definitions** (YAML or CUE) that bind a CRD to panels, columns, status
inference, actions, and health checks. This is how Strimzi, KubeVirt, cert-manager,
Keycloak, Tekton, Velero, Karpenter, and Knative are supported *without hardcoding
anything* — and your teams can write their own for internal CRDs and check them into Git.

*Status:* the schema, validator, status/condition rule evaluation, `render_row`, the
`extension.yaml` manifest and its `list/validate/enable/disable` lifecycle, and a lens set
for all eight targets above ship today under [`extensions/`](./extensions) — exercised via
`kaptein viewdef validate|schema|render` and `kaptein extension`. **No frontend discovers
or displays a lens yet**: the TUI still navigates a fixed set of kinds, so the lens set is
proven as *data* but not as *navigation*. Closing that gap is the remaining half of M2.2.

When a lens isn't enough, escalate to a **WASM plugin** (tier 2) or a **shell-out
integration** (tier 3) — see *Extensibility* above. Data first, code second: **this is
the only way "and more" scales.**

### 11. Fleet
- **Fleet query**: one query, all clusters — Clusterpedia-class data layer, with saved
  queries in Git, scheduled reports, and **query-as-policy** (fail CI if a query returns
  rows)
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
- **Operational memory**: owner resolution from labels/annotations (incl. Backstage),
  on-call from PagerDuty/Opsgenie/Grafana OnCall, and runbook from `runbook_url` or a
  Git-backed markdown folder.
- The incident timeline records what *you* did **and** what the *cluster* did (deploys,
  scaling, node events, alerts) — an actual postmortem, not a command log.

### 14. Cluster lifecycle, certificates & DR
- **Version matrix and EOL** per cluster, with operator compatibility and PDB blockers
  before an upgrade
- **Control-plane health for on-prem**: etcd DB size, defrag, leader elections,
  apiserver latency
- **Certificate expiry across the fleet**: kubelet certs, cert-manager, webhook CA
  bundles, mesh CAs — "what expires in the next 90 days" across 40 clusters
- **Backup gap, not just backup status**: RPO/RTO per namespace as a DORA compliance hook

---

## Non-functional requirements

- **One static binary** for the native and headless builds. No runtime dependencies.
  External tools that can't be embedded
  (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`, cloud billing CLIs) are
  invoked when present and degrade gracefully when absent — the core binary stays
  self-contained. The browser UI is a **wasm bundle** served by `serve`, not a binary.
- **No telemetry, no account, works in airgaps.**
- **Read-only default** for unknown contexts.
- **Signed releases with SBOM** — practice what we scan for. *Implemented: cosign keyless
  signatures, `SHA256SUMS`, a cosign-signed CycloneDX SBOM, and SLSA provenance on every
  release. The installer does not yet check the signature it ships alongside — see
  [#24](https://github.com/egkristi/Kaptein/issues/24).*
- **Informer-based, not polling.** *Implemented for the TUI's live view (a bounded seed
  plus a reconnecting watch feeds an in-memory data plane; no per-keystroke `api.list`).
  The ADR-0006 lifecycle policy — lazy per-view watches, TTL eviction, a hard cap with
  degradation to on-demand list — exists as a validated, config-backed policy that is not
  yet consulted by the watch path
  ([#25](https://github.com/egkristi/Kaptein/issues/25)).*
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

Only the crates that carry real structure exist now; the rest are split out when they
have code.

```
crates/
  kaptein-core/       # kube-rs client, watchers/reflectors, CRD discovery, stores
  kaptein-viewmodel/  # renderer-agnostic logic: columns, sort/filter, status, action graphs
  frontend-tui/       # ratatui
  # future (split out when they have code): frontend-gui, serve, headless,
  # viewdef, plugins, ext-sdk
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

**MVP — Phase 1 functional against a live cluster.** The k9s-parity surface is
implemented and cluster-verified: resource listing (built-ins + CRDs), RBAC preflight,
context guardrails, diagnostics, describe (with secret redaction), logs, events,
port-forward, exec, ephemeral containers, and the governed MCP server (`kaptein mcp`) —
exposed through a CLI (`kaptein`) and a ratatui TUI (`kaptein-tui`).

Writes are **opt-in and gated**: `delete`, `scale`, `restart`, `cordon`, `uncordon`,
`evict`, and `debug` default to dry-run, require an explicit `--confirm`, and (for
prod/unknown contexts) a break-glass justification. Everything else is read-only.

See [`ROADMAP.md`](./ROADMAP.md) for the full phased plan and
[`ISSUES.md`](./ISSUES.md) for known issues; Phases 1b–3b are tracked as GitHub issues.

**Known limitations you should read before relying on this** (all tracked, all open):

- **Log output is not yet redacted.** Resource output is — `describe` and the MCP
  `describe` tool mask `Secret` values and sensitive-named fields — but `logs`,
  multi-pod logs, and `--follow` return raw lines, including through the MCP `logs`
  tool. Do not point an agent at logs you would not paste into a chat window
  ([#22](https://github.com/egkristi/Kaptein/issues/22)).
- **The governed MCP surface refuses many CRDs.** RBAC preflight guesses a resource
  plural that can differ from the one the request uses, and because preflight fails
  closed the call is rejected rather than allowed
  ([#21](https://github.com/egkristi/Kaptein/issues/21)).
- **The TUI can show stale rows after a watch expires** (~5 min): reconnect re-watches
  without relisting, so objects deleted during the gap linger until the view is rebuilt
  ([#20](https://github.com/egkristi/Kaptein/issues/20)).
- **Lenses are data, not navigation yet.** The lens engine and the shipped lens set
  validate and render via `kaptein viewdef`, but no frontend discovers or displays them
  — the TUI still lists a fixed set of kinds (M2.2).
- **Performance targets are not yet measured.** The budget in `ROADMAP.md` is a
  commitment, not a benchmark result; the kwok harness that will prove or disprove it is
  M1.8.

### Install

Prebuilt, signed binaries ship on every release, and the install script verifies them.
The fastest path (no `cargo` required — it downloads the binary, verifies its SHA-256
checksum against the release's `SHA256SUMS`, and cosign-verifies that file's signature
against the GitHub Actions OIDC identity, then installs to `~/.local/bin`):

```bash
curl -fsSL https://raw.githubusercontent.com/egkristi/Kaptein/main/install.sh | bash
# pick a version / install dir:
KAPTEIN_VERSION=v0.27.0 KAPTEIN_INSTALL_DIR="$HOME/.local/bin" ./install.sh
```

Alternatives:

- **kubectl plugin**: `kubectl krew install kaptein` — the release workflow renders
  `krew/kaptein.yaml` with the real tag and per-platform sha256 checksums.
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
./target/release/kaptein-tui
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
