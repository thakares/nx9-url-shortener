# ==========================================
# Stage 1: Builder (with optimized caching)
# ==========================================
FROM rust:1.89-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy only Cargo files first (best caching)
COPY Cargo.toml Cargo.lock ./

# Create dummy source for dependency caching
RUN mkdir -p src && \
    echo "fn main() { println!(\"dummy\"); }" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/bzod*

# Copy real source code + assets
COPY src ./src
COPY templates ./templates
COPY www ./www

# Build the real application
RUN cargo build --release

# ==========================================
# Stage 2: Runtime (slim)
# ==========================================
FROM debian:bookworm-slim

WORKDIR /app

# Runtime dependencies
RUN apt-get update && apt-get install -y \
    openssl \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/bzod /usr/local/bin/bzod

# Copy assets
COPY --from=builder /app/templates ./templates
COPY --from=builder /app/www ./www

# Create non-root user
RUN groupadd -g 1000 bzod && \
    useradd -u 1000 -g bzod -m -s /bin/bash bzod

# Create data directory
RUN mkdir -p /app/data && \
    chown -R bzod:bzod /app

USER bzod

ENV DATA_DIR=/app/data \
    PORT=8654 \
    HOST=0.0.0.0 \
    COOKIE_SECURE=true

EXPOSE 8654

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:${PORT}/status || exit 1

ENTRYPOINT ["bzod"]
CMD ["serve"]