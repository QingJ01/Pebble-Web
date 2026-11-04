# Stage 1: Build React frontend
FROM node:20-alpine AS frontend
WORKDIR /app
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ .
RUN npm run build

# Stage 2: Build Rust backend
FROM rust:1.90-alpine AS backend
RUN apk add --no-cache musl-dev pkgconfig openssl-dev
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ crates/
COPY src/ src/
RUN cargo build --release

# Stage 3: Final minimal image
FROM alpine:3.20
RUN apk add --no-cache ca-certificates
COPY --from=backend /app/target/release/pebble-web /usr/local/bin/
COPY --from=frontend /app/dist /usr/local/share/pebble-web/static
EXPOSE 8080
VOLUME /data
ENV PEBBLE_DATA_DIR=/data
ENV PEBBLE_STATIC_DIR=/usr/local/share/pebble-web/static
CMD ["pebble-web"]
