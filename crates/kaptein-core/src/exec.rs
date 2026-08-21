//! Exec — run a one-shot command in a pod's container and stream output back.
//!
//! The read-only exec feature (M1.2): a non-interactive `exec` that runs a command and
//! returns combined stdout/stderr. Interactive TTY/attach is deferred; this is the
//! safe, scriptable form.

use kube::api::AttachParams;
use kube::{Api, Client};
use tokio::io::AsyncReadExt;

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
