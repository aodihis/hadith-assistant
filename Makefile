.PHONY: dev run infra-up infra-down check

# Start dependencies, then run Topcoat with asset rebuilds and browser reloads.
dev: infra-up
	topcoat dev

# Bundle browser assets, then run the application once on the host.
run:
	topcoat asset bundle --bin hadith-assistant
	cargo run --bin hadith-assistant

infra-up:
	docker compose up -d postgres qdrant

infra-down:
	docker compose down

check:
	cargo fmt --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test
