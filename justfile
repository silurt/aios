# AIOS — one entry point across Rust, Swift and TypeScript (plan §11.1).
# `just` is the only tool that spans all three; everything else is delegated.

default: check

# Build the aios binary (debug)
build:
    cargo build

# Build the release binary — this is what the Mac app bundles
release:
    cargo build --release
    @echo "→ target/release/aios"

# Everything CI runs, in the order that fails fastest
check: fmt-check clippy test

test:
    cargo test --workspace

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# Install into ~/.local/bin for day-to-day use
install:
    cargo install --path crates/aios-cli --force

# Regenerate the OpenAPI spec. Lands in phase 4.5; the recipe exists now so the
# staleness gate (§15.2) has a stable name to call from the start.
openapi:
    @echo "not yet — phase 4.5"

# Run against a scratch registry instead of ~/.aios
scratch *ARGS:
    AIOS_HOME=$(mktemp -d)/aios cargo run -q -- {{ARGS}}
