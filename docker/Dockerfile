FROM lukemathwalker/cargo-chef:latest-rust-1.60.0 AS chef
WORKDIR app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
RUN apt-get update && apt-get install -y libssl-dev pkg-config clang
RUN cargo install bunyan

COPY --from=planner /app/recipe.json recipe.json

# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --bin relayer-rs

FROM debian:bullseye-slim as runtime

RUN apt-get update && apt-get install -y libssl-dev

WORKDIR app

COPY --from=builder /app/configuration /app/configuration
COPY --from=builder /app/docker/run.sh /app/run.sh
COPY --from=builder /app/target/release/relayer-rs /usr/local/bin
COPY --from=builder /usr/local/cargo/bin/bunyan /usr/local/bin

RUN apt-get update && apt-get install -y ca-certificates && apt-get clean
RUN mkdir -p /app/tests/data

CMD ["./run.sh"]
