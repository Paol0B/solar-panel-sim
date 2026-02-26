# Build stage
FROM rust:1.91 AS builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml ./

# Copy source code
COPY src ./src

# Build for release
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy the binary from builder
COPY --from=builder /app/target/release/solar-panel-sim /app/solar-panel-sim

# Copy static files and configuration
COPY static ./static
COPY config.json ./config.json

# Expose HTTP API port (REST + Web UI)
EXPOSE 3000

# Expose Modbus TCP port
EXPOSE 5020

# Run the application
CMD ["/app/solar-panel-sim"]
