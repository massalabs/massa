# Builder Image
FROM rust:1.63.0 AS builder

ARG IMAGE_TAG
ARG GIT_REPOSITORY

# Set Working Directory is /app
WORKDIR /app

# Node project build
RUN git clone --depth 1 --branch $IMAGE_TAG $GIT_REPOSITORY .
RUN apt-get update && apt-get install -y build-essential libssl-dev libclang-dev
RUN rustup toolchain install nightly-2022-11-14 && rustup default nightly-2022-11-14
RUN cargo build --release --bin massa-node --bin massa-client 

# Production Image
FROM debian:bullseye-slim AS runtime

ARG IMAGE_TAG
ARG BUILD_DATE
LABEL image-tag=$IMAGE_TAG
LABEL build-date=$BUILD_DATE

WORKDIR /app

RUN apt-get update && apt-get upgrade -y \
    && apt-get install -y tzdata ca-certificates curl wget jq expect\
    && apt-get -y purge && apt-get -y clean \
    && apt-get -y autoremove && rm -rf /var/lib/apt/lists/*
 
COPY --from=builder /app/massa-client/config /source/massa-client/config
COPY --from=builder /app/massa-client/base_config /source/massa-client/base_config
COPY --from=builder /app/target/release/massa-client /source/massa-client

COPY --from=builder /app/massa-node/config /source/massa-node/config
COPY --from=builder /app/massa-node/base_config /source/massa-node/base_config
COPY --from=builder /app/massa-node/storage /source/massa-node/storage
COPY --from=builder /app/target/release/massa-node /source/massa-node

RUN echo "{\n\t\"version\":\"$IMAGE_TAG\"\n}" > /source/version

COPY run_node.sh /

EXPOSE 31244 31245 33034 33035

ENTRYPOINT [ "/bin/bash", "/run_node.sh" ]