SHELL := /bin/bash
SCRATCH := $(CURDIR)/.tmp

.PHONY: help backend web ios test test-rust test-swift test-web fmt lint smoke clean

help:
	@grep -E '^[a-z-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-12s\033[0m %s\n", $$1, $$2}'

backend: ## Run the API server
	cargo run -p scriptorium-backend

web: ## Run the web client (proxies /api to the backend)
	cd web && pnpm install --prefer-offline && pnpm dev

ios: ## Generate and open the Xcode project (needs xcodegen)
	cd ios && xcodegen generate && open Scriptorium.xcodeproj

test: test-rust test-swift test-web ## Run every test suite

test-rust: ## Backend unit tests
	cargo test -p scriptorium-backend

test-swift: ## Shared iOS package tests
	cd ios/ScriptoriumKit && swift test

test-web: ## Web type-check
	cd web && pnpm install --prefer-offline && pnpm typecheck

fmt: ## Format Rust
	cargo fmt --all

lint: ## Clippy, denying warnings
	cargo clippy --all-targets -- -D warnings

smoke: ## End-to-end API walkthrough against a throwaway database
	@./scripts/smoke.sh

clean:
	cargo clean
	rm -rf web/dist web/node_modules ios/ScriptoriumKit/.build $(SCRATCH)
