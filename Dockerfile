# syntax=docker/dockerfile:1

# Cross-compile on the build host with zig as the linker, so multi-arch builds
# never run rustc under QEMU. The image pins a tested Rust/zig/cargo-zigbuild set.
FROM --platform=$BUILDPLATFORM ghcr.io/rust-cross/cargo-zigbuild:0.23.4 AS builder
ARG TARGETARCH
WORKDIR /src
COPY Cargo.toml Cargo.lock askama.toml ./
COPY src ./src
COPY templates ./templates
COPY assets ./assets
# cargo's package-cache lock lives outside the mounted registry, so the
# per-arch builds must take turns on it.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=sublink-target-$TARGETARCH \
    case "$TARGETARCH" in \
        amd64) target=x86_64-unknown-linux-musl ;; \
        arm64) target=aarch64-unknown-linux-musl ;; \
        *) echo "unsupported TARGETARCH: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && cargo zigbuild --release --locked --target "$target" \
    && cp "target/$target/release/sublink-worker" /sublink-worker \
    && mkdir /data

FROM scratch
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /sublink-worker /sublink-worker
COPY --from=builder --chown=65534:65534 /data /data
ENV PORT=38471
# The app writes data/sublink.aof relative to the working directory.
WORKDIR /
VOLUME /data
EXPOSE 38471
USER 65534:65534
ENTRYPOINT ["/sublink-worker"]
