FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libssl-dev

WORKDIR app

COPY ./target/release/relayer-rs /usr/local/bin
RUN mkdir -p /app/tests/data

CMD ["relayer-rs"]
