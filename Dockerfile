FROM rust:1.90.0-bookworm AS chef
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends g++ make \
    && cargo install cargo-chef \
    && rm -rf /var/lib/apt/lists/*


FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src/main.rs src/
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin logs


FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
WORKDIR /app
COPY --from=builder /app/target/release/logs /usr/local/bin
ENTRYPOINT ["/usr/local/bin/logs"]
