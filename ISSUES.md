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

**Open:**

- **#16** `dry_run_apply_patch force:true` must not carry into the Phase 2 write path.

**Closed since the last cycle:** #17 (`kaptein edit` redaction round-trip — fixed by
`RedactionPolicy::Unredacted` + `SecretViewed` audit, commit 42ce99f) and #18 (watch
reconnect — `LivePlane::watch_loop` now reconnects with backoff and `run_informer` has a
caller, commit 59b421a). See *Re-audit findings* below for what those fixes did **not**
cover.

### Re-audit findings (v0.27.0) — to file as issues

Found by an external re-audit of the shipped v0.27.0 artifact. Each is a defect in code
that ships today, so each is issue material rather than a milestone; the milestone column
names where the fix belongs. **Filed as GitHub issues #20–#30.**

| # | Issue | Severity | Finding | Owner |
|---|-------|----------|---------|-------|
| A | #20 | High | **Reconnect never relists → ghost rows.** `LivePlane::watch_loop` reconnects, but on reconnect it calls `list_metadata(limit(1))` *only to read a resourceVersion* and then watches from there. Objects deleted while the watch was down are never removed from the `MemPlane` — no `Deleted` event will ever arrive for them — so the TUI shows deleted resources indefinitely after any watch expiry (routine, ~5 min). The doc comment claims it "relists and reconnects"; it does not relist. A correct informer relists into the store, reconciles (removing keys absent from the relist), then watches from the *list's* RV. | M2.0c |
| B | #25 | High | **`InformerManager` has no caller.** `kaptein-core::informer` (361 lines) is referenced only by a doc-comment in `config.rs`. `LivePlane` opens watches without consulting it, so the hard cap, TTL eviction, and degrade-to-list path are never exercised against real watch sockets. The "watches ≤ N" budget is asserted only in the manager's own unit test — the policy layer is correct and unenforced. | M2.0c |
| C | #26 | Medium | **The "LRU + TTL" manager has no LRU.** `InformerManager::register` evicts by TTL only. When the cap is full and every entry is fresh, *every* new view is `Denied` until a TTL expires — the first `max_watches` views win permanently. `Registration::Denied`'s doc says "this view was not the most-recently-used", and the `watches` field comment says "in insertion order for LRU scanning", but the field is a `HashMap` (no order) and no least-recently-used entry is ever evicted to admit a hot view. Either implement LRU admission or correct ADR-0006 and the docs to say "TTL-only". | M2.0c |
| D | #23 | High | **Krew manifest ships placeholders and CI passes it.** `krew/kaptein.yaml` contains `PLACEHOLDER_VERSION` and `PLACEHOLDER_*_SHA256`, and no release step substitutes them. The CI `dist` job asserts only that `uri`/`sha256`/`bin` are *truthy*, which the placeholder strings satisfy — so `kubectl krew install kaptein` cannot work, and CI reports the manifest valid. Needs a release-time render step plus a CI assertion that the values are not placeholders. | Distribution |
| E | #24 | High | **`install.sh` ignores the signatures we produce.** It downloads `SHA256SUMS` from the *same* release URL as the archive and checks the archive against it. That is an integrity check against corruption, not authenticity: whoever can serve a bad release serves both files. The release already publishes `SHA256SUMS.bundle`, and `SECURITY.md` documents `cosign verify-blob` — the official installer just never runs it. Add cosign verification (with a documented `--certificate-identity`), or state plainly in the script that it does not verify authenticity. | Distribution |
| F | #21 | Medium | **MCP preflight pluralizes kinds differently from the request.** `mcp.rs::resource_from_kind` falls back to `lowercase + "s"` for anything outside a 21-entry table, so `NetworkPolicy` → `networkpolicys`, `PriorityClass` → `priorityclasss`, and most CRDs get a wrong plural. Because `auth::can` fails **closed**, a wrong plural means the rule never matches and the call is **refused** — the governed MCP surface silently rejects `list_resources`/`describe` for a large class of CRDs. The preflight must use the same plural the request uses (`ApiResource::from_gvk(&gvk).plural`), or resolve it from discovery. | M1b.4 |
| G | #22 | Medium | **Logs are still not redaction-aware.** `describe::pod_logs`, `multi_pod_logs`, and `follow_logs` return raw lines, and the MCP `logs` tool ships them verbatim to a model. There is no log-redaction function in the workspace. M1.7's DoD explicitly requires "the `logs` path is redaction-aware"; the resource path is done, the log path is not. Logs are where credentials leak in practice. | M1.7 |
| H | #27 | Medium | **The bounded list path is still not on the frontend path.** `LivePlane::seed` calls the unbounded `discovery::list`; `list_metadata_bounded` and `store::run_informer` are reached only from CLI commands. M2.0's "bounded" DoD is satisfied by code that exists, not by the path the TUI actually takes. | M2.0 |
| I | #28 | Medium | **`query_plane` still asks for 50 000 rows at ~10 Hz.** Rendering is windowed now (only `scroll..scroll+page_height` becomes ratatui `Row`s — the fix that landed), but `frontend-tui::query_plane` still issues `Query { start: 0, end: 50_000 }` on every loop iteration, and `MemPlane::query` deep-clones the whole row `Vec` and sorts it before windowing. The per-frame allocation is fixed; the per-frame clone-and-sort is not. The TUI needs `page.total` for `rows.len()`/`G`, so the fix is to query the visible window and carry `total` separately. | M1.8 |
| J | #29 | Low | **Secret annotations are over-redacted.** The `last-applied-configuration` leak is fixed by masking *every* annotation value on a Secret — which also masks `meta.helm.sh/*`, Argo CD tracking ids, and `kubectl.kubernetes.io/last-applied-configuration`'s harmless neighbours, making `describe` on a Secret much less useful. Prefer masking `last-applied-configuration` plus sensitive-named annotation keys. | M1.7 |
| K | #30 | Low | **`install.sh` computes `VERSION_TAG` and never uses it** (dead variable, line 64). | Distribution |

## Remaining review backlog (owned by milestones)

The external review ranked these; they are now **milestones in `ROADMAP.md`** rather than
unowned debt. Done items are struck through.

1. ~~**M1b.4 — MCP governance conformance**~~ (done, commit 1cbe417): RBAC preflight +
   context classification + read-only guardrail run per tool call; audit emits
   `Outcome::Rejected`, real `target`, real `session_id`, post-execution outcome.
2. ~~**M2.0 — wire `DataPlane` + informer store**~~ (done, commits ad1cb5b → 13d8aae):
   `MemPlane` + `table` (view-model DataPlane), `InformerStore`/`run_informer` +
   `list_metadata_bounded` (core), `KubernetesPlane`/`LivePlane` (integration), the TUI
   renders from a live informer-backed `DataPlane`, and a live `#[tokio::test]` exercises
   the real kube client when `KUBECONFIG` is present.
3. **M2.0b — integration-test tier + platform CI matrix**: kind/envtest + Windows/macOS +
   latest-three-minors conformance. *Windows/macOS test matrix added to CI; the
   kind/envtest tier and Kubernetes-minor conformance remain open. A live integration-test
   tier (`crates/kaptein-core/tests/live.rs`, gated on `KAPTEIN_LIVE_TESTS=1`) now
   exercises the read path and the delete write path against a real cluster.*
4. **M1.8 — kwok performance harness**: the performance budget is measured, not
   aspirational.
5. ~~**Signed releases + SBOM**~~ (done, commit eba14d9 + SLSA provenance in
   `.github/workflows/release.yml`): cosign keyless + CycloneDX SBOM + SHA256SUMS, the
   SBOM is cosign-signed, and SLSA provenance is generated per release.
6. ~~**Config schema/precedence/validation**~~ (done, commit 29cc1e6): `kaptein config validate`
   / `explain-context` flag invalid guardrail regexes and explain classification.
7. ~~**Redaction-aware error boundary**~~ (done, commit 98a39cc): `kaptein-integration`
   maps core errors into a real enum instead of passing through.
8. **Distribution & release sync** (cross-cutting): Homebrew/Krew/container/checksums +
   site/docs/tag sync. *Landed: `install.sh`, `krew/kaptein.yaml`, and `Dockerfile`; the
   Homebrew tap and the release-triggered site/README version bump remain open.*
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

- **PVC-binding diagnostics** need the PVC resources themselves — deferred to a Phase 3a
  rule pack (the scheduler's `PodScheduled` message is the Phase 1 signal).
- **`blast_radius`** walks the Pod ownership chain only when `gvk.kind == "Deployment"`
  (Deployment → ReplicaSet → Pod). StatefulSet, DaemonSet, and CronJob → Job → Pod
  chains are **not** covered; a full cross-kind topology scan is a Phase 3a fleet feature.
- **`dry_run_apply_patch` uses `force: true`** — correct for dry-run, but the Phase 2
  write path must **not** carry `force` forward (it would silently steal field ownership
  from Flux/Argo).
- **OIDC token forwarding** (ADR-0007 mode 1) is a `serve`/hub-mode concern (Phase 2),
  not the MCP stdio server.
- **No frontend consumes a lens yet.** The M2.2 engine (`kaptein-viewmodel::lens`) and the
  eight shipped lenses under `extensions/` are validated and rendered only by
  `kaptein viewdef validate` / `kaptein viewdef render`. The TUI still lists a hardcoded
  five-kind `Kind` enum and does no lens discovery or CRD auto-navigation, so the lens set
  is proven as *data* but not yet as *navigation*. This is the remaining half of M2.2, not
  a defect in the engine — recorded here so "lenses shipped" is not read as "lenses are in
  the UI".
- **`Cell::Redacted` and `Operation::SecretViewed` have exactly one producer each.**
  `SecretViewed` is emitted by `kaptein edit`'s unredacted fetch; `Cell::Redacted` is still
  only pattern-matched (`table::cell_text`), never constructed, because no surface has an
  unmask-in-place affordance. M1.7 keeps that bullet open deliberately.

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
