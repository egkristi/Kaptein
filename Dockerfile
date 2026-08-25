# Kaptein container image — the airgap-friendly, static single-binary distribution
# (Distribution & release sync, ROADMAP.md).
#
# Built from the signed release binary (not from source), so the image and the
# tarball on GitHub Releases are the same artifact. No shell, no package manager,
# no runtime dependencies: a distroless-style static image.
#
#   docker build \
#     --build-arg KAPTEIN_VERSION=v0.27.0 \
#     -t kaptein:latest .
#
# The build downloads the release tarball and verifies it against the release's
# SHA256SUMS before extracting, so a tampered download fails the build.

ARG KAPTEIN_VERSION=v0.27.0

# ---- download + verify stage ----
FROM alpine:3.20 AS fetch
ARG KAPTEIN_VERSION
RUN apk add --no-cache curl tar

# Detect the architecture and map it to the release triple.
RUN set -eu; \
    case "$(uname -m)" in \
      x86_64)  ARCH=x86_64  ;; \
      aarch64) ARCH=aarch64 ;; \
      *) echo "unsupported arch"; exit 1 ;; \
    esac; \
    echo "${ARCH}-unknown-linux-gnu" > /tmp/target

ARG TARGETPLATFORM
RUN set -euo pipefail; \
    TARGET="$(cat /tmp/target)"; \
    ARCHIVE="kaptein-${TARGET}.tar.gz"; \
    BASE="https://github.com/egkristi/Kaptein/releases/download/${KAPTEIN_VERSION}"; \
    curl -fsSL "${BASE}/${ARCHIVE}" -o "/tmp/${ARCHIVE}"; \
    curl -fsSL "${BASE}/SHA256SUMS" -o /tmp/SHA256SUMS; \
    cd /tmp; \
    grep "  ${ARCHIVE}$" SHA256SUMS | sha256sum -c -; \
    tar -xzf "${ARCHIVE}" kaptein

# ---- runtime stage ----
FROM scratch
ARG KAPTEIN_VERSION
LABEL org.opencontainers.image.title="kaptein" \
      org.opencontainers.image.description="Kubernetes workbench CLI" \
      org.opencontainers.image.source="https://github.com/egkristi/Kaptein" \
      org.opencontainers.image.version="${KAPTEIN_VERSION}"
COPY --from=fetch /tmp/kaptein /usr/local/bin/kaptein
ENTRYPOINT ["/usr/local/bin/kaptein"]
