//! Port-forward — forward a pod port to a local TCP listener.
//!
//! The read-only k9s-parity feature (M1.2): creates a `Portforwarder` for a pod and
//! bridges the Kubernetes stream to a local `TcpListener`, staying up until cancelled
//! or the upstream closes. No cluster mutation — this only reads the pod's stream.
//!
//! Also provides a **named, persistent forward manager**: forwards are identified by a
//! name, survive restarts via a small config file, and reconnect automatically when the
//! upstream pod stream drops.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;

use kube::api::Portforwarder;
use kube::{Api, Client};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::Error;

/// A single named forward definition.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ForwardSpec {
    /// The user-assigned name (used to reference/remove the forward).
    pub name: String,
    pub namespace: String,
    pub pod: String,
    pub target_port: u16,
    /// Local bind port (0 = ephemeral).
    pub local_port: u16,
}

/// Bridge a single upstream `Portforwarder` port to a local TCP listener.
///
/// Binds `local_addr` and, for each accepted connection, pipes bytes both ways between
/// the local socket and the pod stream. Returns the bound address (useful when binding
/// port 0 to request an ephemeral port).
pub async fn forward(
    client: &Client,
    namespace: &str,
    pod: &str,
    target_port: u16,
    local_addr: SocketAddr,
) -> Result<SocketAddr, Error> {
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client.clone(), namespace);
    let mut pf: Portforwarder = pods.portforward(pod, &[target_port]).await?;
    let stream = pf
        .take_stream(target_port)
        .ok_or_else(|| Error::Internal(format!("port {target_port} not forwarded")))?;

    let listener = TcpListener::bind(local_addr)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    let bound = listener
        .local_addr()
        .map_err(|e| Error::Internal(e.to_string()))?;

    // Serve one connection at a time (sufficient for MVP; multi-connection mux later).
    tokio::spawn(async move {
        let (mut upstream_r, mut upstream_w) = tokio::io::split(stream);
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                break;
            };
            let (mut local_r, mut local_w) = socket.into_split();
            let mut up_buf = vec![0u8; 64 * 1024];
            let mut down_buf = vec![0u8; 64 * 1024];
            let _ = copy_bidi(
                &mut local_r,
                &mut local_w,
                &mut upstream_r,
                &mut upstream_w,
                &mut up_buf,
                &mut down_buf,
            )
            .await;
        }
    });

    Ok(bound)
}

/// Bidirectional copy between a local socket and the pod stream.
async fn copy_bidi(
    local_r: &mut tokio::net::tcp::OwnedReadHalf,
    local_w: &mut tokio::net::tcp::OwnedWriteHalf,
    upstream_r: &mut (impl AsyncRead + Unpin),
    upstream_w: &mut (impl AsyncWrite + Unpin),
    up_buf: &mut [u8],
    down_buf: &mut [u8],
) -> std::io::Result<()> {
    loop {
        tokio::select! {
            n = local_r.read(up_buf) => {
                let n = n?;
                if n == 0 { break; }
                upstream_w.write_all(&up_buf[..n]).await?;
            }
            n = upstream_r.read(down_buf) => {
                let n = n?;
                if n == 0 { break; }
                local_w.write_all(&down_buf[..n]).await?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Named, persistent forward manager
// ---------------------------------------------------------------------------

/// Resolve the named-forward manager file path: `$KAPTEIN_FORWARDS`, else
/// `$XDG_STATE_HOME/kaptein/forwards.json`, else `~/.local/state/kaptein/forwards.json`.
pub fn manager_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("KAPTEIN_FORWARDS") {
        return std::path::PathBuf::from(p);
    }
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        return std::path::PathBuf::from(xdg)
            .join("kaptein")
            .join("forwards.json");
    }
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("kaptein")
            .join("forwards.json");
    }
    std::path::PathBuf::from("forwards.json")
}

/// A named forward manager: forwards are persisted to a small JSON file so they survive
/// restarts, and a `reconnect_loop` re-establishes a dropped forward automatically.
#[derive(Debug, Clone, Default)]
pub struct ForwardManager {
    /// Forward name → spec.
    specs: BTreeMap<String, ForwardSpec>,
    /// On-disk persistence path (None = in-memory only).
    path: Option<std::path::PathBuf>,
}

impl ForwardManager {
    /// Load a manager from `path` (missing/empty file → empty manager).
    pub fn load(path: Option<&Path>) -> Self {
        let mut manager = Self {
            path: path.map(|p| p.to_path_buf()),
            ..Self::default()
        };
        if let Some(p) = path
            && let Ok(text) = std::fs::read_to_string(p)
            && let Ok(specs) = serde_json::from_str::<Vec<ForwardSpec>>(&text)
        {
            for spec in specs {
                manager.specs.insert(spec.name.clone(), spec);
            }
        }
        manager
    }

    /// List all known forwards.
    pub fn list(&self) -> Vec<ForwardSpec> {
        self.specs.values().cloned().collect()
    }

    /// Add or replace a forward, persisting to disk when a path is set.
    pub fn upsert(&mut self, spec: ForwardSpec) -> Result<(), Error> {
        self.specs.insert(spec.name.clone(), spec);
        self.persist()
    }

    /// Remove a forward by name.
    pub fn remove(&mut self, name: &str) -> Result<(), Error> {
        self.specs.remove(name);
        self.persist()
    }

    /// Persist the current specs to disk (no-op for in-memory managers).
    fn persist(&self) -> Result<(), Error> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::Internal(e.to_string()))?;
        }
        let specs: Vec<&ForwardSpec> = self.specs.values().collect();
        let json =
            serde_json::to_string_pretty(&specs).map_err(|e| Error::Internal(e.to_string()))?;
        std::fs::write(path, json).map_err(|e| Error::Internal(e.to_string()))
    }
}

/// A running forward, wrapping the spawned serve task.
pub struct RunningForward {
    pub spec: ForwardSpec,
    /// The bound local address (useful when the spec used port 0).
    pub local_addr: SocketAddr,
    /// A cancel handle to stop the forward.
    pub cancel: tokio::sync::watch::Sender<bool>,
}

/// Start a named forward with auto-reconnect: if the upstream pod stream drops, this
/// re-establishes the port-forward and re-binds the local listener until cancelled.
///
/// The returned `cancel` sender is a `watch` channel; sending `true` (or dropping the
/// sender) stops the loop.
pub async fn start_named_forward(
    client: Client,
    spec: ForwardSpec,
) -> Result<RunningForward, Error> {
    let local_addr = format!("127.0.0.1:{}", spec.local_port)
        .parse::<SocketAddr>()
        .map_err(|e| Error::Internal(e.to_string()))?;

    // Bind the listener once up front so the caller learns the actual port.
    let listener = TcpListener::bind(local_addr)
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    let bound = listener
        .local_addr()
        .map_err(|e| Error::Internal(e.to_string()))?;

    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(reconnect_loop(client, spec.clone(), listener, cancel_rx));

    Ok(RunningForward {
        spec,
        local_addr: bound,
        cancel: cancel_tx,
    })
}

/// The reconnect loop: serve one upstream stream, and when it drops, reconnect.
async fn reconnect_loop(
    client: Client,
    spec: ForwardSpec,
    listener: TcpListener,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) {
    let pods: Api<k8s_openapi::api::core::v1::Pod> =
        Api::namespaced(client.clone(), &spec.namespace);

    loop {
        if *cancel_rx.borrow() {
            break;
        }

        // Establish the upstream port-forward stream (reconnect each iteration).
        let stream = match pods.portforward(&spec.pod, &[spec.target_port]).await {
            Ok(mut pf) => match pf.take_stream(spec.target_port) {
                Some(s) => s,
                None => {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    continue;
                }
            },
            Err(_) => {
                // Back off and retry (auto-reconnect).
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
        };

        let (mut upstream_r, mut upstream_w) = tokio::io::split(stream);
        loop {
            tokio::select! {
                _ = cancel_rx.changed() => {
                    return;
                }
                accepted = listener.accept() => {
                    let Ok((socket, _)) = accepted else {
                        break;
                    };
                    let (mut local_r, mut local_w) = socket.into_split();
                    let mut up_buf = vec![0u8; 64 * 1024];
                    let mut down_buf = vec![0u8; 64 * 1024];
                    let _ = copy_bidi(
                        &mut local_r,
                        &mut local_w,
                        &mut upstream_r,
                        &mut upstream_w,
                        &mut up_buf,
                        &mut down_buf,
                    )
                    .await;
                }
            }
        }
        // Stream dropped — loop reconnects.
    }
}
