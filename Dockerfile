FROM rust:1.77.2 as builder
WORKDIR /opt/pax/
COPY . ./
RUN cargo build --release
FROM ubuntu:latest as pax
COPY --from=builder /opt/pax/target/release/pax /usr/bin/pax
