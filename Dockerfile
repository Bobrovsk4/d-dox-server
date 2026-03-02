FROM rust:slim-trixie as builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY migration ./migration
RUN mkdir -p src/bin && touch src/bin/main.rs src/bin/tool.rs
RUN cargo fetch

COPY . .
RUN cargo build --release --bin server-cli

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y ca-certificates openssl curl minio-client && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/server-cli /usr/local/bin/server

COPY config/ config/

EXPOSE 3000
CMD ["server", "start"]

