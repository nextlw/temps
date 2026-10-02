# MinIO server and mc client images

Build context for `ghcr.io/nextlw/minio` and `ghcr.io/nextlw/mc`, published by
`.github/workflows/minio-images.yml`.

## Why these images are ours

The official `quay.io/minio/minio`, `quay.io/minio/mc`, `minio/minio` and
`minio/mc` images can no longer be pulled anonymously: the registries answer
`401 unauthorized`. Temps depends on both images: the S3 managed service,
the S3 backup mirror (`crates/temps-backup/src/engines/s3_mirror.rs`), the
backup integration tests and the `apps/temps-e2e` fixtures. So we compile the
same releases from the public source and publish them ourselves.

| Image | Release | Source commit | Go |
|---|---|---|---|
| `minio` | `RELEASE.2025-10-15T17-29-55Z` | [`9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a`](https://github.com/minio/minio/tree/9e49d5e7a648f00e26f2246f4dc28e6b07f8c84a) | 1.24.13 |
| `mc` | `RELEASE.2025-08-13T08-35-41Z` | [`7394ce0dd2a80935aded936b09fa12cbb3cb8096`](https://github.com/minio/mc/tree/7394ce0dd2a80935aded936b09fa12cbb3cb8096) | 1.23.12 |

The image tag is the release name, the same tags Temps already references.
The build fails if a release tag stops pointing to the pinned commit.

### Release choice

Both projects are archived upstream, so these are their last source
releases. `RELEASE.2025-10-15T17-29-55Z` fixes CVE-2025-62506
([GHSA-jjjj-jwhf-8rgr](https://github.com/minio/minio/security/advisories/GHSA-jjjj-jwhf-8rgr),
high: privilege escalation via session policy bypass in service accounts and
STS). `mc` has no release after `RELEASE.2025-08-13T08-35-41Z`.

Later minio advisories (CVE-2026-33322, CVE-2026-33419, CVE-2026-34204,
CVE-2026-39414, CVE-2026-40344, CVE-2026-41145, CVE-2026-42600) are fixed
only in the proprietary MinIO AIStor; the open-source tree has no patch for
them. They stay open in this image.

The earlier image `RELEASE.2025-09-07T16-13-09Z` (commit
`07c3a429bfed433e49018cb0f78a52145d4bedeb`) stays published; a volume it
wrote is read by the newer server as-is. The S3 managed service rewrites a
persisted reference to that release (official `minio/minio` or our
`ghcr.io/nextlw/minio` tag, with or without its digest) to the current image,
so existing services pick up the CVE-2025-62506 fix on their next start.

## Source code (AGPL-3.0)

MinIO and mc are licensed AGPL-3.0-only by MinIO, Inc. We build them without
modification. The corresponding source for each binary is the commit linked
above, in <https://github.com/minio/minio> and <https://github.com/minio/mc>,
and the build recipe is the `Dockerfile` in this directory. Each image ships
the upstream `LICENSE` and `CREDITS` in `/licenses/`.

## Differences from the official images

- Alpine base (pinned by digest) instead of UBI micro. Both images keep
  `/bin/sh`, because Temps runs `sh -c` in the mc image and a `CMD-SHELL`
  healthcheck (`mc ready local`) in the minio image.
- Both images run as the non-root user `1000:1000`. `/data` belongs to that
  user, so new named and anonymous volumes work as-is. A bind mount or an
  existing volume written by the official image (root-owned) has to be
  `chown`ed to `1000:1000` first, or the container run as root.
- The S3 managed service of Temps (`crates/temps-providers/src/externalsvc/s3.rs`)
  creates its MinIO container with `user: 0:0` whenever the image is this one
  or an official one, the same privilege the official image had, so volumes
  created before this image keep working without touching their data. An
  image the operator picked keeps its own user.
- Known limitation: services created on a remote node (multi-node) go through
  `RemoteServiceCreateParams`, which has no `user` field. There the container
  runs as `1000:1000`, so a remote volume written by the official image fails
  to start until it is `chown`ed. That path belongs to the deprecated `Minio`
  service type.
- `MC_CONFIG_DIR=/tmp/.mc`, since the non-root user has no writable home.
- Unchanged: `ENTRYPOINT`/`CMD` (`docker-entrypoint.sh minio` and `mc`),
  `EXPOSE 9000`, `VOLUME /data`, the `MINIO_*_FILE` defaults, and `mc` bundled
  inside the minio image.

## Local build

```sh
docker build --target minio -t temps-minio:local images/minio
docker build --target mc    -t temps-mc:local    images/minio
```
