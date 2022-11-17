FROM rust:1.61.0-slim-buster

WORKDIR /usr/src/relayer-rs
COPY . .

RUN cargo install --path .

CMD ["relayer-rs"]