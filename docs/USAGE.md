# Kaptein Usage Manual

This is the operator's manual for **Kaptein** — the unified Kubernetes workbench. It
covers the command-line interface (`kaptein`), the terminal UI (`kaptein-tui`), the
governed MCP server (`kaptein mcp`), configuration, and the extension/lens system.

For the *why* and the roadmap, see [`README.md`](../README.md) and
[`ROADMAP.md`](../ROADMAP.md). For known limitations, see [`ISSUES.md`](../ISSUES.md).

---

## 1. Installation

Two binaries ship:

| Binary | Purpose |
|--------|---------|
| `kaptein` | The CLI — scripting, one-shots, MCP server, and extension lifecycle. |
| `kaptein-tui` | The interactive terminal UI (the daily driver). |

**Recommended — `cargo install` (CLI):** if you have a Rust toolchain (≥ 1.97), the
simplest way to get the CLI is the crate published on crates.io:

```bash
cargo install kaptein          # the CLI
cargo install frontend-tui     # the terminal UI (separate crate)
```

`cargo install kaptein` installs only the CLI (the `kaptein` crate); the TUI is a
separate crate, `frontend-tui`. Both are version-pinned on crates.io, so you get the
same release as the tag.

**Recommended — signed release (both binaries, no Rust):** the install script downloads
the prebuilt, signed binaries for your platform, verifies the SHA-256 checksum against
the release's `SHA256SUMS`, cosign-verifies that file's signature against the GitHub
Actions OIDC identity, and installs to `~/.local/bin` (or `KAPTEIN_INSTALL_DIR`):

```bash
curl -fsSL https://raw.githubusercontent.com/egkristi/Kaptein/main/install.sh | bash
# pick a version / install dir:
KAPTEIN_VERSION=v0.29.0 KAPTEIN_INSTALL_DIR="$HOME/.local/bin" ./install.sh
```

Which to use: `cargo install` is the default for CLI-only users who already have Rust.
`install.sh` is the default when you want **both** binaries and the verified signature
chain (no Rust required).

Other install methods:

- **kubectl plugin (Krew)**: Kaptein is BUSL-1.1 (source-available), and Krew's central
  index requires plugins to be open source under an OSI-approved license — so it is not
  submitted to `kubernetes-sigs/krew-index`. Install from Kaptein's **custom index**, or
  directly from the release manifest:

  ```bash
  kubectl krew index add kaptein https://github.com/egkristi/krew-index.git
  kubectl krew install kaptein/kaptein

  # or, straight from the release asset (no index):
  kubectl krew install --manifest-url=https://github.com/egkristi/Kaptein/releases/latest/download/kaptein.yaml
  ```

  See [#34](https://github.com/egkristi/Kaptein/issues/34) for the licensing rationale.
- **Container image**: `docker run ghcr.io/egkristi/kaptein get --gvk v1/Pod`.
- **From source**: `cargo build --release` (requires a Rust toolchain ≥ 1.97).

Verify a download yourself (`cosign` must be installed):

```bash
cosign verify-blob \
  --certificate-identity "https://github.com/egkristi/Kaptein/.github/workflows/release.yml@refs/tags/<tag>" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --bundle SHA256SUMS.bundle SHA256SUMS
```

Shell completions are generated from the same parser the CLI uses, so they can never
drift from the command surface:

```bash
kaptein completions bash > ~/.local/share/bash-completion/completions/kaptein
kaptein completions zsh  > "${fpath[1]}/_kaptein"
kaptein completions fish > ~/.config/fish/completions/kaptein.fish
```

---

## 2. Concepts

### 2.1 GVK addressing

Kaptein addresses resources by `group/version/kind` (GVK) throughout — built-ins and
CRDs are handled uniformly via the discovery API. The `--gvk` flag accepts:

| Form | Meaning |
|------|---------|
| `v1/Pod` | Core group (`""`), version `v1`, kind `Pod` |
| `apps/v1/Deployment` | Group `apps`, version `v1`, kind `Deployment` |
| `postgresql.cnpg.io/v1/Cluster` | A CRD group |

### 2.2 Context guardrails (M1.1)

Kaptein classifies every kubeconfig context as `Prod`, `Staging`, or `Unknown`:

- **`Prod`** — read-only by default; writes require `--break-glass "<reason>"`.
- **`Staging`** — writes allowed without break-glass.
- **`Unknown`** — read-only by default (the safe fallback for anything unmatched).

Every mutating command (`delete`, `scale`, `restart`, `cordon`, `uncordon`, `evict`,
`debug`) is **dry-run by default** and requires an explicit `--confirm` to actually
apply. On `prod`/`unknown` contexts, a non-empty `--break-glass` reason is *also*
required — the gate is enforced **before** the request reaches the API server (defense
in depth with RBAC).

### 2.3 Secrets are masked by default (M1.7)

`describe` (and the MCP `describe` tool) redact `Secret` values and sensitive-named
fields before serialization. Logs are likewise redaction-aware. `kaptein edit` fetches
**unredacted** so you can edit real values, but that unmask is audited as
`SecretViewed`.

### 2.4 Read-only default

Unknown contexts are read-only. There is no path that writes to the API server without
going through the guardrail gate — see §2.2.

---

## 3. The CLI (`kaptein`)

Run `kaptein --help` or `kaptein <command> --help` for the authoritative, up-to-date
help. The sections below are the human guide; flags may gain new options between
releases.

### 3.1 Reading resources — `get`

```bash
# Pods (all namespaces), the default kind
kaptein get
kaptein get --gvk v1/Pod

# Deployments in one namespace, sorted + filtered
kaptein get --gvk apps/v1/Deployment --namespace kaptein --sort name
kaptein get --gvk v1/Pod --namespace prod --filter "api-" --descending

# Cluster-scoped kinds (no namespace)
kaptein get --gvk v1/Node
kaptein get --gvk v1/Namespace

# Metadata-only listing (bounded, cheap — for list-heavy views)
kaptein get --gvk v1/Pod --metadata

# Use a different kubeconfig context
kaptein get --context staging --gvk v1/Service
```

Sortable columns: `name`, `namespace`, `kind`, `created` (or `age`).

### 3.2 Describing — `describe`

```bash
kaptein describe --name my-pod --namespace default          # gvk defaults to v1/Pod
kaptein describe --gvk apps/v1/Deployment --name web -n prod
kaptein describe --gvk v1/Node --name node-1                # cluster-scoped: no -n
```

Secret values are masked by default (see §2.3).

### 3.3 Diagnostics — `diagnose`

```bash
kaptein diagnose --name crashy-pod --namespace default
```

Produces evidence-based findings (crash-loop backoff, image-pull backoff, unschedulable,
readiness failures, exit-0 jobs, etc.), not raw strings.

### 3.4 Logs — `logs`

```bash
# Tail one pod
kaptein logs --name my-pod --namespace default --tail 200
kaptein logs --name my-pod -n default --follow            # stream until Ctrl-C

# All pods matching a selector
kaptein logs --selector app=web -n prod --tail 50

# Regex filter + JSON parsing into typed columns
kaptein logs --name my-pod -n default --regex "ERROR"
kaptein logs --name my-pod -n default --json
```

### 3.5 Events & the landing view

```bash
kaptein events --namespace prod --minutes 30               # what changed recently
kaptein overview --minutes 15                              # is anything broken?
```

### 3.6 Watching — `watch` and `watch-store`

```bash
# Ring-buffer watch (in-memory, no persistence)
kaptein watch --gvk v1/Pod --namespace prod --max 50

# Informer-backed bounded store (ADR-0006)
kaptein watch-store --gvk v1/Pod --namespace prod --limit 500
```

### 3.7 RBAC preflight — `can` and `preflight`

```bash
kaptein can --verb delete --resource pods --namespace prod
kaptein can --verb create --resource clusters --group postgresql.cnpg.io -n default
kaptein preflight --resource deployments --group apps -n default   # whole action set
```

### 3.8 Context inspection

```bash
kaptein context                                              # current + its class
kaptein contexts                                             # every context
kaptein config-explain-context --context prod-eu-west       # why it classifies as it does
kaptein config-validate                                      # check your config.toml
```

### 3.9 Writes — dry-run by default

Every write command prints a **dry-run** result unless you pass `--confirm`. On
`prod`/`unknown` contexts you must *also* pass a `--break-glass` reason.

```bash
# Apply / edit (dry-run only — Kaptein never applies a manifest)
kaptein apply --file manifest.yaml
kaptein edit --gvk v1/ConfigMap --name my-config -n default   # opens $EDITOR

# Delete — dry-run, then real
kaptein delete --gvk v1/ConfigMap --name x -n default
kaptein delete --gvk v1/ConfigMap --name x -n default --confirm
kaptein delete --gvk v1/ConfigMap --name x -n default --confirm --break-glass "incident-123"
kaptein delete --gvk apps/v1/Deployment --name web -n prod --cascade orphan --confirm

# Scale
kaptein scale --name web --replicas 5 -n prod               # dry-run
kaptein scale --name web --replicas 5 -n prod --confirm --break-glass "scale-up"

# Restart (no dry-run exists — --confirm is required)
kaptein restart --gvk apps/v1/Deployment --name web -n prod --confirm --break-glass "..."

# Nodes
kaptein cordon --name node-1 --confirm
kaptein uncordon --name node-1 --confirm
kaptein evict --name my-pod -n default --confirm
kaptein drain --name node-1                                 # read-only preview, never evicts

# Ephemeral debug container
kaptein debug-containers --name my-pod -n default
kaptein debug --pod my-pod -n default --name debug --image busybox -- sleep 3600 --confirm
```

### 3.10 Exec & port-forward (read-only)

```bash
kaptein exec --pod my-pod -n default -- ls -la
kaptein exec --pod my-pod -n default --container sidecar -- cat /etc/hosts
kaptein exec --pod my-pod -n default --tty -- /bin/sh        # interactive

kaptein port-forward --pod my-pod -n default --port 8080
kaptein port-forward --pod my-pod -n default --port 5432 --local 15432
kaptein port-forward --pod my-pod -n default --port 80 --name web-dev   # persistent
kaptein port-forward-list
kaptein port-forward-remove --name web-dev
```

### 3.11 External tools — `krew`

```bash
kaptein krew                                  # list krew plugins
kaptein krew --tool kustomize -- build ./base # kustomize args
kaptein krew --tool helm -- list -A           # helm args
```

External tools degrade gracefully when absent — never a panic, a clear error message.

---

## 4. The TUI (`kaptein-tui`)

The daily driver: a ratatui table over cluster resources with vim navigation, a detail
pane, and lens-driven navigation.

```bash
kaptein-tui
KAPTEIN_EXTENSIONS_DIR=./extensions kaptein-tui   # control where lenses are discovered
```

### 4.1 Keys

| Key | Action |
|-----|--------|
| `j` / `k` or `↓` / `↑` | Move selection |
| `g` / `G` | Jump to top / bottom |
| `Tab` | Cycle resource kind (built-ins, then discovered lens kinds) |
| `n` | Cycle namespace |
| `s` / `S` | Cycle sort column / toggle direction |
| `/` | Fuzzy-jump filter (type to filter, `Enter` to accept, `Esc` to cancel) |
| `:` | Command palette (fuzzy-match commands, `Enter` to run, `Esc` to cancel) |
| `d` | Describe the selected resource |
| `i` | Diagnose the selected pod |
| `q` / `Esc` / `Ctrl-C` | Quit |

### 4.2 Lens-driven navigation (M2.2)

The TUI discovers **lens kinds** at startup. A lens is a declarative view definition
(`extension.yaml` + a lens YAML) that binds a CRD to columns, status inference, and
actions. Dropping a new lens file into the extension path (`KAPTEIN_EXTENSIONS_DIR`,
defaulting to `./extensions`) makes its CRD navigable **with no recompile** — press
`Tab` to cycle to it. The lens's declared columns become the table columns; its status
rules drive the status chip.

See §6 for how lenses work and how to author them.

---

## 5. The governed MCP server (`kaptein mcp`)

`kaptein mcp` exposes a **read-only, governed** Model Context Protocol server over
stdio, so AI agents can drive Kaptein through the *same* guardrails as a human.

```bash
kaptein mcp
```

### 5.1 Tools

| Tool | Description |
|------|-------------|
| `list_resources` | List resources of a `gvk` in a namespace (or cluster-wide). |
| `describe` | YAML-describe a single resource. |
| `logs` | Tail recent logs from a pod. |
| `get_events` | Recent events in a namespace (or all) within a window. |
| `diagnose` | Explain why a pod is not ready. |
| `explain_pod_failure` | Findings + related warning events (evidence-based). |
| `why_is_job_pending` | Analyze a stuck/pending Job. |
| `blast_radius` | Owners + dependents of a resource (cascade-delete impact). |
| `what_changed_between` | Events in a time window. |

### 5.2 Governance (ADR-0010)

- **RBAC preflight** runs before every tool call, against the *call's own* arguments
  (the right verb, resource, group, namespace).
- **Context guardrails** apply — the read-only default is enforced.
- **Agent identity** (ADR-0007): the server prefers an in-cluster ServiceAccount, then a
  `KAPTEIN_SA_TOKEN` bearer token, then the operator's kubeconfig — and *warns* (or
  refuses) when it falls back to the operator's own credentials. Set `KAPTEIN_AGENT` to
  name the actor in the audit log.
- **Audit log** records every tool call with the *agent* as the actor, the real target,
  and the outcome (`Rejected` for denied calls).
- **An agent never writes to the API server** — it can only open a PR (Phase 2 / M2.7).

### 5.3 Contract versioning

The server advertises its contract version and refuses a client whose declared
`_meta["io.kaptein/apiVersion"]` has a different major.

---

## 6. Extensions & lenses

Kaptein's extension model is **data first, code second** (ADR-0004). Three tiers:

| Tier | Kind | Loaded by |
|------|------|-----------|
| 1 | `lens` — a declarative view definition (YAML/JSON, no code) | The view-definition engine |
| 2 | `plugin` — a WASM component-model plugin (sandboxed code) | wasmtime host (M2.6) |
| 3 | `integration` — a shell-out to an external binary | the `krew` command |

Every extension is declared by an `extension.yaml` manifest, discovered from
Git-backed paths (no central marketplace), and can be enabled/disabled.

### 6.1 The extension manifest (`extension.yaml`)

```yaml
id: com.example.cnpg-lens
name: CNPG Cluster lens
version: 1.0.0
api_version: 1
kind: lens
entrypoint: lens.cnpg.yaml
permissions: []   # empty = default-deny (tiers 2/3)
```

### 6.2 A lens (view definition)

```yaml
id: com.example.cnpg-lens
api_version: 1
target:
  group: postgresql.cnpg.io
  version: v1
  kind: Cluster

columns:
  - id: name
    header_key: col.name
    kind: text
    sortable: true
    field: metadata.name
  - id: instances
    header_key: col.instances
    kind: number
    sortable: true
    field: spec.instances
  - id: status
    header_key: col.status
    kind: status
    sortable: true

status:
  - field: status.phase
    op: eq
    value: "ClusterIsReady"
    level: ok

conditions:
  - condition_type: Ready
    status: "True"
    level: ok

actions:
  - id: describe
    label_key: action.describe
    state: allowed
```

- **`columns[].field`** is a dotted JSON path (`spec.instances`, `metadata.name`); a
  non-status column *must* declare it (ADR-0012). A `status` column's value is
  **inferred** by the `status`/`conditions` rules.
- **`status`** rules match a scalar field against a value (`eq`, `ne`, `gt`, `gte`,
  `lt`, `lte`, `contains`).
- **`conditions`** rules match Kubernetes `status.conditions[]` by `type` + `status`
  (`True`/`False`/`Unknown`) — the shape most CRDs use to signal readiness.
- **`actions`** declare what a lens makes available, with their RBAC-preflight state.

Shipped lenses (under `extensions/`): CNPG, Strimzi Kafka, KubeVirt, cert-manager,
Keycloak, Tekton, Velero, Karpenter, Knative — all MIT/Apache-2.0.

### 6.3 Extension & lens lifecycle

```bash
# Discover + validate manifests in a directory
kaptein extension list -d extensions
kaptein extension validate -d extensions

# Enable / disable an extension by id (writes the disabled set to config)
kaptein extension enable com.example.cnpg-lens
kaptein extension disable com.example.cnpg-lens

# Lens discovery: the set of CRDs that are lens-navigable
kaptein lenses -d extensions
```

### 6.4 Lens authoring tools

```bash
# Validate a lens against the schema (reviewable in PRs)
kaptein viewdef-validate -f extensions/lens.cnpg.yaml

# Emit the versioned JSON Schema (for CI / PR review)
kaptein viewdef-schema

# Render a lens against a fixture or live resource (proves status inference)
kaptein viewdef-render -f extensions/lens.cnpg.yaml -r fixture.json
```

### 6.5 Lens-driven `get`

```bash
kaptein get --gvk postgresql.cnpg.io/v1/Cluster --lens extensions/lens.cnpg.yaml
```

Renders each object through the lens — lens columns + lens-inferred status instead of
the built-in four-column view.

---

## 7. Configuration

Kaptein reads a single TOML config file:

- `$KAPTEIN_CONFIG`, else
- `$XDG_CONFIG_HOME/kaptein/config.toml`, else
- `~/.config/kaptein/config.toml`

A missing or unparseable config never blocks startup and never weakens guardrails (the
default is `Unknown` = read-only for unmatched contexts). Validate with
`kaptein config-validate`.

```toml
# ~/.config/kaptein/config.toml

[guardrails]
# Context-name regexes → classification. First match wins (prod before staging).
prod    = ["^prod-", ".*-prod$"]
staging = ["^stag-", ".*-dev$"]

[extensions]
# Reverse-DNS ids of *disabled* extensions. Discovery finds every manifest; only
# non-disabled extensions are loaded.
disabled = ["com.example.experimental-lens"]

[informer]
# ADR-0006 watch lifecycle policy.
max_watches   = 16   # hard cap on simultaneous live watches (degrade-to-list beyond)
idle_ttl_secs = 300  # idle watches are evicted after this many seconds
```

`kaptein config-explain-context --context <name>` explains why a context classifies the
way it does; `kaptein config-validate` surfaces a bad regex instead of silently
degrading.

### Environment variables

| Variable | Purpose |
|----------|---------|
| `KAPTEIN_CONFIG` | Override the config file path. |
| `KAPTEIN_EXTENSIONS_DIR` | Where the TUI discovers lens extensions (default `./extensions`). |
| `KAPTEIN_AGENT` | Name the MCP agent actor in the audit log. |
| `KAPTEIN_SA_TOKEN` | Dedicated bearer-token identity for the MCP server (ADR-0007). |
| `KAPTEIN_VERSION` | Installer: which release tag to install. |
| `KAPTEIN_LIVE_TESTS` | `=1` to run the non-destructive live integration tests. |

---

## 8. Worked examples

### 8.1 "Why is this pod crashing?"

```bash
kaptein diagnose --name crashy-pod --namespace prod
kaptein logs --name crashy-pod --namespace prod --tail 100 --regex "panic|error"
kaptein describe --name crashy-pod --namespace prod
```

### 8.2 "What broke in the last 30 minutes?"

```bash
kaptein overview --minutes 30
kaptein events --namespace prod --minutes 30
```

### 8.3 "Can I even delete this?"

```bash
kaptein can --verb delete --resource deployments --namespace prod
kaptein preflight --resource deployments --group apps -n prod
# safe dry-run, then the real thing with a reason:
kaptein delete --gvk apps/v1/Deployment --name web -n prod --confirm --break-glass "incident-123"
```

### 8.4 "Make this CRD navigable in the TUI"

1. Write `extensions/my-crd/extension.yaml` and a lens YAML (see §6.2).
2. Validate: `kaptein viewdef-validate -f extensions/my-crd/lens.yaml`
3. Check discovery: `kaptein lenses -d extensions`
4. Run `kaptein-tui` and press `Tab` until your CRD appears — no recompile.

### 8.5 "Let an agent read the cluster, safely"

```bash
export KAPTEIN_SA_TOKEN="$(kubectl create token kaptein-agent -n kaptein)"
export KAPTEIN_AGENT="my-assistant"
kaptein mcp
# point your MCP client at stdio; the agent is read-only, RBAC-preflighted, and audited.
```

---

## 9. Testing & verification

- **Unit/contract tests** (renderer-agnostic, no cluster):
  `cargo test --workspace`
- **Live integration tests** (non-destructive, self-cleaning, gated on
  `KAPTEIN_LIVE_TESTS=1`): `crates/kaptein-core/tests/live.rs`
- **Quality gates** (run before committing):
  `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`

---

## 10. Versioning & compatibility

Kaptein is SemVer. Three contracts are versioned independently:

1. **WIT worlds** (WASM plugin interface — M2.6)
2. **Lens schema** (`api_version` on each lens; this release validates schema v1)
3. **MCP surface** (`io.kaptein/apiVersion`)

A release refuses an unsupported lens/WIT/MCP version with a migration error, never a
silent break. See [`docs/versioning.md`](./versioning.md) for the full policy and the
MSRV (currently pinned Rust `1.97.1`).
