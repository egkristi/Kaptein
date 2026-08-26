# kaptein-integration

The native integration layer that binds `kaptein-core` to frontends. It is the
redaction-aware error boundary: it maps raw core errors to a user-facing form and
implements the informer-backed `DataPlane` (`LivePlane`) the TUI consumes.

See the [Kaptein repository](https://github.com/egkristi/Kaptein) for the full
architecture, roadmap, and license terms (BUSL-1.1, converting to MIT on the Change
Date).
