.PHONY: dev run infra-up infra-down check test-integration

# Start dependencies, then run Topcoat with asset rebuilds and browser reloads.
dev: infra-up
	topcoat dev

# Bundle browser assets, then run the application once on the host.
run:
	topcoat asset bundle --bin sanad
	cargo run --bin sanad

# No explicit service names here on purpose: Compose decides which services
# to start from active profiles alone, and it reads COMPOSE_PROFILES from
# .env itself — postgres/qdrant have no profile (always start), app is
# behind the "app" profile (starts only when COMPOSE_PROFILES=app is set).
infra-up:
	docker compose up -d

infra-down:
	docker compose down

check:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test

# Requires `make infra-up` first. Not part of `check` — needs live Postgres
# and Qdrant, so it stays opt-in.
test-integration: infra-up
	cargo test --test retrieval_integration -- --ignored
