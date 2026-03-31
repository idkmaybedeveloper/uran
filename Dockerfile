# BUILDING STAGE
FROM rust:alpine AS builder

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app

# cache
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

COPY . .
RUN cargo build --release

# RUNNING STAGE
FROM alpine:3.20
RUN apk add --no-cache ca-certificates libgcc

COPY --from=builder /app/target/release/uran /usr/local/bin/uran

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/uran"]
