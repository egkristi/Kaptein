# Issues & known limitations

This file tracks **known issues and limitations** that are not yet fixed, plus the
external-review backlog. GitHub issues are the source of truth for active work; this file
is the durable, version-controlled summary for contributors who read the repo, not the
tracker.

> Convention: a bug that exists in the shipped artifact is a **GitHub issue** (numbered,
> with a repro). A limitation by design is documented here with its owning milestone.

---

## Fixed in this review cycle

| Area | Finding | Fix |
|------|---------|-----|
| Security | `kaptein describe`/MCP `describe` emitted plaintext Secret values | `kaptein-core::redact` masks `Secret` data + sensitive-named fields before serialization |
| Security | Secret `metadata.annotations` (incl. `last-applied-configuration`) leaked plaintext | `redact_object` masks a Secret's annotations map |
| Diagnostics | exit-0 Job misreported as `crash_loop` | `container_crash_finding` skips exit 0 |
| Diagnostics | CrashLoopBackOff pod reported as `container_not_ready` | new `crash_loop_backoff` reads `last_state.terminated` |
| Diagnostics | `ImagePullBackOff` misreported as `crash_loop_backoff` (substring) | matches only `CrashLoopBackOff`/`BackOff`; `image_pull` also fires for Running pods |
| Events | every event reported twice (core/v1 + events.k8s.io/v1) | dedup on `(namespace, kind, name, reason, ts)` |
| RBAC | fail-open on absent `api_groups`/`resources` | fail **closed** |
| RBAC | subresource pattern `*/{resource}` backwards | `{resource}/*` |
| MCP | preflight hardcoded `pods`/`default` (ignored the caller's `gvk`/`namespace`) | preflight derives (verb, resource, group, ns) from the call's own args |
| MCP | `get_events` audited as `Operation::Logs`; `target.group` always empty | `List` op; gvk split into (group, kind) |
| MCP | agent identity silently fell back to the operator's kubeconfig | `agent_identity_resolution()` + startup warning |
| Moat | `blast_radius` empty for Deployments | walks Deployment → ReplicaSet → Pod |
| Port-forward | second connection half-closed (reused stream) | fresh upstream stream per connection |
| TUI | fabricated status column | real pod phase from `ResourceSummary.status` |
| TUI | fuzzy jump destroyed filtered rows on backspace | master list preserved |
| TUI | double keystrokes on Windows | `KeyEventKind::Press` filter |
| TUI | hardcoded 10-row scroll | scroll follows terminal height |
| TUI | raw mode left broken on error | cleanup on every exit path |
| TUI | materialized every row (50k) per frame | only the visible window is materialized |
| TUI | command palette `quit` didn't quit | returns a signal that breaks the loop |
| MemPlane | history replay lost deletions (resurrected removed rows) | history records full `RowPatch` |
| MemPlane | `Vec::remove(0)` O(n) at cap | `VecDeque` |
| MemPlane | sender leak per subscribe | closed senders dropped |

## Known issues (live bugs)

Per the convention above, live bugs are GitHub issues.

**Open:** #34 — `kubectl krew install kaptein` from the *central* index (a CNCF repo
that requires OSI-approved open source, which BUSL-1.1 is not). Resolved in practice by
a **custom Krew index** — see the Distribution backlog below and
`https://github.com/egkristi/krew-index`.

**Closed since the last cycle:** #35 (lens-driven views leaked secret values — the lens
render path `map_object_with` serialized the object and fed it to `render_row` without
redacting it, so a lens bound to a secret-shaped field reached a `Row` as plaintext; now
redacts the object before rendering, with a test asserting the `[REDACTED]` marker),
#33 (release container cosign signing failed — two root causes: the digest was read from
`metadata-action` (which has no `digest` output) instead of `build-push-action`, and the
mixed-case `github.repository` (`egkristi/Kaptein`) was used where OCI repository names
are lowercase; fixed by signing `steps.build.outputs.digest` with the repo name
lowercased, verified in v0.28.2), #16 (`dry_run_apply_patch force:true` must not carry
into the Phase 2 write path — the force flag is now a parameter, `apply_patch` refuses
`force && !dry_run`, and the real write path `apply_patch_real` always applies with
`force: false`; a test asserts the real path never forces), #17 (`kaptein edit` redaction
round-trip — fixed by `RedactionPolicy::Unredacted` + `SecretViewed` audit, commit
42ce99f), #18 (watch reconnect — `LivePlane::watch_loop` now reconnects with backoff
and `run_informer` has a caller, commit 59b421a), #36 (`kaptein watch-store` hung forever
— it called `run_informer`, whose watch loop runs indefinitely, so the seed-then-snapshot
code was never reached; `run_informer` now takes a `max_events` bound, and `watch-store`
gains `--max` (default 0 = seed-only), so it seeds and returns), and #37 (`kaptein exec`
silently returned exit 0 with no output when the remote command failed inside the
container — e.g. `echo` not found in a distroless image — because `AttachedProcess::join`
does not carry the remote exit status, which arrives on the separate `take_status`
channel; `exec` now reads that channel and surfaces a `Failure` status). See *Re-audit
findings* below for what those fixes did **not** cover.

### Re-audit findings (v0.27.0) — all fixed (issues #20–#31)

Found by an external re-audit of the shipped v0.27.0 artifact. Every finding is now
**fixed and closed** (commits 9645c58 → 81fea67). The table records the fix for history;
the milestone column names where the fix landed.

| # | Issue | Severity | Finding (fixed) | Owner |
|---|-------|----------|-----------------|-------|
| A | #20 | High | Reconnect never relisted → ghost rows. `watch_loop` now relists + reconciles on every reconnect. | M2.0c |
| B | #25 | High | `InformerManager` had no caller. Now wired into `LivePlane` (shared `Arc`, config-driven policy, degrade-to-list on `Denied`). | M2.0c |
| C | #26 | Medium | LRU admission implemented (`register` evicts the least-recently-used entry when the cap is full). | M2.0c |
| D | #23 | High | Krew manifest now rendered at release time (tag + real sha256s); CI asserts placeholders. | Distribution |
| E | #24 | High | `install.sh` now cosign-verifies `SHA256SUMS` against the OIDC identity (degrades with a warning if cosign absent). | Distribution |
| F | #21 | Medium | MCP preflight pluralizes via `ApiResource::from_gvk` (kube's pluralizer). | M1b.4 |
| G | #22 | Medium | Logs redacted via `redact::redact_line` in `pod_logs`/`multi_pod_logs`/`follow_logs`. | M1.7 |
| H | #27 | Medium | `LivePlane::seed` now pages through `list_bounded` (bounded frontend path). | M2.0 |
| I | #28 | Medium | TUI re-queries the plane only on a revision change (no per-frame 50k clone+sort). | M1.8 |
| J | #29 | Low | Secret annotation redaction narrowed to data-embedding + sensitive keys. | M1.7 |
| K | #30 | Low | Dead `VERSION_TAG` removed from `install.sh`. | Distribution |
| L | #31 | High | Release workflow now builds, pushes, and cosign-signs `ghcr.io/egkristi/kaptein`. | Distribution |

Together, D, E, and L mean **all three advertised install paths in `README.md` are either
unverified or non-functional**: the script checks integrity but not authenticity, the Krew
manifest is a placeholder template, and the container image does not exist. Treat that
cluster as one release-blocking item, not three cosmetic ones.

## Remaining review backlog (owned by milestones)

The external review ranked these; they are now **milestones in `ROADMAP.md`** rather than
unowned debt. Done items are struck through.

1. **M1b.4 — MCP governance conformance** — *mostly done, one gap.* (commit 1cbe417):
   RBAC preflight + context classification + read-only guardrail run per tool call; audit
   emits `Outcome::Rejected`, real `target`, real `session_id`, post-execution outcome.
   **Open: #21** — the preflight's kind→plural guess disagrees with the plural the request
   uses, and because RBAC fails closed the governed surface *refuses* most CRDs.
2. **M2.0 — wire `DataPlane` + informer store** — *re-opened.* (commits ad1cb5b → 13d8aae):
   `MemPlane` + `table` (view-model DataPlane), `InformerStore`/`run_informer` +
   `list_metadata_bounded` (core), `KubernetesPlane`/`LivePlane` (integration), the TUI
   renders from a live informer-backed `DataPlane`, and a live `#[tokio::test]` exercises
   the real kube client when `KUBECONFIG` is present. **Open: #27** — the DoD requires the
   *shipped frontend path* to use the bounded store, and `LivePlane::seed` still calls the
   unbounded `discovery::list`. Giving `run_informer` a CLI caller closed #18 but did not
   satisfy this half of the DoD.
3. **M2.0b — integration-test tier + platform CI matrix**: kind/envtest + Windows/macOS +
   latest-three-minors conformance. *Windows/macOS test matrix added to CI; the
   kind/envtest tier and Kubernetes-minor conformance remain open. A live integration-test
   tier (`crates/kaptein-core/tests/live.rs`, gated on `KAPTEIN_LIVE_TESTS=1`) now
   exercises the read path and the delete write path against a real cluster.*
3b. **M2.0c — watch resilience & informer lifecycle** *(added by the v0.27.0 re-audit)*:
   relist-on-reconnect, and the ADR-0006 lifecycle policy actually enforced.
   *`InformerManager` landed with a config-backed `[informer]` policy.* **Open: #20**
   (reconnect re-watches without relisting → ghost rows), **#25** (the manager has no
   caller, so the cap is policy without enforcement), **#26** (it is TTL-only despite the
   "LRU + TTL" name).
4. **M1.8 — kwok performance harness**: the performance budget is measured, not
   aspirational. **Related: #28** — `query_plane` still requests 50 000 rows at ~10 Hz and
   `MemPlane::query` clones-and-sorts the whole set per frame; that is the hot spot the
   harness will trip over first. *Landed (v0.29.0 →): the view-model half is measured —
   `benches/query.rs` drives `MemPlane::query` over 50k rows and gates p99 <8 ms via a
   `bench` CI job. Remaining: the kwok synthetic-cluster harness and end-to-end
   RSS/cold-start numbers.*
5. ~~**Signed releases + SBOM**~~ (done, commit eba14d9 + SLSA provenance in
   `.github/workflows/release.yml`): cosign keyless + CycloneDX SBOM + SHA256SUMS, the
   SBOM is cosign-signed, and SLSA provenance is generated per release.
6. ~~**Config schema/precedence/validation**~~ (done, commit 29cc1e6): `kaptein config validate`
   / `explain-context` flag invalid guardrail regexes and explain classification.
7. ~~**Redaction-aware error boundary**~~ (done, commit 98a39cc): `kaptein-integration`
   maps core errors into a real enum instead of passing through.
8. **Distribution & release sync** (cross-cutting): Homebrew/Krew/container/checksums +
   site/docs/tag sync. *Landed as files: `install.sh`, `krew/kaptein.yaml`, and
   `Dockerfile`.* **Resolved (v0.27.0 re-audit → v0.29.0):** #24 (installer cosign-verifies
   `SHA256SUMS` against the OIDC identity), #23 (release-time manifest rendering), finding L
   (#31, container image pushed + cosign-signed), #30 (dead `VERSION_TAG`). **Krew install
   works end to end** via a **custom index** — `kubectl krew index add kaptein
   https://github.com/egkristi/krew-index.git && kubectl krew install kaptein/kaptein`, or
   `kubectl krew install --manifest-url=https://github.com/egkristi/Kaptein/releases/latest/download/kaptein.yaml`.
   The **central** `kubernetes-sigs/krew-index` is a CNCF repo that requires OSI-approved
   open source, so a BUSL-1.1 plugin is not eligible (issue #34 stays open as the
   license-blocked central-index PR; the custom index is the shipped resolution). *Remaining:
   a Homebrew tap and a release-triggered site/README version bump.*
9. ~~**Contract-version enforcement**~~ (done, commit 485045c): MCP server advertises the
   contract version and refuses a client whose declared `_meta["io.kaptein/apiVersion"]`
   has a different major; rule in `kaptein-viewmodel::versioned` (lens/WIT gates land
   with their engines in M2.2/M2.6).
10. ~~**Events API v1 + scoped queries**~~ (done, commit 98a39cc): `recent_events` reads
    `events.k8s.io/v1` `eventTime`/`series` and merges with `core/v1`. (Field-selector
    scoping for list-heavy views remains part of M2.0.)
11. ~~**Diagnostics fixture corpus**~~ (done, commit 1ab7e6b): canonical pod JSONs
    (crashloop_backoff, exit_zero_job, image_pull_backoff, unschedulable,
    readiness_probe, ready) with expected findings as integration tests.

## By-design limitations

- **PVC-binding diagnostics** now detect a `persistentvolumeclaim "<name>" not found`
  message in the `PodScheduled` condition (surfacing as `pvc_binding` with the claim name
  extracted). A *full* PVC analysis (storage class, provisioner, volume topology) is
  deferred to a Phase 3a rule pack.
- **`blast_radius`** walks the ownership chain generically now: the intermediate
  controllers that can be owned by a workload and own Pods (`ReplicaSet`, `Job`) are
  listed and matched transitively, so `Deployment → ReplicaSet → Pod`,
  `StatefulSet → Pod`, `DaemonSet → Pod`, and `CronJob → Job → Pod` are all covered.
  A full cross-kind topology scan (volumes, selectors, RBAC) is a Phase 3a fleet feature.
- **`dry_run_apply_patch` uses `force: true`** — correct for dry-run, and now
  *enforced* (issue #16): the flag is a parameter, `apply_patch` refuses `force && !dry_run`,
  and the real write path (`apply_patch_real`) applies with `force: false` so it cannot
  silently steal field ownership from Flux/Argo.
- **OIDC token forwarding** (ADR-0007 mode 1) is a `serve`/hub-mode concern (Phase 2),
  not the MCP stdio server.
- **Lens-driven navigation landed, but the TUI's lens *rendering* is the built-in table
  geometry.** The M2.2 engine (`kaptein-viewmodel::lens`) and the shipped lenses under
  `extensions/` are now *navigable*: `kaptein lenses` discovers the enabled lens set, and
  the TUI discovers lens kinds at startup (`KAPTEIN_EXTENSIONS_DIR`, defaulting to
  `./extensions`) so a lens file dropped into the path makes its CRD navigable with no
  recompile — the lens's declared columns become the table's columns and its status rules
  drive the status chip. The remaining M2.2 refinement is the *falsifiable* DoD test
  asserting a lens-declared column reaches a `Row` through the live data plane (added in
  `kaptein-integration`), and per-lens action/health surfaces (M2.4+).
- **`Cell::Redacted` and `Operation::SecretViewed` have exactly one producer each.**
  `SecretViewed` is emitted by `kaptein edit`'s unredacted fetch; `Cell::Redacted` is still
  only pattern-matched (`table::cell_text`), never constructed, because no surface has an
  unmask-in-place affordance. M1.7 keeps that bullet open deliberately. *(The lens render
  path no longer leaks: `map_object_with` redacts the object before `render_row`, so a
  lens bound to a secret field reads the `[REDACTED]` marker — issue #35.)*

## Hygiene notes

- `core` / `core.*` dumps are git-ignored; if a process crashes to a core dump in the
  working tree, find and fix the crashing process rather than committing around it.
  **Two dumps (~240 MB) were present again during the v0.27.0 re-audit** — the ignore rule
  is doing its job, but something is still crashing repeatedly and nothing tracks what.
  Worth one session with `coredumpctl`/`gdb` to identify the binary before the next
  release.

## Audit provenance

The tables above record findings from successive external audits of the shipped artifact.
When closing one, prefer a **falsifiable** DoD in `ROADMAP.md` over a checkbox here: the
recurring pattern across three cycles has been a milestone that a partial implementation
satisfies literally (bounded-list code that exists but is not on the frontend path; a
policy manager with no caller; a signed release whose own installer skips verification).
A useful smell test before marking anything done: *does the shipped path take it, and does
a test fail if someone removes it?*
