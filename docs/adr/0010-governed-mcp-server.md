# ADR-0010: Kaptein is a *governed* MCP server, not just an LLM assistant

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

The README's LLM section was 2024-framing: an opt-in diagnostics helper with redaction.
The 2026 landscape has moved. KubeCon EU added a dedicated **Agentics Day**; the hard
problem is not *running* agents but **governing** them — how an agent gets identity, how
it is authorized, and how it is prevented from acting outside its scope when it has
access to the production API. OWASP has published an **MCP Top 10**, and "Shadow MCP" is
a recognized risk. MCP-enabled agents *widen* the permission surface; infrastructure that
simultaneously reduces cognitive load and strengthens access control is a real moat.

Kaptein already builds the entire control layer for humans: RBAC preflight, context
guardrails, read-only default, break-glass, structural redaction, and `AuditEvent`.

## Decision

Expose `kaptein mcp` as a **governed MCP server** where every tool call:

1. passes through the **same guardrails** as a human (context guardrails, read-only
   default, break-glass),
2. is **impersonated as a real Kubernetes identity** via `--as` (see ADR-0007), so the
   cluster sees the agent, not `serve`,
3. lands in the **same `AuditEvent` log**, with the agent as the actor.

Crucially, an agent **never writes to the API server** — it can only open a **PR**, the
same GitOps write path as a human (ADR-0008). This is the safest agent-write path that
has been proposed, and the infrastructure already exists.

## Consequences

- **Positive:** this is the answer to Shadow MCP — governed, auditable, scoped agent
  access that reuses the human control plane. It composes with the GitOps differentiator
  and becomes the **fifth differentiator**.
- **Negative:** the MCP surface is a new, stable API and must carry the same versioning
  discipline as WIT worlds (ADR-0004); "every tool call → same guardrails" must be
  enforced, not assumed.
- **Non-goal boundary:** Kaptein does **not run agents** (that is `kagent`'s job); it is
  the *governed tool surface* agents call.

## Alternatives considered

- **LLM assistant only (status quo)** — rejected: 2024 framing, misses the governance
  problem that now dominates.
- **Ungoverned MCP passthrough** — rejected: reintroduces exactly the Shadow MCP risk
  this ADR exists to close.
