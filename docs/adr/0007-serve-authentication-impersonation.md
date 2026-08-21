# ADR-0007: Authentication and impersonation in `serve`/hub mode

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

- **`serve` authenticates its own users** (TLS client certs or OIDC tokens, per
  deployment policy) and then **impersonates the authenticated user** against the cluster
  via Kubernetes `--as` / `--as-group` impersonation headers.
- **RBAC preflight runs as the impersonated user** (`SelfSubjectRulesReview`), so
  action-greying is correct for the actual caller in every frontend.
- **`serve` itself holds a minimal bootstrap identity** whose only privilege is the
  ability to impersonate (`impersonate` RBAC verb) — never full cluster admin.

## Consequences

- **Positive:** RBAC preflight stays truthful across TUI, GUI, browser, and hub; least
  privilege for `serve`; audit events carry the real actor, not `serve`.
- **Negative:** impersonation must be explicitly granted in each cluster's RBAC; not all
  environments allow it. Where impersonation is unavailable, the behavior is **read-only
  with a clear warning**, not silent escalation.
- **Security model:** this is documented in `SECURITY.md`'s threat model as a first-class
  section.

## Alternatives considered

- **`serve` acts with its own ServiceAccount** — rejected: breaks RBAC-preflight truth,
  widens the blast radius, and breaks the "audit records operations per actor" guarantee.
- **Per-user credentials stored in `serve`** — rejected: credential storage is a non-goal
  and an unacceptable risk surface.
