# Rift task runner. Run `just` to list targets.

kwin_dir := "rift-kwin"

default:
    @just --list

# Build all Rust crates.
build:
    cargo build

# Build the release binaries.
build-release:
    cargo build --release

# Build the KWin script bundle (TypeScript -> JS).
build-kwin:
    npm --prefix {{kwin_dir}} install
    npm --prefix {{kwin_dir}} run build

# Install and enable the KWin script for the current user.
install-kwin:
    ./scripts/install-kwin.sh

# Run the full test suite (Rust + KWin harness).
test:
    cargo test
    npm --prefix {{kwin_dir}} test

# Format check (Rust) and TypeScript typecheck.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    npm --prefix {{kwin_dir}} run typecheck

# Apply formatting.
fmt:
    cargo fmt

# Lint with clippy, treating warnings as errors.
clippy:
    cargo clippy --all-targets -- -D warnings
