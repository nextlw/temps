# Docker image build contexts

This directory contains the Dockerfiles and entrypoints for the container
images Temps uses to run managed database services. It is **not** a folder of pictures —
repo screenshots and media live in [`assets/`](../assets/).

| Directory | Image | Purpose |
|---|---|---|
| `postgres-walg/` | `gotempsh/postgres-walg` | PostgreSQL with WAL-G for PITR backups |
| `pgvector-walg/` | `gotempsh/pgvector-walg` | PostgreSQL + pgvector with WAL-G |
| `timescaledb-walg/` | `gotempsh/timescaledb-walg` | TimescaleDB with WAL-G |
| `postgres-ha/` | `gotempsh/postgres-ha` | PostgreSQL high-availability cluster image |
| `redis-walg/` | `gotempsh/redis-walg` | Redis with WAL-G-based backups |
| `mongodb-walg/` | `gotempsh/mongodb-walg` | MongoDB with WAL-G-based backups |
| `mariadb-walg/` | `ghcr.io/gotempsh/mariadb-walg` | MariaDB physical streaming backups through WAL-G |
| `minio/` | `ghcr.io/<owner>/minio`, `ghcr.io/<owner>/mc` | MinIO server and mc client built from source (see [`minio/README.md`](minio/README.md)) |

The default image tags used by the platform are set in
`crates/temps-providers/src/externalsvc/`.

The MariaDB image is published for `linux/amd64` and `linux/arm64` by the
upstream repository (gotempsh/temps). This fork does not ship that workflow, so
installations keep pulling `ghcr.io/gotempsh/mariadb-walg`. The sandbox, daemon
and preview-gateway images, by contrast, are published by this fork's release
workflow under `ghcr.io/<owner>/`.
