//! Exec — run a one-shot command in a pod's container and stream output back.
//!
//! The read-only exec feature (M1.2): a non-interactive `exec` that runs a command and
//! returns combined stdout/stderr, plus an interactive TTY exec that allocates a TTY and
//! proxies stdin/stdout between the local terminal and the pod.

use kube::Client;
use kube::api::{Api, AttachParams};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::Error;

/// The output of an exec: the combined stdout/stderr of the command.
#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub output: String,
}

/// Run `command` in the pod's container and return combined stdout/stderr.
pub async fn exec(
    client: &Client,
    namespace: &str,
    pod: &str,
    command: &[String],
    container: Option<&str>,
) -> Result<ExecOutput, Error> {
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client.clone(), namespace);
    let ap = AttachParams {
        container: container.map(|c| c.to_string()),
        stdin: false,
        stdout: true,
        stderr: true,
        tty: false,
        ..AttachParams::default()
    };
    let mut attached = pods.exec(pod, command, &ap).await?;

    // Extract the streams first (owned values), then read them concurrently. The
    // streams stay open until the process exits, so reading sequentially would block.
    let mut stdout = attached.stdout();
    let mut stderr = attached.stderr();

    let mut stdout_buf = String::new();
    let mut stderr_buf = String::new();

    let stdout_fut = async {
        if let Some(mut s) = stdout.take() {
            let mut buf = [0u8; 4096];
            loop {
                let n = s
                    .read(&mut buf)
                    .await
                    .map_err(|e| Error::Internal(e.to_string()))?;
                if n == 0 {
                    break;
                }
                stdout_buf.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        }
        Ok::<(), Error>(())
    };
    let stderr_fut = async {
        if let Some(mut s) = stderr.take() {
            let mut buf = [0u8; 4096];
            loop {
                let n = s
                    .read(&mut buf)
                    .await
                    .map_err(|e| Error::Internal(e.to_string()))?;
                if n == 0 {
                    break;
                }
                stderr_buf.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
        }
        Ok::<(), Error>(())
    };

    let (out_res, err_res) = tokio::join!(stdout_fut, stderr_fut);
    out_res?;
    err_res?;
    attached
        .join()
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;

    let mut output = stdout_buf;
    output.push_str(&stderr_buf);
    Ok(ExecOutput { output })
}

/// Run an **interactive TTY** exec: allocate a TTY and proxy stdin/stdout between the
/// provided reader/writer and the pod process. The caller owns the raw-mode terminal and
/// passes an async reader (e.g. `tokio::io::stdin()`) and writer (`tokio::io::stdout()`).
///
/// Returns when the remote command exits or the streams close.
pub async fn exec_tty(
    client: &Client,
    namespace: &str,
    pod: &str,
    command: &[String],
    container: Option<&str>,
    mut local_in: impl AsyncRead + Unpin,
    mut local_out: impl AsyncWrite + Unpin,
) -> Result<(), Error> {
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client.clone(), namespace);
    let ap = AttachParams {
        container: container.map(|c| c.to_string()),
        stdin: true,
        stdout: true,
        // A TTY multiplexes stderr into stdout — the API rejects `stderr: true`
        // together with `tty: true`.
        stderr: false,
        tty: true,
        ..AttachParams::default()
    };
    let mut attached = pods.exec(pod, command, &ap).await?;

    let mut stdin_w = attached
        .stdin()
        .ok_or_else(|| Error::Internal("TTY stdin unavailable".into()))?;
    let mut stdout_r = attached
        .stdout()
        .ok_or_else(|| Error::Internal("TTY stdout unavailable".into()))?;

    // Proxy: local stdin -> pod stdin; pod stdout -> local stdout. (With a TTY there is
    // no separate stderr stream — the pod's stderr is multiplexed into stdout.)
    let stdin_to_pod = async {
        let mut buf = [0u8; 4096];
        loop {
            let n = local_in
                .read(&mut buf)
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;
            if n == 0 {
                break;
            }
            stdin_w
                .write_all(&buf[..n])
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
        // Close the pod's stdin on EOF so the remote process can exit.
        let _ = stdin_w.shutdown().await;
        Ok::<(), Error>(())
    };
    let pod_to_local = async {
        let mut buf = [0u8; 4096];
        loop {
            let n = stdout_r
                .read(&mut buf)
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;
            if n == 0 {
                break;
            }
            local_out
                .write_all(&buf[..n])
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;
            local_out
                .flush()
                .await
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
        Ok::<(), Error>(())
    };

    // Drive both directions concurrently.
    let (a, b) = tokio::join!(stdin_to_pod, pod_to_local);
    a?;
    b?;
    attached
        .join()
        .await
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}
