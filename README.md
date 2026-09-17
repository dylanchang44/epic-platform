# EPIC Platform

A local-first personal investment platform, built gradually in Rust on Linux.
Stage 1 provides Axum, Leptos server-side rendering + WebAssembly hydration,
three placeholder pages, and `GET /health`. All displayed data is fictional.

## Run (Linux / fish)

Prerequisites: Rust 1.94 or newer via rustup, a C linker/build toolchain, and fish.
For example, on Arch/CachyOS install build tools with `sudo pacman -S --needed base-devel`;
on Debian/Ubuntu use `sudo apt install build-essential pkg-config libssl-dev`.
The first build needs internet access to download dependencies and build tools.
The built application uses only local assets and needs no network data source.

Run these commands in fish. The current checkout directory is `epic-platform`;
the Cargo project and executable are named `epic-platform`.

```fish
cd /home/dylan/git_repo/epic-platform
rustup target add wasm32-unknown-unknown
cargo install cargo-leptos --version 0.3.5 --locked --root ./target/tools
cargo install wasm-bindgen-cli --version 0.2.128 --locked --root ./target/tools
fish_add_path --path "$PWD/target/tools/bin"
cargo leptos watch
```

Open <http://127.0.0.1:3000>. `/` redirects to `/portfolio`; the other pages are
`/research` and `/review`. On Review, click **Show sample prompt** to exercise
hydration. Navigation supports direct URLs, reload, and browser back/forward.
Stop with **Ctrl+C**. If opening a new fish session, repeat `fish_add_path` above.
The pinned wasm-bindgen CLI must match the crate version in `Cargo.toml`.

For a build and run without the watcher, from the same directory:

```fish
cargo leptos build
./target/debug/epic-platform
```

In another terminal:

```fish
curl --fail http://127.0.0.1:3000/health
# Expected: ok
```

Port 3000 is the loopback-only application listener; 3001 is the development
reload port. To use different ports in fish:

```fish
set -gx LEPTOS_SITE_ADDR 127.0.0.1:3100
set -gx LEPTOS_RELOAD_PORT 3101
cargo leptos watch
```

## Checks

```fish
cargo fmt --all -- --check
cargo clippy --locked --features ssr --all-targets -- -D warnings
cargo clippy --locked --lib --target wasm32-unknown-unknown --features hydrate -- -D warnings
cargo leptos build
```

Build the `ssr` and `hydrate` features separately: they represent different
execution environments. Plain `cargo run` does not build the browser assets;
use cargo-leptos and run its built executable. The manifest passes `--locked`
to both Cargo builds. Node, npm, Trunk, and handwritten JavaScript are not needed.

Read [Stage 1 architecture](docs/architecture-stage-01.md) for the repository
inspection, request flow, future module boundaries, and a short learning exercise.
No Schwab files, external retrieval, authentication, database, scheduling, AI,
or background jobs are implemented in this stage.
