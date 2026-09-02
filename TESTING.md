# Testing Kaptein against real clusters

Kaptein's unit tests are hermetic and its live tier
(`crates/kaptein-core/tests/live.rs`, gated on `KAPTEIN_LIVE_TESTS=1`) runs in CI against a
throwaway `kind` cluster on a latest-three-minors matrix. That covers correctness of the
paths we wrote tests for. It does **not** cover the thing this document is about: whether
Kaptein behaves sensibly against the *variety* of clusters, distros, auth mechanisms, and
operator ecosystems real operators actually have.

This is a catalogue of clusters you can get for free (or nearly), and — more usefully — a
map of **which Kaptein features each tier can actually verify**.

---

## Read this first: public sandboxes are the *least* useful tier for Kaptein

The instinct is to reach for a browser-based playground. For this project that is mostly a
dead end, and it is worth knowing why before spending time on it:

- **The live tier creates a namespace and performs real deletes, evictions, and scales.**
  Shared sandboxes rarely grant the rights, and doing it on someone else's cluster is
  rude at best.
- **Guardrail testing needs a context you can *name*.** Prod classification is a regex over
  the kubeconfig context name (`[guardrails] prod = [...]`), so verifying break-glass means
  controlling the context, which a hosted playground doesn't give you.
- **Lens testing needs CRD installation** — cluster-scoped, usually not permitted.
- **Fleet and drift (M3a.2) need several clusters at once.**
- **Auth is the one thing local clusters cannot test at all.** kind/k3d/minikube all use a
  client certificate. Exec credential plugins (`kubelogin`, `aws eks get-token`,
  `gcloud`), OIDC, and SA-token auth — all named in M1.1/M1b.2 — only appear on a real
  managed cluster.

So: **Tier A for almost everything, Tier B specifically for auth, Tier C for smoke tests
and demos only.**

---

## Tier A — disposable local clusters (the workhorse)

Full admin, free, reproducible, destroyable. This is where 90 % of feature verification
should happen.

| Tool | Distro / flavour | Why it earns a slot |
|------|------------------|---------------------|
| [kind](https://kind.sigs.k8s.io/) | upstream kubeadm | Already our CI baseline. Multi-node, pinned versions, swappable CNI. |
| [k3d](https://k3d.io/) / [k3s](https://k3s.io/) | **k3s** | A genuinely *different* distro: Traefik ingress, ServiceLB, `local-path` StorageClass, optional SQLite datastore. Catches assumptions baked in against kubeadm defaults. |
| [minikube](https://minikube.sigs.k8s.io/) | upstream | Best add-on library — `metrics-server`, `ingress`, `csi-hostpath-driver`, `registry`, `volumesnapshots`. Fastest way to get a CSI/snapshot surface. |
| [MicroK8s](https://microk8s.io/) | Canonical | Add-ons for MetalLB, cert-manager, observability. A third set of defaults. |
| [Talos](https://www.talos.dev/) (`talosctl cluster create`) | Talos Linux | Immutable, API-driven, **no SSH and no shell on nodes**. Excellent stress test for anything that assumes node access. |
| [k0s](https://k0sproject.io/) | k0s | Another packaging with its own defaults. |
| [OpenShift Local](https://developers.redhat.com/products/openshift-local/) (was CRC) | **OpenShift** | Single-node OpenShift, free with a Red Hat developer account. The only easy way to exercise OLM, `Route`, `SecurityContextConstraints`, and the OpenShift API surface the README positions against. |
| [kwok](https://kwok.sigs.k8s.io/) (`kwokctl`) | simulated | **Directly the M1.8 harness.** Thousands of fake nodes/pods with no kubelets — the only practical way to hit the 50 000-object performance budget. |
| [vcluster](https://www.vcluster.com/) | virtual | Many isolated clusters on one host cluster. The cheap path to **M3a.2 fleet + drift matrix** testing without N real clusters. |
| Rancher Desktop / Podman Desktop / Docker Desktop | k3s or kind | Convenient; also what many users will actually run Kaptein against. |

**Minimum useful spread:** kind (kubeadm) + k3d (k3s) + OpenShift Local (OpenShift) +
kwok (scale). Those four cover more real-world variance than a dozen of the same thing.

---

## Tier B — free or cheap *real* managed clusters (the only way to test auth)

Prices change; treat the cost column as "last checked, verify before relying on it".

| Provider | Control plane | Free allowance | Why it matters to Kaptein |
|----------|---------------|----------------|---------------------------|
| **Oracle OKE** | Free (basic clusters) | **Always Free**: 2 ARM oCPU / 12 GB across worker nodes | The only genuinely *permanent* zero-cost managed cluster. Best default for a long-lived test cluster. |
| **Azure AKS** | Free tier: free | Signup credit | **Entra ID / `kubelogin` exec credentials** — named explicitly in M1.1 and untestable anywhere else. |
| **Google GKE** | One zonal/Autopilot cluster's management fee covered | Signup credit | `gcloud` exec credentials; **VPA is built in**, so it is the fastest route to testing ADR-0015 rightsizing against real recommendations. |
| **AWS EKS** | ~$0.10/hr — *not* free | Signup credit | `aws eks get-token` exec credentials, IRSA, and **Karpenter**, which cannot be meaningfully exercised on a local cluster. |
| **DigitalOcean DOKS** | Free | Signup credit | Cheap, fast to create/destroy; plain upstream-ish. |
| **Akamai/Linode LKE**, **Civo** (k3s), **Scaleway Kapsule**, **Vultr VKE** | Free | Signup credit | Cheap extra clusters for fleet/drift; Civo is k3s-flavoured. |

> **Cost discipline:** control-plane-free does not mean free — worker nodes bill by the
> hour. Destroy clusters after a session, or use Oracle's Always Free tier for anything
> long-lived.

---

## Tier C — public browser playgrounds (smoke tests and demos only)

| Service | Status | Notes |
|---------|--------|-------|
| [Killercoda](https://killercoda.com/playgrounds) | Alive | Free 60-minute sessions with a login; single- and multi-node Kubernetes scenarios. Usable for `kaptein get`/`describe`/`logs` smoke tests and screen recordings. |
| [Play with Kubernetes](https://labs.play-with-k8s.com/) | **Retired — shut down 1 March 2026** | Listed only so nobody wastes time on stale blog links pointing at it. |
| [Argo CD public demo](https://cd.apps.argoproj.io/) | Alive | **Web UI only — no kubeconfig**, so Kaptein cannot connect to it. Useful for *reading* what real Argo CRDs look like when authoring a lens; useless as a test target. |
| Google Cloud Skills Boost, KodeKloud, Instruqt | Varies | Credit- or subscription-gated temporary clusters. Fine for exploration, awkward for repeatable testing. |

---

## What to install to exercise Kaptein's actual features

A bare cluster verifies very little. The value is in the *services* layered on top.

### The shipped lens set (M2.2 — `extensions/`)

| Lens | Install | Cluster needed |
|------|---------|----------------|
| cert-manager | Helm chart | any |
| Strimzi (Kafka) | Helm / operator YAML | any (needs a few GB) |
| CNPG (Postgres) | Helm chart | any |
| Tekton | release YAML | any |
| Knative | `kn quickstart kind` — one command | kind |
| Velero | Helm + **MinIO** for object storage | any |
| KubeVirt | operator YAML; needs nested virt or `useEmulation` | kind with nested virt |
| Karpenter | — | **EKS/AKS only**; not meaningfully testable locally |

Installing all of these on one kind cluster is the single highest-value test of the lens
engine — it is also the honest test of *lens discovery* (drop a lens file in, does the CRD
become navigable with no recompile?).

### Realistic workloads (for the table, diagnostics, and topology)

- [Online Boutique](https://github.com/GoogleCloudPlatform/microservices-demo) — ~11
  microservices; good for blast-radius and the landing view.
- [Sock Shop](https://github.com/microservices-demo/microservices-demo) — classic, polyglot.
- [podinfo](https://github.com/stefanprodan/podinfo) — tiny, HPA/ingress-friendly.
- Istio's Bookinfo — if/when the mesh surface lands (M3b.3).

### Deliberately broken workloads (for the M1.6 rule pack)

The fixture corpus covers pod *shapes*; a live broken cluster covers the wiring. Create
one pod per rule and confirm each `Finding` code appears:

`no_status` · `pending` · `unschedulable` (huge resource request) · `taint` ·
`image_pull` (bad image tag) · `resource_pressure` · `pvc_binding` (missing PVC) ·
`readiness_probe` (probe on a closed port) · `crash_loop` / `crash_loop_backoff`
(`exit 1`) · `oom_killed` (tight memory limit + allocator) · `init_container_error` ·
`no_requests` / `no_limits`

### Kaptein-specific surfaces

| Feature | How to create the condition |
|---------|-----------------------------|
| **Guardrails / break-glass** (M1.1) | Rename a kubeconfig context to match your `[guardrails] prod` regex; confirm writes refuse without `--break-glass`. |
| **RBAC preflight + MCP governance** (M1b.4) | Create a ServiceAccount with deliberately narrow RBAC, build a kubeconfig from its token, and confirm the governed surface *refuses* — including for a CRD, which is where pluralization bugs hide. |
| **Secret redaction** (M1.7) | `kubectl apply` a Secret (so it carries `last-applied-configuration`) and confirm `describe` and MCP `describe` mask both `data` *and* the annotation. |
| **Log redaction** (M1.7) | Run a pod that logs `password=hunter2`, `Authorization: Bearer …`, and a JSON `{"api_key":"…"}`. |
| **Watch resilience** (M2.0c) | Delete *and* create objects while the watch is down (restart the API server, or block it) and confirm both directions reconcile — ghosts removed, new objects appear. |
| **Informer cap** (M2.0c) | Set `[informer] max_watches = 2`, cycle through more views than that, and confirm degradation to on-demand list — and that the view you are *looking at* survives eviction. |
| **Rightsizing** (M3b.1 / ADR-0015) | GKE (VPA built in), or install VPA + metrics-server on kind. Confirm Kaptein *renders* the recommendation and never computes one. |
| **Fleet / drift** (M3a.2) | Several kind clusters or vcluster instances with deliberately divergent manifests. |
| **Scale budget** (M1.8) | `kwokctl` with thousands of fake nodes/pods. |

---

## Running the live tier

```bash
KAPTEIN_LIVE_TESTS=1 KUBECONFIG=~/.kube/config cargo test -p kaptein-core --test live
```

The tier is non-destructive and self-cleaning — it creates a throwaway namespace, exercises
dry-run and real write paths inside it, and tears everything down. It never touches
existing namespaces. Even so, **point it at a disposable cluster**, never at anything you
would mind losing.

---

## Known gaps in coverage

Recorded here so the list is honest about what it does *not* yet prove (see `ISSUES.md`
finding AB and the M2.0b milestone):

- **port-forward**, **the MCP protocol**, and **the `kaptein` binary itself** have no live
  coverage — every live test drives the library, so argument parsing, the
  `--confirm`/`--break-glass` wiring, and the audit-file write are untested end to end.
- **cordon/uncordon** are skipped as "would mutate a real node" — true of a shared cluster,
  no longer true now the tier runs on a throwaway kind node.
- **No managed cluster is in CI**, so no exec-credential auth path (AKS/EKS/GKE) is
  exercised automatically. That is a manual, pre-release check today.
