# syntax=docker/dockerfile:1.3-labs
FROM jonoh/sccache-rust AS builder

ENV PATH=/usr/local/cargo/bin:$PATH \
    RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    CARGO_TARGET_DIR=/tmp/target \
    CARGO_NET_GIT_FETCH_WITH_CLI=true

ARG RUST_VERSION="nightly"
RUN <<END
    if [ "${RUST_VERSION}" != "stable" ]; then
        rustup toolchain install "${RUST_VERSION}"
        rustup default "${RUST_VERSION}"
    fi
END

RUN <<END
    apt-get update && \
    apt-get install -y clang \
        && rm -rf /var/lib/apt/lists/*
END

ARG BUILD_ENV=dev
ARG RUSTC_WRAPPER
ARG SCCACHE_BUCKET
ARG SCCACHE_S3_KEY_PREFIX
RUN --mount=type=secret,id=git \
    --mount=type=cache,target=/tmp/target \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=bind,target=/src <<END
    set -eu

    cd /src
    case "${BUILD_ENV}" in
      (dev)
        target=/tmp/target/debug
        apps="massa-node"
        ;;
      (release)
        target=/tmp/target/release
        apps="massa-node --release"
        ;;
    esac

    echo "${apps}" | tr ';' '\n' | awk NF | while read bin args; do
      rm -f ${target}/${bin}
      RUST_BACKTRACE=full cargo build --bin ${bin} ${args}
      cp ${target}/${bin} /
    done

    echo ${RUSTC_WRAPPER} | grep -q sccache && sccache --show-stats || true
END

# Build a dev image
FROM debian:bullseye-slim AS dev

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    dnsutils \
    jq \
    libmcrypt4 \
    libssl1.1 \
    netcat \
    net-tools \
    procps \
    telnet \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /massa-node .
COPY /entrypoint.sh .
COPY /massa-node/base_config ./base_config
COPY /massa-node/config ./config
COPY /massa-node/storage ./storage

ENTRYPOINT ["/app/entrypoint.sh"]

# Build a release image
FROM debian:bullseye-slim AS release

RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    dnsutils \
    jq \
    libmcrypt4 \
    libssl1.1 \
    netcat \
    net-tools \
    procps \
    telnet \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /massa-node .
COPY /entrypoint.sh .
COPY /massa-node/base_config ./base_config
COPY /massa-node/config ./config
COPY /massa-node/storage ./storage

ENV APP_BIN=/app/massa-node
ENTRYPOINT ["/app/entrypoint.sh"]
