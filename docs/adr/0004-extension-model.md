# ADR-0004: Three-tier extension model with a shared manifest

- **Status:** Accepted
- **Date:** 2026-08-20
- **Deciders:** Kaptein maintainers

## Context

Kaptein must be extensible ("and more" must scale), but "plugins", "modules", and
"extensions" were being used loosely. Three mechanisms already exist implicitly — view
definitions (declarative lenses), WASM component-model plugins, and shell-out to external
binaries — with no single, coherent way to declare, discover, secure, or version them.

## Decision

Unify them under one concept — an **extension** — with exactly **three tiers**, chosen
**data-first**:

1. **View definitions (lenses)** — declarative YAML/CUE; no code.
2. **WASM component-model plugins (WIT)** — sandboxed compiled code.
3. **Shell-out integrations** — external binaries.

Every extension is declared by a shared **`extension.yaml` manifest** and discovered from
configurable, **Git-backed paths** (no central marketplace).

### Manifest (minimal shape)

```yaml
id: com.example.cnpg-lens        # unique, reverse-DNS
name: CNPG lens
version: 1.2.0
api_version: 1                    # bumped on breaking WIT/schema change
kind: lens | plugin | integration # exactly one
entrypoint: lens.cnpg.yaml        # lens file, .wasm, or command spec
permissions: []                   # capabilities for plugin/integration tiers
```

### Sandbox defaults (tier 2)

WASM plugins run under the **wasmtime** runtime with:

- **fuel metering** (bounded compute),
- a **memory cap**,
- **no network and no filesystem** by default,
- host calls available only via the declared **WIT world** *and* the manifest
  `permissions` allowlist.

A plugin that needs a capability it did not declare fails to load, not at runtime.

### Versioning

WIT worlds are versioned; the manifest `api_version` is bumped on any breaking interface
or schema change. The host refuses to load an extension whose `api_version` it does not
support, with a clear migration error — so plugins do not silently break across releases.

### Distribution

Extensions are Git-native: they live in a directory (or their own repo) referenced by the
shared workspace config. Lifecycle is managed with `kaptein extension {validate,list,
enable,disable}`; `validate` checks the manifest, lens schema, and WIT signature before
anything is enabled.

## Licensing split

The extension *surface* must not inherit the BUSL terms, or the "and more" ecosystem
cannot form: no one writes lenses for a source-available tool without knowing what
happens to their own work. Therefore:

- **BUSL-1.1** on the Kaptein core (`kaptein-core`, `kaptein-viewmodel`, frontends,
  `serve`, `headless`).
- **MIT (or Apache-2.0)** on `ext-sdk/`, the **WIT worlds**, the **view-definition
  schema**, and the **example extensions** under `extensions/`.

This keeps the monetizable core protected while making the extension surface safe to
build on.

## Consequences

- **Positive:** one mental model and one lifecycle for all extensibility; a secure default
  for the highest-risk tier; a versioning story that makes plugins survive releases; a
  permissively-licensed surface that a third-party ecosystem can build on.
- **Negative:** the manifest and versioned WIT worlds are a stable contract — churn is
  expensive. For this reason the WIT worlds are defined **late in Phase 2**, after
  real lenses exist, not in Phase 0.
- **Scope note:** the `ext-sdk` crate is the only supported way to author tier-2 plugins;
  it publishes the WIT worlds and host-import bindings plugin authors compile against.

## Alternatives considered

- **One mechanism for everything** (WASM only, or lenses only) — rejected: forces code
  where data suffices, or data where logic is required.
- **A central plugin marketplace / registry** — rejected: contradicts the airgap and
  no-account requirements.
