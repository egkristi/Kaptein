//! Governed MCP server — `kaptein mcp`.
//!
//! A read-only Model Context Protocol server over stdio that exposes the Kubernetes
//! primitives (`list_resources`, `describe`, `logs`, `diagnose`) through the *same*
//! guardrails as the CLI and TUI (ADR-0010, ADR-0013). Every tool call is audited and
//! — for now — the server is strictly read-only: it never writes to the API server.
//!
//! This is the first deliverable of Phase 1b, built on the M1.6 diagnostics engine.

use kaptein_core::{Error as CoreError, diagnostics, discovery};
use kube::core::GroupVersionKind;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};

/// The Kaptein MCP server: holds a Kubernetes client, tool state, and agent identity.
pub struct KapteinMcp {
    client: kube::Client,
    /// The dedicated agent identity (ADR-0007): `$KAPTEIN_AGENT` or the current
    /// context name. Agent actions are attributed to this identity, not a human.
    agent_name: String,
    /// The current context name (for the audit `context` field).
    context_name: String,
    /// A per-session identifier (a fresh, non-constant value per server instance), so the
    /// incident-timeline can group related events (ADR-0010).
    session_id: String,
}

impl KapteinMcp {
    pub async fn new() -> Result<Self, CoreError> {
        // Dedicated agent identity (ADR-0007 mode 3): the MCP server runs with its own
        // ServiceAccount/token, not a shared human credential. Falls back to the default
        // kubeconfig when no agent identity is configured.
        let agent_name = discovery::agent_identity_name();
        let context_name = discovery::current_context_name().unwrap_or_default();
        let session_id = format!("mcp-{}", now_ms_hex());
        Ok(Self {
            client: discovery::agent_client().await?,
            agent_name,
            context_name,
            session_id,
        })
    }

    /// The tool's (verb, plural resource, api group) for RBAC preflight — used to refuse a
    /// call the agent's identity is not permitted to make **before** it reaches the API
    /// server (M1b.4 / ADR-0010). Returns `None` for tools with no RBAC-relevant resource.
    fn preflight_target(name: &str) -> Option<(&'static str, &'static str, &'static str)> {
        match name {
            "list_resources" | "get_events" => Some(("list", "pods", "")),
            "describe" => Some(("get", "pods", "")),
            "logs" => Some(("get", "pods/log", "")),
            "diagnose"
            | "explain_pod_failure"
            | "why_is_job_pending"
            | "blast_radius"
            | "what_changed_between" => Some(("get", "pods", "")),
            _ => None,
        }
    }

    /// The tool definitions advertised to MCP clients.
    fn tools() -> Vec<Tool> {
        use serde_json::json;
        let schema =
            |v: serde_json::Value| -> std::sync::Arc<serde_json::Map<String, serde_json::Value>> {
                std::sync::Arc::new(v.as_object().expect("schema must be an object").clone())
            };
        vec![
            Tool::new(
                "list_resources",
                "List Kubernetes resources of a given group/version/kind in a namespace (or cluster-wide).",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "gvk": {"type": "string", "description": "group/version/kind, e.g. v1/Pod or apps/v1/Deployment"},
                        "namespace": {"type": ["string", "null"], "description": "namespace (omit for cluster-scoped)"}
                    },
                    "required": ["gvk"]
                })),
            ),
            Tool::new(
                "describe",
                "YAML-describe a single Kubernetes resource.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "gvk": {"type": "string"},
                        "name": {"type": "string"},
                        "namespace": {"type": ["string", "null"]}
                    },
                    "required": ["gvk", "name"]
                })),
            ),
            Tool::new(
                "logs",
                "Tail recent logs from a pod's containers.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "namespace": {"type": "string"},
                        "tail": {"type": "integer"}
                    },
                    "required": ["name", "namespace"]
                })),
            ),
            Tool::new(
                "get_events",
                "Recent cluster events in a namespace (or all namespaces) within a time window.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "namespace": {"type": ["string", "null"], "description": "namespace (omit for all)"},
                        "minutes": {"type": "integer", "description": "look back N minutes (default 15)"}
                    },
                    "required": []
                })),
            ),
            Tool::new(
                "diagnose",
                "Explain why a pod is not ready (evidence-based diagnostics).",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "namespace": {"type": "string"}
                    },
                    "required": ["name", "namespace"]
                })),
            ),
            Tool::new(
                "explain_pod_failure",
                "Explain why a pod is failing: rule-engine findings + related warning events (evidence-based).",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "namespace": {"type": "string"},
                        "minutes": {"type": "integer", "description": "event window in minutes (default 15)"}
                    },
                    "required": ["name", "namespace"]
                })),
            ),
            Tool::new(
                "why_is_job_pending",
                "Analyze why a Job is pending or stuck (conditions + pod diagnostics).",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "namespace": {"type": "string"}
                    },
                    "required": ["name", "namespace"]
                })),
            ),
            Tool::new(
                "blast_radius",
                "Report a resource's owners and its dependents (what cascade-delete would affect).",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "gvk": {"type": "string", "description": "group/version/kind"},
                        "name": {"type": "string"},
                        "namespace": {"type": "string"}
                    },
                    "required": ["gvk", "name", "namespace"]
                })),
            ),
            Tool::new(
                "what_changed_between",
                "Events in a time window (or the last N minutes) for a namespace.",
                schema(json!({
                    "type": "object",
                    "properties": {
                        "namespace": {"type": "string"},
                        "minutes": {"type": "integer", "description": "look back N minutes (default 15)"},
                        "from_ms": {"type": "integer", "description": "start unix millis (optional)"},
                        "to_ms": {"type": "integer", "description": "end unix millis (optional)"}
                    },
                    "required": ["namespace"]
                })),
            ),
        ]
    }

    async fn exec_tool(
        &self,
        name: &str,
        args: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<String, String> {
        let a = |k: &str| {
            args.and_then(|m| m.get(k))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };
        let ns = |k: &str| -> Option<String> {
            args.and_then(|m| m.get(k))
                .and_then(|v| if v.is_null() { None } else { v.as_str() })
                .map(|s| s.to_string())
        };

        match name {
            "list_resources" => {
                let gvk = a("gvk").ok_or("missing 'gvk'")?;
                let gvk = parse_gvk(&gvk);
                let namespace = ns("namespace");
                let items = discovery::list(&self.client, &gvk, namespace.as_deref())
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(items
                    .iter()
                    .map(|r| format!("{}\t{}\t{}", r.namespace, r.kind, r.name))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "describe" => {
                let gvk = a("gvk").ok_or("missing 'gvk'")?;
                let name = a("name").ok_or("missing 'name'")?;
                let gvk = parse_gvk(&gvk);
                let namespace = ns("namespace");
                kaptein_core::describe::describe_dynamic(
                    &self.client,
                    &gvk,
                    namespace.as_deref(),
                    &name,
                )
                .await
                .map_err(|e| e.to_string())
            }
            "logs" => {
                let name = a("name").ok_or("missing 'name'")?;
                let namespace = a("namespace").ok_or("missing 'namespace'")?;
                let tail = args.and_then(|m| m.get("tail")).and_then(|v| v.as_i64());
                let logs = kaptein_core::describe::pod_logs(&self.client, &namespace, &name, tail)
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(logs
                    .iter()
                    .map(|(c, l)| format!("[{c}] {l}"))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "get_events" => {
                let namespace = ns("namespace");
                let minutes = args
                    .and_then(|m| m.get("minutes"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(15);
                let since_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
                    .saturating_sub(minutes * 60 * 1000);
                let events = kaptein_core::events::recent_events(
                    &self.client,
                    namespace.as_deref(),
                    Some(since_ms),
                )
                .await
                .map_err(|e| e.to_string())?;
                Ok(events
                    .iter()
                    .map(|e| {
                        format!(
                            "{} {} {}/{}: {}",
                            e.type_, e.reason, e.kind, e.name, e.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "diagnose" => {
                let name = a("name").ok_or("missing 'name'")?;
                let namespace = a("namespace").ok_or("missing 'namespace'")?;
                let pod = kaptein_core::pods::get_pod(&self.client, &namespace, &name)
                    .await
                    .map_err(|e| e.to_string())?;
                let findings = diagnostics::diagnose(&pod);
                if findings.is_empty() {
                    Ok(format!("{name}: ready (no findings)"))
                } else {
                    Ok(findings
                        .iter()
                        .map(|f| format!("{}: {}", f.code, f.summary))
                        .collect::<Vec<_>>()
                        .join("\n"))
                }
            }
            "explain_pod_failure" => {
                let name = a("name").ok_or("missing 'name'")?;
                let namespace = a("namespace").ok_or("missing 'namespace'")?;
                let minutes = args
                    .and_then(|m| m.get("minutes"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(15);
                let expl = kaptein_core::moat::explain_pod_failure(
                    &self.client,
                    &namespace,
                    &name,
                    minutes,
                )
                .await
                .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                if expl.findings.is_empty() {
                    out.push(format!(
                        "{}/{}: ready (no findings)",
                        expl.namespace, expl.name
                    ));
                } else {
                    for f in &expl.findings {
                        out.push(format!("{}: {}", f.code, f.summary));
                    }
                }
                out.push(format!(
                    "related warning events ({}):",
                    expl.related_events.len()
                ));
                for e in &expl.related_events {
                    out.push(format!("  {}: {}", e.reason, e.message));
                }
                Ok(out.join("\n"))
            }
            "why_is_job_pending" => {
                let name = a("name").ok_or("missing 'name'")?;
                let namespace = a("namespace").ok_or("missing 'namespace'")?;
                let expl = kaptein_core::moat::why_is_job_pending(&self.client, &namespace, &name)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut out = vec![format!(
                    "{}/{}: failed={}, active={}, succeeded={}",
                    expl.namespace, expl.name, expl.failed, expl.active, expl.succeeded
                )];
                for (ty, status, msg) in &expl.conditions {
                    out.push(format!("  condition {ty}={status}: {msg}"));
                }
                out.push(format!("pods ({}):", expl.pods.len()));
                for p in &expl.pods {
                    out.push(format!("  {p}"));
                }
                Ok(out.join("\n"))
            }
            "blast_radius" => {
                let gvk = a("gvk").ok_or("missing 'gvk'")?;
                let name = a("name").ok_or("missing 'name'")?;
                let namespace = a("namespace").ok_or("missing 'namespace'")?;
                let gvk = parse_gvk(&gvk);
                let br = kaptein_core::moat::blast_radius(&self.client, &namespace, &gvk, &name)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut out = vec![format!("{}/{} ({})", br.namespace, br.kind, br.name)];
                out.push(format!("owners ({}):", br.owners.len()));
                for o in &br.owners {
                    out.push(format!("  {o}"));
                }
                out.push(format!("dependents ({}):", br.dependents.len()));
                for d in &br.dependents {
                    out.push(format!("  {d}"));
                }
                Ok(out.join("\n"))
            }
            "what_changed_between" => {
                let namespace = a("namespace").ok_or("missing 'namespace'")?;
                let minutes = args.and_then(|m| m.get("minutes")).and_then(|v| v.as_i64());
                let from_ms = args.and_then(|m| m.get("from_ms")).and_then(|v| v.as_i64());
                let to_ms = args.and_then(|m| m.get("to_ms")).and_then(|v| v.as_i64());
                let wc = kaptein_core::moat::what_changed_between(
                    &self.client,
                    &namespace,
                    from_ms,
                    to_ms,
                    minutes,
                )
                .await
                .map_err(|e| e.to_string())?;
                let mut out = vec![format!(
                    "{}: {} events between {} and {}",
                    wc.namespace,
                    wc.events.len(),
                    wc.from_ms,
                    wc.to_ms
                )];
                for e in &wc.events {
                    out.push(format!(
                        "{} {} {}/{}: {}",
                        e.type_, e.reason, e.kind, e.name, e.message
                    ));
                }
                Ok(out.join("\n"))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }
}

fn parse_gvk(s: &str) -> GroupVersionKind {
    let parts: Vec<&str> = s.split('/').collect();
    match parts.as_slice() {
        [kind] => GroupVersionKind::gvk("", "v1", kind),
        [version, kind] => GroupVersionKind::gvk("", version, kind),
        [group, version, kind] => GroupVersionKind::gvk(group, version, kind),
        _ => GroupVersionKind::gvk("", "v1", s),
    }
}

/// A per-instance, non-constant session id: unix millis rendered as lowercase hex (not a
/// secret, just a cheap uniqueness source for audit grouping).
fn now_ms_hex() -> String {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{ms:x}")
}

/// Contract-version compatibility gate (review item #12, `docs/versioning.md`).
///
/// A client that declares a Kaptein tool-schema version (`_meta["io.kaptein/apiVersion"]`)
/// is refused when its major differs from the one this server implements — a clear
/// migration error, never a silent break. Clients that omit the field are accepted for
/// backward compatibility (the pre-versioning surface has no declaration).
fn check_client_contract_version(context: &RequestContext<RoleServer>) -> Result<(), String> {
    let declared = context
        .meta
        .get(kaptein_viewmodel::MCP_VERSION_META_KEY)
        .and_then(|v| v.as_str());
    check_declared_version(declared)
}

/// The pure version-check, separated from `RequestContext` so it is unit-testable.
fn check_declared_version(declared: Option<&str>) -> Result<(), String> {
    let Some(declared) = declared else {
        return Ok(());
    };
    let Some(requested) = kaptein_viewmodel::parse_api_version(declared) else {
        return Err(format!(
            "contract: client declared an unparseable Kaptein api version {declared:?}"
        ));
    };
    if !kaptein_viewmodel::is_compatible(kaptein_viewmodel::MCP_API_VERSION, requested) {
        return Err(format!(
            "contract: this server implements Kaptein MCP schema {}, which is incompatible \
             with the client's {} — upgrade the client or the server (docs/versioning.md)",
            kaptein_viewmodel::MCP_API_VERSION,
            requested
        ));
    }
    Ok(())
}

/// Serve the MCP protocol over stdio until the client closes.
pub async fn serve() -> Result<(), CoreError> {
    let server = KapteinMcp::new().await?;
    let transport = (tokio::io::stdin(), tokio::io::stdout());
    server
        .serve(transport)
        .await
        .map_err(|e| CoreError::Internal(e.to_string()))?
        .waiting()
        .await
        .map(|_| ())
        .map_err(|e| CoreError::Internal(e.to_string()))
}

impl ServerHandler for KapteinMcp {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder().enable_tools().build();
        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new("kaptein", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Read-only, governed Kubernetes access. No write operations are available.",
            )
    }

    fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(Self::tools())))
    }

    #[allow(clippy::manual_async_fn)] // the trait's RPIT return forces the `impl Future` form
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResponse, ErrorData>> + Send + '_ {
        async move {
            let args = request.arguments.as_ref();
            let name = request.name.clone();

            // Contract-version gate (docs/versioning.md): refuse a client whose declared
            // Kaptein tool-schema version we do not support — a clear migration error,
            // never a silent break. Absent declaration is accepted (backward-compatible).
            if let Err(msg) = check_client_contract_version(&context) {
                self.audit(&name, args, kaptein_viewmodel::audit::Outcome::Rejected);
                return Ok(CallToolResponse::Complete(CallToolResult::error(vec![
                    ContentBlock::text(msg),
                ])));
            }

            // Governance gate (M1b.4): refuse the call before it reaches the API server
            // when RBAC preflight denies it (or the context is read-only). Refusals are
            // audited as `Outcome::Rejected`, not silently dropped.
            if let Some((verb, resource, group)) = Self::preflight_target(&name) {
                match self.governance_check(verb, resource, group).await {
                    Ok(()) => {}
                    Err(msg) => {
                        self.audit(&name, args, kaptein_viewmodel::audit::Outcome::Rejected);
                        return Ok(CallToolResponse::Complete(CallToolResult::error(vec![
                            ContentBlock::text(msg),
                        ])));
                    }
                }
            }

            match self.exec_tool(&name, args).await {
                Ok(text) => {
                    self.audit(&name, args, kaptein_viewmodel::audit::Outcome::Applied);
                    Ok(CallToolResponse::Complete(CallToolResult::success(vec![
                        ContentBlock::text(text),
                    ])))
                }
                Err(msg) => {
                    // A failed tool call is a rejection, not a success.
                    self.audit(&name, args, kaptein_viewmodel::audit::Outcome::Rejected);
                    Ok(CallToolResponse::Complete(CallToolResult::error(vec![
                        ContentBlock::text(msg),
                    ])))
                }
            }
        }
    }
}

impl KapteinMcp {
    /// RBAC preflight + context guardrail, enforced *before* a tool call reaches the API
    /// server (M1b.4 / ADR-0010). Returns `Err` with a user-facing reason to refuse.
    async fn governance_check(
        &self,
        verb: &str,
        resource: &str,
        group: &str,
    ) -> Result<(), String> {
        // Context classification + read-only default: the MCP surface is read-only, so a
        // prod/unknown context must not permit any write verb. (All current tools are
        // reads; this gate is the control that would refuse a write if one were added.)
        let config = kaptein_core::config::load();
        let class = config.guardrails.classify(&self.context_name);
        if verb != "get"
            && verb != "list"
            && verb != "watch"
            && let Err(msg) = kaptein_core::guardrails::gate_write(class, None)
        {
            return Err(format!("governance: {msg}"));
        }

        // RBAC preflight: refuse a call the agent's own identity cannot make.
        let namespace = "default";
        let perm = kaptein_core::auth::can(&self.client, verb, resource, group, namespace)
            .await
            .map_err(|e| format!("governance: RBAC preflight failed: {e}"))?;
        if !perm.allowed {
            return Err(format!(
                "governance: agent '{agent}' is not permitted to {verb} {resource} in {namespace}",
                agent = self.agent_name
            ));
        }
        Ok(())
    }

    /// Emit a best-effort audit event for an MCP tool call, recorded **after** execution
    /// with the real outcome (Applied/Rejected), a real resource target, and the server's
    /// per-instance session id (ADR-0010). An audit-write failure must not block the tool.
    fn audit(
        &self,
        tool_name: &str,
        args: Option<&serde_json::Map<String, serde_json::Value>>,
        outcome: kaptein_viewmodel::audit::Outcome,
    ) {
        use kaptein_viewmodel::audit::{
            Actor, ActorKind, AuditEvent, Operation, ResourceRef, Source,
        };
        let operation = match tool_name {
            "list_resources" => Operation::List,
            "describe" => Operation::Describe,
            "logs" | "get_events" => Operation::Logs,
            "diagnose"
            | "explain_pod_failure"
            | "why_is_job_pending"
            | "blast_radius"
            | "what_changed_between" => Operation::Diagnose,
            _ => return,
        };
        // Extract the real target resource from the call args (not the tool name).
        let get = |k: &str| -> String {
            args.and_then(|m| m.get(k))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
        };
        let namespace = get("namespace");
        let name = if tool_name == "list_resources" {
            String::new() // a list has no single target name
        } else {
            get("name")
        };
        let kind = match tool_name {
            "describe" | "list_resources" => get("gvk"),
            "logs" | "diagnose" | "explain_pod_failure" => "Pod".to_string(),
            "why_is_job_pending" => "Job".to_string(),
            "blast_radius" => get("gvk"),
            "get_events" | "what_changed_between" => "Event".to_string(),
            _ => tool_name.to_string(),
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let event = AuditEvent {
            timestamp: now_ms,
            actor: Actor {
                kind: ActorKind::Agent,
                name: self.agent_name.clone(),
            },
            context: self.context_name.clone(),
            operation,
            target: ResourceRef {
                group: "".into(),
                kind,
                namespace,
                name,
            },
            outcome,
            source: Source::Mcp,
            session_id: self.session_id.clone(),
            reason: None,
            on_behalf_of: None,
        };
        let _ = crate::audit::append(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::check_declared_version;

    #[test]
    fn absent_version_is_accepted() {
        assert!(check_declared_version(None).is_ok());
    }

    #[test]
    fn same_major_is_accepted() {
        assert!(check_declared_version(Some("v1")).is_ok());
        assert!(check_declared_version(Some("1")).is_ok());
        assert!(check_declared_version(Some("1.5")).is_ok());
    }

    #[test]
    fn different_major_is_refused() {
        let err = check_declared_version(Some("v2")).unwrap_err();
        assert!(err.contains("incompatible"), "got {err:?}");
    }

    #[test]
    fn unparseable_version_is_refused() {
        let err = check_declared_version(Some("banana")).unwrap_err();
        assert!(err.contains("unparseable"), "got {err:?}");
    }
}
