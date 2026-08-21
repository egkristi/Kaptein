//! Kaptein CLI — the thin command-line entry point.
//!
//! The CLI drives `kaptein-core` directly; it is a projection, not a home for logic.

mod audit;
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
    /// Show the current context and its guardrail classification.
    Context,
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
        /// pod name
        #[arg(short = 'p', long)]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default")]
        namespace: String,
        /// number of lines to tail
        #[arg(long, default_value_t = 100)]
        tail: i64,
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
    /// Server-side dry-run a YAML manifest (validate without mutating the cluster).
    Apply {
        /// path to a YAML manifest, or "-" for stdin
        #[arg(short = 'f', long)]
        file: String,
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
        Command::Get { gvk, namespace } => {
            let gvk = parse_gvk(&gvk);
            let items = kaptein_core::discovery::list(&client, &gvk, namespace.as_deref()).await?;
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
        Command::Context => {
            let config = kaptein_core::config::load();
            let ctx = kaptein_core::discovery::current_context_name()?;
            let class = config.guardrails.classify(&ctx);
            println!("context: {ctx}");
            println!("class: {class:?}");
            Ok(())
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
            tail,
        } => {
            let logs =
                kaptein_core::describe::pod_logs(&client, &namespace, &name, Some(tail)).await?;
            for (container, line) in logs {
                println!("[{container}] {line}");
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
            let overview = kaptein_core::overview::overview(&client, None, since_ms).await?;
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
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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
