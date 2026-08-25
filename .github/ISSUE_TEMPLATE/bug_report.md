---
name: Bug report
about: Report a defect in Kaptein
title: ""
labels: bug
assignees: ""
---

## Describe the bug

A clear, concise description.

> **Security:** if this involves secret leakage, a privilege-escalation path, or a
> governance/RBAC gap, do **not** open a public issue — use private vulnerability
> reporting instead (see `SECURITY.md`).

## Steps to reproduce

1.
2.
3.

## Expected vs. actual behavior

## Environment

- OS:
- Kaptein version / commit (`kaptein --version`):
- Kubernetes version (`kubectl version --short`):
- kubeconfig auth type (kubeconfig / exec credential / OIDC / SA token):
- Frontend (TUI / CLI / `kaptein mcp`):

## Diagnostic context

- If it is a "why isn't this pod ready?" misdiagnosis, paste the pod's `status`
  (with any Secret values redacted) or a minimal fixture JSON that reproduces it.
- If it is a TUI/render issue, note whether it reproduces in the CLI headless path too.

## Additional context
