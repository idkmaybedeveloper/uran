# Copyright 2026 NotVkontakte LLC (aka Lain)
#
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# BUILDING STAGE
FROM rust:trixie AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# cache
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src/main.rs

COPY . .
RUN touch src/main.rs && cargo build --release

# RUNNING STAGE
FROM debian:trixie
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    openssl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/uran /usr/local/bin/uran

EXPOSE 8080

ENTRYPOINT ["/usr/local/bin/uran"]
