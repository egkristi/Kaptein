# Kaptein Installation

How to install the **Kaptein** Kubernetes workbench — a single static binary, `kaptein`.
The TUI, GUI, and headless agent are all projections of the same view-model, invoked as
subcommands (`kaptein tui`, `kaptein mcp`, …).

For the *why* and the roadmap, see [`README.md`](../README.md) and
[`ROADMAP.md`](../ROADMAP.md). For how to *use* Kaptein once installed, see
[`USAGE.md`](./USAGE.md). For known limitations, see [`ISSUES.md`](../ISSUES.md).

One static binary ships:

| Command | Purpose |
|---------|---------|
| `kaptein` | The CLI — scripting, one-shots, MCP server, extension lifecycle, **and** the TUI (`kaptein tui`). |

**Recommended — `cargo install` (CLI):** if you have a Rust toolchain (≥ 1.97), the
simplest way to get the CLI is the crate published on crates.io:

```bash
cargo install kaptein          # the CLI + TUI (one binary)
kaptein tui                    # launch the TUI
```

**Recommended — signed release (one binary, no Rust):** the install script downloads
the prebuilt, signed binary for your platform, verifies the SHA-256 checksum against
the release's `SHA256SUMS`, cosign-verifies that file's signature against the GitHub
Actions OIDC identity, and installs to `~/.local/bin` (or `KAPTEIN_INSTALL_DIR`):

```bash
curl -fsSL https://raw.githubusercontent.com/egkristi/Kaptein/main/install.sh | bash
# pick a version / install dir:
KAPTEIN_VERSION=v0.31.0 KAPTEIN_INSTALL_DIR="$HOME/.local/bin" ./install.sh
```

Which to use: `cargo install` is the default for CLI-only users who already have Rust.
`install.sh` is the default when you want the verified signature chain (no Rust
required).

Other install methods:

- **kubectl plugin (Krew)**: Kaptein is BUSL-1.1 (source-available), and Krew's central
  index requires plugins to be open source under an OSI-approved license — so it is not
  submitted to `kubernetes-sigs/krew-index`. Install from Kaptein's **custom index**, or
  directly from the release manifest:

  ```bash
  kubectl krew index add kaptein https://github.com/egkristi/krew-index.git
  kubectl krew install kaptein/kaptein

  # or, straight from the release asset (no index):
  kubectl krew install --manifest-url=https://github.com/egkristi/Kaptein/releases/latest/download/kaptein.yaml
  ```

  See [#34](https://github.com/egkristi/Kaptein/issues/34) for the licensing rationale.
- **Container image**: `docker run ghcr.io/egkristi/kaptein get --gvk v1/Pod`.
- **From source**: `cargo build --release` (requires a Rust toolchain ≥ 1.97).

Verify a download yourself (`cosign` must be installed):

```bash
cosign verify-blob \
  --certificate-identity "https://github.com/egkristi/Kaptein/.github/workflows/release.yml@refs/tags/<tag>" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  --bundle SHA256SUMS.bundle SHA256SUMS
```

## Shell completions

Shell completions are generated from the same parser the CLI uses, so they can never
drift from the command surface:

```bash
kaptein completions bash > ~/.local/share/bash-completion/completions/kaptein
kaptein completions zsh  > "${fpath[1]}/_kaptein"
kaptein completions fish > ~/.config/fish/completions/kaptein.fish
```

**Dynamic (live-cluster) completion.** The static `completions` output covers flags and
subcommands; for **live values** — namespaces, kubeconfig contexts, pod names, GVKs, and
plural resources — the CLI also ships a *dynamic* completer that queries the cluster at
completion time. Source the dynamic registration (which re-invokes `kaptein` as you type)
instead of, or alongside, the static file:

```bash
# bash
source <(COMPLETE=bash kaptein)
# zsh
source <(COMPLETE=zsh kaptein)
# fish
COMPLETE=fish kaptein | source
```

What completes dynamically:

| Argument | Candidates |
|----------|-----------|
| `--namespace` / `-n` | live namespace names |
| `--context` | kubeconfig context names (no cluster call) |
| `--gvk` / `-g` | common built-in GVKs (`v1/Pod`, `apps/v1/Deployment`, …) |
| `--resource` / `-r` (`can`, `preflight`) | common plural resources (`pods`, `deployments`, …) |
| `--name` / `-p` / `--pod` | pod names in the `default` namespace |

Known limitation: the pod-name completer completes from the `default` namespace (the
completer cannot read a sibling `-n` flag you already typed), so `-n <ns>` first to
complete pods in another namespace. Completion always degrades gracefully — an
unreachable cluster or a permission denial yields no candidates, never an error or hang.
