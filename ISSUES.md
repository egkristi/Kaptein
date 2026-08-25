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

Per the convention above, live bugs are GitHub issues. Currently open:

- **#16** `dry_run_apply_patch force:true` must not carry into the Phase 2 write path.
- **#17** `kaptein edit` will corrupt Secrets once the write path lands (redacted values
  round-trip as `[REDACTED]`); needs a redaction policy + `SecretViewed` audit.
- **#18** watch reconnect: the bounded `store::watch_from` path is still unreferenced
  (dead code) and lacks relist-on-410; `LivePlane` reconnects but the ADR-0006 store
  does not.

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
   kind/envtest tier and Kubernetes-minor conformance remain open.*
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

## Hygiene notes

- `core` / `core.*` dumps are git-ignored; if a process crashes to a core dump in the
  working tree, find and fix the crashing process rather than committing around it.
