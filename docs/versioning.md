# Versioning

Kaptein releases are **SemVer** (`MAJOR.MINOR.PATCH`). Three things are versioned
contracts in their own right, and this document defines how they relate to the release
version.

## The three versioned contracts

1. **WIT worlds** (WASM component-model plugin interface) — see ADR-0004.
2. **Lens schema** (view-definition schema, YAML/CUE) — see ADR-0004, ADR-0012.
3. **MCP surface** (`kaptein mcp` tool schema) — see ADR-0010.

Each carries its own `api_version`/schema version, bumped independently on a breaking
change to *that* contract.

## Relationship to the release version

- **Patch (`x.y.Z`)**: bug fixes; no change to any contract.
- **Minor (`x.Y.0`)**: new features; may add to a contract (additive, non-breaking), but
  does not break an existing `api_version`/schema version.
- **Major (`X.0.0`)**: at least one contract has a breaking change, i.e. a new
  `api_version` (WIT), a new lens-schema major, or an incompatible MCP tool change.

## Compatibility rule

A release must refuse to load a plugin, lens, or MCP client whose `api_version`/schema
version it does not support, with a clear migration error — never silently break.

## Change log

Each release is accompanied by an entry in `CHANGELOG.md`, generated from conventional
commits and kept in sync with the tag.

## MSRV

The Minimum Supported Rust Version is **the pinned toolchain in `rust-toolchain.toml`**,
currently `1.97.1`. The MSRV is deliberately **not** "latest stable" — it lags to
accommodate airgapped and distro toolchains. Policy: MSRV is the current pinned version;
a bump happens only when a dependency raises its own MSRV above it, and CI runs on both
the pinned version and `stable` to catch breakage before the bump.
