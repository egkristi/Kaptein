//! The optional audit sink.
//!
//! The local audit log is a file on each laptop, which is **not** an audit trail for a
//! reviewer. An optional `AuditSink` — syslog, OTLP, or webhook — forwards audit events,
//! buffered locally during downtime. This is the hook that makes guardrails, break-glass,
//! RBAC preflight, and agent governance mean something in a team, and it is the concrete
//! hook against CRA / NIS2 / DORA.

use serde::{Deserialize, Serialize};

/// Where audit events are forwarded, beyond the local log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuditSink {
    Syslog {
        facility: String,
    },
    Otlp {
        endpoint: String,
    },
    Webhook {
        url: String,
    },
    /// No forwarding — local log only.
    #[default]
    Local,
}

/// Configuration for audit forwarding, per context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditConfig {
    pub sink: AuditSink,
    /// Buffer locally (and replay) while the sink is unreachable.
    pub buffer_when_unreachable: bool,
}
