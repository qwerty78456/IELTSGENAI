# --- BUILD STAGE ---
# Edition 2024 needs Rust >= 1.85; the crate is developed on 1.92.
FROM rust:1.92-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev build-essential ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# The browser bundle is wasm; dx needs the target and wasm-bindgen (dx fetches the latter).
RUN rustup target add wasm32-unknown-unknown
# Same minor as Cargo.toml's dioxus (0.7.x).
RUN cargo install dioxus-cli --version 0.7.9 --locked

WORKDIR /app
COPY . .

# Fullstack release build: server binary + public/ folder under target/dx/<crate>/release/web/
RUN dx build --release

# --- RUNTIME STAGE ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# dx 0.7 layout: target/dx/vmq_mvp/release/web/{server, public/}
COPY --from=builder /app/target/dx/vmq_mvp/release/web/ ./

ENV IP=0.0.0.0
ENV PORT=8080
ENV DATA_DIR=/app/data
RUN mkdir -p /app/data

EXPOSE 8080
CMD ["./server"]
