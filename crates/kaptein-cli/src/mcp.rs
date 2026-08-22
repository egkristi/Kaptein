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
}

impl KapteinMcp {
    pub async fn new() -> Result<Self, CoreError> {
        let agent_name = std::env::var("KAPTEIN_AGENT")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "mcp-client".to_string());
        let context_name = discovery::current_context_name().unwrap_or_default();
        Ok(Self {
            client: discovery::client().await?,
            agent_name,
            context_name,
        })
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
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResponse, ErrorData>> + Send + '_ {
        async move {
            let args = request.arguments.as_ref();
            self.audit(&request.name);
            match self.exec_tool(&request.name, args).await {
                Ok(text) => Ok(CallToolResponse::Complete(CallToolResult::success(vec![
                    ContentBlock::text(text),
                ]))),
                Err(msg) => Ok(CallToolResponse::Complete(CallToolResult::error(vec![
                    ContentBlock::text(msg),
                ]))),
            }
        }
    }
}

impl KapteinMcp {
    /// Emit a best-effort audit event for an MCP tool call. Audit is a governance
    /// requirement (ADR-0010), but an audit-write failure must not block the tool.
    fn audit(&self, tool_name: &str) {
        use kaptein_viewmodel::audit::{
            Actor, ActorKind, AuditEvent, Operation, Outcome, ResourceRef, Source,
        };
        let operation = match tool_name {
            "list_resources" => Operation::List,
            "describe" => Operation::Describe,
            "logs" => Operation::Logs,
            "diagnose"
            | "explain_pod_failure"
            | "why_is_job_pending"
            | "blast_radius"
            | "what_changed_between" => Operation::Diagnose,
            _ => return,
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
                kind: tool_name.into(),
                namespace: "".into(),
                name: "".into(),
            },
            outcome: Outcome::Applied,
            source: Source::Mcp,
            session_id: "mcp".into(),
            reason: None,
            on_behalf_of: None,
        };
        let _ = crate::audit::append(&event);
    }
}
