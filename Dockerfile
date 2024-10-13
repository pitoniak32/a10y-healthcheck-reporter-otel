FROM rust:1.80 as builder

WORKDIR /usr/src/a10y-healthcheck-reporter-otel

COPY . .
RUN cargo install --path .

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl-dev && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/cargo/bin/a10y-reporter /usr/local/bin/a10y-reporter

CMD ["a10y-reporter"]
