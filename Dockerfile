# syntax=docker/dockerfile:1

# ---- Build stage 1: Static musl binary for Rust daemon ----
FROM rust:1-alpine AS rust-build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && cp target/release/hi5bot /hi5bot

# ---- Build stage 2: Next.js Web Dashboard static assets ----
FROM node:20-alpine AS web-build
WORKDIR /app
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web ./
RUN npm run build

# ---- Runtime stage: minimal, non-root, with CA roots + tzdata ----
FROM alpine:3
RUN apk add --no-cache ca-certificates tzdata \
 && addgroup -S app && adduser -S app -G app \
 && mkdir -p /app/data && chown -R app:app /app
WORKDIR /app
COPY --from=rust-build /hi5bot /app/hi5bot
COPY --from=web-build /app/.next/standalone /app/web-standalone
COPY --from=web-build /app/.next/static /app/web-standalone/.next/static

# Run as non-root; the data dir is the single mounted volume.
USER app
ENV HI5BOT_DATA_DIR=/app/data \
    TZ=America/Toronto \
    RUST_LOG=info

# Long-running daemon; it self-schedules the 15:30 America/Toronto wake.
ENTRYPOINT ["/app/hi5bot"]
