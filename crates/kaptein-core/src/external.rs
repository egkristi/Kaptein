//! External-tool shell-out — invoke `kubectl krew`, `kustomize`, `helm`, and other
//! binaries that cannot be embedded (M1.2, "Krew shell-out").
//!
//! The contract (per `AGENTS.md`): external tools are shelled out to and must **degrade
//! gracefully when absent** — never panic, never unwrap the subprocess result. Every
//! function here first checks presence and returns a descriptive `Error` when the tool
//! is missing, so the frontend can render "install `krew` to use this" instead of a
//! crash.

use std::process::Command;

use crate::Error;

/// A known external tool Kaptein can shell out to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Krew,
    Kustomize,
    Helm,
}

impl Tool {
    /// The canonical binary name (and a short human label).
    pub fn binary(self) -> &'static str {
        match self {
            Tool::Krew => "kubectl",
            Tool::Kustomize => "kustomize",
            Tool::Helm => "helm",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Krew => "krew",
            Tool::Kustomize => "kustomize",
            Tool::Helm => "helm",
        }
    }
}

/// Whether a tool's binary is present on `PATH`.
pub fn is_available(tool: Tool) -> bool {
    which_bin(tool).is_some()
}

/// Locate a tool on `PATH` (returns `None` if absent).
fn which_bin(tool: Tool) -> Option<std::path::PathBuf> {
    let bin = tool.binary();
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(bin))
        .find(|candidate| candidate.is_file())
}

/// Run a tool with the given arguments, capturing stdout. Returns the trimmed output, or
/// a graceful `Error::External` describing the failure (missing binary, non-zero exit,
/// or I/O error).
pub fn run(tool: Tool, args: &[&str]) -> Result<String, Error> {
    let Some(_path) = which_bin(tool) else {
        return Err(Error::External {
            tool: tool.label().into(),
            message: format!(
                "'{}' is not installed. {}",
                tool.label(),
                install_hint(tool)
            ),
        });
    };

    let output = Command::new(tool.binary())
        .args(args)
        .output()
        .map_err(|e| Error::External {
            tool: tool.label().into(),
            message: format!("failed to launch '{}': {e}", tool.binary()),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::External {
            tool: tool.label().into(),
            message: format!("'{}' exited with {}\n{stderr}", tool.label(), output.status),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// A human-readable install hint for a missing tool (never a hard dependency).
fn install_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Krew => {
            "Install it with `(set -x; cd \"$(mktemp -d)\" && OS=\"$(uname | tr '[:upper:]' '[:lower:]')\" && ARCH=\"$(uname -m | sed -e 's/x86_64/amd64/' -e 's/arm64/arm64/')\" && curl -fsSLO \"https://github.com/kubernetes-sigs/krew/releases/latest/download/krew-${OS}_${ARCH}.tar.gz\" && tar zxf krew-${OS}_${ARCH}.tar.gz && ./krew-${OS}_${ARCH} install krew)`."
        }
        Tool::Kustomize => {
            "Install it with `brew install kustomize` or `go install sigs.k8s.io/kustomize/kustomize/v5@latest`."
        }
        Tool::Helm => {
            "Install it with `brew install helm` or from https://helm.sh/docs/intro/install/."
        }
    }
}

/// List installed krew plugins. Returns an empty list when krew is absent (graceful
/// degradation, never an error for a missing tool in the *read* path).
pub fn list_krew_plugins() -> Vec<String> {
    match run(Tool::Krew, &["krew", "list"]) {
        Ok(stdout) => stdout
            .lines()
            .map(|l| l.split_whitespace().next().unwrap_or("").to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_hint_mentions_tool() {
        assert!(install_hint(Tool::Helm).contains("helm"));
        assert!(install_hint(Tool::Krew).contains("krew"));
        assert!(install_hint(Tool::Kustomize).contains("kustomize"));
    }

    #[test]
    fn missing_tool_degrades_gracefully() {
        // A tool name that will never exist on PATH.
        let err = run(Tool::Helm, &["--definitely-not-a-real-flag"]).err();
        // Either the tool is absent (External error) or it ran and failed (External error).
        // In both cases it must be an Error::External, never a panic.
        match err {
            Some(Error::External { tool, .. }) => assert_eq!(tool, "helm"),
            _ => panic!("expected a graceful External error"),
        }
    }
}
