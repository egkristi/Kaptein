# AGENTS.md

Guidance for AI coding agents working in this repository. The product is **Kaptein**
(never `k8stui`), a Kubernetes workbench written in Rust.

## Project identity

- **Product name:** Kaptein (repo folder is also `Kaptein`).
- **Language:** Rust (2024 edition).
- **Core thesis:** *The domain layer is the product.* The TUI, GUI, and headless agent
  are thin projections of one renderer-agnostic view-model.
- **Target platforms:** Linux/macOS/Windows native + WASM (browser) + headless/CI.

## Workflow

The standing workflow for every session, in order:

1. **Solve open work** — resolve open GitHub issues
   (https://github.com/egkristi/Kaptein/issues) and the tracked items in `ISSUES.md`.
2. **Ship the roadmap** — implement the planned features in `ROADMAP.md`, one milestone
   at a time, landing on the shared view-model.
3. **Register new issues** — if any problem, bug, or follow-up arises during the work
   that is not fixed in the same change, register it as a GitHub issue (with a repro
   where applicable) rather than leaving it undocumented.

### Repository health checks (run each session)

Check and fix any issues in each of these, before and after landing work:

- **GitHub Actions pipelines** — https://github.com/egkristi/Kaptein/actions. Any
  failed/red run is a defect to fix, not background noise.
- **CodeQL code scanning** — https://github.com/egkristi/Kaptein/security/code-scanning.
  No open findings left unaddressed (fix or dismiss with a reason).
- **Dependabot alerts / dependency updates** — check for open alerts and unmerged
  dependency PRs; fix open alerts, and review/merge (or close) Dependabot PRs.
- **Security advisories & policy** —
  https://github.com/egkristi/Kaptein/security/advisories. Draft, publish, or respond
  to advisories as needed; keep the security policy (`SECURITY.md`) accurate.
- **Repository Insights / pulse** — https://github.com/egkristi/Kaptein/pulse. Review for
  anomalous activity (e.g. a stalled feature, a runaway diff); fix any issues surfaced.

### Cluster testing

Use the live cluster at `/config/.kube/config` to test and verify features in the
**`kaptein`** namespace. **Never make destructive changes** to the cluster or its
existing namespaces — verify with read-only commands and, where a write must be
exercised, do it against throwaway resources in `kaptein` only.

### Release

Commit and push per completed feature; test and verify before committing (see
*Build & test commands*); keep `ROADMAP.md`/`ISSUES.md`/`CHANGELOG.md` in sync with each
change so the issue tracker and the roadmap stay the source of truth. When the shipped
set is tested and stable, cut a new release (bump `CHANGELOG.md` → tag → push) rather
than releasing every change; never release an untested or unstable build.

## Architecture rule (enforce this in every change)

Layer dependencies are strictly one-directional:

```
kaptein-core ──► kaptein-viewmodel ──► kaptein-tui
                                 ──► frontend-gui
                                 ──► headless / serve
```

- **All business logic lives in `kaptein-viewmodel`**: columns, sorting, filtering,
  status inference, permission decisions, action graphs.
- **Frontends render, never compute — semantics vs. geometry.** The view-model owns
  *meaning* (which columns, actions, status, row content); the frontend owns *layout*
  (column width in cells vs. font metrics, text truncation, scroll/focus/hover, modal
  z-order). Never let layout math leak into the view-model, and never let meaning drift
  into a frontend.
- If a change adds logic to a frontend that should be reusable across frontends, move it
  into `kaptein-viewmodel`.
- `kaptein-core` owns the Kubernetes client, watchers/reflectors, CRD discovery, and
  stores. It must not depend on the view-model or any frontend.
- Errors: `kaptein-core::Error` is the raw type (network/auth/watch/discovery);
  `kaptein-viewmodel::Error` is the user-facing, redaction-aware type with a `From` impl.
  Do not leak raw core errors to users.

## Crate layout

Only four crates exist now — split a module out only when it has code (splitting is an
afternoon; holding nine synchronized crates through Phase 1 is weekly friction):

```
crates/
  kaptein-core/       # kube-rs client, watchers/reflectors, CRD discovery, stores
  kaptein-viewmodel/  # renderer-agnostic logic (the product)
  kaptein-tui/        # ratatui
  # future (split out when they have code): frontend-gui, serve, headless,
  # viewdef, plugins, ext-sdk
extensions/        # example extensions (lenses, plugins, integrations)
docs/adr/          # architecture decision records
```

Note: the project is pre-alpha. When scaffolding, follow this layout and split crates
only when they carry real code.

## Build & test commands

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

- `clippy` is treated as a hard gate (`-D warnings`). Do not introduce clippy warnings.
- `cargo fmt` (rustfmt) output is expected; run `cargo fmt --all` before committing.
- Use `cargo` for all builds; do not invoke `rustc` directly.

## Code conventions

- **Idiomatic Rust**: prefer `Result`/`Option` over panics; `expect` only for
  programmer-invariant bugs; no `unwrap` in library code.
- **Async**: use `tokio` as the runtime. Kubernetes access goes through `kube-rs`.
- **Informer-based, not polling**: state is derived from watchers/reflectors/watch
  streams. Do not add periodic API-server scraping loops.
- **No telemetry, no account, no hidden network calls.** Features must work airgapped.
- **External tools** (Krew plugins, `kustomize`, `helm`, Trivy/Grype, `istioctl`) are
  shelled out to and must **degrade gracefully** when absent — never `unwrap` the
  subprocess result or panic on a missing binary.
- **Read-only default** for unknown contexts; writes require explicit opt-in/guardrails.
- **Secrets are masked by default.** Never log or persist secret values; the audit log
  records operations, not values.
- **Same keymap** in TUI and GUI: keyboard behavior is defined in the view-model's
  action graph, not duplicated per frontend.
- **Extensions: data first, code second.** Prefer a view definition (lens) over a WASM
  plugin; add a shell-out integration only for an existing external binary. Every
  extension is declared by an `extension.yaml` manifest and lives under `extensions/`.
- **Extension sandbox by default.** WASM plugins get no network and no filesystem unless
  declared in the WIT world and the manifest allowlist; enforce fuel metering and a
  memory cap. Version WIT worlds and bump `api_version` on breaking change.
- **The MCP surface is governed, not open.** Any `kaptein mcp` tool call must go through
  the same guardrails (RBAC preflight, context guardrails, read-only default,
  break-glass), be impersonated via `--as`, and land in the audit log. An agent never
  writes to the API server — PR only (ADR-0010).

## Testing expectations

- Unit-test view-model logic directly (it is renderer-agnostic, so no UI needed).
- **Contract tests**: when adding view-model output, assert that the TUI, GUI, and
  headless paths all consume the *same* render-intent for the same input.
- New `kaptein-core` watcher/reflector code should have tests with mock or synthetic data;
  avoid requiring a live cluster in CI.
- Benchmark-sensitive paths (informer-driven views) are validated against a synthetic
  cluster with thousands of CRDs; k9s is the baseline to beat.

## Documentation & decisions

- Significant architectural decisions go in `docs/adr/` as numbered ADRs (see
  `docs/adr/0001-egui-over-iced.md` for the format). Open one when a change shifts a
  documented decision.
- Keep `README.md`, `ROADMAP.md`, `CONTRIBUTING.md`, and `SECURITY.md` in sync with
  behavior changes.
- When referencing roadmap work, cite the milestone (`M#.#`).
- **License**: Kaptein core is BUSL-1.1 (source-available), converting to MIT on the
  Change Date (rolling, per version). The extension surface (`ext-sdk/`, WIT worlds,
  view-definition schema, `extensions/`) is **MIT/Apache-2.0**. Do not label the project
  "open source"; do not put BUSL terms on the extension surface.

## Naming & consistency

- Use **"Kaptein"** in all user-facing text, not `k8stui`.
- GUI framework is **`egui`** (ADR-0001), not `iced`. Do not reintroduce `iced`.
- The three frontends are **TUI, GUI, and headless/serve** — not "two frontends."

## Common pitfalls

- Adding UI-only state that duplicates view-model state (keep one source of truth).
- Introducing a frontend dependency on `kaptein-core` directly (go through the view-model).
- Hardcoding per-CRD UIs — use view definitions (data) or WASM plugins (code) instead.
- Writing a WASM plugin where a view definition (lens) suffices (data first, code second).
- Granting network/FS or any capability to a plugin by default instead of via the
  extension manifest allowlist.
- Adding polling loops or blocking calls inside async tasks.
- Logging secrets or persisting kubeconfig/exec-credential output.

## When in doubt

- Re-read the "Non-goals" section of `README.md` before adding scope — in particular:
  **no CI/CD, no service catalog, no policy engine, no agent runtime, no metrics/log
  store.** Kaptein is the operator's console and the governed control point, not the
  platform.
- Prefer the smallest change that lands on the shared view-model.
- If a change would break the one-directional dependency rule, refactor instead of
  short-circuiting.
