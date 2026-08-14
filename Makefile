.PHONY: check test fmt clippy build release audit

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

test:
	cargo test --workspace --all-features --locked

build:
	cargo build --workspace --all-features --locked

release:
	cargo build --workspace --release --all-features --locked

check:
	cargo generate-lockfile
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo test --workspace --all-features --locked
	python3 scripts/static_audit.py

audit:
	cargo deny check
