//! Kaptein CLI — the thin command-line entry point.
//!
//! The CLI drives `kaptein-core` directly; it is a projection, not a home for logic.

mod audit;
mod completion;
mod edit;
mod mcp;
mod schema;

use clap::{Parser, Subcommand};
use clap_complete::engine::ArgValueCompleter;
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
        #[arg(short, long, default_value = "v1/Pod", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// Namespace (omit for cluster-scoped resources)
        #[arg(short, long, add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: Option<String>,
        /// sort by column: name, namespace, created
        #[arg(short, long)]
        sort: Option<String>,
        /// sort descending (newest/last first)
        #[arg(long)]
        descending: bool,
        /// case-insensitive substring filter on name/namespace/status
        #[arg(short, long)]
        filter: Option<String>,
        /// label selector (server-side, e.g. "app=orders"); kubectl-style `-l`
        #[arg(short = 'l', long)]
        selector: Option<String>,
        /// list metadata only (PartialObjectMetadata — no full object bodies)
        #[arg(long)]
        metadata: bool,
        /// kubeconfig context to use (context switching)
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: Option<String>,
        /// render each object through a view-definition (lens) file — lens columns +
        /// lens-inferred status instead of the built-in four-column view (M2.2).
        #[arg(long)]
        lens: Option<String>,
    },
    /// RBAC preflight: check whether the current user may perform a verb.
    Can {
        /// verb, e.g. "get", "create", "delete"
        #[arg(short, long)]
        verb: String,
        /// plural resource name, e.g. "pods", "deployments"
        #[arg(short, long, add = ArgValueCompleter::new(completion::resource_completer))]
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
        #[arg(short, long, add = ArgValueCompleter::new(completion::resource_completer))]
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
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: String,
    },
    /// Validate a view-definition (lens) file against the lens schema.
    ViewdefValidate {
        /// path to a lens YAML/JSON file
        #[arg(short = 'f', long)]
        file: String,
    },
    /// Print the versioned JSON Schema for view definitions (for CI/PR review).
    ViewdefSchema,
    /// Generate shell completions for a supported terminal (bash, elvish, fish,
    /// powershell, zsh).
    Completions {
        /// shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Render a lens against a live (or fixture) resource as the render contract's Row.
    ViewdefRender {
        /// path to a lens YAML/JSON file
        #[arg(short = 'f', long)]
        file: String,
        /// JSON/YAML resource to render (a file path or an inline JSON object)
        #[arg(short = 'r', long)]
        resource: String,
    },
    /// Discover + validate extensions (`extension.yaml` manifests) in a directory.
    Extension {
        /// list discovered extensions, or validate their manifests
        #[command(subcommand)]
        command: ExtensionCommand,
    },
    /// List enabled lens extensions and the `group/version/kind` each targets (M2.2
    /// lens discovery — the set of CRDs that are lens-navigable).
    Lenses {
        /// directory to search recursively for lens extension.yaml (default: ./extensions)
        #[arg(short = 'd', long, default_value = "extensions")]
        dir: String,
    },
    /// Diagnose why a pod is not ready.
    Diagnose {
        /// pod name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: String,
        /// kubeconfig context to use (context switching)
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: Option<String>,
    },
    /// YAML-describe a single resource.
    Describe {
        /// group/version/kind
        #[arg(short, long, default_value = "v1/Pod", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short = 'n', long, add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: Option<String>,
        /// kubeconfig context to use (context switching)
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: Option<String>,
    },
    /// Tail recent logs from a pod.
    Logs {
        /// pod name (omit to stream all pods via --selector)
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: Option<String>,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: String,
        /// label selector for multi-pod streaming (e.g. app=foo)
        #[arg(short = 'l', long)]
        selector: Option<String>,
        /// regex filter applied to each log line
        #[arg(short = 'r', long)]
        regex: Option<String>,
        /// number of lines to tail (per pod, per container — like kubectl)
        #[arg(long, default_value_t = 100)]
        tail: i64,
        /// follow the log stream until interrupted (only with --name)
        #[arg(short = 'f', long)]
        follow: bool,
        /// parse JSON log lines into typed columns (M1.2 "JSON -> columns")
        #[arg(long)]
        json: bool,
        /// kubeconfig context to use (context switching)
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: Option<String>,
    },
    /// Run the governed MCP server over stdio (read-only).
    Mcp,
    /// Show recent cluster events ("what changed in the last N minutes").
    Events {
        /// namespace (omit for all namespaces)
        #[arg(short = 'n', long, add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: Option<String>,
        /// look back this many minutes
        #[arg(long, default_value_t = 15)]
        minutes: i64,
        /// kubeconfig context to use (context switching)
        #[arg(long)]
        context: Option<String>,
    },
    /// Landing view: is anything broken, and what changed recently?
    Overview {
        /// look back this many minutes
        #[arg(long, default_value_t = 15)]
        minutes: i64,
        /// kubeconfig context to use (context switching)
        #[arg(long)]
        context: Option<String>,
    },
    /// Watch a resource kind and report changes (in-memory ring buffer, no persistence).
    Watch {
        /// group/version/kind, e.g. v1/Pod
        #[arg(short, long, default_value = "v1/Pod", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short, long, add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: Option<String>,
        /// stop after this many change events
        #[arg(long, default_value_t = 20)]
        max: usize,
        /// kubeconfig context to use (context switching)
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: Option<String>,
    },
    /// Run the informer-backed bounded store (ADR-0006) for a resource kind.
    WatchStore {
        /// group/version/kind, e.g. v1/Pod
        #[arg(short, long, default_value = "v1/Pod", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short, long, add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: Option<String>,
        /// page size for the bounded (continue-token) seed
        #[arg(long, default_value_t = 500)]
        limit: u32,
        /// stop after this many change events (0 = seed only, no watch)
        #[arg(long, default_value_t = 0)]
        max: usize,
    },
    /// Server-side dry-run a YAML manifest (validate without mutating the cluster).
    Apply {
        /// path to a YAML manifest, or "-" for stdin
        #[arg(short = 'f', long)]
        file: String,
        /// kubeconfig context to use (context switching)
        #[arg(long)]
        context: Option<String>,
    },
    /// Edit a resource's YAML in $EDITOR, then dry-run the result (never applies).
    Edit {
        /// group/version/kind, e.g. v1/ConfigMap or apps/v1/Deployment
        #[arg(short, long, default_value = "v1/ConfigMap", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short = 'n', long, add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: Option<String>,
        /// kubeconfig context to use (context switching)
        #[arg(long, add = ArgValueCompleter::new(completion::context_completer))]
        context: Option<String>,
    },
    /// Forward a pod port to a local TCP listener (Ctrl-C to stop).
    PortForward {
        /// pod name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
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
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
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
        #[arg(short, long, default_value = "v1/ConfigMap", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace (omit for cluster-scoped)
        #[arg(short = 'n', long, add = ArgValueCompleter::new(completion::namespace_completer))]
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
        #[arg(short, long, default_value = "apps/v1/Deployment", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
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
        #[arg(short, long, default_value = "apps/v1/Deployment", add = ArgValueCompleter::new(completion::gvk_completer))]
        gvk: String,
        /// resource name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
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
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
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
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
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
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        name: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
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
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
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
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
        namespace: String,
    },
    /// Attach an ephemeral (debug) container to a running pod (requires --confirm).
    Debug {
        /// pod name
        #[arg(short = 'p', long, add = ArgValueCompleter::new(completion::pod_completer))]
        pod: String,
        /// namespace
        #[arg(short = 'n', long, default_value = "default", add = ArgValueCompleter::new(completion::namespace_completer))]
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

/// Subcommands of `kaptein extension` (ADR-0004 lifecycle).
#[derive(Subcommand)]
enum ExtensionCommand {
    /// List discovered extensions (id, name, version, kind) in a directory.
    List {
        /// directory to search recursively for extension.yaml (default: ./extensions)
        #[arg(short = 'd', long, default_value = "extensions")]
        dir: String,
    },
    /// Validate the extension.yaml manifests in a directory.
    Validate {
        /// directory to search recursively for extension.yaml (default: ./extensions)
        #[arg(short = 'd', long, default_value = "extensions")]
        dir: String,
    },
    /// Enable an extension by id (removes it from the disabled set).
    Enable {
        /// reverse-DNS extension id, e.g. "com.example.cnpg-lens"
        #[arg(short, long)]
        id: String,
    },
    /// Disable an extension by id (adds it to the disabled set).
    Disable {
        /// reverse-DNS extension id, e.g. "com.example.cnpg-lens"
        #[arg(short, long)]
        id: String,
    },
}

fn main() {
    // Dynamic shell completion is handled *before* the async runtime starts: when the
    // shell invokes `COMPLETE=<shell> kaptein -- …`, `CompleteEnv` resolves the
    // completers (which block on their own single-threaded runtime to query the cluster)
    // and exits with the candidates on stdout. A runtime cannot be started from within a
    // runtime, so this must be the synchronous entry point.
    use clap::CommandFactory as _;
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();

    let cli = Cli::parse();
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    if let Err(err) = rt.block_on(run(cli)) {
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
            selector,
            metadata,
            context,
            lens,
        } => {
            let gvk = parse_gvk(&gvk);
            // Use the context-specific client when --context is supplied.
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };

            // Lens-driven rendering (M2.2): list the full objects and render each through
            // the lens — lens columns + lens-inferred status, the same `render_row` a
            // frontend uses. This is the first real surface that *consumes* a lens.
            if let Some(lens_file) = lens {
                let lens_text = std::fs::read_to_string(&lens_file).map_err(|e| {
                    kaptein_core::Error::Internal(format!("cannot read {lens_file}: {e}"))
                })?;
                let lens_value: serde_json::Value =
                    serde_yaml::from_str(&lens_text).map_err(|e| {
                        kaptein_core::Error::Internal(format!("cannot parse {lens_file}: {e}"))
                    })?;
                let vd: kaptein_viewmodel::ViewDefinition = serde_json::from_value(lens_value)
                    .map_err(|e| {
                        kaptein_core::Error::Internal(format!(
                            "cannot deserialize {lens_file}: {e}"
                        ))
                    })?;
                let problems = kaptein_viewmodel::validate_viewdef(&vd);
                if !problems.is_empty() {
                    for p in &problems {
                        eprintln!("error: {p}");
                    }
                    return Err(kaptein_core::Error::Internal(format!(
                        "lens {lens_file} is invalid ({} problem(s))",
                        problems.len()
                    )));
                }

                let objs = kaptein_core::discovery::list_objects_with_selector(
                    &client,
                    &gvk,
                    namespace.as_deref(),
                    selector.as_deref(),
                )
                .await?;
                for obj in objs {
                    let value = serde_json::to_value(&obj).map_err(|e| {
                        kaptein_core::Error::Internal(format!("serialize object: {e}"))
                    })?;
                    let row = kaptein_viewmodel::render_row(&vd, &value);
                    let cells: Vec<String> =
                        row.cells.iter().map(kaptein_viewmodel::cell_text).collect();
                    println!("{}", cells.join("\t"));
                }
                return Ok(());
            }

            // Metadata-only listing (ADR-0006) is the bounded, cheap path for
            // list-heavy views: the API server returns `metadata` only.
            let items = if metadata {
                let mut all = Vec::new();
                let mut token: Option<String> = None;
                loop {
                    let (page, next) =
                        kaptein_core::discovery::list_metadata_bounded_with_selector(
                            &client,
                            &gvk,
                            namespace.as_deref(),
                            selector.as_deref(),
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
            } else if selector.as_deref().is_some_and(|s| !s.is_empty()) {
                kaptein_core::discovery::list_with_selector(
                    &client,
                    &gvk,
                    namespace.as_deref(),
                    selector.as_deref(),
                )
                .await?
            } else {
                kaptein_core::discovery::list(&client, &gvk, namespace.as_deref()).await?
            };

            // Sort + filter in the view-model (the single sort/filter implementation —
            // the CLI/TUI consume the same semantics; no core-side duplicate, issue #32).
            // Map each `ResourceSummary` to a render-contract `Row` with the 4 standard
            // columns (name/namespace/status/created), then apply the view-model sort/filter.
            let column_ids: Vec<String> = ["name", "namespace", "status", "created"]
                .map(String::from)
                .to_vec();
            let mut rows: Vec<kaptein_viewmodel::Row> = items
                .into_iter()
                .map(|s| kaptein_viewmodel::Row {
                    id: kaptein_viewmodel::RowId(s.uid.clone().unwrap_or_else(|| {
                        if s.namespace.is_empty() {
                            s.name.clone()
                        } else {
                            format!("{}/{}", s.namespace, s.name)
                        }
                    })),
                    cells: vec![
                        kaptein_viewmodel::Cell::Text { value: s.name },
                        kaptein_viewmodel::Cell::Text { value: s.namespace },
                        kaptein_viewmodel::Cell::Text { value: s.status },
                        kaptein_viewmodel::Cell::Text {
                            value: s.created.map(|t| t.0.to_string()).unwrap_or_default(),
                        },
                    ],
                })
                .collect();

            let sort_spec = sort.as_deref().and_then(|s| {
                let column = match s.to_ascii_lowercase().as_str() {
                    "name" => "name",
                    "namespace" | "ns" => "namespace",
                    "created" | "age" => "created",
                    _ => return None,
                };
                Some(kaptein_viewmodel::SortSpec {
                    column: column.to_string(),
                    descending,
                })
            });
            kaptein_viewmodel::sort_rows(&mut rows, &column_ids, sort_spec.as_ref());
            let filter_spec = filter.as_deref().map(|f| kaptein_viewmodel::Filter {
                expression: f.to_string(),
            });
            rows = kaptein_viewmodel::filter_rows(rows, filter_spec.as_ref());

            for row in rows {
                println!(
                    "{}\t{}\t{}\t{}",
                    kaptein_viewmodel::cell_text(&row.cells[1]), // namespace
                    gvk.kind,                                    // kind
                    kaptein_viewmodel::cell_text(&row.cells[0]), // name
                    kaptein_viewmodel::cell_text(&row.cells[3]), // created
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
                    "lens {} ({}) is valid ({} columns, {} status rules, {} conditions, {} actions)",
                    vd.id,
                    vd.target.display(),
                    vd.columns.len(),
                    vd.status.len(),
                    vd.conditions.len(),
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
        Command::ViewdefSchema => {
            // The lens schema is the MIT/Apache-2.0 extension surface (ADR-0004). Emit it
            // so CI and contributors can validate lenses against the exact schema the
            // release implements. Embedded (not `include_str!`) so `cargo publish`
            // packages it correctly.
            print!("{}", schema::VIEWDEF_SCHEMA);
            Ok(())
        }
        Command::Completions { shell } => {
            // Emit shell completions for the whole CLI (subcommands, flags, and their
            // arguments) via clap_complete — the same definitions the parser uses, so
            // completions can never drift from the command surface. `Shell` also derives
            // `ValueEnum`, so `--shell` is a validated choice rather than a free string.
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            let name = cmd.get_name().to_string();
            clap_complete::generate(shell, &mut cmd, name, &mut std::io::stdout());
            Ok(())
        }
        Command::ViewdefRender { file, resource } => {
            // Load the lens, then render a resource (a file path or inline JSON) through
            // it into the render contract's `Row`. This proves status-rule *rendering*
            // end-to-end and gives lens authors a way to test a lens against a fixture
            // without a live cluster.
            let lens_text = std::fs::read_to_string(&file)
                .map_err(|e| kaptein_core::Error::Internal(format!("cannot read {file}: {e}")))?;
            let lens_value: serde_json::Value = serde_yaml::from_str(&lens_text)
                .map_err(|e| kaptein_core::Error::Internal(format!("cannot parse {file}: {e}")))?;
            let vd: kaptein_viewmodel::ViewDefinition = serde_json::from_value(lens_value)
                .map_err(|e| {
                    kaptein_core::Error::Internal(format!("cannot deserialize {file}: {e}"))
                })?;
            let problems = kaptein_viewmodel::validate_viewdef(&vd);
            if !problems.is_empty() {
                for p in &problems {
                    eprintln!("error: {p}");
                }
                return Err(kaptein_core::Error::Internal(format!(
                    "lens {file} is invalid ({} problem(s))",
                    problems.len()
                )));
            }

            // The resource is either a file path or inline JSON/YAML.
            let resource_value: serde_json::Value = if std::path::Path::new(&resource).is_file() {
                let text = std::fs::read_to_string(&resource).map_err(|e| {
                    kaptein_core::Error::Internal(format!("cannot read {resource}: {e}"))
                })?;
                serde_yaml::from_str(&text).map_err(|e| {
                    kaptein_core::Error::Internal(format!("cannot parse {resource}: {e}"))
                })?
            } else {
                serde_yaml::from_str(&resource).map_err(|e| {
                    kaptein_core::Error::Internal(format!("cannot parse inline resource: {e}"))
                })?
            };

            let row = kaptein_viewmodel::render_row(&vd, &resource_value);
            let json = serde_json::to_string_pretty(&row)
                .map_err(|e| kaptein_core::Error::Internal(format!("cannot serialize row: {e}")))?;
            println!("{json}");
            Ok(())
        }
        Command::Extension { command } => match command {
            ExtensionCommand::List { dir } => {
                let (found, _problems) =
                    kaptein_core::extension::discover(std::path::Path::new(&dir));
                let config = kaptein_core::config::load();
                for ext in &found {
                    let state = if config.extensions.is_enabled(&ext.manifest.id) {
                        "enabled"
                    } else {
                        "disabled"
                    };
                    println!(
                        "{:<30} {:<20} v{:<8} {:<11} {:?}",
                        ext.manifest.id,
                        ext.manifest.name,
                        ext.manifest.version,
                        state,
                        ext.manifest.kind
                    );
                }
                if found.is_empty() {
                    println!("no extensions found in {dir}");
                }
                Ok(())
            }
            ExtensionCommand::Validate { dir } => {
                let (found, problems) =
                    kaptein_core::extension::discover(std::path::Path::new(&dir));
                for p in &problems {
                    eprintln!("error: {p}");
                }
                if problems.is_empty() {
                    println!("{} extension(s) valid in {dir}", found.len());
                    Ok(())
                } else {
                    Err(kaptein_core::Error::Internal(format!(
                        "{} invalid extension manifest(s)",
                        problems.len()
                    )))
                }
            }
            ExtensionCommand::Enable { id } => {
                kaptein_core::config::update_config(|config| {
                    config.extensions.disabled.retain(|d| d != &id);
                })
                .map_err(kaptein_core::Error::Internal)?;
                println!("enabled {id}");
                Ok(())
            }
            ExtensionCommand::Disable { id } => {
                kaptein_core::config::update_config(|config| {
                    if !config.extensions.disabled.iter().any(|d| d == &id) {
                        config.extensions.disabled.push(id.clone());
                    }
                })
                .map_err(kaptein_core::Error::Internal)?;
                println!("disabled {id}");
                Ok(())
            }
        },
        Command::Lenses { dir } => {
            // Lens discovery (M2.2): walk the extension directory, keep `kind: lens`,
            // resolve each lens' target GVK, and honor the enable/disable set. This is
            // the "which CRDs are lens-navigable" answer a frontend reads at startup.
            let (lenses, problems) =
                kaptein_core::extension::discover_lenses(std::path::Path::new(&dir));
            for p in &problems {
                eprintln!("error: {p}");
            }
            let config = kaptein_core::config::load();
            let mut shown = 0usize;
            for lens in &lenses {
                if !config.extensions.is_enabled(&lens.id) {
                    continue;
                }
                println!(
                    "{:<40} {}/{}/{}",
                    lens.id, lens.target.group, lens.target.version, lens.target.kind
                );
                shown += 1;
            }
            if shown == 0 {
                println!("no enabled lenses found in {dir}");
            }
            Ok(())
        }
        Command::Diagnose {
            name,
            namespace,
            context,
        } => {
            // Use the context-specific client when --context is supplied.
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
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
            context,
        } => {
            let gvk = parse_gvk(&gvk);
            // Use the context-specific client when --context is supplied (k9s-parity
            // context switching, the same as `get --context`).
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
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
            context,
        } => {
            // Use the context-specific client when --context is supplied.
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
            // JSON mode: parse JSON log lines into typed columns. Applies to both
            // single-pod and multi-pod paths; plain lines fall back to `_raw`.
            if json {
                // JSON mode honors `--regex` too: filter raw lines before parsing, so
                // the typed columns reflect only matching lines (consistent with the
                // plain single-pod and multi-pod paths).
                let re = regex
                    .as_deref()
                    .map(regex::Regex::new)
                    .transpose()
                    .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                let raw_lines: Vec<String> = match name.as_deref() {
                    Some(pod_name) => {
                        let logs = kaptein_core::describe::pod_logs(
                            &client,
                            &namespace,
                            pod_name,
                            Some(tail),
                        )
                        .await?;
                        logs.into_iter()
                            .map(|(_, line)| line)
                            .filter(|line| re.as_ref().is_none_or(|r| r.is_match(line)))
                            .collect()
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
                        // Apply the regex filter here too — the single-pod non-follow
                        // path must honor `--regex` exactly like the `follow` and
                        // `--selector` paths do (it previously ignored it).
                        let re = regex
                            .as_deref()
                            .map(regex::Regex::new)
                            .transpose()
                            .map_err(|e| kaptein_core::Error::Internal(e.to_string()))?;
                        for (container, line) in logs {
                            if let Some(re) = &re
                                && !re.is_match(&line)
                            {
                                continue;
                            }
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
        Command::Events {
            namespace,
            minutes,
            context,
        } => {
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
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
        Command::Overview { minutes, context } => {
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
            let since_ms = now_ms().saturating_sub(minutes * 60 * 1000);
            // Feed a small watch ring of pod changes (M1.4) so "what changed" is the
            // informer's view, then compose the landing view from events + ring.
            let ring = kaptein_core::watchring::WatchRing::new(50);
            let pod_gvk = kube::core::GroupVersionKind::gvk("", "v1", "Pod");
            let _ = kaptein_core::watchring::snapshot_into_ring(&client, &pod_gvk, None, &ring, 50)
                .await;

            let overview = kaptein_core::overview::overview_with_health(
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
            println!("  unhealthy pods: {}", overview.unhealthy_pods.len());
            for u in overview.unhealthy_pods {
                println!("    [UNHEALTHY] {}/{}", u.namespace, u.name);
                for f in u.findings {
                    println!("        - {f}");
                }
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
            context,
        } => {
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
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
        Command::WatchStore {
            gvk,
            namespace,
            limit,
            max,
        } => {
            let gvk = parse_gvk(&gvk);
            let store = kaptein_core::store::InformerStore::new();
            // The bounded, metadata-only informer store (ADR-0006) — this is the caller
            // that proves `run_informer` is reachable outside its own tests (issue #18).
            // `--max 0` (default) seeds and returns; `--max N` also applies N watch
            // deltas before returning, so the command terminates instead of watching
            // forever.
            kaptein_core::store::run_informer(
                &client,
                &gvk,
                namespace.as_deref(),
                &store,
                limit,
                Some(max),
            )
            .await?;
            let snap = store.snapshot();
            println!("watch-store {gvk:?}: {} objects in store", snap.len());
            for s in snap {
                println!("{}\t{}\t{}", s.namespace, s.kind, s.name);
            }
            Ok(())
        }
        Command::Apply { file, context } => {
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
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
            context,
        } => {
            let client = match context.as_deref() {
                Some(ctx) => kaptein_core::discovery::client_for_context(Some(ctx)).await?,
                None => client.clone(),
            };
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

#[cfg(test)]
mod tests {
    /// The bundled JSON Schema's `api_version` `const` must equal the Rust
    /// `LENS_SCHEMA_VERSION` — a drift between the schema and the code would let a lens
    /// validate against one version and be refused by the other. This test catches that.
    #[test]
    fn bundled_schema_api_version_matches_lens_schema_version() {
        let schema = crate::schema::VIEWDEF_SCHEMA;
        let value: serde_json::Value = serde_json::from_str(schema).expect("schema is valid JSON");
        let const_version = value
            .pointer("/properties/api_version/const")
            .and_then(|v| v.as_u64())
            .expect("schema has properties.api_version.const");
        assert_eq!(
            const_version,
            kaptein_viewmodel::LENS_SCHEMA_VERSION as u64,
            "viewdef.schema.json api_version const drifts from LENS_SCHEMA_VERSION"
        );
    }

    /// The bundled JSON Schema must declare the `conditions` property (condition-based
    /// status rules) — if the Rust `ConditionRule` type is added but the schema omits it,
    /// a JSON-Schema validation would reject a lens the Rust validator accepts.
    #[test]
    fn bundled_schema_declares_conditions() {
        let schema = crate::schema::VIEWDEF_SCHEMA;
        let value: serde_json::Value = serde_json::from_str(schema).expect("schema is valid JSON");
        assert!(
            value.pointer("/properties/conditions").is_some(),
            "viewdef.schema.json must declare a conditions property"
        );
    }

    /// A condition-based lens must validate and deserialize (Kubernetes-condition
    /// status inference is how the hardest lenses — Strimzi, KubeVirt, cert-manager —
    /// signal readiness; ADR-0012).
    #[test]
    fn condition_lens_validates_cleanly() {
        let yaml = "id: com.kaptein.kafka-lens\napi_version: 1\ntarget: {group: kafka.strimzi.io, version: v1beta2, kind: Kafka}\ncolumns: [{id: name, header_key: col.name, kind: text, sortable: true, field: metadata.name}]\nconditions: [{condition_type: Ready, status: \"True\", level: ok}, {condition_type: Ready, status: \"False\", level: error}]\nactions: [{id: describe, label_key: action.describe, state: allowed}]\n";
        let value: serde_json::Value = serde_yaml::from_str(yaml).expect("lens is valid YAML");
        let vd: kaptein_viewmodel::ViewDefinition =
            serde_json::from_value(value).expect("lens deserializes");
        assert!(
            kaptein_viewmodel::validate_viewdef(&vd).is_empty(),
            "condition lens must be valid"
        );
        let ready =
            serde_json::json!({"status": {"conditions": [{"type": "Ready", "status": "True"}]}});
        assert_eq!(
            kaptein_viewmodel::evaluate_status(&vd, &ready),
            Some(kaptein_viewmodel::StatusLevel::Ok)
        );
    }

    /// The example lens must validate cleanly against the real validator (it is the
    /// "reviewable in PRs" acceptance test from ADR-0012).
    #[test]
    fn example_cnpg_lens_validates_cleanly() {
        // A minimal in-repo fixture (the canonical lens lives under `extensions/`, which
        // `cargo publish` does not package — so the test uses an inline document).
        let yaml = "id: com.example.cnpg-lens\napi_version: 1\ntarget: {group: postgresql.cnpg.io, version: v1, kind: Cluster}\ncolumns: [{id: name, header_key: col.name, kind: text, sortable: true, field: metadata.name}]\nstatus: [{field: status.phase, op: eq, value: ClusterIsReady, level: ok}]\nactions: [{id: describe, label_key: action.describe, state: allowed}]\n";
        let value: serde_json::Value = serde_yaml::from_str(yaml).expect("lens is valid YAML");
        let vd: kaptein_viewmodel::ViewDefinition =
            serde_json::from_value(value).expect("lens deserializes");
        assert!(
            kaptein_viewmodel::validate_viewdef(&vd).is_empty(),
            "example lens must be valid"
        );
    }

    /// `render_row` maps a lens + resource into the render contract's `Row` — the
    /// "status-rule rendering" half of M2.2, exercised end-to-end from a lens fixture.
    #[test]
    fn render_row_binds_fields_and_infers_status() {
        let yaml = "id: com.example.cnpg-lens\napi_version: 1\ntarget: {group: postgresql.cnpg.io, version: v1, kind: Cluster}\ncolumns: [{id: name, header_key: col.name, kind: text, sortable: true, field: metadata.name}, {id: instances, header_key: col.instances, kind: number, sortable: true, field: spec.instances}, {id: status, header_key: col.status, kind: status, sortable: true}]\nstatus: [{field: status.phase, op: eq, value: ClusterIsReady, level: ok}]\n";
        let vd: kaptein_viewmodel::ViewDefinition =
            serde_yaml::from_str(yaml).expect("lens deserializes");
        let resource = serde_json::json!({
            "metadata": {"uid": "u1", "name": "pg", "namespace": "db"},
            "spec": {"instances": 3},
            "status": {"phase": "ClusterIsReady"}
        });
        let row = kaptein_viewmodel::render_row(&vd, &resource);
        assert_eq!(row.id, kaptein_viewmodel::RowId("u1".into()));
        assert_eq!(
            row.cells[0],
            kaptein_viewmodel::Cell::Text { value: "pg".into() }
        );
        assert_eq!(row.cells[1], kaptein_viewmodel::Cell::Number { value: 3 });
        assert_eq!(
            row.cells[2],
            kaptein_viewmodel::Cell::Status {
                level: kaptein_viewmodel::StatusLevel::Ok,
                label_key: "status.ok".into(),
            }
        );
    }
}
