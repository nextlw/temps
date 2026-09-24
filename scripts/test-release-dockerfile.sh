#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2024-2026 Temps Contributors
# SPDX-License-Identifier: MIT OR Apache-2.0

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
dockerfile="$repo_root/Dockerfile.release"

# Every assertion below carries the reason it exists. A bare `grep -Fq` under
# `set -e` fails with an empty log: whoever edited the Dockerfile learns only
# that something here said no, and has to reverse-engineer which line and why.
# The contract is worth stating out loud precisely because the failure it
# guards against -- a runtime the release binary cannot start in -- was itself
# invisible until an operator hit it.
fail() {
    printf 'Dockerfile.release: %s\n' "$1" >&2
    exit 1
}

require_exact_line() {
    grep -Fqx "$1" "$dockerfile" || fail "$2"
}

require_fragment() {
    grep -Fq "$1" "$dockerfile" || fail "$2"
}

require_exact_line \
    'FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251' \
    'the runtime base must stay pinned to this exact digest. Release artifacts are glibc binaries; a floating tag lets the runtime move under them without anything failing until an operator runs the image'

require_fragment \
    'RUN chmod +x /usr/local/bin/temps && /usr/local/bin/temps --version' \
    'the image build must execute the binary it just copied in. This is the only step that proves the artifact can start in this runtime, and dropping it moves that discovery to whoever pulls the image'

require_fragment \
    'COPY crates/temps-cli/GeoLite2-City.mmdb /usr/share/temps/GeoLite2-City.mmdb' \
    'the city database must be copied outside /app, where a persistent data volume cannot mount over it'

require_fragment \
    'ln -s /usr/share/temps/GeoLite2-City.mmdb /app/GeoLite2-City.mmdb' \
    'the city database must still be reachable at its /app path, or the proxy looks for a file that is present in the image but not where it reads it'

require_fragment \
    'useradd --uid 1000 --gid temps --create-home' \
    'the service user needs a home directory it owns; without --create-home anything the process writes to $HOME fails as a non-root user'

if grep -Eq 'FROM alpine|\bapk (add|update|upgrade)' "$dockerfile"; then
    fail 'this must not use a musl/Alpine runtime for glibc release artifacts. That combination builds and publishes cleanly and fails at `docker run`'
fi

echo 'Dockerfile.release libc and runtime contract passed'
