# Versioning

Kaptein releases are **SemVer** (`MAJOR.MINOR.PATCH`). Three things are versioned
contracts in their own right, and this document defines how they relate to the release
version.

## The three versioned contracts

1. **WIT worlds** (WASM component-model plugin interface) — see ADR-0004.
2. **Lens schema** (view-definition schema, YAML — CUE authoring planned) — see
   ADR-0004, ADR-0012.
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

**Status:** the **MCP** gate is implemented. The server advertises the contract version
it implements and refuses a `tools/call` from a client whose declared
`_meta["io.kaptein/apiVersion"]` has a different **major** (a client that omits the field
is accepted for backward compatibility). The rule lives in
`kaptein-viewmodel::versioned` (`ApiVersion`, `is_compatible`, `MCP_API_VERSION`) and is
enforced in `crates/kaptein-cli/src/mcp.rs`. The **lens** gate is implemented too:
`kaptein-viewmodel::lens::LENS_SCHEMA_VERSION` + `validate_viewdef` refuse a lens whose
`api_version` differs (via `kaptein viewdef validate`). The **WIT** gate lands with its
engine (M2.6), which is when that contract first exists.

## Change log

Each release is accompanied by an entry in `CHANGELOG.md`, generated from conventional
commits and kept in sync with the tag.

## MSRV

The Minimum Supported Rust Version is **the pinned toolchain in `rust-toolchain.toml`**,
currently `1.97.1`. Policy: MSRV is the current pinned version; a bump happens only when
a dependency raises its own MSRV above it, and CI runs on both the pinned version and
`stable` to catch breakage before the bump.

**Honest status:** the pin is **not currently lagging** — `1.97.1` is effectively latest,
because the codebase uses edition-2024 and `LazyLock`, which no distro-stable channel
carries yet. The *intent* is for the MSRV to lag once distro channels catch up, so
airgapped and distro-toolchain users can build from source; today that intent is
aspirational, not achieved by the chosen value. (Finding AI — the policy text previously
stated the lag as fact rather than intent.)
