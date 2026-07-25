FROM rust:1.96-bookworm AS builder

WORKDIR /app
RUN cargo install topcoat-cli --version 0.4.0 --locked
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
COPY migrations ./migrations
COPY build.rs ./
RUN cargo build --locked --release --bin hadith-assistant
RUN topcoat asset bundle --release --bin hadith-assistant

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/hadith-assistant /usr/local/bin/hadith-assistant
COPY --from=builder /app/target/assets /usr/local/bin/assets

EXPOSE 3000
CMD ["hadith-assistant"]
