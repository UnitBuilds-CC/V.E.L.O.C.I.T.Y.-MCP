# Multi-stage build for VELOCITY-MCP
# Floating stable toolchain: Cargo.lock pins crate versions, and a hard rustc
# pin breaks whenever a locked dependency raises its rust-version (1.85 -> 1.88 did).
FROM rust:1-bookworm AS builder

# Set working directory
WORKDIR /build

# Copy all workspace member manifests (required for cargo to resolve the workspace)
COPY Cargo.toml Cargo.lock ./
COPY macros/Cargo.toml macros/
COPY client/Cargo.toml client/
COPY crates/velocity-mcp-core/Cargo.toml crates/velocity-mcp-core/
COPY crates/velocity-mcp-edge/Cargo.toml crates/velocity-mcp-edge/

# Create stub lib.rs files so cargo can resolve dependencies
RUN mkdir -p src macros/src client/src crates/velocity-mcp-core/src crates/velocity-mcp-edge/src && \
    echo "" > src/lib.rs && \
    echo "" > macros/src/lib.rs && \
    echo "" > client/src/lib.rs && \
    echo "" > crates/velocity-mcp-core/src/lib.rs && \
    echo "fn main() {}" > crates/velocity-mcp-edge/src/main.rs

# Build dependencies (cached layer)
RUN cargo build --release --features http,database,oauth2 --lib || true

# Copy source code
COPY . .

# Build the binary
RUN cargo build --release --features http,database,oauth2 --bin velocity_mcp

# Final stage
FROM debian:bookworm-slim

# Install runtime dependencies (including curl for healthcheck)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    python3 \
    nodejs \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -s /bin/bash velocity

# Set working directory
WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/target/release/velocity_mcp /app/velocity_mcp

# Copy default plugins directory
COPY plugins /app/plugins

# Create data directories
RUN mkdir -p /app/data /app/marketplace && \
    chown -R velocity:velocity /app

# Switch to non-root user
USER velocity

# Expose ports
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Set environment variables
ENV RUST_LOG=info
ENV VELOCITY_HTTP_ADDR=0.0.0.0:3000
ENV VELOCITY_PLUGIN_DIR=/app/plugins

# Run the binary
CMD ["/app/velocity_mcp", "--mode", "http"]
