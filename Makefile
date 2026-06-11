.PHONY: build release run dev test fmt lint clean install web-deps web-build

BIN := agent-ops

build:
	cargo build

release:
	cargo build --release

run:
	cargo run

dev:
	cargo run

test:
	cargo test

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets -- -D warnings

web-deps:
	cd web && npm install

web-build:
	cd web && npm run build

install: release
	cargo install --path .

clean:
	cargo clean
	rm -rf web/dist web/node_modules
