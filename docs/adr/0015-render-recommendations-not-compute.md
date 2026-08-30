# ADR-0015: Kaptein renders resource recommendations, it does not compute them

- **Status:** Accepted
- **Date:** 2026-08-30
- **Deciders:** Kaptein maintainers

## Context

"Rightsizing from actual usage" is on the roadmap (M3b.1), and tools like Fairwinds
**Goldilocks** make it look cheap: run the Vertical Pod Autoscaler in recommendation mode
(`updateMode: "Off"`), read `status.recommendation`, render current-vs-recommended. The
temptation is to go one step further and have Kaptein compute the recommendation itself,
so it works without VPA installed.

That step is much larger than it looks. VPA's recommender (`kubernetes/autoscaler`,
SIG-Autoscaling) maintains **decaying histograms per container**, models CPU and memory
differently (CPU as a percentile of the usage distribution, memory as peaks over a
multi-day window — ~8 days by default), persists aggregated state in
`VerticalPodAutoscalerCheckpoint` CRDs so a restart does not lose history, and emits
`target`/`lowerBound`/`upperBound`/`uncappedTarget`. Reproducing that requires a
**per-container usage-history store** at metrics cadence across the fleet.

There is a specific trap in our own roadmap. M3a.1 introduces a local embedded store
(redb) for the time machine, which invites the reasoning *"we already have a store — just
add usage samples."* The two are not the same data: the time machine persists resource
**state** from the watch stream (low-frequency, event-driven, one write per object
change), while a recommender needs **usage samples** at metrics cadence per container.
The write volume and retention profile differ by orders of magnitude. Conflating them
turns Kaptein into a time-series database, which `README.md` lists as a non-goal ("No
metrics/log store. Kaptein queries Prometheus/Loki/etc., it does not store").

## Decision

Kaptein **renders and adjudicates** resource recommendations; it **never computes** them
from its own stored usage history. Concretely, three tiers:

1. **Read VPA recommendations** when the `VerticalPodAutoscaler` CRD is present — a
   cross-resource join between the workload's `resources.requests` and the VPA's
   `status.recommendation.containerRecommendations[]`.
2. **Query a coarse estimate from Prometheus** (e.g. a percentile over a window) when VPA
   is absent — executed live, **nothing stored**, and labelled as the cruder estimate it
   is.
3. **Do not build a recommender.** No usage-history store, no histogram/decay model, no
   checkpointing. That is VPA's job.

Every rendered number **names its source** (VPA / PromQL / none available) and carries a
confidence signal, in the same spirit as the DCGM GPU-attribution honesty note
(`README.md` §8b) and "show the source, never the value" for secrets.

The **moat is adjudication, not computation** — the same split as ADR-0013's MCP taxonomy
(primitives are commodity, diagnostics are the differentiator). Deciding whether a
recommendation is *trustworthy* is the part nobody ships, and it is cross-referencing
work Kaptein is already built for:

- **Provenance** — how much history backs it (VPA checkpoints carry sample counts); two
  hours of samples must not render like eight days.
- **HPA conflict** — an HPA scaling on CPU alongside a VPA recommending CPU on the same
  workload is a documented upstream footgun; Kaptein sees both objects.
- **Staleness from deploys** — the time machine (M3a.1) knows the workload was redeployed
  since the samples were taken, so the recommendation describes the *old* image. Nothing
  else in this space can say that.
- **Pod-level `resources` incompatibility** — VPA states it does not support workloads
  defining pod-level resource stanzas; flag it rather than show a number the admission
  controller will reject.
- **Blast radius** — applying it may make the pod unschedulable, breach a ResourceQuota,
  or change QoS class.
- **Remediation** — open a PR against the owning manifest (M2.3/ADR-0008). Goldilocks
  stops at the number; the loop is the differentiator.

These adjudication rules are **diagnostics rules** (M1.6 engine, extended in M3a.4), not
cost-surface code.

A related split follows: *detecting missing* requests/limits needs no metrics at all and
is a Phase 1 diagnostics rule; *recommending a value* needs VPA or Prometheus and is
Phase 3b. `README.md` §4 currently reads as one feature.

## Consequences

- **Positive:** the non-goal boundary stays intact and the redb layer cannot drift into a
  TSDB; we inherit VPA's tuning instead of re-deriving it; the differentiating work
  (adjudication + PR remediation) is where no competitor is.
- **Positive:** honest degradation — a cluster with neither VPA nor Prometheus gets "no
  recommendation available" rather than a fabricated number.
- **Negative:** out of the box, recommendation quality depends on an external component
  (VPA is not installed on most clusters; GKE ships it, EKS/AKS do not). Tier 2 softens
  this but is genuinely cruder, and we must say so.
- **Negative:** the tier-1 join is not expressible in the current lens schema, which has a
  single `target` GVK. That gap must be closed first (see ADR-0012 — this is a better
  cross-resource acceptance test than the three single-object lenses originally chosen).

## Alternatives considered

- **Build a recommender on the time-machine store** — rejected: violates the
  "no metrics/log store" non-goal, and the store shapes are not compatible (state vs.
  usage samples).
- **Ship a naive p95 and call it rightsizing** — rejected: it is materially weaker than
  VPA's model, and presenting it as equivalent is the class of overclaim the external
  audits keep catching. Available as tier 2 *only* when labelled as an estimate.
- **Vendor VPA's recommender as a library** — rejected: it is a Go controller built around
  its own CRD and checkpoint lifecycle, not a library; embedding it would make Kaptein a
  controller (see the "no agent runtime" non-goal).
- **Run the VPA controller ourselves** (create recommendation-mode VPA objects per
  workload, as Goldilocks does) — rejected: a reconciling controller is out of scope, and
  a VPA object is *configuration*, so it belongs in a PR like any other manifest.
