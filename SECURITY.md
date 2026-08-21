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

**Do not open a public issue.** Report security issues privately via **GitHub private
vulnerability reporting** (enabled in repo settings → Security → Private vulnerability
reporting). We will acknowledge within 3 business days and aim to publish a fix and
advisory for confirmed issues in supported versions.

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

### `serve` / hub identity (three modes)

`serve` holds cluster credentials on behalf of multiple users (browser and hub modes).
To keep RBAC preflight truthful for the *actual* caller, it uses one of three identity
modes (ADR-0007):

1. **Token forwarding (default for human browser access)** — the browser's own OIDC
   token is forwarded as a bearer token, **not stored**. The API server sees the real
   user natively; the `impersonate` verb is never needed.
2. **Impersonation** (where policy permits) — `serve` impersonates via `--as`/`--as-group`
   for hub-relayed human actions.
3. **Dedicated agent identity** (default for MCP) — each agent has its own ServiceAccount
   with narrow RBAC.

`serve` holds a **minimal bootstrap identity** (least privilege for the chosen mode) —
never cluster admin. Audit events record the **real actor** (user or agent), not `serve`.

This is the largest privilege-escalation surface in the design; it is specified in
ADR-0007 and must not be weakened.

### LLM assistance (opt-in only)

- Disabled by default, and enabled **per context**, never globally.
- Secrets are **structurally redacted** (CRD schema + well-known secret keys such as
  `env`, `data`, and annotations) — not regex-only.
- The exact payload sent to any model endpoint is shown for review before it leaves the
  machine; a **local endpoint is supported** for airgapped deployments.

### Governed MCP surface (`kaptein mcp`)

This is the answer to **Shadow MCP** (OWASP MCP Top 10). It reuses the human control
plane, not a new one:

- Every agent tool call passes through **the same guardrails**: RBAC preflight, context
  guardrails, read-only default, and break-glass.
- Agents run under a **dedicated agent identity** (own ServiceAccount, narrow RBAC) —
  **not** impersonated as the operator, so the audit log distinguishes agent actions
  from human actions.
- Agent calls land in the **same `AuditEvent` log**, with the agent as the actor.
- An agent **never writes to the API server** — the only write path is a PR (ADR-0010,
  ADR-0008).
- Kaptein does **not run agents**; it is the governed surface they call.

### Supply chain

- Releases are **signed** and ship an **SBOM**.
- Dependencies are minimized and pinned; scanner integrations (Trivy/Grype, etc.) are
  shelled out to, never vendored.
