# Stage 1: Build React frontend
FROM node:20-bookworm-slim AS frontend
WORKDIR /app
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:1.91-bookworm AS backend
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY src/ src/
RUN cargo build --release

# Stage 3: Final minimal image
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=backend /app/target/release/pebble-web /usr/local/bin/
COPY --from=frontend /app/dist /usr/local/share/pebble-web/static
EXPOSE 8080
VOLUME /data
ENV PEBBLE_DATA_DIR=/data
ENV PEBBLE_STATIC_DIR=/usr/local/share/pebble-web/static
CMD ["pebble-web"]
