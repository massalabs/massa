# Builder Image
FROM rust:1.63.0 AS builder

ARG GIT_COMMIT
ARG BUILD_DATE 

WORKDIR /app

RUN git clone --depth 1 --branch $GIT_COMMIT https://github.com/massalabs/massa.git .
RUN apt-get update && apt-get install -y build-essential libclang-dev
RUN rustup toolchain install nightly && rustup default nightly    
RUN cargo build --release --bin massa-node --bin massa-client 

# Production Image
FROM debian:bullseye-slim AS runtime

ARG BUILD_DATE
ARG GIT_COMMIT
LABEL build-date=$BUILD_DATE
LABEL git-revision=$GIT_COMMIT

WORKDIR /app

COPY --from=builder /app/target/release/massa-client ./massa-client/
COPY --from=builder /app/target/release/massa-node ./massa-node/
COPY ./run_node.sh .

RUN apt-get update && apt-get install -y tzdata ca-certificates jq curl wget expect\
    && apt-get -y purge && apt-get -y clean \
    && apt-get -y autoremove && rm -rf /var/lib/apt/lists/*
    
EXPOSE 31244 31245

ENTRYPOINT [ "/bin/bash", "-c", "./run_node.sh" ]