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
`force: false`; a test asserts the real path never forces — note `apply_patch_real` is
**pre-positioned for M2.3 and has no caller yet**: `kaptein apply` and `kaptein edit` are
both dry-run-only, so no live apply ships today — finding Y), #17 (`kaptein edit` redaction
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

D, E, and L were, together, the "all three advertised install paths are broken" cluster
flagged at v0.27.0. **All three are now resolved** — the installer cosign-verifies against
the OIDC identity, the Krew manifest is rendered at release time with real digests, and the
container image is built, pushed, and signed. Distribution is a working channel, not a set
of files (the one exception is the *central* Krew index, which is license-blocked — #34).

### Re-audit findings (v0.30.0) — new, not yet filed

Found by an external re-audit of the shipped v0.30.0 artifact (216 commits, 15 703 lines,
192 tests green, clippy clean). The v0.27.0 batch is genuinely fixed — these are new, and
three of them (M, N, O) are the *second-order* consequences of those fixes.

| # | Severity | Finding | Owner |
|---|----------|---------|-------|
| M | High | **The informer cap is unreachable in the shipped TUI.** `LivePlane`'s `informers` field is documented as "Shared across clones so the cap is enforced across *all* live planes in a process, not per plane (issue #25)" — but `new_with_policy` and `new_lens_with_policy` each construct `Arc::new(InformerManager::new(policy))`, and the TUI's `rebuild_plane` calls `new_plane(...)` (a constructor, not `clone`) on every kind/namespace switch. Each plane therefore gets a **fresh manager holding exactly one watch**, so `max_watches` (default 16) can never be reached and the LRU, the TTL, and the degrade-to-list path are unreachable in the shipped TUI. #25 moved the problem one level rather than closing it; the field's own doc comment states the invariant its constructor violates. *Fixed: the TUI holds one session-scoped `InformerManager` and passes it to every plane via `LivePlane::with_shared_informers`; a DoD test (`shared_manager_cap_is_enforced_across_planes_and_released_on_close`) drives distinct planes through a shared manager with a small cap and asserts the cap is reached.* | M2.0c |
| N | Medium | **`InformerManager::release` and `::touch` have no callers anywhere in the workspace.** `rebuild_plane` aborts the watch task without releasing its slot, and nothing refreshes recency — so every entry's `last_touched` is its registration time and the LRU has no usage signal. This is latent only because of M: fixing M alone (one shared manager per session) immediately turns it into a **slot leak**, where after `max_watches` view switches every new view degrades to a one-shot list and the TUI silently stops being live. **M and N must be fixed in the same change.** *Fixed: `watch_loop` holds a `WatchSlotGuard` that releases the slot on exit (view close or task abort), so a session-scoped manager no longer leaks a slot per view switch — verified by the same DoD test.* | M2.0c |
| O | Medium | **`relist_and_reconcile` removes but never adds.** It builds `live_ids` from a metadata list and removes plane rows absent from it, but never upserts rows that are *in* the relist and missing from the plane. Objects **created during a watch outage stay invisible** until they next change — the exact mirror of the ghost-row bug #20 that this function was written to fix. Note the relist is metadata-only, so it cannot upsert a correct `status` (status comes from the full object): the fix is a design decision (full-object relist, or a metadata upsert with a deferred status fetch), not a one-liner. *Fixed: `relist_and_reconcile` now relists **full objects** (not metadata summaries) and upserts rows missing from the plane with a correct `status`, so objects created during an outage appear immediately.* | M2.0c |
| P | Medium | **`redact_line` compiles two regexes on every log line.** `regex::Regex::new` is called inside the per-line function — including the ~20-branch `SENSITIVE_KEY_ALT` alternation — and `redact_line` is invoked per line by `pod_logs`, `multi_pod_logs`, and `follow_logs` (the streaming path). There is no `LazyLock`/`OnceLock` anywhere in `kaptein-core`. On a follow stream the regex compilation dominates the work. Introduced by the #22 fix; `std::sync::LazyLock` is the fix (MSRV 1.97 allows it). *Fixed: both regexes are now `LazyLock`.* | M1.7 |
| Q | Medium | **`query_plane` still requests the whole set.** The M1.8 work is real and substantial — sorting is an index permutation, the TUI re-queries only on a revision change, and `total` is carried separately — but `kaptein-tui::query_plane` still issues `Query { start: 0, end: 50_000 }`, so up to 50 000 `TableRow`s (each with per-cell `String`s) are materialized on **every revision change**, and on a busy cluster the revision advances per watch delta. `ROADMAP.md` is accurate here (it records "still open: querying *only* the visible window"), but `query_plane`'s own doc comment claims the TUI shows "N rows" and jumps to the bottom "**without materializing the whole set** — the M1.8 windowing fix", which is not what the function does. Fix the comment now; the windowing itself needs `selected`/`scroll`/fuzzy-jump reworked off a full `rows` vector, which is the deeper nav refactor M1.8 already names. *Fixed: `query_plane` now takes `start`/`end` and materializes only the visible window; `selected`/`scroll` are clamped by a pure `clamp_viewport` helper (unit-tested), navigation re-queries the window, and fuzzy-jump snapshots the full set once on entry then re-windows on exit.* | M1.8 |
| R | Medium | **Dynamic shell completion can hang.** `completion.rs` states its contract as "completion degrades to 'no candidates' — never a panic or a **hang**", but `rt.block_on(...)` wraps the kube call with no `tokio::time::timeout`. Against a blackholed endpoint (firewall drops rather than refuses) tab-completion blocks for the client's full timeout. Wrap each completer in a short timeout (~300 ms) so the stated contract is enforced. *Fixed: `cluster_query` wraps each cluster-querying completer in a 300 ms `tokio::time::timeout`.* | M1.2 |
| S | Low | **`README.md` tells users to run a binary that no longer exists.** Line 521 still shows `./target/release/kaptein-tui`, but v0.30.0 collapsed the TUI into `kaptein tui` and the `kaptein-tui` crate no longer declares a `[[bin]]`. `docs/INSTALL.md` and `docs/USAGE.md` are correct — only the README's *Build & test* block is stale. *Fixed: README now shows `./target/release/kaptein tui`.* | Docs |
| T | Hygiene | **Core dumps are escalating, not static.** 14 dumps totalling **641 MB** are in the working tree, dated across four days. The *Hygiene notes* section below still says "Two dumps (~240 MB)" — understated ~3× and no longer a one-off. Something crashes on essentially every session and nothing identifies it. *Still climbing: **16 dumps / 732 MB** at the second v0.30.0 pass. *Resolved: the working tree is now clean (0 dumps); `coredumpctl list` reports no journal/coredumps in this environment, so the crashing binary could not be named retroactively. The ignore rule stays as the guard, and the hygiene note below records the actual state rather than an undercount.* | Hygiene |

### Re-audit findings (v0.30.0, second pass) — new, not yet filed

A second pass over the same release after P/Q/R/S landed (218 commits, 15 732 lines, 192
tests green, clippy and `fmt` clean). M, N, and O above remain open and unchanged. The
findings below are new, and **U is the most serious thing this audit has found in several
cycles** — it is a hole in the guardrail model itself, not a gap between a doc and its
implementation.

| # | Severity | Finding | Owner |
|---|----------|---------|-------|
| U | **High** | **`kaptein exec` is completely ungoverned — no gate and no audit trace.** The `Command::Exec` arm takes no `--confirm` and no `--break-glass`, never calls `gate_write`, and emits **no `AuditEvent`**. So `kaptein exec --pod x -n prod -- sh -c '…'` runs arbitrary code inside a production container with no break-glass justification and leaves nothing in the audit log; `--tty` gives a full interactive shell on the same terms. Compare what *is* gated and audited: `cordon`/`uncordon` (reversible, low blast radius), `evict`, `scale`, `restart`, `delete`, and `debug` (attaching an ephemeral container). Exec is the highest-privilege operation on the surface and it is the only mutating one with no control at all. SECURITY.md presents the audit log as the accountability control and context guardrails as the prod control; for exec, both are absent. **Fix:** `--confirm` + `gate_write(break_glass)` + an `Operation::Exec` audit event, matching `debug` exactly. *Fixed: `exec` now requires `--confirm` + `--break-glass`, gates through `gate_write`, and emits `Operation::Exec`; a coverage test derives the governed set from the command tree so a new mutating command can't slip past.* | M1.1 |
| V | Medium | **`Operation::Exec` is emitted for the *debug* path, mislabelling it.** The single emission site is `Command::Debug` (ephemeral-container attach) at `main.rs:1761`. So the one operation named `Exec` in the audit log never means exec, and once U is fixed the two actions become indistinguishable in the log. The `Operation` enum needs a distinct variant for ephemeral-container attach (`Debug`/`EphemeralAttach`), with `exec` taking `Exec`. *Fixed: added `Operation::EphemeralAttach`; `debug` emits it and `exec` emits `Exec`.* | M1.1 |
| W | Medium | **`Operation::PortForward` is defined but never emitted.** A named, persistent port-forward is a live tunnel from the operator's laptop into a pod — on most threat models the second-most audit-relevant action after exec, and the one most likely to outlive the session that created it. `port-forward` and `port-forward-remove` write no audit record. (`Operation::Drain` is also never emitted, but drain is preview-only today, so that one is correct.) *Fixed: opening a forward (named or anonymous) and removing a named forward now write `Operation::PortForward` audit records.* | M1.1 |
| X | Medium | **`KubernetesPlane` is superseded dead code with *divergent* semantics.** Zero callers anywhere. It is a `pub` `DataPlane` implementation in `kaptein-integration` that uses the **unbounded** `discovery::list` (the path #27 moved off), always returns `Revision(0)` so staleness detection silently no-ops, and whose `subscribe` returns `stream::empty()` so no consumer ever gets a delta. Given this project's repeated "the code exists but the shipped path doesn't take it" findings, an unused implementation that behaves *differently from the real one* is precisely the trap a future contributor falls into. Delete it, or make it `#[doc(hidden)]` and rename it to something that cannot be mistaken for the supported plane. *Fixed: deleted.* | M2.0 |
| Y | Low | **`apply_patch_real` has no caller, but the docs read as though a live write path ships.** Pre-positioning the safe path before M2.3 is good practice and the `force`-refusal test is real — but the #16 entry above says "the real write path `apply_patch_real` always applies with `force: false`", which reads as extant. It is not reachable: `kaptein apply` and `kaptein edit` are both dry-run-only, and nothing else calls it. One clarifying clause ("pre-positioned for M2.3; no caller yet") prevents "Kaptein can apply" being inferred. *Fixed: the doc now states "pre-positioned for M2.3 — no caller yet".* | M2.3 |

### Re-audit findings (v0.31.0) — new, not yet filed

Audit of the shipped v0.31.0 artifact (233 commits, 16 829 lines, **212 tests green**,
clippy and `fmt` clean). **Findings M–Y are all genuinely fixed** — each verified in code,
not taken from the commit message: session-scoped `InformerManager` + `WatchSlotGuard`,
full-object relist that upserts, `exec`/`debug`/`port-forward` governed with distinct
`Operation`s and a *derived* coverage test, `KubernetesPlane` deleted, real visible-window
querying with a unit-tested `clamp_viewport`. The M2.0b live tier is now a genuine
kind-backed conformance matrix across k8s v1.35/v1.36/v1.37 with digest-pinned nodes, and
the `no_requests`/`no_limits` split from ADR-0015 landed wired into the landing view with a
live test pinning the shipped path. Core dumps are gone.

| # | Severity | Finding | Owner |
|---|----------|---------|-------|
| Z | Medium | **Finding N was half-fixed and marked fully fixed — `touch` still has no caller.** N's text covered *both* halves: "`InformerManager::release` **and** `::touch` have no callers … nothing refreshes recency — so every entry's `last_touched` is its registration time and the LRU has no usage signal." The fix added `WatchSlotGuard` (the `release` half) and the N row now reads *Fixed*. But `grep -rn '\.touch('` outside `informer.rs` still returns **nothing**: `last_touched` is only ever written by `register`. **This got worse, not better, when M landed.** While the cap was unreachable the missing recency signal was inert; now that the cap is enforced at session scope, `register`'s LRU eviction (`min_by_key(last_touched)`) picks the **oldest-registered** view — which, for an operator who opens one view and then cycles through others, is *the view currently on screen*. The LRU inverts and evicts the hottest entry. The DoD test `shared_manager_cap_is_enforced_across_planes_and_released_on_close` asserts cap + release only; it makes no recency assertion, so it passes. **Fix:** call `informers.touch(&watch_key)` from `LivePlane::query` (the TUI already re-queries per revision change, so the hook point is free), and extend the DoD test to assert that under a full cap the *most recently queried* view survives eviction. *Fixed: `LivePlane::query` now calls `self.informers.touch(&self.watch_key())`, and a new `lru_evicts_the_coldest_not_the_hottest_view` DoD test asserts the most-recently-queried view survives a full-cap eviction.* | M2.0c |
| AA | Medium | **Fuzzy-jump re-ranking deep-clones the whole master list on every keystroke — the allocation pattern M1.8 just removed, reintroduced one screen over.** Entering `/` snapshots the full set (`query_plane(.., 0, 50_000)`) into `jump_master`, which is correct — search must span the store, not the window. But every typed character *and* every backspace then runs `fuzzy_rerank(jump_master.clone(), q)`. `TableRow` is `{ String, String, Vec<String> }`, so on a 50 000-row view that is ~150 k `String` allocations per keystroke for the clone alone; `fuzzy_jump` then returns `FuzzyMatch { candidate: String, .. }` — owned — for every match, and an empty query matches everything, so add another ~50 k, plus a 50 k-entry `HashMap` and a sort. A 10-character query costs ~2 M allocations. **The M1.8 benchmark does not cover this path** — it gates `MemPlane::query` only — so the budget's own guard has a blind spot precisely where the interactive latency now lives. **Fix:** take `&[TableRow]` and return indices (or `Vec<&TableRow>`), and add a fuzzy-rerank case to `benches/query.rs` so the gate covers keystroke-to-frame on the search path, not just the table path. *Fixed: `fuzzy_rerank` now takes `&[TableRow]` and returns `Vec<usize>` (indices), the jump mode renders from `jump_master`+`jump_order` without cloning, `fuzzy_rank_indices` + an allocation-free `fuzzy_score` removed the per-candidate `String`/`Vec<char>`, and `benches/query.rs` now gates a fuzzy re-rank (measured 4 ms vs 11 ms before).* | M1.8 |
| AB | Low | **M2.0b's live tier is excellent but does not yet meet its own DoD.** The milestone names "the real kube client, **the MCP protocol**, **the CLI**, and **every write path** (scale/delete/restart/cordon/evict/apply/exec/**portforward**)". Nine live tests now cover list/describe, delete, scale, apply-dry-run, blast_radius, restart, evict, exec, and the missing-resources overview path — genuinely the hard part. Still uncovered: **port-forward** (0 references), **the MCP protocol** (0), and **the CLI binary itself** (0 — every test drives the library). `cordon`/`uncordon` is skipped with the comment that it "would mutate a *real* node" — true of a shared cluster, but the tier now runs on a **throwaway kind cluster**, where cordoning the single node is safe and self-cleaning, so that exclusion no longer holds. Either cover them or narrow the DoD; leaving both as-is is how a milestone gets marked done on a subset. *Resolved: **port-forward** is covered (`port_forward_binds_and_bridges`); the **CLI binary** is covered (`delete_confirm_round_trips_through_the_cli` drives `run(cli)` end-to-end); the **MCP protocol** is covered (`governance_check_runs_real_preflight_against_a_live_server` drives the governance gate against a live `SelfSubjectRulesReview`); and **cordon/uncordon** is covered (`cordon_marks_node_unschedulable_then_uncordon_restores_it` cordons the throwaway kind node and restores it). All four run in the CI `live` job — the milestone's full DoD is now met.* | M2.0b |

### Re-audit findings (v0.32.0) — new, not yet filed

Audit of the shipped v0.32.0 artifact (247 commits, 17 819 lines, **226 tests green**,
clippy and `fmt` clean). **Z, AA, and AB are all genuinely fixed** — verified in code:
`touch` is wired into `LivePlane::query` with an `lru_evicts_the_coldest_not_the_hottest_view`
test that asserts the *recency* clause (not just the cap), `fuzzy_rerank` now takes
`&[TableRow]` and returns indices with a bench case gating the search path, and the live
tier gained port-forward, cordon/uncordon, the MCP governance gate, and the CLI binary
end to end — all four running in the CI `live` job. M2.0b's full DoD is met.

| # | Severity | Finding | Owner |
|---|----------|---------|-------|
| AC | **High** | **The M1.7 "single choke point" is a convention on the lens path, not a choke point — and it has now leaked twice.** `render_row`, `evaluate_status`, and `evaluate_health` all take a bare `&serde_json::Value` and *trust the caller* to have redacted it. Nothing in the type system, and no test, enforces it. Two plaintext-Secret leaks have shipped on exactly this pattern: **#35** (the TUI's `map_object_with` fed an unredacted object to `render_row`) and **commit 6692b10** (the CLI `get --lens` path did the same, independently, months later). Both were found by audit rather than by tests or types, and both were fixed **pointwise**. Today the three cluster-facing paths each redact by a *different* mechanism — `map_object_with` (TUI rows), an inline `redact_object` call (CLI `get --lens`), and `get_dynamic_redacted` (TUI health, new in v0.32.0 and correct by luck of review, not by construction). `kaptein-core::describe` is the counter-example that proves the point: it *is* safe by default (`describe_dynamic` redacts; the opt-out is a separate, explicitly-named `describe_dynamic_policy` used only by audited `edit`). **Fix: make the guarantee unrepresentable to violate.** Introduce a `Redacted` newtype constructible only by the redactor and take it in `render_row`/`evaluate_*`; a bare `Value` then simply does not compile. The lens-*authoring* path (`viewdef render`, which renders a user-supplied file and has no cluster secret to leak) opts out through a visibly-named constructor so the exemption is greppable. This is the same "derive, don't restate" lesson that fixed the exec guardrail — applied to types instead of tests. *Fixed: `render_row`/`evaluate_status`/`evaluate_health` now take `&Redacted` (a newtype wrapping `serde_json::Value`, constructible only via `Redacted::from_redacted` — the cluster paths — or the greppable `Redacted::from_unredacted_for_lens_authoring` — `viewdef render` only); a bare `Value` no longer compiles, so the guarantee lives in the signature, not in reviewer memory.* | M1.7 |
| AD | Low | **Per-lens health checks shipped without reaching the user manual.** v0.32.0 added health checks to the lens schema, `evaluate_health`, a `viewdef render` health output, and a TUI detail-pane surface bound to **`h`**. `README.md` documents it (7 mentions) and `ROADMAP.md` tracks it (11) — but `docs/USAGE.md`, the actual user manual, has **zero**. Its TUI keymap table lists `n`, `/`, `:`, `d`, `i` and not `h`; §5 (lenses, including 5.4 *Lens authoring tools* and 5.5 *Lens-driven `get`*) and §7.4 (*Make this CRD navigable in the TUI*) never mention health. A user reading the manual cannot discover the feature. The version-sync CI gate catches *version* drift between docs; it does not catch a feature landing without documentation. *Fixed: `docs/USAGE.md` now documents `h` in the keymap, the `health:` block in §5.2, health findings in `viewdef-render` (§5.4), and the health step in §7.4.* | M2.2 |
| AE | Low | **Health checks are declared in 1 of the 9 shipped lenses.** Only `extensions/lens.cnpg.yaml` carries a `health:` block — deliberate, per the commit ("demonstrate health in CNPG"), and a reasonable first step. But ADR-0012's whole argument is that the lens schema is proven by the *hardest* lenses, and health predicates are exactly where a schema gets stressed: Strimzi Kafka readiness across broker/zookeeper conditions, cert-manager `Certificate` expiry windows, and KubeVirt `VirtualMachine` migration/run state each want shapes CNPG's checks do not exercise. Adding health to two or three more of the shipped set is the cheapest available test of whether the health-check schema is expressive enough before it is versioned as stable. *Fixed: Strimzi Kafka (`status.observedGeneration != 0`), cert-manager Certificate (`status.notAfter != ""`), and KubeVirt VirtualMachine (`status.printableStatus` contains `Running`) now declare `health:` blocks — four of the nine shipped lenses.* | M2.2 |

The external review ranked these; they are now **milestones in `ROADMAP.md`** rather than
unowned debt. Done items are struck through.

1. ~~**M1b.4 — MCP governance conformance**~~ (done, commits 1cbe417 → #21 fix):
   RBAC preflight + context classification + read-only guardrail run per tool call; audit
   emits `Outcome::Rejected`, real `target`, real `session_id`, post-execution outcome.
   The preflight plural now comes from `ApiResource::from_gvk` — the same pluralizer the
   request uses — so the gate and the call can no longer disagree (#21, closed).
2. ~~**M2.0 — wire `DataPlane` + informer store**~~ (done, commits ad1cb5b → 13d8aae, #27):
   `MemPlane` + `table` (view-model DataPlane), `InformerStore`/`run_informer` +
   `list_metadata_bounded` (core), `KubernetesPlane`/`LivePlane` (integration), the TUI
   renders from a live informer-backed `DataPlane`, and a live `#[tokio::test]` exercises
   the real kube client when `KUBECONFIG` is present. `LivePlane::seed` now pages through
   `list_bounded`, so the **shipped frontend path** is the bounded one — the half of the
   DoD that #18's CLI caller did not satisfy (#27, closed).
3. **M2.0b — integration-test tier + platform CI matrix**: kind/envtest + Windows/macOS +
   latest-three-minors conformance. *Windows/macOS test matrix added to CI; a live
   integration-test tier (`crates/kaptein-core/tests/live.rs`, gated on
   `KAPTEIN_LIVE_TESTS=1`) exercises the read path and the delete write path against a
   real cluster, and now runs **in CI** via a `live` job on a throwaway `kind` cluster,
   as a **latest-three-minors conformance matrix** (v1.37 / v1.36 / v1.35). Extended
   (v0.30.1 →) to eight paths: `restart`, `evict`, and `exec` are now live-tested too
   (cordon/uncordon are deliberately excluded — they mutate a real node). This closes
   the milestone.*
3b. **M2.0c — watch resilience & informer lifecycle** *(added by the v0.27.0 re-audit)*:
   relist-on-reconnect, and the ADR-0006 lifecycle policy actually enforced.
   *Landed: `InformerManager` with a config-backed `[informer]` policy; LRU admission
   (#26); `watch_loop` relists and reconciles on every reconnect (#20); `LivePlane`
   registers with the manager and degrades to a list on `Denied` (#25).* **Resolved —
   findings M, N, O:** the manager is now session-scoped (M), the watch task releases its
   slot via a `WatchSlotGuard` (N), and the relist upserts objects created during an
   outage as well as removing deletions (O) — all pinned by the
   `shared_manager_cap_is_enforced_across_planes_and_released_on_close` DoD test.
4. **M1.8 — kwok performance harness**: the performance budget is measured, not
   aspirational. *Landed (v0.29.0 →): the view-model half is measured —
   `benches/query.rs` drives `MemPlane::query` over 50k rows and gates p99 <8 ms via a
   `bench` CI job; sorting is an index permutation and `cmp_cells` is allocation-free for
   the common columns; the TUI re-queries only on a revision change and carries `total`
   separately (#28, closed).* **Resolved — finding Q:** `query_plane` now materializes
   only the visible window (`start`/`end`), with `clamp_viewport` keeping
   `selected`/`scroll` valid and fuzzy-jump snapshotting the full set once on entry.
   *The bench now also gates steady-state RSS (250 MB) and **cold start** (500 ms, seed +
   first query), so all three view-model-ownable numbers are measured. Remaining: the
   kwok synthetic-cluster harness and the end-to-end frontend keystroke-to-frame number.*
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
   license-blocked central-index PR; the custom index is the shipped resolution).
   *Version drift is now **guarded**: a `version-sync` CI job derives the workspace
   version from `Cargo.toml` and fails if any of `README.md`/`install.sh`/`Dockerfile`/
   `docs/INSTALL.md` drift from `v<version>`. Remaining: a Homebrew tap and an automated
   release-triggered site/README version bump.*
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
  `SecretViewed` is emitted by `kaptein edit`'s unredacted fetch. `Cell::Redacted` **is now
  constructed** (v0.30.0 →): `render_row`'s `cell_for_column` recognizes the
  `[REDACTED]` marker that `kaptein-core::redact` substitutes and emits the *typed*
  `Cell::Redacted` variant (instead of a `Text` cell carrying the marker string), and
  `cell_text` renders it as the `[REDACTED]` mask — so a frontend renders a mask with no
  special-case string comparison. The one remaining M1.7 open is an unmask-in-place
  affordance (kept deliberately). *(The lens render path no longer leaks: `map_object_with`
  redacts the object before `render_row`, so a lens bound to a secret field reaches the
  `Row` as `Cell::Redacted` — issue #35.)*

## Hygiene notes

- `core` / `core.*` dumps are git-ignored; if a process crashes to a core dump in the
  working tree, find and fix the crashing process rather than committing around it.
  **Resolution (finding T):** the tree is clean as of this pass (0 dumps), and
  `coredumpctl list` reports no journal/coredumps in the dev environment — the dumps seen
  across the v0.27.0 → v0.30.0 window (2 → 14 → 16, ~240 → 641 → 732 MB) were cleared and
  could not be attributed retroactively. The two recurring sizes (~158 MB and ~88 MB)
  suggested two specific binaries crashing reproducibly, but without a retained dump the
  identity cannot be confirmed. **Going forward:** if a `core`/`core.*` file appears in
  the tree, capture it before deleting (`file core.<pid>` to name the binary, then
  `gdb <binary> core.<pid>` → `bt`) and register the crash as an issue — a process that
  dumps core on every session is a defect regardless of whether it is Kaptein's own binary
  or a toolchain/editor process in the same tree. `ulimit -c unlimited` is the current
  shell setting; a dev loop that sets `ulimit -c 0` prevents silent accumulation without
  losing the `file`/`gdb` triage path (set it back to `unlimited` to capture).

## Audit provenance

The tables above record findings from successive external audits of the shipped artifact.
When closing one, prefer a **falsifiable** DoD in `ROADMAP.md` over a checkbox here: the
recurring pattern across three cycles has been a milestone that a partial implementation
satisfies literally (bounded-list code that exists but is not on the frontend path; a
policy manager with no caller; a signed release whose own installer skips verification).
A useful smell test before marking anything done: *does the shipped path take it, and does
a test fail if someone removes it?*
