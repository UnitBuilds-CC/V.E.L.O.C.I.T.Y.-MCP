# Multi-stage build for VELOCITY-MCP
FROM rust:1.75-bookworm as builder

# Set working directory
WORKDIR /build

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY macros/Cargo.toml macros/

# Build dependencies (cached layer)
RUN cargo build --release --features http,database,oauth2 --lib

# Copy source code
COPY . .

# Build the binary
RUN cargo build --release --features http,database,oauth2 --bin velocity_mcp

# Final stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
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
