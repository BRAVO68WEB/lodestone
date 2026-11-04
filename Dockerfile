# All-in-one Lodestone image: Core on [::1]:16662, dashboard on port 80.

FROM node:18-bookworm AS dashboard
WORKDIR /src/dashboard
COPY dashboard/package.json dashboard/package-lock.json ./
COPY dashboard/patches ./patches
RUN npm ci --legacy-peer-deps
COPY dashboard/ ./
RUN npm run build

FROM rust:1.70.0-bookworm AS core
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev git ca-certificates clang \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
# sqlx checks queries against this file at compile time. *.db is ignored otherwise.
COPY dev.db ./dev.db
COPY core ./core
COPY dashboard/src-tauri ./dashboard/src-tauri
ENV DATABASE_URL=sqlite:///src/dev.db
RUN cargo build --release --features vendored-openssl -p lodestone_core

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends nginx ca-certificates libssl3 gosu wget \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 1000 lodestone \
    && useradd --uid 1000 --gid lodestone --create-home --home-dir /var/lib/lodestone lodestone \
    && mkdir -p /etc/lodestone

COPY --from=core /src/target/release/lodestone_core /usr/local/bin/lodestone_core
COPY --from=dashboard /src/dashboard/out/ /usr/share/nginx/html/
COPY docker/nginx.conf /etc/nginx/nginx.conf
COPY docker/config.js.template /etc/lodestone/config.js
COPY docker/entrypoint.sh /usr/local/bin/lodestone-entrypoint
RUN chmod +x /usr/local/bin/lodestone-entrypoint /usr/local/bin/lodestone_core

ENV LODESTONE_PATH=/var/lib/lodestone
EXPOSE 80
VOLUME ["/var/lib/lodestone"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=40s --retries=3 \
    CMD wget -q -O /dev/null http://127.0.0.1/api/v1/info || exit 1
ENTRYPOINT ["/usr/local/bin/lodestone-entrypoint"]
