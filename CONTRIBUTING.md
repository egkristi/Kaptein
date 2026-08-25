# Contributing to Kaptein

Thanks for your interest. Kaptein's core rule is simple and applies to every change:

> **The domain layer is the product.** All logic lives in `kaptein-viewmodel`; the TUI,
> GUI, and headless agent are thin projections and must never own business logic.

All participation is governed by our [Code of Conduct](./CODE_OF_CONDUCT.md) — please
read it before contributing.

## Architecture at a glance

```
kaptein-core ──► kaptein-viewmodel ──► frontend-tui
                                 ──► frontend-gui
                                 ──► headless / serve
```

Layer dependencies are **one-directional**: `frontend-*` → `viewmodel` → `core`.
A pull request that adds logic to a frontend that should live in the view-model is the
single most likely reason for a review hold.

**Semantics vs. geometry:** the view-model owns *meaning* (columns, actions, status, row
content); the frontend owns *layout* (column width in cells vs. font metrics, text
truncation, scroll/focus/hover, modal z-order). Do not put layout math in the view-model,
and do not compute meaning in a frontend.

## Getting started

```bash
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Before you submit

1. **Put logic in `kaptein-viewmodel`.** If the TUI needs a column, a sort, a filter, a
   status, or an action, define it once in the view-model.
2. **Keep the projections thin.** A frontend should render a render-intent (columns,
   rows, actions, status) produced by the view-model, not compute it.
3. **Add a contract test** if you add view-model output: assert the TUI, GUI, and
   headless paths all consume the same render-intent for the same input.
4. **Record significant decisions** as an ADR under `docs/adr/` (see ADR-0001 for the
   format).
5. **For view-definition changes**, validate with `kaptein viewdef validate` and keep the
   schema versioned.
6. **Don't add polling.** State comes from informers/watch streams.

## Commit & PR expectations

- One logical change per PR; reference the roadmap milestone (`M#.#`) where relevant.
- CI must be green: format, clippy (with `-D warnings`), tests, and the informer
  performance benchmark.
- New external-tool integrations must degrade gracefully when the tool is absent (see
  the "One static binary" non-functional requirement).
- Signed releases ship an SBOM — keep dependency changes reviewable and minimal.

## License & contributions

Kaptein's **core** is source-available under the Business Source License 1.1 (`LICENSE`),
which converts to MIT on the rolling Change Date. The **extension surface** (`ext-sdk/`,
WIT worlds, view-definition schema, `extensions/`) is MIT/Apache-2.0.

To permit commercial (paid) licensing of the core, contributions to the core require a
**Contributor License Agreement (CLA)** — see [`CLA.md`](./CLA.md) — that grants EGK AS
the right to relicense your contribution. A DCO sign-off alone is **not** sufficient for
commercial relicensing. Sign the CLA before submitting core changes.

Extension-surface contributions use **DCO sign-off only** (`git commit -s`) — no CLA
required. See [`DCO`](./DCO).

## Security-sensitive changes

See `SECURITY.md`. Features that touch secrets, audit logging, RBAC preflight, or the
LLM assistance path have a higher bar: include a threat-model note in the PR description.
