# Changelog

All notable changes to Kaptein are documented in this file, kept in sync with releases
(see `docs/versioning.md`). The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **M2.0b — the live integration-test tier now covers the remaining write paths.**
  `crates/kaptein-core/tests/live.rs` grows from five to eight live tests: `restart`
  (asserts the `kube.kubernetes.io/restartedAt` annotation lands on the pod template),
  `evict` (dry-run + real evict against a throwaway standalone pod), and `exec` (runs
  `echo` in a running pod and asserts the output). This closes the "exec/restart/evict
  not unit-tested today" gap the milestone names. `cordon`/`uncordon` are deliberately
  excluded — they mutate a real node, violating the tier's never-touch-shared-cluster-state
  rule (they remain CLI-gated and documented).
- **M1.6 — `missing_resources`: detect missing CPU/memory requests/limits (ADR-0015).**
  A new `kaptein-core::diagnostics::missing_resources(pod)` predicate emits `no_requests`
  and `no_limits` for any app or init container that declares no
  `resources.requests.cpu`/`memory` (or limits). It is the cheap, metrics-free half of
  "missing limits/requests" that ADR-0015 split off from the Phase 3b recommendation
  work (Kaptein renders recommendations, it does not compute them) — four unit tests and
  two fixture-corpus tests (`no_requests.json`/`no_limits.json`) pin it. The **shipped
  path** is `kaptein overview`, which now lists "misconfigured pods (missing
  requests/limits)" alongside the existing "unhealthy pods" section.

## [0.30.1] - 2026-08-30

### Fixed
- **Re-audit findings (v0.30.0):**
  - **P** — `redact_line` recompiled two regexes (including the ~20-branch sensitive-key
    alternation) on every log line; now `LazyLock`ed (a follow stream no longer spends its
    time compiling regexes).
  - **R** — dynamic shell completion could hang on a blackholed endpoint; the
    cluster-querying completers are now wrapped in a 300 ms `tokio::time::timeout`.
  - **S** — `README.md`'s *Build & test* block still showed the removed
    `./target/release/kaptein-tui` binary; now `./target/release/kaptein tui`.
  - **Q** — `query_plane`'s doc comment claimed it avoided materializing the whole set,
    which it does not; corrected to state the actual behavior (the visible-window refactor
    remains M1.8).
- **Re-audit findings (v0.30.0, second pass) — governance:**
  - **U** — `kaptein exec` was the only mutating command with no gate and no audit trace
    (arbitrary code in a container, a full interactive shell with `--tty`, no
    `--confirm`, no `gate_write`, no `AuditEvent`). It now requires `--confirm` +
    `--break-glass` and emits an `Operation::Exec` audit record, matching `debug`.
  - **V** — `Operation::Exec` was emitted for the *debug* path (ephemeral attach),
    mislabelling it. A new `Operation::EphemeralAttach` is emitted for `kaptein debug`,
    and `Exec` now means exec, so the two are distinguishable in the audit log.
  - **W** — `Operation::PortForward` was defined but never emitted. Opening a
    port-forward (named or anonymous) and removing a named forward now write audit
    records.
  - **X** — deleted the superseded `KubernetesPlane` (an unused `DataPlane` impl with
    divergent semantics: unbounded `discovery::list`, `Revision(0)`, empty subscribe).
  - **Y** — clarified that `apply_patch_real` is pre-positioned for M2.3 with no caller
    yet (`kaptein apply`/`edit` are dry-run-only).
- **Re-audit findings (v0.30.0) — informer lifecycle (M2.0c):**
  - **M** — the informer cap was unreachable: the TUI's `rebuild_plane` called
    `new_plane(...)` (a constructor, not `clone`) on every view switch, so each plane got
    a fresh `InformerManager` holding exactly one watch and `max_watches` could never be
    reached. The TUI now holds **one session-scoped `InformerManager`** and passes it to
    every plane via `LivePlane::with_shared_informers`, so the cap is enforced over the
    process, not per plane.
  - **N** — `InformerManager::release`/`touch` had no callers. `watch_loop` now holds a
    `WatchSlotGuard` that releases the slot on exit (view close or task abort), so a
    session-scoped manager no longer leaks a slot per view switch.
  - **O** — `relist_and_reconcile` removed but never added. It now relists **full
    objects** (not metadata summaries) and upserts rows missing from the plane with a
    correct `status`, so objects created during a watch outage appear immediately.
  - **DoD test** — `shared_manager_cap_is_enforced_across_planes_and_released_on_close`
    drives distinct planes through a shared manager with a small cap and asserts both that
    the cap is reached (the M invariant) and that releasing a slot returns it (the N
    invariant).

### Added
- **M1.1 — a governance coverage test (derive, don't restate).** The set of governed CLI
  subcommands is derived from the clap command tree: any subcommand that declares
  `--confirm` must also declare `--break-glass`, and `Exec`/`EphemeralAttach`/
  `PortForward` are asserted to be distinct governed operations. This is the DoD that
  would have caught finding U — a new mutating command that forgets half its gate fails
  CI instead of shipping silently.
- **M1.8 — the benchmark now also measures steady-state RSS.** `benches/query.rs` holds
  the 50 000-row plane and reads `VmRSS` from `/proc/self/status`, gating it against the
  250 MB budget (measured ~16 MB; the RSS gate is Linux-only, the p99 latency gate still
  applies everywhere). The "steady-state RSS < 250 MB at 50 000 objects" budget is now
  measured, not aspirational.
- **M1b.3 — the diagnostic moat is now reachable from the CLI.** `blast_radius` and
  `why_is_job_pending` were previously exposed only through the MCP server. Two new CLI
  commands surface them: `kaptein blast-radius --gvk <gvk> --name <n> [-n <ns>]` (owners +
  dependents, cascade-delete impact) and `kaptein why-job-pending --name <n> -n <ns>`
  (Job conditions + pod diagnostics). They reuse `kaptein-core::moat` — the same engine
  the MCP tools call — so the moat is one implementation, two surfaces.
- **M1.7 — `Cell::Redacted` is now actually constructed.** The typed `Cell::Redacted`
  render-contract variant existed but was never produced — `render_row` mapped the
  `[REDACTED]` marker (from `kaptein-core::redact`) to a plain `Text` cell, so a frontend
  had to special-case the marker string. `render_row` now recognizes the marker and emits
  `Cell::Redacted`, and `cell_text` renders it as the `[REDACTED]` mask — a frontend
  renders a mask with no string comparison, and the lens-driven view reaches the `Row` as
  the typed variant (with a test pinning it).
- **M1.6 — init-container diagnostics.** `diagnose` only inspected `container_statuses`,
  so a pod stuck on a failed init container fell through to the generic `not_ready`.
  `diagnose` now inspects `init_container_statuses` and surfaces `init_container_error`
  (terminated non-zero) / `init_container_waiting` (waiting with a reason), checked before
  the scheduling reasons in the `Pending` branch — an `Init:Error` pod reads as "init
  container X failed". Added a `init_container_error.json` fixture + unit tests.

## [0.30.0] - 2026-08-28

### Changed
- **One static binary — the TUI is now `kaptein tui`, not a separate `kaptein-tui`**
  executable. `kaptein-tui` was converted to a *library* hosted by the `kaptein` binary
  (the "one static binary" thesis: TUI, GUI, and headless are all projections of the same
  view-model, invoked as subcommands — `kaptein tui`, `kaptein mcp`, …). `install.sh` and
  the release now ship a single `kaptein` binary; the legacy `frontend-tui` crates.io
  crate is yanked. `cargo install kaptein` gives you the CLI **and** the TUI.
- **The TUI quit key is now vim-style `:q`** (plus `:q!`, `:x`, and `:wq`), instead of
  the bare `q`. The `:` palette now matches the vim quit family exactly before the fuzzy
  fallback, so `:q` quits deterministically. `Esc` and `Ctrl-C` still quit. The status
  line, header doc, and `docs/USAGE.md` key table reflect the change.

### Fixed
- **`kaptein logs --name … --regex …` silently ignored the regex (#38).** The
  single-pod **non-follow** path (`pod_logs`) never applied `--regex` — the filter was
  only wired into the `--follow` streaming branch and the `--selector` (`multi_pod_logs`)
  branch. The single-pod `--json` path had the same gap. Both now compile and apply the
  regex filter, so all four log paths honor `--regex` consistently.

### Added
- **Dynamic (live-cluster) shell completion.** `kaptein completions <shell>` only emits
  static flags/subcommands; now the CLI also ships a dynamic completer
  (`clap_complete`'s `unstable-dynamic` `CompleteEnv` + `ArgValueCompleter`) that queries
  the cluster at completion time. Source `source <(COMPLETE=bash kaptein)` (or
  `zsh`/`fish`) to complete **namespaces** and **pod names** from the live cluster,
  **kubeconfig contexts** from the local kubeconfig, and **GVKs**/plural **resources**
  from a fixed built-in set. Completion degrades gracefully to no candidates when the
  cluster is unreachable (never an error or hang).

### Fixed
- **`kaptein get -l` was a silent flag collision with `--lens`.** `-l` was bound to
  `--lens` (a view-definition file path), so `kaptein get -l cnpg` tried to read a file
  named `cnpg` and failed with `cannot read cnpg: No such file or directory` — instead of
  the kubectl-conventional `-l`/`--selector` label filter the operator expected. `-l` is
  now `--selector` (a server-side label selector, e.g. `app=orders`), and `--lens` is
  long-only. Label selection is threaded through `discovery::list_with_selector`,
  `list_objects_with_selector`, and `list_metadata_bounded_with_selector`, so a `get -l`
  filters at the API server (matching `kubectl get -l`), across the summary, lens, and
  metadata paths.
- **`kaptein watch-store` hung forever (#36)** — it called `run_informer`, whose watch
  loop runs indefinitely, so the seed-then-snapshot code was unreachable and the command
  never printed or exited. `run_informer` now takes a `max_events: Option<usize>` bound,
  and `watch-store` gains `--max` (default 0 = seed-only), so it seeds, applies any
  requested deltas, and returns.
- **`kaptein exec` silently returned exit 0 with no output on remote failure (#37)** —
  `exec` awaited `AttachedProcess::join()` but never read the remote exit status, which
  kube delivers on the separate `take_status()` channel, so a command that failed inside
  the container (e.g. `echo` not found in a distroless image) was silently discarded.
  `exec` now reads `take_status()` and surfaces a `Failure` status (`NonZeroExitCode`,
  `InternalError`, …) as an error.

### Added
- **M1.8 — the query p99 budget is now measured, not aspirational**:
  `crates/kaptein-viewmodel/benches/query.rs` is a dependency-free, release-mode
  benchmark that drives `MemPlane::query` (sort + filter + window) over a 50 000-row
  synthetic plane and reports p50/p99/max over 200 iterations, exiting non-zero if p99
  exceeds an 8 ms budget. A `bench` job in `ci.yml` runs it and fails on regression.
  *(The kwok synthetic-cluster harness and end-to-end RSS/cold-start numbers remain the
  frontend-level Phase 1 tail.)*
- **M2.2 — per-action RBAC grey-out** (the `Forbidden` state was constructed but not
  preflight-driven): `semantic::action_verb` maps an action id to its RBAC verb
  (`describe`/`logs`/`exec`/`diagnose` → `get`, `scale`/`restart`/`delete` →
  `update`/`delete`, unknown → `get`); `downgrade_forbidden` downgrades an action to
  `Forbidden` (with the structured verb/resource/namespace) when preflight denies its
  verb. `kaptein-integration::preflight_actions` runs one `SelfSubjectRulesReview` for
  the target GVK (pluralized via kube's own pluralizer) and downgrades in place — the
  shipped path — and the TUI renders the forbidden marker and refuses the `d`/`i`
  bindings for a greyed-out action.

### Changed
- **`cargo install kaptein` is now the recommended CLI install path.** The `kaptein`
  crate is published to crates.io, so `cargo install kaptein` (CLI) and
  `cargo install kaptein-tui` (TUI) install the version-pinned binaries with one
  standard command. The signed-release `install.sh` path remains the recommended way to
  get **both** binaries with the verified cosign signature chain (no Rust required).
  `README.md` and `docs/USAGE.md` now document both as recommended, with a table of
  which binary each method installs.
- **The `frontend-tui` crate is renamed to `kaptein-tui`** so the crate name matches the
  `kaptein-tui` binary name — `cargo install kaptein-tui` now works (previously the
  crate was named `frontend-tui`, so `cargo install` could only find it under that
  name). This completes the `kaptein-*` naming started in ADR-0009 (the TUI was the odd
  one out). The ADRs (0009/0014) remain historical records of the earlier name.

## [0.29.0] - 2026-08-27

### Fixed
- **#34 — `kubectl krew install kaptein` (Distribution)**: Krew's central
  `kubernetes-sigs/krew-index` is a CNCF repo that requires plugins be open source under
  an OSI-approved license, so a BUSL-1.1 plugin is not eligible. Kaptein now ships a
  **custom Krew index** (`https://github.com/egkristi/krew-index`, `plugins/kaptein.yaml`
  with real version + per-platform sha256s) and a release-published `kaptein.yaml`.
  Install via `kubectl krew index add kaptein https://github.com/egkristi/krew-index.git
  && kubectl krew install kaptein/kaptein`, or directly via `kubectl krew install
  --manifest-url=https://github.com/egkristi/Kaptein/releases/latest/download/kaptein.yaml`.
  Both verified end-to-end against a real `krew` install.
- **#35 — lens-driven views leak secret values (High)**: the lens render path
  (`map_object_with`) serialized the object and fed it to `render_row` without redacting
  it, so a lens bound to a secret-shaped field (`data.password`, env `value`) reached the
  `Row` as plaintext. `map_object_with` now redacts the object before rendering, and a
  test asserts a Secret lens column yields the `[REDACTED]` marker, not plaintext.
- **`kaptein get` sort/filter help was misleading**: the help advertised `--sort kind` and
  `--filter ...kind`, but a single-GVK list has no `kind` column (every row is the same
  kind), so `--sort kind` silently no-oped. Corrected the help and sort mapping to
  `name`, `namespace`, `created` (filter: `name`/`namespace`/`status`).

### Added
- **Resource-pressure diagnostic** (M1.6): a Pending pod rejected for insufficient
  CPU/memory (`"N Insufficient cpu/memory."`) now surfaces as a distinct
  `resource_pressure` finding (resource name extracted), checked before the generic
  `unschedulable` fallback. Added a fixture (`resource_pressure.json`) to the corpus.
- **Landing view surfaces unhealthy pods** (M1.5 + M1.6): `kaptein overview` now lists
  pods that are not ready, with their diagnostics findings (crash-loop, image-pull, taint,
  PVC binding, etc.) — answering "is anything broken" directly rather than only via
  warning events. Degrades gracefully to events-only if pod listing is denied.
- **Taint/toleration diagnostic** (M1.6): a Pending pod rejected for an untolerated taint
  (`"N node(s) had untolerated taint {key=value: effect}"`) now surfaces as a distinct
  `taint` finding (with the taint extracted), checked before the generic `unschedulable`
  fallback. Added a fixture (`taint.json`) to the diagnostics corpus.
- **`blast_radius` supports cluster-scoped resources** (M1b.3): `namespace` is now optional
  — pass `None` (or omit it in the MCP tool) to compute the blast radius of a Node,
  Namespace, or cluster-scoped CRD. The dependents traversal lists controllers/pods
  cluster-wide for cluster-scoped targets.
- **Context switching parity across read *and* manifest commands** (M1.2): `describe`,
  `diagnose`, `logs`, `events`, `overview`, `watch`, `apply`, and `edit` now accept
  `--context` (matching `get`), so an operator can target any kubeconfig context without
  switching the whole session.
- **M1.8 — `MemPlane::query` no longer deep-clones the whole row set**: it now sorts and
  filters an index permutation (`sort_indices`/`filter_indices`, identical semantics to
  `sort_rows`/`filter_rows`) and clones only the windowed rows — a 50k-row view clones the
  visible window per query instead of 50k `Row`s. The TUI now carries `total` separately
  (`query_plane` returns `(rows, total)`), so the "N rows" status and `G`/`j` navigation
  use the true post-filter count, decoupled from the materialized window.
- **PVC-binding diagnostic** (M1.6): a Pending pod whose `PodScheduled` condition carries
  `persistentvolumeclaim "<name>" not found` now surfaces as `pvc_binding` (with the claim
  name extracted) — checked before the generic `unschedulable` fallback. Added a fixture
  (`pvc_binding.json`) to the diagnostics corpus.
- **OOM forensics rule** (M1.6 diagnostics): a distinct `oom_killed` finding detects a
  container killed by the kernel (reason `OOMKilled` or exit 137) from either the current
  `terminated` state or `last_state.terminated` after a restart — so a memory kill reads
  as a capacity signal, not a generic crash. Added a fixture (`oom_killed.json`) to the
  canonical diagnostics corpus.
- **Lens action graph (M2.2)**: `ViewDefinition::actions_as_semantic` maps a lens's
  declared `actions` into the render contract's `semantic::Action` (lens-native
  `allowed`/`gated`/`forbidden` → `ActionState`), and the TUI surfaces the selected
  resource's action graph — a lens-driven kind shows its declared action ids; built-ins
  show `describe, diagnose`. The action *id* maps to the existing bindings (`d`/`i`).
- **`blast_radius` walks the full ownership chain** (M1b.3 / M2.4 groundwork): the
  traversal is now generic over intermediate controllers (`ReplicaSet`, `Job`) rather
  than hardcoded to `Deployment → ReplicaSet → Pod`, so `StatefulSet → Pod`,
  `DaemonSet → Pod`, and `CronJob → Job → Pod` are all covered. A live integration test
  asserts a Deployment's dependents include both its ReplicaSet and its Pod.
- **M1.8 — sort comparisons are allocation-free** for the common columns: `cmp_cells`
  compares `Text`-vs-`Text` and `Status`-vs-`Status` by `&str` instead of cloning two
  `String`s per comparison (~1.7M allocations per 50k-row sort).

## [0.28.2] - 2026-08-27

### Fixed
- **#33 — release container cosign signing (complete)**: the `Publish container image`
  job failed to cosign-sign the pushed image for two reasons, both now fixed:
  1. it signed `steps.meta.outputs.digest`, but `docker/metadata-action` has no `digest`
     output (it is emitted by `docker/build-push-action`) — the build step now has
     `id: build` and is signed from `steps.build.outputs.digest`;
  2. the reference used the mixed-case `github.repository` (`egkristi/Kaptein`), but OCI
     repository names are lowercase (`ghcr.io/egkristi/kaptein`), so cosign's reference
     parse failed — the repo name is now lowercased (`${GITHUB_REPOSITORY,,}`) before
     signing.

## [0.28.0] - 2026-08-27

### Added
- **M2.2 — lens-driven TUI navigation**:
  - `LivePlane::new_lens` / `new_lens_with_policy` renders objects through a lens's
    `render_row`, so the lens's declared columns become the plane's schema and its status
    rules drive the status chip. The TUI discovers lens kinds at startup
    (`KAPTEIN_EXTENSIONS_DIR`, defaulting to `./extensions`) and navigates them with Tab —
    a lens file dropped into the path makes its CRD navigable with no recompile.
  - `core::discovery::list_objects_bounded` pages full objects for lens-driven views
    (ADR-0006); `core::extension::DiscoveredLens` now carries the resolved entrypoint so a
    frontend can load the full `ViewDefinition`.
  - `kaptein-integration::load_lens` is the shared load-and-validate path.
- **M2.2 — lens discovery (`kaptein lenses`)**:
  - `kaptein-core::extension::discover_lenses` walks configured extension paths, resolves
    each lens entrypoint's `target` into a `DiscoveredLens` GVK, and skips non-lens
    extensions; the `kaptein lenses` command prints the enabled lens set (honouring the
    `enable`/`disable` config), so the discovered lens set is queryable with no recompile.
- **M2.2 — the CLI consumes a lens (lens-driven `get`)**:
  - `kaptein get --gvk <gvk> --lens <file>` lists full objects
    (`core::discovery::list_objects`) and renders each through `render_row` — lens
    columns + lens-inferred status, the first real surface that consumes a lens (not
    just the `viewdef render` fixture path).
- **CLI — shell completions**: `kaptein completions --shell <bash|elvish|fish|powershell|zsh>`
  emits completions for the whole command surface (via `clap_complete`), so completions
  can never drift from the parser definitions.
- **M2.0c — informer lifecycle policy (ADR-0006)**:
  - `kaptein-core::informer::InformerManager` — lazy per-view `register`/`touch`/
    `release`, LRU `evict_idle` with TTL, and a hard cap that returns `Denied`
    (degrade-to-on-demand-list) instead of exceeding the cap.
  - The cap and idle TTL are now configurable via a new `[informer]` config section
    (`max_watches`, `idle_ttl_secs`), satisfying ADR-0006's "the cap must be a policy,
    exposed in the config file". `kaptein config validate` now bounds-checks them
    (`0` cap/ttl is flagged, not silently defaulted).
  - The ADR-0006 performance-budget link ("simultaneous watches ≤ N") is now
    *enforceable and regression-tested*: a fleet-scale test drives 4000 views through a
    16-watch cap and asserts it never exceeds N, plus a bookkeeping guard against an
    accidental O(n²) in the LRU scan.
- **Distribution & release sync (cross-cutting)**:
  - `install.sh` — checksum-verified install of the signed release binaries (no `cargo`
    required).
  - `krew/kaptein.yaml` — Krew plugin manifest for `kubectl krew install kaptein`.
  - `Dockerfile` — distroless static image built from the verified release tarball.
- **M2.0b — live integration-test tier**:
  - `crates/kaptein-core/tests/live.rs` exercises the real kube client (list, describe,
    the delete dry-run vs. real write path, the scale dry-run vs. real write path, and
    both `dry_run_apply` create and `dry_run_apply_patch` apply paths) against a
    cluster, self-cleaning in a throwaway namespace and gated on `KAPTEIN_LIVE_TESTS=1`
    so the default run stays hermetic.

### Fixed
- **#16 — `force: true` write-path guardrail made structural**: `dry_run_apply_patch`'s
  field-ownership `force` flag is now a parameter threaded through `apply_patch`, and the
  new real write path `apply_patch_real` always applies with `force: false` — it can never
  silently steal field ownership from Flux/Argo/GitOps. `apply_patch` refuses
  `force && !dry_run` outright, and a test asserts the real path never forces.
- **Re-audit findings #20–#32** (external re-audit of the v0.27.0 artifact, all fixed):
  watch reconnect now relists and reconciles (#20); MCP preflight pluralizes via kube's
  pluralizer (#21); logs are redaction-aware (#22); the Krew manifest is rendered at
  release time (#23); `install.sh` cosign-verifies `SHA256SUMS` (#24); `InformerManager`
  is wired into `LivePlane` (#25) with LRU admission (#26); the bounded list path is on
  the frontend seed (#27); the TUI re-queries only on a revision change (#28); Secret
  annotation redaction is narrowed (#29); dead `VERSION_TAG` removed (#30); the container
  image is published and signed to GHCR (#31); the dual sort/filter collapsed into the
  view-model and the layer rule is enforced in CI (#32).

## [0.27.0] - 2026-08-25

### Added
- **M2.2 — status-rule rendering (lens → render contract)**:
  - `kaptein-viewmodel::lens::render_row` maps a `ViewDefinition` + a resource into the
    render contract's `Row`/`Cell` — the "status-rule rendering" half of M2.2, shared by
    every frontend.
  - `Column.field` is a new data-binding on lens columns: a non-status column's value
    must come from an explicit dotted JSON path (ADR-0012 — the schema is no longer
    implicit); `validate_viewdef` enforces it. `Status` columns are still *inferred*.
  - `kaptein viewdef render -f <lens> -r <resource>` renders a lens against a live or
    fixture resource.

## [0.26.0] - 2026-08-25

### Added
- **M2.2 — condition-based lens rules + shipped lens set**:
  - `kaptein-viewmodel::lens::ConditionRule` — declarative Kubernetes-condition status
    inference (`status.conditions[]` by `type` + `status`), because Strimzi/KubeVirt/
    cert-manager/etc. signal readiness via typed conditions, not a bare phase.
  - `evaluate_status` now evaluates `conditions` rules after scalar `status` rules;
    `validate_viewdef` validates condition types/statuses; the lens JSON Schema gained a
    `conditions` property (with a drift-guard test).
  - Shipped the example lens set under `extensions/`: Strimzi Kafka, cert-manager
    Certificate, KubeVirt VirtualMachine, Keycloak, Tekton PipelineRun, Karpenter
    NodePool, Knative Service, Velero Backup (alongside CNPG) — all MIT/Apache-2.0.
- **Supply chain**: `publish.yml` is now idempotent (skips crates already at the tagged
  version) and gated on `cargo test`; `release.yml` cosign-signs the SBOM and generates
  SLSA provenance via `slsa-framework/slsa-github-generator`.

## [0.25.0] - 2026-08-25

### Added
- **M1.8 performance regression guard** — a 50k-row query benchmark-style test asserts
  sort+filter+window stays linear (fails loudly on accidental O(n²)), giving the perf
  budget its first CI signal before the kwok harness lands.

### Changed
- CI: advanced CodeQL workflow (`security-extended`, rust + actions) replaces the
  implicit GitHub default setup; clippy owns Rust quality linting.

## [0.24.0] - 2026-08-25

### Fixed
- **#17 — `kaptein edit` no longer round-trips `[REDACTED]`** — the edit path now
  fetches unredacted (via `describe_dynamic_policy`) and emits `Operation::SecretViewed`
  when editing a secret-kind resource; `describe` stays redacted by default.
- **#18 — bounded informer store wired + watch reconnect** — `store::watch_from`
  reconnects with backoff and relists on watch expiry/410; new `kaptein watch-store`
  gives `run_informer` its first caller outside tests.

### Changed
- Documented the `force: true` Phase 2 guardrail on `dry_run_apply_patch` (#16) so the
  M2.3 write path can't silently carry it into Flux/Argo ownership theft.

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
- (CI) Added `kaptein-integration` crate as the integration layer; `kaptein-tui` now
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
