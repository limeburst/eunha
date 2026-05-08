# ── Stage 1: Build Elk ──────────────────────────────────────────────────────
FROM node:24-alpine AS elk-builder
RUN corepack enable pnpm
WORKDIR /elk
COPY elk/ .
COPY elk-patches/plugins/eunha.client.ts plugins/eunha.client.ts
RUN pnpm install --frozen-lockfile && pnpm generate

# ── Stage 2: Build console ──────────────────────────────────────────────────
FROM node:24-alpine AS console-builder
RUN corepack enable pnpm
WORKDIR /console
COPY console/package.json console/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY console/ .
RUN pnpm build

# ── Stage 3: Build Rust binary ──────────────────────────────────────────────
FROM rust:1-slim-bookworm AS rust-builder
WORKDIR /app
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin eunha

# ── Stage 4: Runtime ────────────────────────────────────────────────────────
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=rust-builder /app/target/release/eunha .
COPY --from=elk-builder /elk/.output/public/ elk/.output/public/
COPY --from=console-builder /console/dist/ console/dist/
COPY migrations/ migrations/
EXPOSE 3000
CMD ["./eunha"]
