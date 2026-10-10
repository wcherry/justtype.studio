# =============================================================================
# Just Type Studio — the whole service in one image: the API, and the web app
# it serves beside it. Built the same way as neutrino's.
# =============================================================================

# ── Web Build Stage ───────────────────────────────────────────────────────────
FROM node:23-alpine AS web-builder

WORKDIR /app/web

# The version CI uses, so the lockfile is read the same way in both.
RUN corepack enable && corepack prepare pnpm@9 --activate

# Manifests first, so a source change does not reinstall every package.
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY web/ ./

# Stamp the image's identity into web/version.txt, which the Vite build reads and
# inlines into the bundle (vite.config.ts → src/lib/version.ts) for the About
# dialog. Generated, never committed: outside an image build there is no version
# to report, and a stale file would report the wrong one. Both args are optional
# — an unstamped build reports no version rather than failing.
ARG APP_VERSION=""
ARG BUILD_COMMIT=""
RUN node scripts/write-version.mjs

RUN pnpm build

# ── Rust Build Stage ──────────────────────────────────────────────────────────
# Pinned to bookworm so the binary's glibc matches the runtime image below.
FROM rust:1.95-bookworm AS rust-builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./

# Every workspace member needs a manifest and a stub entry point here, even the
# one this image never builds: cargo reads the whole workspace before it will
# fetch or build any part of it. Building the stubs compiles every dependency
# into a layer that only a Cargo.lock change invalidates.
COPY backend/Cargo.toml backend/build.rs backend/
COPY xtask/Cargo.toml xtask/
RUN mkdir -p backend/src xtask/src backend/migrations \
    && echo "fn main(){}" > backend/src/main.rs \
    && echo "fn main(){}" > xtask/src/main.rs
RUN cargo fetch
RUN cargo build --release -p justtype-backend
RUN rm -rf backend/src backend/migrations

COPY backend/src backend/src
COPY backend/migrations backend/migrations
COPY backend/assets backend/assets
COPY backend/diesel.toml backend/terms.md backend/

# What /api/health and /api/status report. Optional: an unstamped build
# reports the crate's own version.
ARG APP_VERSION=""
ENV JUSTTYPE_VERSION=${APP_VERSION}
RUN touch backend/src/main.rs && cargo build --release -p justtype-backend

# ── Runtime Stage ─────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN useradd -m appuser

WORKDIR /app

RUN mkdir -p /usr/local/data/works \
    && chown -R appuser:appuser /usr/local/data

# No OpenSSL: TLS (SMTP, the summaries API) is rustls, and SQLite is bundled.
# CA certificates for those outbound calls; curl for the health check; gosu to
# drop root after fixing the volume's ownership.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gosu \
    && rm -rf /var/lib/apt/lists/*

COPY --from=rust-builder /app/target/release/justtype-backend /usr/local/bin/service
COPY --from=web-builder /app/web/dist /app/web
COPY deploy/entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ARG APP_VERSION=""
ARG BUILD_COMMIT=""
LABEL org.opencontainers.image.title="Just Type Studio" \
      org.opencontainers.image.url="https://www.justtype.studio" \
      org.opencontainers.image.version="${APP_VERSION}" \
      org.opencontainers.image.revision="${BUILD_COMMIT}"

# Everything has a working default but JUSTTYPE_JWT_SECRET, which the
# server refuses to start without. See README, "Configuration".
ENV JUSTTYPE_BIND_ADDR=0.0.0.0:8080 \
    JUSTTYPE_DATABASE_URL=/usr/local/data/justtype.db \
    JUSTTYPE_DATA_DIR=/usr/local/data/works \
    JUSTTYPE_WEB_DIR=/app/web \
    JUSTTYPE_PUBLIC_URL=https://www.justtype.studio \
    JUSTTYPE_CORS_ORIGINS=https://www.justtype.studio \
    JUSTTYPE_LOG_FORMAT=json

EXPOSE 8080

# The database, every scene's files and the mail outbox. Logs go to stderr.
VOLUME ["/usr/local/data"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1

ENTRYPOINT ["/entrypoint.sh"]
CMD ["/usr/local/bin/service"]
