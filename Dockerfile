# ==========================================
# Stage 1: Builder
# ==========================================
FROM rust:1.89-bookworm AS builder

WORKDIR /app

# Build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Dependency metadata first for Docker layer caching
COPY Cargo.toml Cargo.lock ./

# Dummy build to cache Rust dependencies
RUN mkdir -p src && \
    printf 'fn main() {}\n' > src/main.rs && \
    cargo build --release --locked && \
    rm -rf src

# Actual application source and runtime assets
COPY src ./src
COPY templates ./templates
COPY www ./www

# Reproducible production build
RUN cargo build --release --locked


# ==========================================
# Stage 2: Runtime
# ==========================================
FROM debian:bookworm-slim AS runtime

WORKDIR /app

# Runtime dependencies only
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create unprivileged runtime user
RUN groupadd --gid 1000 bzod && \
    useradd --uid 1000 --gid 1000 \
    --create-home \
    --shell /usr/sbin/nologin \
    bzod

# Application binary
COPY --from=builder /app/target/release/bzod /usr/local/bin/bzod
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

# Application-owned immutable assets
COPY --from=builder /app/templates /app/templates
COPY --from=builder /app/www /app/www

# Persistent runtime directories.
# /app/images is intentionally external/persistent in Compose.
RUN mkdir -p \
        /app/data \
        /app/config \
        /app/images && \
    chown -R bzod:bzod \
        /app/data \
        /app/config \
        /app/images \
        /app/templates \
        /app/www \
        /usr/local/bin/bzod \
        /usr/local/bin/docker-entrypoint.sh

# Runtime configuration
ENV DATA_DIR=/app/data \
    CONFIG_DIR=/app/config \
    IMAGES_DIR=/app/images \
    PORT=8654 \
    HOST=0.0.0.0 \
    COOKIE_SECURE=true

EXPOSE 8654

HEALTHCHECK \
    --interval=30s \
    --timeout=5s \
    --start-period=10s \
    --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${PORT}/status" || exit 1

USER bzod

ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["serve"]
