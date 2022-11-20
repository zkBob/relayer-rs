FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libssl-dev

WORKDIR app

COPY ./relayer-rs /usr/local/bin
COPY ./bunyan /usr/local/bin
COPY ./run.sh /app/run.sh
RUN mkdir -p /app/tests/data

CMD ["./run.sh"]
