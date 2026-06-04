# Building and Installing

## Requirements

- Rust (edition 2024; toolchain pinned in `rust-toolchain.toml`).
- Node.js (for building the KWin script bundle).
- A KDE Plasma 6 / Wayland session to run against.

## Build the daemon and CLI

```sh
cargo build --release
```

This produces `riftd` and `riftctl` under `target/release/`.

## Build the KWin script

The script is TypeScript, bundled to a single JS file:

```sh
cd rift-kwin
npm install
npm run build
```

The build emits `rift-kwin/contents/code/main.js` (the artifact KWin runs).

## Install the KWin script

```sh
scripts/install-kwin.sh
```

This installs the script into the KPackage layout KWin expects. Enable it in
**System Settings → Window Management → KWin Scripts**.

## Run

Start the daemon:

```sh
target/release/riftd
```

With the script enabled and the daemon on the session bus, windows tile. Use
`riftctl status` to confirm the daemon sees them.

## Tests and gates

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check

cd rift-kwin
npm test          # Node handshake harness
npm run typecheck # tsc --noEmit
```
