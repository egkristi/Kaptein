//! The single write-audit record: one format, two consumers.
//!
//! Used by both the local audit log and the incident-timeline export. Records
//! **operations, not values** — secrets are never persisted here. Fully `serde`-
//! serializable so it can cross the `serve`/gRPC-Web boundary and be exported.

use serde::{Deserialize, Serialize};

/// A reference to a Kubernetes resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResourceRef {
    pub group: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

/// The operation that was performed. `McpToolCall` is intentionally **absent**: MCP is a
/// *transport*, captured in `AuditEvent::source`, not a distinct operation. An agent that
/// scales a deployment logs `Operation::Scale` with `source: Surface::Mcp`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operation {
    // Read operations (governance requires visibility into reads, not just writes).
    List,
    Describe,
    Logs,
    Diagnose,
    // Write operations.
    Delete,
    Scale,
    Restart,
    Cordon,
    Drain,
    Evict,
    /// Attach an ephemeral container to a running pod (`kaptein debug`). Distinct from
    /// [`Operation::Exec`], which runs a command in an *existing* container — conflating
    /// the two made the audit log's one `Exec` record impossible to attribute (finding V).
    EphemeralAttach,
    /// Run a command in an existing pod container (`kaptein exec`).
    Exec,
    PortForward,
    /// A GitOps write path action (branch + PR), not an API-server write.
    GitPrOpened,
    /// An operator viewed (unmasked) a secret — the single most audit-relevant event for
    /// a tool that masks secrets by default.
    SecretViewed,
}

impl Operation {
    /// Whether this operation mutates the cluster or opens a channel into a pod — the
    /// set that must be **gated and audited** when performed through the CLI.
    ///
    /// The CLI's governance coverage test derives its assertion from this method rather
    /// than hand-enumerating subcommands, which is how `exec` slipped past the gate
    /// (finding U): a mutating operation that is never emitted by any subcommand is a
    /// hole, and a subcommand that declares `--confirm` without `--break-glass` (or vice
    /// versa) is a hole. Both are caught by reflecting over this set + the clap command
    /// tree, so a new mutating operation fails CI until it is wired up.
    pub fn is_governed(&self) -> bool {
        !matches!(
            self,
            // Reads (visibility is audited via List/Describe/Logs/Diagnose, but they do
            // not require a write gate).
            Operation::List
                | Operation::Describe
                | Operation::Logs
                | Operation::Diagnose
                | Operation::SecretViewed
                // Preview-only today: `drain` never evicts (no live write to gate).
                | Operation::Drain
                // A Git PR is a branch + review, not an API-server write.
                | Operation::GitPrOpened
        )
    }
}

/// The outcome of a write attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Applied,
    DryRun,
    Rejected,
}

/// Which projection initiated the action. MCP is a *source*, not an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Tui,
    Gui,
    Browser,
    Mcp,
    Headless,
}

/// The actor who performed the operation. An agent has its **own** identity, so agent
/// actions are distinguishable from human actions (ADR-0007, ADR-0010).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Human,
    Agent,
}

/// A single audit record. Serialized with `serde`; the same shape feeds the incident
/// timeline export (one format, two consumers).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unix epoch milliseconds — a typed instant, not a pre-formatted string, so it
    /// sorts and localizes correctly.
    pub timestamp: i64,
    pub actor: Actor,
    /// Cluster/context id — never a secret.
    pub context: String,
    pub operation: Operation,
    pub target: ResourceRef,
    pub outcome: Outcome,
    /// Which projection initiated the action (MCP is a source, not an operation).
    pub source: Source,
    /// Identifies the debugging session / multi-step agent invocation, so the incident
    /// timeline can group related events.
    pub session_id: String,
    /// Recorded break-glass justification (required for the break-glass guardrail to be
    /// a complete control).
    pub reason: Option<String>,
    /// Who initiated the session on the agent's behalf (an agent acts under its own
    /// ServiceAccount, but the audit question is still "who asked").
    pub on_behalf_of: Option<String>,
}
