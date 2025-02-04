FROM rust:1.82-alpine3.20 as builder
RUN apk update && apk add musl-dev
#RUN useradd -d /craftip -s /bin/bash -u 1001 craftip
RUN addgroup -S craftip && adduser -S craftip -G craftip
WORKDIR /craftip

RUN chown -R craftip:craftip /craftip
USER craftip

COPY Cargo.toml .
COPY Cargo.lock .
COPY crates/ ./crates/
COPY server/ ./server/
COPY client-gui/ ./client-gui/
COPY util ./util

WORKDIR /craftip/server
RUN RUSTFLAGS=-g cargo build --release


FROM alpine:3.20
RUN addgroup -S craftip && adduser -S craftip -G craftip
USER craftip
COPY --from=builder /craftip/target/release/server /usr/local/bin/server
CMD ["server"]
