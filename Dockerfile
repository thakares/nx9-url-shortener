# ==========================================
# Stage 1: Build
# ==========================================
# FROM rust:1.82-slim-bookworm AS builder
FROM rust:1.89-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy configuration files
COPY Cargo.toml ./

# Pre-build dependencies to cache them
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy source and templates
COPY src ./src
COPY templates ./templates

# Trigger rebuilding with actual source
RUN touch src/main.rs
RUN cargo build --release

# ==========================================
# Stage 2: Runner
# ==========================================
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    openssl \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/bzod /usr/local/bin/bzod

# Create non-root user and data directory
RUN groupadd -g 10001 bzod && \
    useradd -u 10001 -g bzod -m -s /bin/bash bzod

RUN mkdir -p /app/data && chown -R bzod:bzod /app/data

USER bzod

ENV DATA_DIR=/app/data
ENV PORT=8654
ENV HOST=0.0.0.0
ENV COOKIE_SECURE=true

EXPOSE 8654

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD curl -f http://localhost:$${PORT:-8654}/status || exit 1

ENTRYPOINT ["bzod"]
CMD ["serve"]
