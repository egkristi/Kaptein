//! Kaptein core: the Kubernetes client, watchers/reflectors, CRD discovery, and stores.
//!
//! This crate owns the Kubernetes data plane. It must **not** depend on
//! `kaptein-viewmodel` or any frontend — layer dependencies are strictly one-directional
//! (see ADR-0005 / `docs/architecture.md`).
//!
//! Scaffold: no implementation yet. See the roadmap (Phase 0 / M1.x).

#![forbid(unsafe_code)]

pub mod apply;
pub mod auth;
pub mod config;
pub mod describe;
pub mod diagnostics;
pub mod discovery;
pub mod error;
pub mod events;
pub mod guardrails;
pub mod overview;
pub mod pods;
pub mod portforward;

pub use error::Error;
