FROM rust:1.96-bookworm AS builder

WORKDIR /app
RUN cargo install topcoat-cli --version 0.5.0 --locked
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
COPY migrations ./migrations
# The system prompts are include_str!'d into the binary, so they are a build
# input like the source itself, not runtime data.
COPY prompts ./prompts
COPY build.rs ./
RUN cargo build --locked --release --bin sanad
RUN topcoat asset bundle --release --bin sanad

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/sanad /usr/local/bin/sanad
COPY --from=builder /app/target/assets /usr/local/bin/assets

# Topcoat binds 127.0.0.1 unless told otherwise, which inside a container means
# nothing outside it can reach the app — including the reverse proxy.
ENV HOST=0.0.0.0
ENV PORT=3000

EXPOSE 3000
CMD ["sanad"]
