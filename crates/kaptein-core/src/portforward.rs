//! Port-forward — forward a pod port to a local TCP listener.
//!
//! The read-only k9s-parity feature (M1.2): creates a `Portforwarder` for a pod and
//! bridges the Kubernetes stream to a local `TcpListener`, staying up until cancelled
//! or the upstream closes. No cluster mutation — this only reads the pod's stream.

use std::net::SocketAddr;

use kube::api::Portforwarder;
use kube::{Api, Client};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::Error;

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
