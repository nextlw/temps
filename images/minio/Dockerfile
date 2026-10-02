# SPDX-FileCopyrightText: 2024-2026 Temps Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# The binaries built here are MinIO software, licensed AGPL-3.0-only by
# MinIO, Inc. This Dockerfile only compiles the public source; see README.md.

# MinIO server and mc client compiled from the official public source.
#
# Two targets share this file:
#   docker build --target minio -t temps-minio:local images/minio
#   docker build --target mc    -t temps-mc:local    images/minio
#
# Each source is pinned by release tag AND commit SHA: the build fails if
# the tag ever points somewhere else. Bump both together, resolving the SHA
# with `git ls-remote https://github.com/minio/<repo> 'refs/tags/<tag>^{}'`.
# Base images are pinned by multi-arch index digest; resolve with
#   docker buildx imagetools inspect <image>:<tag> --format '{{json .Manifest}}'
#
# The go.mod of minio RELEASE.2025-09-07T16-13-09Z asks for go1.24.2 and the
# one of mc RELEASE.2025-08-13T08-35-41Z for go1.23.10; each builder uses the
# last patch release of that minor line. GOTOOLCHAIN=local forbids the go
# command from downloading a different toolchain during the build.

ARG MINIO_GO_IMAGE=golang:1.24.13-alpine3.22@sha256:3641e0d9b931dc4f2f185dcd669c4679670e9277c8166a838ddb98a2d4389cb5
ARG MC_GO_IMAGE=golang:1.23.12-alpine3.22@sha256:383395b794dffa5b53012a212365d40c8e37109a626ca30d6151c8348d380b5f
ARG RUNTIME_IMAGE=alpine:3.24.2@sha256:294b683cb724975bec92580e1e685676bd4b50bda910ddb8c51d4cabeaec77e6

ARG MINIO_RELEASE=RELEASE.2025-09-07T16-13-09Z
ARG MINIO_COMMIT=07c3a429bfed433e49018cb0f78a52145d4bedeb
ARG MC_RELEASE=RELEASE.2025-08-13T08-35-41Z
ARG MC_COMMIT=7394ce0dd2a80935aded936b09fa12cbb3cb8096

# ── mc ──────────────────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM ${MC_GO_IMAGE} AS mc-build

ARG MC_RELEASE
ARG MC_COMMIT
ARG TARGETOS
ARG TARGETARCH

RUN apk add --no-cache git

WORKDIR /src
RUN set -eux; \
    git clone --quiet --depth 1 --branch "${MC_RELEASE}" https://github.com/minio/mc.git .; \
    test "$(git rev-parse HEAD)" = "${MC_COMMIT}"

# Same flags as the upstream `make build` of a release: the version is the
# tag timestamp with ':' separators and the release prefix is RELEASE.
RUN set -eux; \
    version="$(echo "${MC_RELEASE#RELEASE.}" | sed -E 's/T([0-9]{2})-([0-9]{2})-([0-9]{2})Z$/T\1:\2:\3Z/')"; \
    ldflags="$(GOTOOLCHAIN=local MC_RELEASE=RELEASE go run buildscripts/gen-ldflags.go "${version}")"; \
    CGO_ENABLED=0 GOTOOLCHAIN=local GOOS="${TARGETOS}" GOARCH="${TARGETARCH}" \
        go build -trimpath -tags kqueue -ldflags "${ldflags}" -o /out/mc .

# ── minio ───────────────────────────────────────────────────────────────────
FROM --platform=$BUILDPLATFORM ${MINIO_GO_IMAGE} AS minio-build

ARG MINIO_RELEASE
ARG MINIO_COMMIT
ARG TARGETOS
ARG TARGETARCH

RUN apk add --no-cache git

WORKDIR /src
RUN set -eux; \
    git clone --quiet --depth 1 --branch "${MINIO_RELEASE}" https://github.com/minio/minio.git .; \
    test "$(git rev-parse HEAD)" = "${MINIO_COMMIT}"

RUN set -eux; \
    version="$(echo "${MINIO_RELEASE#RELEASE.}" | sed -E 's/T([0-9]{2})-([0-9]{2})-([0-9]{2})Z$/T\1:\2:\3Z/')"; \
    ldflags="$(GOTOOLCHAIN=local MINIO_RELEASE=RELEASE go run buildscripts/gen-ldflags.go "${version}")"; \
    CGO_ENABLED=0 GOTOOLCHAIN=local GOOS="${TARGETOS}" GOARCH="${TARGETARCH}" \
        go build -trimpath -tags kqueue -ldflags "${ldflags}" -o /out/minio .

# ── runtime base ────────────────────────────────────────────────────────────
# Alpine, not distroless: Temps runs `sh -c` in the mc image (s3_mirror.rs,
# externalsvc/s3.rs, rustfs.rs) and a CMD-SHELL `mc ready local` healthcheck
# in the minio image, so both need /bin/sh.
FROM ${RUNTIME_IMAGE} AS runtime

RUN set -eux; \
    addgroup -S -g 1000 minio; \
    adduser -S -D -H -u 1000 -G minio -h /tmp minio

# mc keeps its alias config here; the non-root user cannot write to /root.
ENV MC_CONFIG_DIR=/tmp/.mc

# ── final: mc ───────────────────────────────────────────────────────────────
FROM runtime AS mc

ARG MC_RELEASE
ARG MC_COMMIT

LABEL org.opencontainers.image.title="mc" \
      org.opencontainers.image.description="MinIO client (mc) built from the official public source" \
      org.opencontainers.image.source="https://github.com/nextlw/temps" \
      org.opencontainers.image.url="https://github.com/minio/mc" \
      org.opencontainers.image.version="${MC_RELEASE}" \
      org.opencontainers.image.revision="${MC_COMMIT}" \
      org.opencontainers.image.licenses="AGPL-3.0-only"

COPY --from=mc-build /out/mc /usr/bin/mc
COPY --from=mc-build /src/LICENSE /licenses/LICENSE
COPY --from=mc-build /src/CREDITS /licenses/CREDITS

USER 1000:1000
ENTRYPOINT ["mc"]

# ── final: minio ────────────────────────────────────────────────────────────
FROM runtime AS minio

ARG MINIO_RELEASE
ARG MINIO_COMMIT

LABEL org.opencontainers.image.title="minio" \
      org.opencontainers.image.description="MinIO server built from the official public source" \
      org.opencontainers.image.source="https://github.com/nextlw/temps" \
      org.opencontainers.image.url="https://github.com/minio/minio" \
      org.opencontainers.image.version="${MINIO_RELEASE}" \
      org.opencontainers.image.revision="${MINIO_COMMIT}" \
      org.opencontainers.image.licenses="AGPL-3.0-only"

# Same file-based credential defaults as the official image, so existing
# `docker run` invocations keep their behaviour.
ENV MINIO_ACCESS_KEY_FILE=access_key \
    MINIO_SECRET_KEY_FILE=secret_key \
    MINIO_ROOT_USER_FILE=access_key \
    MINIO_ROOT_PASSWORD_FILE=secret_key \
    MINIO_KMS_SECRET_KEY_FILE=kms_master_key \
    MINIO_CONFIG_ENV_FILE=config.env

COPY --from=minio-build /out/minio /usr/bin/minio
COPY --from=mc-build /out/mc /usr/bin/mc
COPY --from=minio-build /src/dockerscripts/docker-entrypoint.sh /usr/bin/docker-entrypoint.sh
COPY --from=minio-build /src/LICENSE /licenses/LICENSE
COPY --from=minio-build /src/CREDITS /licenses/CREDITS

RUN set -eux; \
    chmod 0755 /usr/bin/docker-entrypoint.sh; \
    mkdir -p /data; \
    chown 1000:1000 /data

USER 1000:1000
EXPOSE 9000
VOLUME ["/data"]
ENTRYPOINT ["/usr/bin/docker-entrypoint.sh"]
CMD ["minio"]
