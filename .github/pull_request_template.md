## Checklist

- [ ] Logic lives in `kaptein-viewmodel` (semantics), not a frontend (geometry)
- [ ] Frontends only render a render-intent produced by the view-model
- [ ] Contract test added when view-model output changes
- [ ] No polling; informer-based
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` pass
- [ ] `cargo deny check licenses` passes (no new license/banned-crate issues)
- [ ] Core contribution: CLA signed (see `CLA.md`); extension surface: DCO sign-off
- [ ] ADR opened if this shifts a documented decision
- [ ] README/ROADMAP/CONTRIBUTING/SECURITY kept in sync

## Summary

## Security / secret handling

If this touches secrets, audit logging, RBAC preflight, or LLM/MCP: include a
threat-model note.
