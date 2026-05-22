# --- BUILD STAGE ---
FROM rust:1.80-slim-bookworm AS builder

# Install required dependencies for building (like pkg-config, libssl-dev for sqlx/reqwest)
RUN apt-get update && apt-get install -y pkg-config libssl-dev build-essential

# We need dioxus-cli to build the fullstack app
# We can download a pre-built binary to save time, or install via cargo
RUN cargo install dioxus-cli --version 0.6.1 || cargo install dioxus-cli

WORKDIR /app

# Copy the source code
COPY . .

# Build the Dioxus fullstack app for release
# This will output to target/dx/vmq_mvp/release/web (client) and target/dx/vmq_mvp/release/server (server binary)
# Note: Since the app uses Dioxus 0.7.2, we should just use standard dx build
RUN dx build --release

# --- RUNTIME STAGE ---
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies (OpenSSL and CA certificates for HTTPS requests to Gemini)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Copy the built server binary and web assets from the builder stage
# Dioxus CLI typically places the release artifacts in target/dx/vmq_mvp/release/
COPY --from=builder /app/target/dx/vmq_mvp/release/server ./server
COPY --from=builder /app/target/dx/vmq_mvp/release/web ./public

# Provide sensible defaults
ENV PORT=8080
ENV IP=0.0.0.0
ENV DATA_DIR=/app/data

# Expose port
EXPOSE 8080

# Make sure the data directory exists
RUN mkdir -p /app/data

# Run the server binary
CMD ["./server"]
