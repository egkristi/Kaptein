# Security Policy

Kaptein "practices what it scans for": signed releases with an SBOM, no telemetry, no
account, and an airgap-safe single binary.

## Supported versions

We target the latest three Kubernetes minors. Older API versions are handled through the
discovery API's served versions.

| Version | Supported |
|---------|-----------|
| Latest 3 minors | ✅ |
| Older | Best-effort |

## Reporting a vulnerability

**Do not open a public issue.** Report security issues privately to the maintainers.
We will acknowledge within 3 business days and aim to publish a fix and advisory for
confirmed issues in supported versions.

*(Insert the private reporting channel / security contact here before the first release.)*

## Threat model

The core risk surface is that Kaptein holds **cluster credentials and audit-grade write
access** on an operator's workstation. Our mitigations:

### Secrets & credentials

- Secrets are **masked by default**; ESO/Vault/SOPS integrations display the *source*,
  never the value.
- `kubeconfig` and exec-credential plugin output are never persisted by Kaptein beyond
  the user's own config.
- The audit log records *operations*, not secret values.

### Write safety

- **Read-only default** for unknown contexts; **context guardrails** (red frame, "break
  glass" confirmation) for prod contexts configured by regex.
- **RBAC preflight** via `SelfSubjectRulesReview` greys out disallowed actions before
  they're attempted.
- The GitOps write path writes to **Git, not the API server**, and requires an explicit
  PR review, adding a second human gate before any live change.

### LLM assistance (opt-in only)

- Disabled by default, and enabled **per context**, never globally.
- Secrets are **structurally redacted** (CRD schema + well-known secret keys such as
  `env`, `data`, and annotations) — not regex-only.
- The exact payload sent to any model endpoint is shown for review before it leaves the
  machine; a **local endpoint is supported** for airgapped deployments.

### Supply chain

- Releases are **signed** and ship an **SBOM**.
- Dependencies are minimized and pinned; scanner integrations (Trivy/Grype, etc.) are
  shelled out to, never vendored.
