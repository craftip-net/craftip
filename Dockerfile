FROM rust:1-slim-trixie AS chef
# We only pay the installation cost once,
# it will be cached from the second build onwards
RUN groupadd -r craftip && useradd -r -g craftip craftip
RUN mkdir -p /app && chown craftip:craftip /app
USER craftip
WORKDIR /app
RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --bin server --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release --bin server

# We do not need the Rust toolchain to run the binary!
FROM debian:trixie-slim AS runtime
#RUN addgroup -S craftip && adduser -S craftip -G craftip
RUN groupadd -r craftip && useradd -r -g craftip craftip
USER craftip
COPY --from=builder /app/target/release/server /usr/local/bin/server
CMD ["server"]
