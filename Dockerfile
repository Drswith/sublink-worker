# syntax=docker/dockerfile:1

# rust:alpine targets musl, so the binary is fully static and needs no libc
# in the runtime image.
FROM rust:1-alpine AS builder
WORKDIR /src
COPY Cargo.toml Cargo.lock askama.toml ./
COPY src ./src
COPY templates ./templates
COPY assets ./assets
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && cp target/release/sublink-worker /sublink-worker \
    && mkdir /data

FROM scratch
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /sublink-worker /sublink-worker
COPY --from=builder --chown=65534:65534 /data /data
ENV PORT=38471 \
    DB_PATH=/data/sublink.aof
VOLUME /data
EXPOSE 38471
USER 65534:65534
ENTRYPOINT ["/sublink-worker"]
