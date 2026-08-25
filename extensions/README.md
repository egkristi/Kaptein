# Extension surface licensing

Everything under `extensions/` is part of Kaptein's **extension surface** and is
licensed **MIT OR Apache-2.0** (not BUSL-1.1), per ADR-0004's licensing split.

This includes:
- Example view definitions (lenses) — `*.yaml` here
- Future example WASM plugins and shell-out integrations

The lens **schema** (the view-definition format) is likewise MIT/Apache-2.0, so a
third-party ecosystem can author lenses and plugins without taking BUSL terms on their
own work. See `docs/adr/0004-extension-model.md` and the LICENSE section of `README.md`.
