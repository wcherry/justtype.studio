# Thin wrapper over `cargo xtask`, which is where the logic actually lives.
# Everything here has a shorter cargo alias — see .cargo/config.toml.
SHELL := /bin/bash

.PHONY: help setup backend web ios sim testflight dev test check smoke fmt clean

help:
	@grep -E '^[a-z-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}'

setup: ## Everything a fresh clone needs  (cargo setup)
	cargo xtask setup

dev: ## Backend and web dev server together  (cargo dev)
	cargo xtask dev

backend: ## Run just the API server
	cargo run -p scriptorium-backend

web: ## Production build of the web client  (cargo web)
	cargo xtask build-web

ios: ## Generate and open the Xcode project  (cargo ios)
	cargo xtask ios

sim: ## Build and run the iOS app on a simulator  (cargo sim)
	cargo xtask sim

testflight: ## Build, sign and upload to TestFlight
	cargo xtask testflight

test: ## Rust, Swift and web test suites
	cargo xtask test

check: ## What CI enforces: fmt, clippy, web types  (cargo checks)
	cargo xtask check

smoke: ## End-to-end API walkthrough  (cargo smoke)
	cargo xtask smoke

fmt: ## Format the Rust sources
	cargo xtask fmt

clean:
	cargo clean
	rm -rf web/dist web/node_modules ios/ScriptoriumKit/.build ios/Scriptorium.xcodeproj
