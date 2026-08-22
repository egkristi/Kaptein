# Changelog

All notable changes to Kaptein are documented in this file, kept in sync with releases
(see `docs/versioning.md`). The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-08-22

### Added
- `kaptein cordon/uncordon --name N [--confirm]` — node schedulability toggles
  (dry-run by default, break-glass gate).
- `kaptein evict --name N --namespace X [--confirm]` — pod eviction (dry-run by
  default).
- `kaptein drain --name N` — read-only drain preview classifying pods on a node
  as evictable vs. skipped (DaemonSet/mirror/unmanaged).
- `kaptein krew [--tool krew|kustomize|helm] [-- args...]` — external-tool
  shell-out with graceful degradation (never panics when a tool is absent).
- Named, persistent port-forwards: `kaptein port-forward --name N` (auto-reconnect),
  `kaptein port-forward-list`, `kaptein port-forward-remove`.

## [0.4.0] - 2026-08-22

### Added
- `kaptein scale --gvk X --name Y --replicas N [--confirm]` — scale via the scale
  subresource, server-side dry-run by default (M1.2 k9s parity).
- `kaptein restart --gvk X --name Y --confirm` — rollout restart via the
  `kube.kubernetes.io/restartedAt` annotation (kubectl rollout restart equivalent).
- `kaptein logs --name X --follow` — follow log streaming (kubectl logs -f), with
  optional regex filter (M1.2).
- `kaptein get --sort <col> --descending --filter <substr>` — column sort (name/
  namespace/kind/created) and case-insensitive substring filter (k9s parity).
- `kaptein edit --gvk X --name Y` — `$EDITOR` handoff: fetch YAML, edit, dry-run the
  result (never applies) with server-managed field stripping (M1.3).
- Prod/unknown-context break-glass gate: writes require `--break-glass <reason>`
  unless the context is classified `staging` (M1.1).

### Changed
- CLI write operations (scale/delete/restart) now emit `AuditEvent`s with operation,
  target, outcome (Applied/DryRun), and break-glass reason (ADR-0010).
- MCP server attributes agent identity from `$KAPTEIN_AGENT` and records the real
  context name in the audit `context` field (ADR-0007, Phase 1b).

## [0.3.0] - 2026-08-22

### Added
- `kaptein events --minutes N` — recent cluster events (M1.4), the cheap form of the
  time-machine differentiator (no persistence).
- `kaptein overview --minutes N` — the landing view (M1.5): "is anything broken" +
  "what changed recently" (k9s Pulses equivalent).
- `kaptein apply --file X` — server-side dry-run validation (M1.3); never mutates the
  cluster, returns the server-validated object or the admission/validation rejection.
- `kaptein port-forward --pod X --port N --local M` — bridge a pod port to a local
  TCP listener (M1.2), read-only.
- `kaptein exec --pod X -- cmd...` — one-shot command execution with concurrent
  stdout/stderr streaming (M1.2).
- `kaptein delete --gvk X --name Y [--cascade] [--confirm]` — delete with explicit
  cascade selection, dry-run by default (read-only-default guardrail).

### Changed
- Enabled kube `ws` feature for port-forward/exec transport.

## [0.2.0] - 2026-08-22

### Added
- Governed MCP server (`kaptein mcp`) — read-only Model Context Protocol server over
  stdio (the #1 differentiator), exposing `list_resources`, `describe`, `logs`, and
  `diagnose` tools through the same guardrails as the CLI (ADR-0010, ADR-0013).
- MCP audit-log integration — every tool call writes a JSONL `AuditEvent` with agent
  identity (`source=mcp`), via `KAPTEIN_AUDIT`.
- Read-operation audit variants (`List`, `Describe`, `Logs`, `Diagnose`).
- TUI resource-kind switching (pods/deployments/namespaces), namespace cycling, status
  column, and an in-app detail pane (describe + diagnose).

### Changed
- README status promoted to MVP with concrete build/test instructions.

## [0.1.0] - 2026-08-21

First release with a functional core against a live cluster: generic resource listing,
RBAC preflight, context guardrails, diagnostics, describe, logs, and a TUI table view.
