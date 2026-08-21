# ADR-0007: Identity model for `serve`/hub mode

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

ADR-0002 routes the browser through `serve`, which holds cluster credentials on behalf of
multiple users. If `serve` acts with its own ServiceAccount, then RBAC preflight and
action-greying reflect `serve`'s permissions, not the end user's — and "grey out what you
can't do" becomes false in browser and hub mode. This is the largest privilege-escalation
surface in the design and was previously unaddressed.

## Decision

Three identity modes, chosen per deployment. **Impersonation is not the default; it is
one option.**

1. **Token forwarding (default for human browser access).** The browser authenticates
   with its own OIDC token — the same one it would use with `kubectl` — and `serve`
   forwards it as a bearer token on the outgoing request **without storing it**. The API
   server sees the real user natively; RBAC preflight is trivially correct; audit is
   native; and the `impersonate` verb is never needed. *Forwarding is not storage.*
   - Limits: token lifetime/refresh must be handled, `serve` holds a live token in
     memory only for the duration of the request, and it does not work for
     exec-credential-plugin auth where credentials are minted client-side.
2. **Impersonation (where policy permits).** `serve` authenticates its own users and
   impersonates them via `--as`/`--as-group`. Used for the hub relaying human actions in
   environments that allow the `impersonate` verb.
3. **Dedicated agent identity (default for MCP).** Each registered agent gets its **own
   ServiceAccount** with its own narrow RBAC. Better governance on every axis: the agent
   has its own identity in the cluster, its own RBAC surface, its own audit actor, and
   can be revoked without touching humans.

`serve` itself holds a **minimal bootstrap identity** (the least privilege it needs for
the chosen mode) — never cluster admin.

## Consequences

- **Positive:** RBAC preflight stays truthful across TUI, GUI, browser, and hub; least
  privilege for `serve`; audit events carry the real actor; the browser/hub path no
  longer degrades to read-only in environments that deny `impersonate`.
- **Negative:** mode 1 requires OIDC (normal for browser/agent scenarios, not for
  exec-credential auth); mode 3 requires per-agent ServiceAccount provisioning.
- **Security model:** documented in `SECURITY.md`'s threat model as a first-class section.

## Alternatives considered

- **Impersonation only** — rejected: `impersonate` is a privilege-escalation primitive
  often denied by policy, which would force read-only in the environments that need
  governed access most.
- **Per-user credentials stored in `serve`** — rejected: credential storage is a non-goal
  and an unacceptable risk surface.
- **`serve` acts with its own ServiceAccount** — rejected: breaks RBAC-preflight truth,
  widens the blast radius, and breaks the "audit records operations per actor" guarantee.
