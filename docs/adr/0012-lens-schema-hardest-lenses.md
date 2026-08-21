# ADR-0012: Validate the lens schema against the three hardest lenses first

- **Status:** Accepted
- **Date:** 2026-08-21
- **Deciders:** Kaptein maintainers

## Context

ADR-0004 introduced view definitions (lenses) but warned that freezing the schema early
is expensive. The 2026 landscape introduces workloads no console renders well: DRA
(`ResourceSlice`/`ResourceClaim`/`DeviceClass`, GA in 1.34), KubeVirt (a VMware→K8s
migration wave), and CNPG (primary/replica topology, failover, PITR). These are the
hardest lenses — DRA has new first-class resources, KubeVirt needs VM vocabulary
(console, live-migration, snapshots, instance types), CNPG needs topology + replication
lag + WAL state.

## Decision

**Do not freeze the view-definition schema in Phase 0.** Design it in late Phase 2
against the three hardest lenses — **DRA/Kueue/inference**, **KubeVirt**, and **CNPG** —
as acceptance tests. If any of those three cannot be expressed in the schema, the schema
is too weak and must be extended before it is versioned as stable.

## Consequences

- **Positive:** the schema is proven by hard cases before it becomes a contract; every
  capability in the roadmap is *by construction* expressible as a lens (the boundary
  test for the whole product).
- **Negative:** delays the stable schema; early lens authoring must tolerate a moving
  schema (mitigated by keeping the schema unversioned-until-stable).

## Alternatives considered

- **Freeze the schema in Phase 0** — rejected: would either lock a too-weak shape or
  break the versioning promise (see ADR-0004).
