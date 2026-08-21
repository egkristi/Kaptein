# ADR-0013: MCP tool taxonomy — diagnostics over primitives

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

Phase 1b ships a read-only `kaptein mcp` as the **first** distributable artifact, which
makes agents the **first** users and the MCP tool surface the **first** stable contract —
before the TUI. There was no ADR for what the tools *are*, and that is the most
consequential design decision in Phase 1b.

## Decision

The MCP tool surface is divided into two tiers, and the moat is the **second**:

1. **Primitives** — `list_resources`, `describe`, `get_logs`, `get_events`. These are
   what every other Kubernetes MCP server already does. Commodity; Kaptein competes on
   nothing here. They exist for completeness, not differentiation.
2. **Diagnostics** — `explain_pod_failure`, `what_changed_between`, `blast_radius`,
   `why_is_job_pending`. These are things an agent **cannot** assemble from raw `kubectl`
   calls, because they require correlation across events, scheduler reasons, history, and
   owner hierarchy. This is the moat — and the only entry point where "governed MCP" is
   more than an access-control wrapper around `kubectl`.

Every diagnostic tool is backed by the **`kaptein-diagnostics` subsystem** (a
plug-in rule engine whose rule packs are lenses — see ADR-0012), so the MCP diagnostics
tool is not a special case but the same engine the TUI and GUI use.

## Consequences

- **Positive:** Phase 1b is interesting, not merely early; the diagnostic tools exercise
  the rule engine (and therefore the lens schema) harder than any single view.
- **Negative:** diagnostic tools are higher-effort than primitives; the taxonomy must be
  documented and versioned as a public API before the first release.
- **Versioning:** the tool taxonomy is one of the three versioned contracts in
  `docs/versioning.md`.

## Alternatives considered

- **Primitives only** — rejected: commodity, no differentiation, no reason to exist as
  anything but a thin wrapper.
- **Diagnostics only** — rejected: primitives are a cheap on-ramp and a completeness
  baseline agents expect.
