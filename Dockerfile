# syntax=docker/dockerfile:1

FROM rust:1-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release --bin vcp-media-manager

FROM debian:bookworm-slim AS runtime

# 改用阿里云镜像源加速 APT 下载
RUN sed -i 's@//deb.debian.org/@//mirrors.aliyun.com/@g' /etc/apt/sources.list.d/debian.sources \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        wget \
    && rm -rf /var/lib/apt/lists/*

RUN useradd -r -u 10001 -g nogroup vcp

WORKDIR /app

COPY --from=builder /app/target/release/vcp-media-manager /usr/local/bin/vcp-media-manager

RUN mkdir -p /data \
    && chown -R vcp:nogroup /data

USER vcp

# Backend listen port
EXPOSE 8090

# Override via environment variables
ENV PORT=8090
ENV DEVICES_FILE=/data/devices.json
ENV MEDIA_SERVER_URL=http://media-server:8081
ENV MEDIA_PUBLIC_HOST=media-server

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget -qO- http://127.0.0.1:8090/api/stats >/dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/vcp-media-manager"]
