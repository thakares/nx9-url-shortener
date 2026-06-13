# ==========================================
# Stage 1: Build
# ==========================================
FROM rust:1.89-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo metadata
COPY Cargo.toml Cargo.lock ./

# Pre-build dependencies for layer caching
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Copy application source
COPY src ./src
COPY templates ./templates
COPY www ./www

# Build application
RUN touch src/main.rs
RUN cargo build --release

# ==========================================
# Stage 2: Runtime
# ==========================================
FROM debian:bookworm-slim

WORKDIR /app

# Runtime dependencies
RUN apt-get update && apt-get install -y \
    openssl \
    ca-certificates \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Application binary
COPY --from=builder /app/target/release/bzod /usr/local/bin/bzod

# Runtime assets
COPY templates ./templates
COPY www ./www

# Create non-root user
RUN groupadd -g 1000 bzod && \
    useradd -u 1000 -g bzod -m -s /bin/bash bzod

# Create writable data directory
RUN mkdir -p /app/data && \
    chown -R bzod:bzod /app

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