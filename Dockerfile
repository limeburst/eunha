# ── Stage 1: Build Elk ──────────────────────────────────────────────────────
FROM node:24-alpine AS elk-builder
RUN corepack enable pnpm
WORKDIR /elk
COPY elk/ .
COPY elk-patches/plugins/eunha.client.ts app/plugins/eunha.client.ts
RUN sed -i 's/params\.server as string || useRuntimeConfig()\.public\.defaultServer/(params.server as string) || (typeof window !== "undefined" \&\& (window as any).__eunha_instance) || useRuntimeConfig().public.defaultServer/' app/plugins/0.setup-users.ts
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
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
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
