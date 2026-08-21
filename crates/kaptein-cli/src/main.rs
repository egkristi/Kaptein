//! Kaptein CLI — the thin command-line entry point.
//!
//! The CLI drives `kaptein-core` directly; it is a projection, not a home for logic.

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
    }
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
