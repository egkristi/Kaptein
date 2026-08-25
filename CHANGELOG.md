# Changelog

All notable changes to Kaptein are documented in this file, kept in sync with releases
(see `docs/versioning.md`). The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.23.0] - 2026-08-25

### Security
- **Secret leak via `metadata.annotations` fixed** — `redact_object` now masks a
  Secret's annotations map (incl. `kubectl.kubernetes.io/last-applied-configuration`,
  which `kubectl apply` embeds as a full plaintext copy). `ca.crt` removed from
  `SENSITIVE_KEYS` (a public cert, not a secret).
- **MCP agent-identity fallback now warns** — `agent_identity_resolution()` reports how
  the identity was resolved; the server warns loudly when it falls back to the operator's
  kubeconfig (or a name-only agent), instead of silently attributing agent actions to
  human credentials.

### Fixed
- **Events double-count** — `recent_events` dedups `core/v1` + `events.k8s.io/v1` on
  `(namespace, kind, name, reason, last_timestamp_ms)`.
- **MCP preflight hardcoding** — `preflight_target` derives (verb, resource, group,
  namespace) from the call's own `gvk`/`namespace` args; `describe(gvk=v1/Secret,
  ns=kube-system)` preflights `get secrets` in `kube-system`, not `get pods` in `default`.
- **MCP audit accuracy** — `get_events` records `Operation::List`; `target.group`/`kind`
  are split from the gvk instead of stuffing the whole string into `kind`.
- **MemPlane history replay lost deletions** — history records the full `RowPatch`
  (upsert AND remove), so reconnects don't resurrect deleted rows; `VecDeque` cap
  eviction; closed senders dropped (no per-subscriber leak).
- **TUI virtualization** — materializes only the visible window, not all 50k rows per
  frame.
- **Watch reconnect** — `LivePlane::watch_loop` relists (metadata-only) and reconnects
  with exponential backoff on watch expiry/410, instead of going silently stale.
- **Command palette `quit` actually quits.**
- **Diagnostics** — `ImagePullBackOff` no longer misreported as `crash_loop_backoff`;
  `image_pull` also fires for Running pods after an image update.

## [0.22.0] - 2026-08-25

### Added
- **M2.6 — extension enable/disable lifecycle** (ADR-0004):
  - `kaptein-core::config::Extensions` (disabled-id set) + `is_enabled` + `update_config`
    (load-mutate-persist to the XDG TOML config).
  - `kaptein extension enable/disable --id`; `extension list` now shows enabled/disabled
    state.

## [0.21.0] - 2026-08-25

### Added
- **M2.2/M2.6 — extension manifest + lifecycle** (ADR-0004):
  - `kaptein-core::extension`: `ExtensionManifest` + `validate_manifest` + `discover`
    (recursive `extension.yaml` walk with per-manifest problem reporting).
  - `kaptein extension list` / `kaptein extension validate`.
  - Example `extensions/extension.yaml` manifest for the CNPG lens.
- Versioned JSON Schema for view definitions (`extensions/viewdef.schema.json`) +
  `kaptein viewdef schema`, with a drift-guard test against `LENS_SCHEMA_VERSION`.

## [0.20.0] - 2026-08-25

### Added
- **M2.2 — view-definition (lens) schema + validation + evaluation** (data-first):
  - `kaptein-viewmodel::lens`: versioned lens data model (`ViewDefinition`,
    `GroupVersionKind`, `StatusRule`/`RuleOp`, `LensAction`) with `validate_viewdef`.
  - `evaluate_status`: resolve dotted field paths (with `[i]` subscripts) and apply the
    first matching status rule (`Eq`/`Ne`/`Gt`/`Gte`/`Lt`/`Lte`/`Contains`).
  - `kaptein viewdef validate -f` parses a lens (YAML/JSON) and reports every problem.
  - Example CNPG lens under `extensions/` (MIT/Apache-2.0, ADR-0004).
- Lens contract-version gate (`LENS_SCHEMA_VERSION` + `api_version` refusal).

## [0.19.0] - 2026-08-25

### Added
- **M2.0 complete** (review #3):
  - `kaptein-core::discovery::list_metadata_bounded` — `PartialObjectMetadata` path for
    list-heavy views (metadata-only, no full object bodies).
  - `kaptein-integration::LivePlane` — an informer-backed `DataPlane` with a real
    `subscribe`: seeds a `MemPlane` from a bounded list, then a background watch task
    applies `RowPatch` deltas keyed by `uid`.
  - `kaptein-integration::watch_event_to_patch` — `WatchEvent<DynamicObject>` → `RowPatch`.
  - The TUI renders from a live `LivePlane` subscription (no per-key `api.list`).
  - `kaptein get --metadata` exercises the bounded metadata-only path.
- Live-cluster integration test (`#[tokio::test]` gated on `KUBECONFIG`) exercising the
  real kube client.

### Changed
- `discovery::summary_of` is now `pub` (integration layer maps watch events to rows).

## [0.18.0] - 2026-08-25

### Added
- **M2.0 — wired the render contract + informer store** (review #3):
  - `kaptein-viewmodel::table`: renderer-agnostic sort/filter over `Row`/`Cell`.
  - `kaptein-viewmodel::mem_plane::MemPlane`: first concrete `DataPlane` (wasm-pure,
    revision, upsert/remove, bounded `query`, history-replaying `subscribe`).
  - `kaptein-core::store::InformerStore` + `run_informer`: bounded list-then-watch with
    a monotonic `StoreRevision`.
  - `kaptein-core::discovery::list_bounded`: server-side pagination (limit + continue).
  - `kaptein-integration::KubernetesPlane`: the `DataPlane` binding core → view-model.
  - The TUI now queries a `DataPlane` (render-intent) instead of `discovery::list_with`.
- Contract test: the same `Query` over a `DataPlane` yields the same `Page` across
  projections (TUI/GUI/headless share one render-intent).
- First `#[tokio::test]` (informer store concurrent writer/reader contract).

## [0.17.0] - 2026-08-24

### Added
- Config validation (`kaptein config-validate`) and `kaptein config-explain-context`:
  a typo in a prod guardrail regex is surfaced, not silently swallowed, and the operator
  can see why a context is classified the way it is (review #9).
- Diagnostics fixture corpus (review #11): canonical API-server pod JSONs
  (CrashLoopBackOff, exit-0 Job, ImagePullBackOff, unschedulable, probe failure, ready)
  fed through the rule engine as integration tests.
- MCP contract-version enforcement (review #12): the server refuses a client whose
  declared `_meta["io.kaptein/apiVersion"]` has a different major; the rule lives in
  `kaptein-viewmodel::versioned` (wasm-pure).
- Release supply-chain hardening (review #5): cosign keyless signing, a CycloneDX SBOM,
  and `SHA256SUMS` attached to every release.

### Changed
- CI layer-rule check now derives the frontend list from cargo metadata instead of a
  stale hardcoded list.

### Fixed
- README "Status" section corrected: the CLI has a gated write path (delete/scale/
  restart/cordon/uncordon/evict/debug), not "all commands read-only".
- ROADMAP "Immediate next steps" refreshed (M1.7 redaction and M1b.4 MCP governance are
  done; the open work is M2.0/M2.0b and the supply-chain items).

## [0.16.0] - 2026-08-24

### Added
- MCP governance per tool call (M1b.4, ADR-0010): RBAC preflight + context guardrail
  before every call; audit records the real outcome (Applied/Rejected), target, and
  per-instance session id.

### Fixed
- Secret masking/redaction: `kaptein describe` and the MCP `describe` tool now mask
  Secret `data`/`stringData` and sensitive-named fields before serialization.
- Diagnostics: exit-0 Job no longer misreported as `crash_loop`; CrashLoopBackOff
  detected via `last_state` (OOM forensics).
- RBAC preflight fails closed on absent rules; correct `{resource}/*` subresource pattern.
- `blast_radius` walks Deployment → ReplicaSet → Pod ownership.
- Port-forward establishes a fresh stream per connection (connection 2+ no longer fails).
- TUI: real pod status, fuzzy-jump backspace, `KeyEventKind::Press`, dynamic scroll,
  raw-mode restore on every exit.

## [0.15.0] - 2026-08-24

### Added
- Dedicated agent ServiceAccount identity for MCP (ADR-0007 mode 3):
  `kaptein-core::discovery::agent_client` prefers in-cluster SA, then
  `$KAPTEIN_SA_TOKEN`, then kubeconfig. Audit records carry the agent identity.

## [0.14.0] - 2026-08-24

### Added
- TUI command palette (`:` key): fuzzy-matched command list (next-kind,
  next-namespace, cycle-sort, toggle-sort, describe, diagnose, quit) — the last
  M1.2 item, completing resource-navigation parity.

## [0.13.0] - 2026-08-24

### Added
- Composed landing view (M1.5): `kaptein overview` now combines warning events
  (is anything broken) with the M1.4 watch-ring snapshot (what changed).

## [0.12.0] - 2026-08-24

### Added
- Diff before apply (M1.3): `kaptein edit` now shows a unified diff (live vs
  edited) before the dry-run result, via the dependency-free
  `kaptein-viewmodel::diff` module.

### Fixed
- `dry_run_apply_patch` now uses `force = true` so edits to resources created by
  other field managers (e.g. `kubectl create`) no longer fail with
  `FieldManagerConflict`.

## [0.11.0] - 2026-08-24

### Added
- In-memory watch ring buffer (M1.4): `kaptein watch --gvk X --max N` captures
  changes from the watch stream (informer-based, never polling) via
  `kaptein-core::watchring`.

## [0.10.0] - 2026-08-24

### Added
- JSON log parsing (`kaptein logs --json`): structured log lines parse into typed,
  inferred columns (M1.2 "JSON → columns") via the renderer-agnostic
  `kaptein-viewmodel::logparse` module.

## [0.9.0] - 2026-08-24

### Added
- Ephemeral containers (M1.2): `kaptein debug-containers` (list) and `kaptein debug`
  (attach, dry-run by default, break-glass gated) — kubectl debug-style profiling.
- Readiness-probe diagnostics rule (M1.6): a `Ready=False` condition with a
  `last_probe_time` surfaces as a `readiness_probe` finding.

### Changed
- (CI) Added `kaptein-integration` crate as the integration layer; `frontend-tui` now
  reaches `kaptein-core` through it (layer rule).
- (CI) `deny.toml` license allowlist + duplicate-version skip list updated; ratatui
  0.29→0.30 (drops unmaintained `paste`), crossterm 0.28→0.29.

## [0.8.0] - 2026-08-24

### Added
- `kaptein exec --tty` — interactive TTY exec: allocate a TTY and proxy
  stdin/stdout between the local terminal and the pod process (M1.2).
- Fuzzy jump (M1.2): a renderer-agnostic subsequence matcher
  (`kaptein-viewmodel::fuzzy`) with fzf-style scoring, wired into the TUI as
  `/` jump mode (type to re-rank rows, Enter to accept, Esc to cancel).

## [0.7.0] - 2026-08-22

### Added
- MCP diagnostic moat tools (M1b.3, ADR-0013): `explain_pod_failure`,
  `why_is_job_pending`, `blast_radius`, and `what_changed_between` — read-only
  tools backed by the M1.6 rule engine + Events API. MCP surface now has 8 tools.

## [0.6.0] - 2026-08-22

### Added
- TUI resource kinds expanded to Services + Nodes (k9s-parity "list
  pods/deployments/services/nodes" complete).
- `kaptein contexts` — list all kubeconfig contexts (name/cluster/user/current).
- `kaptein get --context X` — context switching for a single list.
- `kaptein preflight --resource X --group G --namespace N` — batch RBAC preflight
  over the standard 8-verb action set (grey-out support).

## [0.5.0] - 2026-08-22

### Added
- `kaptein cordon/uncordon --name N [--confirm]` — node schedulability toggles
  (dry-run by default, break-glass gate).
- `kaptein evict --name N --namespace X [--confirm]` — pod eviction (dry-run by
  default).
- `kaptein drain --name N` — read-only drain preview classifying pods on a node
  as evictable vs. skipped (DaemonSet/mirror/unmanaged).
- `kaptein krew [--tool krew|kustomize|helm] [-- args...]` — external-tool
  shell-out with graceful degradation (never panics when a tool is absent).
- Named, persistent port-forwards: `kaptein port-forward --name N` (auto-reconnect),
  `kaptein port-forward-list`, `kaptein port-forward-remove`.

## [0.4.0] - 2026-08-22

### Added
- `kaptein scale --gvk X --name Y --replicas N [--confirm]` — scale via the scale
  subresource, server-side dry-run by default (M1.2 k9s parity).
- `kaptein restart --gvk X --name Y --confirm` — rollout restart via the
  `kube.kubernetes.io/restartedAt` annotation (kubectl rollout restart equivalent).
- `kaptein logs --name X --follow` — follow log streaming (kubectl logs -f), with
  optional regex filter (M1.2).
- `kaptein get --sort <col> --descending --filter <substr>` — column sort (name/
  namespace/kind/created) and case-insensitive substring filter (k9s parity).
- `kaptein edit --gvk X --name Y` — `$EDITOR` handoff: fetch YAML, edit, dry-run the
  result (never applies) with server-managed field stripping (M1.3).
- Prod/unknown-context break-glass gate: writes require `--break-glass <reason>`
  unless the context is classified `staging` (M1.1).

### Changed
- CLI write operations (scale/delete/restart) now emit `AuditEvent`s with operation,
  target, outcome (Applied/DryRun), and break-glass reason (ADR-0010).
- MCP server attributes agent identity from `$KAPTEIN_AGENT` and records the real
  context name in the audit `context` field (ADR-0007, Phase 1b).

## [0.3.0] - 2026-08-22

### Added
- `kaptein events --minutes N` — recent cluster events (M1.4), the cheap form of the
  time-machine differentiator (no persistence).
- `kaptein overview --minutes N` — the landing view (M1.5): "is anything broken" +
  "what changed recently" (k9s Pulses equivalent).
- `kaptein apply --file X` — server-side dry-run validation (M1.3); never mutates the
  cluster, returns the server-validated object or the admission/validation rejection.
- `kaptein port-forward --pod X --port N --local M` — bridge a pod port to a local
  TCP listener (M1.2), read-only.
- `kaptein exec --pod X -- cmd...` — one-shot command execution with concurrent
  stdout/stderr streaming (M1.2).
- `kaptein delete --gvk X --name Y [--cascade] [--confirm]` — delete with explicit
  cascade selection, dry-run by default (read-only-default guardrail).

### Changed
- Enabled kube `ws` feature for port-forward/exec transport.

## [0.2.0] - 2026-08-22

### Added
- Governed MCP server (`kaptein mcp`) — read-only Model Context Protocol server over
  stdio (the #1 differentiator), exposing `list_resources`, `describe`, `logs`, and
  `diagnose` tools through the same guardrails as the CLI (ADR-0010, ADR-0013).
- MCP audit-log integration — every tool call writes a JSONL `AuditEvent` with agent
  identity (`source=mcp`), via `KAPTEIN_AUDIT`.
- Read-operation audit variants (`List`, `Describe`, `Logs`, `Diagnose`).
- TUI resource-kind switching (pods/deployments/namespaces), namespace cycling, status
  column, and an in-app detail pane (describe + diagnose).

### Changed
- README status promoted to MVP with concrete build/test instructions.

## [0.1.0] - 2026-08-21

First release with a functional core against a live cluster: generic resource listing,
RBAC preflight, context guardrails, diagnostics, describe, logs, and a TUI table view.
