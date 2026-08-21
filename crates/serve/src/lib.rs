//! Kaptein network server: axum (HTTP/REST + gRPC-Web) and tonic gRPC.
//!
//! Authenticates its own users and uses one of three identity modes per ADR-0007
//! (token forwarding / impersonation / dedicated agent identity).

#![forbid(unsafe_code)]
