# ADR-0014: Collapse to four crates; split only when a crate has code

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

ADR-0009 specified nine crates (`kaptein-core`, `kaptein-viewmodel`, `frontend-tui`,
`frontend-gui`, `headless`, `serve`, `plugins`, `viewdef`, `ext-sdk`). Six of them were
empty stubs (a doc-comment and nothing else). Empty crates carry a cost — compile graph,
CI matrix, dependabot noise, and a versioning surface — against **zero** enforcement.

## Decision

Collapse to **four** crates that carry real structure:

- `kaptein-core` — Kubernetes client, watchers/reflectors, CRD discovery, stores.
- `kaptein-viewmodel` — renderer-agnostic logic (the product).
- `frontend-tui` — ratatui.
- *(one binary crate, when the CLI is scaffolded.)*

Split a module into a new crate **only when it has code**, never when it merely has a
name. The split criterion is: *"it has a public API surface that other crates consume and
that must be versioned independently."* Splitting a module into a crate is an afternoon;
holding nine synchronized crates through Phase 1 is weekly friction.

## Consequences

- **Positive:** minimal compile/CI/dependabot surface; boundaries that are actually
  enforced (the `core → viewmodel → frontend` rule, checked in CI).
- **Negative:** when `viewdef`/`plugins`/`ext-sdk`/`serve`/`headless` grow code, they will
  need to be (re)created — but by then the split is driven by real API, not speculation.
- **Supersedes:** ADR-0009's crate-layout decision (the *renaming* part stands).

## Alternatives considered

- **Nine crates from day one** — rejected: empty stubs enforce nothing and add friction.
