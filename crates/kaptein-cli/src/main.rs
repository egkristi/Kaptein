//! Kaptein CLI — the thin command-line entry point.
//!
//! The CLI drives `kaptein-core` directly; it is a projection, not a home for logic.

mod audit;
mod edit;
mod mcp;

use clap::{Parser, Subcommand};
use kube::core::GroupVersionKind;

#[derive(Parser)]
#[command(name = "kaptein", version, about = "Kaptein Kubernetes workbench")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List resources of a given kind.
    Get {
        /// group/version/kind, e.g. "v1/Pod" or "apps/v1/Deployment"
        #[arg(short, long, default_value = "v1/Pod")]
        gvk: String,
        /// Namespace (omit for cluster-scoped resources)
        #[arg(short, long)]
        namespace: Option<String>,
        /// sort by column: name, namespace, kind, created
        #[arg(short, long)]
        sort: Option<String>,
        /// sort descending (newest/last first)
        #[arg(long)]
        descending: bool,
        /// case-insensitive substring filter on name/namespace/kind
        #[arg(short, long)]
        filter: Option<String>,
        /// list metadata only (PartialObjectMetadata — no full object bodies)
        #[arg(long)]
        metadata: bool,
        /// kubeconfig context to use (context switching)
        #[arg(long)]
        context: Option<String>,
    },
    /// RBAC preflight: check whether the current user may perform a verb.
    Can {
        /// verb, e.g. "get", "create", "delete"
        #[arg(short, long)]
        verb: String,
        /// plural resource name, e.g. "pods", "deployments"
        #[arg(short, long)]
        resource: String,
        /// API group (empty for core)
        #[arg(short, long, default_value = "")]
        group: String,
        /// namespace to evaluate
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Batch RBAC preflight: check the standard action set for a resource.
    Preflight {
        /// plural resource name, e.g. "pods", "deployments"
        #[arg(short, long)]
        resource: String,
        /// API group (empty for core)
        #[arg(short, long, default_value = "")]
        group: String,
        /// namespace to evaluate
        #[arg(short, long, default_value = "default")]
        namespace: String,
    },
    /// Show the current context and its guardrail classification.
    Context,
    /// List all contexts in the kubeconfig (for context switching).
    Contexts,
    /// Validate the config file (parse errors + invalid guardrail regexes).
    ConfigValidate,
    /// Explain why a context is classified the way it is.
    ConfigExplainContext {
        /// context name
        #[arg(long)]
        context: String,
    },
    /// Validate a view-definition (lens) file against the lens schema.
    ViewdefValidate {
        /// path to a lens YAML/JSON file
        #[arg(short = 'f', long)]
        file: String,
    },
    /// Diagnose why a pod is not ready.
    Diagnose {
        /// pod name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
    },
    /// YAML-describe a single resource.
    Describe {
        /// group/version/kind
        #[arg(short, long, default_value = "v1/Pod")]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short = 'n', long)]
        namespace: Option<String>,
    },
    /// Tail recent logs from a pod.
    Logs {
        /// pod name (omit to stream all pods via --selector)
        #[arg(short = 'p', long)]
        name: Option<String>,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// label selector for multi-pod streaming (e.g. app=foo)
        #[arg(short = 'l', long)]
        selector: Option<String>,
        /// regex filter applied to each log line
        #[arg(short = 'r', long)]
        regex: Option<String>,
        /// number of lines to tail
        #[arg(long, default_value_t = 100)]
        tail: i64,
        /// follow the log stream until interrupted (only with --name)
        #[arg(short = 'f', long)]
        follow: bool,
        /// parse JSON log lines into typed columns (M1.2 "JSON -> columns")
        #[arg(long)]
        json: bool,
    },
    /// Run the governed MCP server over stdio (read-only).
    Mcp,
    /// Show recent cluster events ("what changed in the last N minutes").
    Events {
        /// namespace (omit for all namespaces)
        #[arg(short = 'n', long)]
        namespace: Option<String>,
        /// look back this many minutes
        #[arg(long, default_value_t = 15)]
        minutes: i64,
    },
    /// Landing view: is anything broken, and what changed recently?
    Overview {
        /// look back this many minutes
        #[arg(long, default_value_t = 15)]
        minutes: i64,
    },
    /// Watch a resource kind and report changes (in-memory ring buffer, no persistence).
    Watch {
        /// group/version/kind, e.g. v1/Pod
        #[arg(short, long, default_value = "v1/Pod")]
        gvk: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short, long)]
        namespace: Option<String>,
        /// stop after this many change events
        #[arg(long, default_value_t = 20)]
        max: usize,
    },
    /// Server-side dry-run a YAML manifest (validate without mutating the cluster).
    Apply {
        /// path to a YAML manifest, or "-" for stdin
        #[arg(short = 'f', long)]
        file: String,
    },
    /// Edit a resource's YAML in $EDITOR, then dry-run the result (never applies).
    Edit {
        /// group/version/kind, e.g. v1/ConfigMap or apps/v1/Deployment
        #[arg(short, long, default_value = "v1/ConfigMap")]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short = 'n', long)]
        namespace: Option<String>,
    },
    /// Forward a pod port to a local TCP listener (Ctrl-C to stop).
    PortForward {
        /// pod name
        #[arg(short = 'p', long)]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// target (pod) port
        #[arg(short = 't', long)]
        port: u16,
        /// local bind port (0 = ephemeral)
        #[arg(long, default_value_t = 0)]
        local: u16,
        /// name for a persistent forward (saved across runs)
        #[arg(short = 'N', long)]
        name: Option<String>,
    },
    /// List named (persistent) port-forwards.
    PortForwardList,
    /// Remove a named port-forward.
    PortForwardRemove {
        /// forward name
        #[arg(short = 'N', long)]
        name: String,
    },
    /// Run a one-shot command in a pod container (read-only).
    Exec {
        /// pod name
        #[arg(short = 'p', long)]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// container name (omit for the default/first container)
        #[arg(short = 'c', long)]
        container: Option<String>,
        /// command + args (e.g. -- cmd arg1 arg2)
        #[arg(last = true, required = true)]
        command: Vec<String>,
        /// interactive TTY session (allocate a TTY and proxy stdin/stdout)
        #[arg(short = 't', long)]
        tty: bool,
    },
    /// Delete a resource (dry-run by default; requires --confirm).
    Delete {
        /// group/version/kind, e.g. v1/ConfigMap or apps/v1/Deployment
        #[arg(short, long, default_value = "v1/ConfigMap")]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short = 'n', long)]
        namespace: Option<String>,
        /// cascade policy: background (default), foreground, or orphan
        #[arg(long, default_value = "background")]
        cascade: String,
        /// actually delete (default is dry-run)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
    /// Scale a workload's replicas (dry-run by default; requires --confirm).
    Scale {
        /// group/version/kind, e.g. apps/v1/Deployment
        #[arg(short, long, default_value = "apps/v1/Deployment")]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// target replica count
        #[arg(long)]
        replicas: i32,
        /// actually scale (default is dry-run)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
    /// Trigger a rollout restart (annotates the pod template; requires --confirm).
    Restart {
        /// group/version/kind, e.g. apps/v1/Deployment
        #[arg(short, long, default_value = "apps/v1/Deployment")]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// actually restart (restart has no dry-run; this is required)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
    /// Cordon a node (mark unschedulable; requires --confirm).
    Cordon {
        /// node name
        #[arg(short = 'p', long)]
        name: String,
        /// actually cordon (default is dry-run)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
    /// Uncordon a node (mark schedulable; requires --confirm).
    Uncordon {
        /// node name
        #[arg(short = 'p', long)]
        name: String,
        /// actually uncordon (default is dry-run)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
    /// Evict a pod (requires --confirm).
    Evict {
        /// pod name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// actually evict (default is dry-run)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
    /// Preview what draining a node would evict (read-only; never cordons or evicts).
    Drain {
        /// node name
        #[arg(short = 'p', long)]
        name: String,
    },
    /// Shell out to an external tool (krew/kustomize/helm); degrades gracefully if absent.
    Krew {
        /// tool to invoke: krew, kustomize, or helm
        #[arg(short = 't', long, default_value = "krew")]
        tool: String,
        /// list plugins (for krew) or run arbitrary args (for kustomize/helm)
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// List ephemeral containers attached to a pod.
    DebugContainers {
        /// pod name
        #[arg(short = 'p', long)]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
    },
    /// Attach an ephemeral (debug) container to a running pod (requires --confirm).
    Debug {
        /// pod name
        #[arg(short = 'p', long)]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// ephemeral container name
        #[arg(long)]
        name: String,
        /// image (e.g. busybox)
        #[arg(long, default_value = "busybox")]
        image: String,
        /// command (optional; e.g. -- sleep 3600)
        #[arg(last = true)]
        command: Vec<String>,
        /// actually attach (default is dry-run)
        #[arg(long)]
        confirm: bool,
        /// break-glass justification (required for writes to prod/unknown contexts)
        #[arg(long)]
        break_glass: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(err) = run(cli).await {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), kaptein_core::Error> {
    let client = kaptein_core::discovery::client().await?;

    match cli.command {
        Command::Get {
            gvk,
            namespace,
            sort,
            descending,
            filter,
            metadata,
            context,
        } => {
            let gvk = parse_gvk(&gvk);
            let sort_key = sort
                .as_deref()
                .map(kaptein_core::discovery::SortKey::parse)
                .unwrap_or(None);
            // Use the context-specific client when --context is supplied.
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
            // Metadata-only listing (ADR-0006) is the bounded, cheap path for
            // list-heavy views: the API server returns `metadata` only.
            let items = if metadata {
                let mut all = Vec::new();
                let mut token: Option<String> = None;
                loop {
                    let (page, next) = kaptein_core::discovery::list_metadata_bounded(
                        &client,
                        &gvk,
                        namespace.as_deref(),
                        500,
                        token.as_deref(),
                    )
                    .await?;
                    all.extend(page);
                    match next {
                        Some(t) => token = Some(t),
                        None => break,
                    }
                }
                all
            } else {
                kaptein_core::discovery::list_with(
                    &client,
                    &gvk,
                    namespace.as_deref(),
                    sort_key,
                    descending,
                    filter.as_deref(),
                )
                .await?
            };
            for item in items {
                let created = item.created.map(|t| t.0.to_string()).unwrap_or_default();
                println!(
                    "{}\t{}\t{}\t{}",
                    item.namespace, item.kind, item.name, created
                );
            }
            Ok(())
        }
        Command::Can {
            verb,
            resource,
            group,
            namespace,
        } => {
            let perm =
                kaptein_core::auth::can(&client, &verb, &resource, &group, &namespace).await?;
            let group_prefix = if group.is_empty() { "" } else { &group };
            let sep = if group.is_empty() { "" } else { "/" };
            println!(
                "{group_prefix}{sep}{verb} {resource} in {namespace}: {}",
                if perm.allowed { "ALLOWED" } else { "DENIED" }
            );
            Ok(())
        }
        Command::Preflight {
            resource,
            group,
            namespace,
        } => {
            let preflight =
                kaptein_core::auth::preflight(&client, &resource, &group, &namespace).await?;
            for (verb, allowed) in preflight.actions {
                println!("{verb:18} {}", if allowed { "ALLOWED" } else { "DENIED" });
            }
            Ok(())
        }
        Command::Context => {
            let config = kaptein_core::config::load();
            let ctx = kaptein_core::discovery::current_context_name()?;
            let class = config.guardrails.classify(&ctx);
            println!("context: {ctx}");
            println!("class: {class:?}");
            Ok(())
        }
        Command::Contexts => {
            let contexts = kaptein_core::discovery::list_contexts()?;
            for c in contexts {
                let marker = if c.current { "*" } else { " " };
                println!(
                    "{marker} {}\tcluster: {}\tuser: {}",
                    c.name, c.cluster, c.user
                );
            }
            Ok(())
        }
        Command::ConfigValidate => {
            let path = kaptein_core::config::config_path();
            match kaptein_core::config::validate_file(&path) {
                Ok(()) => {
                    println!("config at {} is valid", path.display());
                    Ok(())
                }
                Err(problems) => {
                    let count = problems.len();
                    for p in &problems {
                        eprintln!("error: {p}");
                    }
                    Err(kaptein_core::Error::Internal(format!(
                        "config at {} is invalid ({count} problem(s))",
                        path.display()
                    )))
                }
            }
        }
        Command::ConfigExplainContext { context } => {
            let config = kaptein_core::config::load();
            println!(
                "{}",
                kaptein_core::config::explain_context(&config, &context)
            );
            Ok(())
        }
        Command::ViewdefValidate { file } => {
            let path = std::path::Path::new(&file);
            let text = std::fs::read_to_string(path).map_err(|e| {
                kaptein_core::Error::Internal(format!("cannot read {}: {e}", path.display()))
            })?;
            // A lens is YAML (or JSON, which YAML is a superset of) — parse to a
            // serde_json::Value first, then deserialize to the view-model type.
            let value: serde_json::Value = serde_yaml::from_str(&text).map_err(|e| {
                kaptein_core::Error::Internal(format!("cannot parse {}: {e}", path.display()))
            })?;
            let vd: kaptein_viewmodel::ViewDefinition =
                serde_json::from_value(value).map_err(|e| {
                    kaptein_core::Error::Internal(format!(
                        "cannot deserialize {}: {e}",
                        path.display()
                    ))
                })?;
            let problems = kaptein_viewmodel::validate_viewdef(&vd);
            if problems.is_empty() {
                println!(
                    "lens {} ({}) is valid ({} columns, {} status rules, {} actions)",
                    vd.id,
                    vd.target.display(),
                    vd.columns.len(),
                    vd.status.len(),
                    vd.actions.len()
                );
                Ok(())
            } else {
                let count = problems.len();
                for p in &problems {
                    eprintln!("error: {p}");
                }
                Err(kaptein_core::Error::Internal(format!(
                    "lens {} is invalid ({count} problem(s))",
                    path.display()
                )))
            }
        }
        Command::Diagnose { name, namespace } => {
            let pod = kaptein_core::pods::get_pod(&client, &namespace, &name).await?;
            let findings = kaptein_core::diagnostics::diagnose(&pod);
            if findings.is_empty() {
                println!("{name}: ready (no findings)");
            } else {
                for f in findings {
                    println!("{}: {}", f.code, f.summary);
                }
            }
            Ok(())
        }
        Command::Describe {
            gvk,
            name,
            namespace,
        } => {
            let gvk = parse_gvk(&gvk);
            let yaml = kaptein_core::describe::describe_dynamic(
                &client,
                &gvk,
                namespace.as_deref(),
                &name,
            )
            .await?;
            println!("{yaml}");
            Ok(())
        }
        Command::Logs {
            name,
            namespace,
            selector,
            regex,
            tail,
            follow,
            json,
        } => {
            // JSON mode: parse JSON log lines into typed columns. Applies to both
            // single-pod and multi-pod paths; plain lines fall back to `_raw`.
            if json {
                let raw_lines: Vec<String> = match name.as_deref() {
                    Some(pod_name) => {
                        let logs = kaptein_core::describe::pod_logs(
                            &client,
                            &namespace,
                            pod_name,
                            Some(tail),
                        )
                        .await?;
                        logs.into_iter().map(|(_, line)| line).collect()
                    }
                    None => {
                        let lines = kaptein_core::describe::multi_pod_logs(
                            &client,
                            &namespace,
                            selector.as_deref(),
                            regex.as_deref(),
                            Some(tail),
                        )
                        .await?;
                        lines.into_iter().map(|l| l.line).collect()
                    }
                };
                let parsed = kaptein_viewmodel::parse_log_stream(raw_lines);
                let columns = kaptein_viewmodel::infer_columns(&parsed);
                if columns.is_empty() {
                    println!("(no JSON log lines found)");
                } else {
                    // Header row.
                    println!("{}", columns.join("\t"));
                    for line in &parsed {
                        if line.columns.is_empty() {
                            println!("{}", line.raw);
                            continue;
                        }
                        let cells: Vec<String> = columns
                            .iter()
                            .map(|c| match line.columns.get(c) {
                                Some(kaptein_viewmodel::LogCell::Text(s)) => s.clone(),
                                Some(kaptein_viewmodel::LogCell::Number(n)) => n.to_string(),
                                Some(kaptein_viewmodel::LogCell::Float(f)) => f.to_string(),
                                Some(kaptein_viewmodel::LogCell::Bool(b)) => b.to_string(),
                                Some(kaptein_viewmodel::LogCell::Null) => "null".into(),
                                None => String::new(),
                            })
                            .collect();
                        println!("{}", cells.join("\t"));
                    }
                }
                return Ok(());
            }

            match name {
                Some(pod_name) => {
                    if follow {
                        // Stream and follow until Ctrl-C.
                        use futures_util::StreamExt;
                        let stream = kaptein_core::describe::follow_logs(
                            &client,
                            &namespace,
                            &pod_name,
                            None,
                            Some(tail),
                        );
                        let mut stream = Box::pin(stream);
                        let re = regex
                            .as_deref()
                            .map(regex::Regex::new)
                            .transpose()
                            .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                        while let Some(item) = stream.next().await {
                            match item {
                                Ok((_container, line)) => {
                                    if let Some(re) = &re
                                        && !re.is_match(&line)
                                    {
                                        continue;
                                    }
                                    println!("{line}");
                                }
                                Err(e) => {
                                    eprintln!("error: {e}");
                                    break;
                                }
                            }
                        }
                    } else {
                        let logs = kaptein_core::describe::pod_logs(
                            &client,
                            &namespace,
                            &pod_name,
                            Some(tail),
                        )
                        .await?;
                        for (container, line) in logs {
                            println!("[{container}] {line}");
                        }
                    }
                }
                None => {
                    let lines = kaptein_core::describe::multi_pod_logs(
                        &client,
                        &namespace,
                        selector.as_deref(),
                        regex.as_deref(),
                        Some(tail),
                    )
                    .await?;
                    for l in lines {
                        println!("[{}/{}] {}", l.pod, l.container, l.line);
                    }
                }
            }
            Ok(())
        }
        Command::Mcp => mcp::serve()
            .await
            .map_err(|e| kaptein_core::Error::Internal(e.to_string())),
        Command::Events { namespace, minutes } => {
            let since_ms = now_ms().saturating_sub(minutes * 60 * 1000);
            let events =
                kaptein_core::events::recent_events(&client, namespace.as_deref(), Some(since_ms))
                    .await?;
            for e in events {
                println!(
                    "{}\t{}\t{}/{}\t{}\t{}",
                    e.last_timestamp_ms, e.type_, e.kind, e.name, e.reason, e.message
                );
            }
            Ok(())
        }
        Command::Overview { minutes } => {
            let since_ms = now_ms().saturating_sub(minutes * 60 * 1000);
            // Feed a small watch ring of pod changes (M1.4) so "what changed" is the
            // informer's view, then compose the landing view from events + ring.
            let ring = kaptein_core::watchring::WatchRing::new(50);
            let pod_gvk = kube::core::GroupVersionKind::gvk("", "v1", "Pod");
            let _ = kaptein_core::watchring::snapshot_into_ring(&client, &pod_gvk, None, &ring, 50)
                .await;

            let overview = kaptein_core::overview::overview_with_ring(
                &client,
                None,
                since_ms,
                ring.snapshot(),
            )
            .await?;
            println!("Kaptein overview (last {minutes} minutes)");
            println!("  total events: {}", overview.total_events);
            println!(
                "  warnings: {} across namespaces: {}",
                overview.warnings.len(),
                overview.affected_namespaces.join(", ")
            );
            for w in overview.warnings {
                println!(
                    "    [WARN] {}/{}\t{}\t{}",
                    w.kind, w.name, w.reason, w.message
                );
            }
            println!(
                "  recent changes (watch): {}",
                overview.recent_changes.len()
            );
            for c in overview.recent_changes.iter().take(10) {
                println!("    [{}] {}/{}\t{}", c.event, c.kind, c.namespace, c.name);
            }
            Ok(())
        }
        Command::Watch {
            gvk,
            namespace,
            max,
        } => {
            let gvk = parse_gvk(&gvk);
            let ring = kaptein_core::watchring::WatchRing::new(max.max(1));
            let pushed = kaptein_core::watchring::watch_into_ring(
                &client,
                &gvk,
                namespace.as_deref(),
                &ring,
                max,
            )
            .await?;
            println!("watched {gvk:?}: {pushed} changes");
            for r in ring.snapshot() {
                println!(
                    "{} {}\t{}/{}\t({} ms)",
                    r.event, r.kind, r.namespace, r.name, r.observed_at_ms
                );
            }
            Ok(())
        }
        Command::Apply { file } => {
            let manifest = if file == "-" {
                use std::io::Read;
                let mut s = String::new();
                std::io::stdin()
                    .read_to_string(&mut s)
                    .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                s
            } else {
                std::fs::read_to_string(&file)
                    .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?
            };
            let dry_run = kaptein_core::apply::dry_run_apply(&client, &manifest).await?;
            if dry_run.accepted {
                println!("dry-run accepted (no changes applied):");
            } else {
                println!("dry-run REJECTED (no changes applied):");
            }
            println!("{}", dry_run.response_yaml);
            Ok(())
        }
        Command::Edit {
            gvk,
            name,
            namespace,
        } => {
            let gvk = parse_gvk(&gvk);
            let result = edit::edit_in_editor(&client, &gvk, namespace.as_deref(), &name).await?;
            println!("{}", result);
            Ok(())
        }
        Command::PortForward {
            pod,
            namespace,
            port,
            local,
            name,
        } => {
            if let Some(forward_name) = name {
                // Named, persistent forward with auto-reconnect.
                let spec = kaptein_core::portforward::ForwardSpec {
                    name: forward_name.clone(),
                    namespace: namespace.clone(),
                    pod: pod.clone(),
                    target_port: port,
                    local_port: local,
                };
                // Persist the spec.
                let path = kaptein_core::portforward::manager_path();
                let mut manager = kaptein_core::portforward::ForwardManager::load(Some(&path));
                manager.upsert(spec.clone())?;
                let running =
                    kaptein_core::portforward::start_named_forward(client.clone(), spec).await?;
                println!(
                    "forwarding [{forward_name}] {namespace}/{pod}:{port} -> {} (auto-reconnect; Ctrl-C to stop)",
                    running.local_addr
                );
                tokio::signal::ctrl_c()
                    .await
                    .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                let _ = running.cancel.send(true);
                println!("stopped");
                Ok(())
            } else {
                let local_addr = format!("127.0.0.1:{local}")
                    .parse::<std::net::SocketAddr>()
                    .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                let bound =
                    kaptein_core::portforward::forward(&client, &namespace, &pod, port, local_addr)
                        .await?;
                println!("forwarding {namespace}/{pod}:{port} -> {bound} (Ctrl-C to stop)");
                tokio::signal::ctrl_c()
                    .await
                    .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                println!("stopped");
                Ok(())
            }
        }
        Command::PortForwardList => {
            let path = kaptein_core::portforward::manager_path();
            let manager = kaptein_core::portforward::ForwardManager::load(Some(&path));
            let specs = manager.list();
            if specs.is_empty() {
                println!("no named port-forwards");
            } else {
                for s in specs {
                    println!(
                        "{}\t{}/{}\t:{}\t->\t:{}\t",
                        s.name, s.namespace, s.pod, s.target_port, s.local_port
                    );
                }
            }
            Ok(())
        }
        Command::PortForwardRemove { name } => {
            let path = kaptein_core::portforward::manager_path();
            let mut manager = kaptein_core::portforward::ForwardManager::load(Some(&path));
            manager.remove(&name)?;
            println!("removed forward '{name}'");
            Ok(())
        }
        Command::Exec {
            pod,
            namespace,
            container,
            command,
            tty,
        } => {
            if tty {
                // Interactive TTY: allocate a TTY and proxy stdin/stdout. The CLI must
                // run on a real terminal; tokio's stdio is used directly (raw-mode
                // switching is the terminal's job in a full TUI, not the CLI's).
                let stdin = tokio::io::stdin();
                let stdout = tokio::io::stdout();
                kaptein_core::exec::exec_tty(
                    &client,
                    &namespace,
                    &pod,
                    &command,
                    container.as_deref(),
                    stdin,
                    stdout,
                )
                .await?;
            } else {
                let output = kaptein_core::exec::exec(
                    &client,
                    &namespace,
                    &pod,
                    &command,
                    container.as_deref(),
                )
                .await?;
                print!("{}", output.output);
            }
            Ok(())
        }
        Command::Delete {
            gvk,
            name,
            namespace,
            cascade,
            confirm,
            break_glass,
        } => {
            if confirm {
                gate_write(break_glass.as_deref())?;
            }
            let gvk = parse_gvk(&gvk);
            let policy = kaptein_core::delete::parse_propagation(&cascade);
            let outcome = kaptein_core::delete::delete(
                &client,
                &gvk,
                &name,
                namespace.as_deref(),
                policy,
                confirm,
            )
            .await?;
            let audit_outcome = if outcome.deleted {
                kaptein_viewmodel::audit::Outcome::Applied
            } else {
                kaptein_viewmodel::audit::Outcome::DryRun
            };
            audit_write(
                kaptein_viewmodel::audit::Operation::Delete,
                &gvk.kind,
                namespace.as_deref().unwrap_or(""),
                &name,
                audit_outcome,
                break_glass.as_deref(),
            );
            println!("{}", outcome.message);
            Ok(())
        }
        Command::Scale {
            gvk,
            name,
            namespace,
            replicas,
            confirm,
            break_glass,
        } => {
            if confirm {
                gate_write(break_glass.as_deref())?;
            }
            let gvk = parse_gvk(&gvk);
            let outcome = kaptein_core::workloads::scale(
                &client,
                &gvk,
                &name,
                Some(&namespace),
                replicas,
                confirm,
            )
            .await?;
            let audit_outcome = if outcome.scaled {
                kaptein_viewmodel::audit::Outcome::Applied
            } else {
                kaptein_viewmodel::audit::Outcome::DryRun
            };
            audit_write(
                kaptein_viewmodel::audit::Operation::Scale,
                &gvk.kind,
                &namespace,
                &name,
                audit_outcome,
                break_glass.as_deref(),
            );
            println!("{}", outcome.message);
            Ok(())
        }
        Command::Restart {
            gvk,
            name,
            namespace,
            confirm,
            break_glass,
        } => {
            if !confirm {
                return Err(kaptein_core::Error::Internal(
                    "restart has no dry-run; re-run with --confirm to actually restart".into(),
                ));
            }
            gate_write(break_glass.as_deref())?;
            let gvk = parse_gvk(&gvk);
            let outcome =
                kaptein_core::workloads::restart(&client, &gvk, &name, &namespace).await?;
            audit_write(
                kaptein_viewmodel::audit::Operation::Restart,
                &gvk.kind,
                &namespace,
                &name,
                kaptein_viewmodel::audit::Outcome::Applied,
                break_glass.as_deref(),
            );
            println!("{}", outcome.message);
            Ok(())
        }
        Command::Cordon {
            name,
            confirm,
            break_glass,
        } => {
            if confirm {
                gate_write(break_glass.as_deref())?;
            }
            let outcome = kaptein_core::nodes::cordon(&client, &name, confirm).await?;
            if confirm {
                audit_write(
                    kaptein_viewmodel::audit::Operation::Cordon,
                    "Node",
                    "",
                    &name,
                    kaptein_viewmodel::audit::Outcome::Applied,
                    break_glass.as_deref(),
                );
            }
            println!("{}", outcome.message);
            Ok(())
        }
        Command::Uncordon {
            name,
            confirm,
            break_glass,
        } => {
            if confirm {
                gate_write(break_glass.as_deref())?;
            }
            let outcome = kaptein_core::nodes::uncordon(&client, &name, confirm).await?;
            if confirm {
                audit_write(
                    kaptein_viewmodel::audit::Operation::Cordon,
                    "Node",
                    "",
                    &name,
                    kaptein_viewmodel::audit::Outcome::Applied,
                    break_glass.as_deref(),
                );
            }
            println!("{}", outcome.message);
            Ok(())
        }
        Command::Evict {
            name,
            namespace,
            confirm,
            break_glass,
        } => {
            if confirm {
                gate_write(break_glass.as_deref())?;
            }
            let outcome = kaptein_core::nodes::evict(&client, &namespace, &name, confirm).await?;
            if confirm {
                audit_write(
                    kaptein_viewmodel::audit::Operation::Evict,
                    "Pod",
                    &namespace,
                    &name,
                    kaptein_viewmodel::audit::Outcome::Applied,
                    break_glass.as_deref(),
                );
            }
            println!("{}", outcome.message);
            Ok(())
        }
        Command::Drain { name } => {
            let targets = kaptein_core::nodes::drain_preview(&client, &name).await?;
            println!("drain preview for node {name} (read-only, nothing evicted):");
            let mut evictable = 0;
            for t in &targets {
                if t.skip_reason.is_empty() {
                    evictable += 1;
                    println!("  [evict] {}/{}", t.namespace, t.name);
                } else {
                    println!("  [skip]  {}/{} — {}", t.namespace, t.name, t.skip_reason);
                }
            }
            println!(
                "total: {evictable} evictable, {} skipped",
                targets.len() - evictable
            );
            Ok(())
        }
        Command::Krew { tool, args } => {
            let tool = match tool.to_ascii_lowercase().as_str() {
                "krew" => kaptein_core::external::Tool::Krew,
                "kustomize" => kaptein_core::external::Tool::Kustomize,
                "helm" => kaptein_core::external::Tool::Helm,
                other => {
                    return Err(kaptein_core::Error::Internal(format!(
                        "unknown external tool '{other}' (supported: krew, kustomize, helm)"
                    )));
                }
            };
            if args.is_empty() && tool == kaptein_core::external::Tool::Krew {
                // Default: list plugins.
                let plugins = kaptein_core::external::list_krew_plugins();
                if plugins.is_empty() {
                    println!("krew not found or no plugins installed (degraded gracefully)");
                } else {
                    for p in plugins {
                        println!("{p}");
                    }
                }
                return Ok(());
            }
            // For krew, prefix with "krew"; for others, pass args directly.
            let full_args: Vec<&str> = if tool == kaptein_core::external::Tool::Krew {
                std::iter::once("krew")
                    .chain(args.iter().map(|s| s.as_str()))
                    .collect()
            } else {
                args.iter().map(|s| s.as_str()).collect()
            };
            match kaptein_core::external::run(tool, &full_args) {
                Ok(stdout) => {
                    if !stdout.is_empty() {
                        println!("{stdout}");
                    }
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
        Command::DebugContainers { pod, namespace } => {
            let containers = kaptein_core::ephemeral::list(&client, &namespace, &pod).await?;
            if containers.is_empty() {
                println!("{namespace}/{pod}: no ephemeral containers");
            } else {
                for c in containers {
                    let cmd = if c.command.is_empty() {
                        String::new()
                    } else {
                        format!(" cmd=[{}]", c.command.join(" "))
                    };
                    println!("{}  {} {cmd}", c.name, c.image);
                }
            }
            Ok(())
        }
        Command::Debug {
            pod,
            namespace,
            name,
            image,
            command,
            confirm,
            break_glass,
        } => {
            if confirm {
                gate_write(break_glass.as_deref())?;
            }
            let outcome = kaptein_core::ephemeral::add(
                &client, &namespace, &pod, &name, &image, &command, confirm,
            )
            .await?;
            if confirm {
                audit_write(
                    kaptein_viewmodel::audit::Operation::Exec,
                    "Pod",
                    &namespace,
                    &pod,
                    kaptein_viewmodel::audit::Outcome::Applied,
                    break_glass.as_deref(),
                );
            }
            println!("{}", outcome.message);
            Ok(())
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Enforce the context guardrail for a confirmed write: classify the current context
/// and require a non-empty break-glass reason for prod/unknown contexts.
fn gate_write(break_glass: Option<&str>) -> Result<(), kaptein_core::Error> {
    let config = kaptein_core::config::load();
    let ctx = kaptein_core::discovery::current_context_name()?;
    let class = config.guardrails.classify(&ctx);
    kaptein_core::guardrails::gate_write(class, break_glass).map_err(kaptein_core::Error::Internal)
}

/// Emit a best-effort audit event for a CLI write operation (ADR-0010). Audit is a
/// governance requirement; an audit-write failure must not block the operation.
fn audit_write(
    operation: kaptein_viewmodel::audit::Operation,
    kind: &str,
    namespace: &str,
    name: &str,
    outcome: kaptein_viewmodel::audit::Outcome,
    break_glass: Option<&str>,
) {
    use kaptein_viewmodel::audit::{Actor, ActorKind, AuditEvent, ResourceRef, Source};
    let context = kaptein_core::discovery::current_context_name().unwrap_or_default();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let event = AuditEvent {
        timestamp: now_ms,
        actor: Actor {
            kind: ActorKind::Human,
            name: std::env::var("USER").unwrap_or_else(|_| "human".into()),
        },
        context,
        operation,
        target: ResourceRef {
            group: "".into(),
            kind: kind.into(),
            namespace: namespace.into(),
            name: name.into(),
        },
        outcome,
        source: Source::Tui,
        session_id: "cli".into(),
        reason: break_glass.map(|s| s.to_string()),
        on_behalf_of: None,
    };
    let _ = audit::append(&event);
}

/// Parse a "group/version/kind" string. A single-segment input is treated as a core
/// group `v1/Kind` (the common case for built-ins).
fn parse_gvk(s: &str) -> GroupVersionKind {
    let parts: Vec<&str> = s.split('/').collect();
    match parts.as_slice() {
        [kind] => GroupVersionKind::gvk("", "v1", kind),
        [version, kind] => GroupVersionKind::gvk("", version, kind),
        [group, version, kind] => GroupVersionKind::gvk(group, version, kind),
        _ => {
            eprintln!("invalid gvk: {s}");
            std::process::exit(2);
        }
    }
}
