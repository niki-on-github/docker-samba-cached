FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev rustup && rustup default stable
WORKDIR /src
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
RUN rustup target add x86_64-unknown-linux-musl && \
    cargo build --release --target x86_64-unknown-linux-musl

FROM ghcr.io/crazy-max/samba:4.21.4
RUN apk add --no-cache \
    vmtouch --repository=http://dl-cdn.alpinelinux.org/alpine/edge/testing \
    inotify-tools
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/cache-manager /usr/local/bin/
COPY rootfs/ /
RUN chmod +x /etc/services.d/cache-manager/run
