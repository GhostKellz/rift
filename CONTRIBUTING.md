# Contributing to Rift

Thanks for your interest in Rift. This document covers how to set up a development
environment, the conventions the project follows, and how to get changes merged.

## Project Layout

Rift is a Cargo workspace plus a KWin script. Most contributions land in one of:

| Area | Path | Language |
|------|------|----------|
| Daemon (layout engine, IPC server) | `crates/riftd/` | Rust |
| CLI client | `crates/riftctl/` | Rust |
| Shared IPC protocol types | `crates/rift-ipc/` | Rust |
| KWin script (event forwarding, geometry apply) | `rift-kwin/` | TypeScript |

Layout logic belongs in the daemon. The KWin script stays thin — it forwards
events and applies geometry, nothing more. Pull requests that move layout
decisions into the script will be asked to relocate them to `riftd`.

## Prerequisites

- KDE Plasma 6 on Wayland
- Rust toolchain, 2024 edition (Rust 1.90 or newer)
- Node.js (to build the KWin script)
- `just` (optional, for the convenience targets)

## Development Setup

```bash
git clone https://github.com/ghostkellz/rift.git
cd rift

# Build the workspace
cargo build

# Build the KWin script
just build-kwin          # or: cd rift-kwin && npm install && npm run build
```

To iterate on the daemon against a live session:

```bash
# Install the KWin script once
just install-kwin

# Run the daemon in the foreground with logging
RUST_LOG=debug cargo run --bin riftd

# Drive it from another terminal
cargo run --bin riftctl -- status
```

## Code Style

- Format Rust with `cargo fmt`; it must produce no diff
- Lint with `cargo clippy --all-targets --all-features`; warnings are treated as failures
- Prefer the workspace's existing error types over ad-hoc `String` errors
- Comments explain *why*, not *what*; do not annotate self-evident code
- Keep the KWin script free of layout logic

## Testing

```bash
# Unit and integration tests across the workspace
cargo test

# Lint and format gate
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Layout engine changes should come with tests against the cell model. Reconcile
behavior in particular must be covered — a change that alters how orphaned cells
are pruned needs a regression test.

## Commit and Pull Request Conventions

- Keep commits focused; one logical change per commit
- Write commit subjects in the imperative mood ("add spiral layout", not "added")
- Reference the issue number where one exists
- Describe the *why* in the body when the change is non-obvious
- Ensure `cargo fmt --check`, `cargo clippy`, and `cargo test` pass before opening a PR

## Reporting Bugs

Open an issue with:

- Plasma and KWin versions (`plasmashell --version`, `kwin_wayland --version`)
- Output of `riftctl status`
- Monitor topology (outputs, resolutions, primary)
- Steps to reproduce, and whether `riftctl reset` recovers the state

## Security

Do not file security issues as public issues. See [SECURITY.md](SECURITY.md) for
the disclosure process.
