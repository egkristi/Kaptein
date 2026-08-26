# kaptein-core

Kaptein's Kubernetes data plane: the `kube-rs` client, watchers/reflectors, CRD
discovery, stores, diagnostics, RBAC preflight, redaction, and the informer lifecycle
manager (ADR-0006).

This crate must **not** depend on `kaptein-viewmodel` or any frontend — layer
dependencies are strictly one-directional.

See the [Kaptein repository](https://github.com/egkristi/Kaptein) for the full
architecture, roadmap, and license terms (BUSL-1.1, converting to MIT on the Change
Date).
