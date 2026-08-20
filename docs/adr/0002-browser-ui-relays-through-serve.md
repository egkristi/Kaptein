# ADR-0002: Browser UI relays through `serve`, not direct API-server access

- **Status:** Accepted
- **Date:** 2026-08-20
- **Deciders:** Kaptein maintainers

## Context

The `frontend-gui` crate targets WASM to provide a browser UI. `kube-core` depends on
`kube-rs` + `tokio` + TLS (hyper/rustls). Compiling a Kubernetes client to
`wasm32-unknown-unknown` is possible but awkward: browser networking lacks raw TCP, so
a client must tunnel via WebSocket/HTTP, and TLS, DNS, and cert/auth handling all have to
be reimplemented or proxied. The docs previously left this unspecified.

## Decision

The browser UI is a **thin client to `serve`**. The WASM bundle talks to the `serve`
backend (axum + tonic) over a defined HTTP/gRPC API, and `serve` owns the actual
Kubernetes connection via `kube-core`.

## Rationale

- **Reuses the headless path.** `serve` already exists as the headless/agent projection,
  so the browser becomes just another consumer of the same render-intent over the wire.
- **Matches real deployment topology.** Operators use the browser over SSH/bastion or
  against a central hub; direct browser→API-server access is rare and credential-hostile.
- **Keeps `kube-core` native-only.** No wasm-specific transport, TLS, or auth hacks.

## Consequences

- **Positive:** one Kubernetes client implementation (native), a clean
  browser→`serve`→`kube-core` path, and a natural place for hub/agent mode (M3.2).
- **Negative:** the browser UI is not standalone — it requires a running `serve`.
- **NFR impact:** "one static binary" applies to native + headless + `serve`; the browser
  UI is a wasm bundle served by `serve`, not a separate binary.

## Alternatives considered

- **Direct browser→API-server via wasm transport** — rejected for complexity and
  credential-handling risk.
- **WebSocket-only relay** — considered; gRPC (tonic) + HTTP chosen for a richer, typed
  surface that already matches the `serve` crate.
