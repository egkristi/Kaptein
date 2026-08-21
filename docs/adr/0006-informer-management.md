# ADR-0006: Informer resource management

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

"Informer-based, never polling" was stated as a rule with no budget. A naive
one-reflector-per-resource-type-per-namespace strategy can melt an API server faster than
polling would — with hundreds of CRDs and hundreds of namespaces, the number of
simultaneous watches is unbounded. On a large cluster, unmanaged informer use is *worse*
than polling.

## Decision

Adopt an explicit informer management strategy:

- **Lazy informers per view** — a reflector starts when a view needs it, not eagerly for
  the whole world.
- **LRU eviction with TTL** — idle reflectors are evicted after a TTL; hot views keep
  theirs.
- **`PartialObjectMetadata` as the default** for list-heavy views (metadata only, no full
  object bodies).
- **Label/field selectors** where the view is scoped, to keep the watch narrow.
- **A hard cap on concurrent watches**, with **degradation to on-demand list** when the
  cap is reached, rather than exceeding it.

## Consequences

- **Positive:** bounded API-server load; predictable memory; works at fleet scale.
- **Negative:** the cap requires a policy (per-cluster vs. global, configurable) that
  must be exposed in the config file.
- **Performance budget link:** "simultaneous watches ≤ N for a given view set" becomes a
  measurable CI criterion (see roadmap performance budget).

## Alternatives considered

- **Eager informers for everything** — rejected: unbounded watches, melts the API server.
- **Polling** — rejected: violates the core rule and is worse for freshness/scale.
