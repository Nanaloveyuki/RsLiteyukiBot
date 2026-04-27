FROM node:22-bookworm-slim AS frontend-builder

WORKDIR /workspace

COPY package.json pnpm-lock.yaml pnpm-workspace.yaml .npmrc ./
COPY frontend ./frontend

RUN corepack enable && pnpm install --frozen-lockfile && pnpm build

FROM rust:1-bookworm AS rust-builder

WORKDIR /workspace

ENV CARGO_BUILD_JOBS=1
ENV CARGO_INCREMENTAL=0
ENV RUSTFLAGS=-Cdebuginfo=0

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 python3-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY assets ./assets
COPY i18n ./i18n
COPY src ./src

RUN cargo build --release --locked --bin web -j 1

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates python3 libpython3.11 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /data --shell /usr/sbin/nologin liteyuki \
    && mkdir -p /app/frontend /app/i18n /data/.liteyuki/configs /data/.liteyuki/plugins

COPY --from=rust-builder /workspace/target/release/web /app/web
COPY --from=frontend-builder /workspace/frontend/dist /app/frontend/dist
COPY i18n/core /app/i18n/core

RUN chown -R liteyuki:liteyuki /app /data

ENV HOME=/data
ENV USERPROFILE=/data
ENV LY_RUNTIME_TARGET=docker-web

VOLUME ["/data"]
EXPOSE 14500
STOPSIGNAL SIGINT

USER liteyuki

CMD ["/app/web"]
