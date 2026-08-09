FROM rust:1.90-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY public ./public
RUN cargo build --release
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*
ENV DATA_PATH=/data
ENV PORT=8080
WORKDIR /app
COPY --from=build /app/target/release/two-seven /app/two-seven
COPY public ./public
ENTRYPOINT ["/app/two-seven"]
