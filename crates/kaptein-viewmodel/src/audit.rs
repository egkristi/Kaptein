//! The single write-audit record: one format, two consumers.
//!
//! Used by both the local audit log and the incident-timeline export. Records
//! **operations, not values** — secrets are never persisted here.

/// A reference to a Kubernetes resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceRef {
    pub group: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
}

/// The operation that was performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Delete,
    Scale,
    Restart,
    Cordon,
    Drain,
    Evict,
    Exec,
    PortForward,
    /// A GitOps write path action (branch + PR), not an API-server write.
    GitPrOpened,
    /// An MCP tool call, distinguished by the actor being an agent.
    McpToolCall,
}

/// The outcome of a write attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    DryRun,
    Rejected,
}

/// The actor who performed the operation. An agent has its **own** identity, so agent
/// actions are distinguishable from human actions (ADR-0007, ADR-0010).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub kind: ActorKind,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    Human,
    Agent,
}

/// A single audit record. Serialized with `serde`; the same shape feeds the incident
/// timeline export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub timestamp: String,
    pub actor: Actor,
    /// Cluster/context id — never a secret.
    pub context: String,
    pub operation: Operation,
    pub target: ResourceRef,
    pub outcome: Outcome,
}
