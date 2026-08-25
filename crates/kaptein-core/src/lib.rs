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
pub mod delete;
pub mod describe;
pub mod diagnostics;
pub mod discovery;
pub mod ephemeral;
pub mod error;
pub mod events;
pub mod exec;
pub mod extension;
pub mod external;
pub mod guardrails;
pub mod moat;
pub mod nodes;
pub mod overview;
pub mod pods;
pub mod portforward;
pub mod redact;
pub mod store;
pub mod watchring;
pub mod workloads;

pub use error::Error;
