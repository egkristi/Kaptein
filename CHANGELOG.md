# Changelog

All notable changes to Kaptein are documented in this file, kept in sync with releases
(see `docs/versioning.md`). The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Project planning documents (README, ROADMAP, architecture, ADRs, security, contributing).
- Three-tier extension model (ADR-0004).
- Governed MCP surface design (ADR-0010).
- AI/GPU (DRA/Kueue/inference) and KubeVirt lens designs.
- Business Source License 1.1 with rolling MIT conversion.
- `kaptein` CLI: `get`, `can` (RBAC preflight), `context` (guardrails), `diagnose`, `describe`, `logs`.
- `kaptein-tui` ratatui table view with vim navigation.
- Diagnostics rule engine ("why isn't this pod ready").

### Changed
- License: MIT → BUSL-1.1 (core) + MIT/Apache-2.0 (extension surface).
- Render contract redefined as three layers (ADR-0005).

### Fixed
- Architecture diagram, framework choice (egui), and crate naming.

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
