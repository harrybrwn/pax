ARG RUST_VERSION=1.84.0
ARG DEBIAN_VERSION=bookworm

FROM rust:${RUST_VERSION} AS builder
ARG RUST_VERSION
RUN \
    --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-${RUST_VERSION}-registry \
    cargo install sccache

ENV SCCACHE_DIR=/opt/${RUST_VERSION}/sccache
ENV SCCACHE_CACHE_SIZE="2G"
ENV RUSTC_WRAPPER="/usr/local/cargo/bin/sccache"
WORKDIR /opt/pax/
COPY Cargo.lock Cargo.toml ./
COPY pax pax
COPY pax-derive pax-derive
RUN --mount=type=cache,target=/opt/${RUST_VERSION}/sccache \
    --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-${RUST_VERSION}-registry \
    --mount=type=cache,target=/opt/pax/target \
    cargo fetch
COPY . ./
RUN --mount=type=cache,target=/opt/${RUST_VERSION}/sccache \
    --mount=type=cache,target=/usr/local/cargo/registry,id=cargo-${RUST_VERSION}-registry \
    --mount=type=cache,target=/opt/pax/target \
    cargo build --release && \
    cp target/release/pax /usr/local/bin/pax

FROM debian:${DEBIAN_VERSION} AS pax
COPY --from=builder /usr/local/bin/pax /usr/bin/pax

FROM scratch AS pax-dist
COPY --from=builder /usr/local/bin/pax .
