# AGENTS.md

Guidance for AI coding agents working in this repository. The product is **Kaptein**
(never `k8stui`), a Kubernetes workbench written in Rust.

## Project identity

- **Product name:** Kaptein (repo folder is also `Kaptein`).
- **Language:** Rust (2021 edition or newer as configured).
- **Core thesis:** *The domain layer is the product.* The TUI, GUI, and headless agent
  are thin projections of one renderer-agnostic view-model.
- **Target platforms:** Linux/macOS/Windows native + WASM (browser) + headless/CI.

## Architecture rule (enforce this in every change)

Layer dependencies are strictly one-directional:

```
kube-core ──► kube-viewmodel ──► frontend-tui
                             ──► frontend-gui
                             ──► headless / serve
```

- **All business logic lives in `kube-viewmodel`**: columns, sorting, filtering, status
  inference, permission decisions, action graphs.
- **Frontends render, never compute.** They consume a render-intent produced by the
  view-model.
- If a change adds logic to `frontend-tui`, `frontend-gui`, `headless`, or `serve` that
  should be reusable across frontends, move it into `kube-viewmodel`.
- `kube-core` owns the Kubernetes client, watchers/reflectors, CRD discovery, and
  stores. It must not depend on the view-model or any frontend.

## Planned crate layout

```
kube-core/         # kube-rs client, watchers/reflectors, CRD discovery, stores
kube-viewmodel/    # renderer-agnostic logic (the product)
crates/
  frontend-tui/    # ratatui
  frontend-gui/    # egui (+ wasm)
  headless/        # agent mode, CI, fleet-hub
  serve/           # serve backend (axum + tonic)
  plugins/         # WASM component model host + WIT interfaces
  viewdef/         # view definition schema + engine (YAML/CUE)
extensions/        # example view definitions & plugins
docs/adr/          # architecture decision records
```

Note: not all crates exist yet — the project is pre-alpha. When scaffolding, follow this
layout and create the workspace members accordingly.

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

## Testing expectations

- Unit-test view-model logic directly (it is renderer-agnostic, so no UI needed).
- **Contract tests**: when adding view-model output, assert that the TUI, GUI, and
  headless paths all consume the *same* render-intent for the same input.
- New `kube-core` watcher/reflector code should have tests with mock or synthetic data;
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

## Naming & consistency

- Use **"Kaptein"** in all user-facing text, not `k8stui`.
- GUI framework is **`egui`** (ADR-0001), not `iced`. Do not reintroduce `iced`.
- The three frontends are **TUI, GUI, and headless/serve** — not "two frontends."

## Common pitfalls

- Adding UI-only state that duplicates view-model state (keep one source of truth).
- Introducing a frontend dependency on `kube-core` directly (go through the view-model).
- Hardcoding per-CRD UIs — use view definitions (data) or WASM plugins (code) instead.
- Adding polling loops or blocking calls inside async tasks.
- Logging secrets or persisting kubeconfig/exec-credential output.

## When in doubt

- Re-read the "Non-goals" section of `README.md` before adding scope.
- Prefer the smallest change that lands on the shared view-model.
- If a change would break the one-directional dependency rule, refactor instead of
  short-circuiting.
