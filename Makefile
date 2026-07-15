.PHONY: dev run infra-up infra-down check

# Start Dockerized dependencies, then run the Rust API on the host with reloads.
dev: infra-up
	cargo watch -x "run --bin hadith-assistant"

# Run the Rust API once on the host. Pending migrations run during startup.
run:
	cargo run --bin hadith-assistant

infra-up:
	docker compose up -d postgres qdrant

infra-down:
	docker compose down

check:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test
