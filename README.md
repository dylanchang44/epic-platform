# EPIC Platform

A local-first personal investment platform, built gradually in Rust on Linux.
Stage 2 adds local Schwab positions loading, an in-memory portfolio and a working
Portfolio page to the Axum + Leptos application. Research and Review remain
fictional placeholders. CSV files stay on the Linux machine; the browser sends
reload requests and receives parsed holdings.

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

The command above loads `/home/dylan/schwab-data` by default. Put a Schwab
positions CSV directly in that directory; its filename must contain `position`
and end in `.csv` (case-insensitive). The newest modification time wins. Use
exports from a single account. Balances, transactions and realized gain/loss
are not required. The total is the signed sum of exported holdings values,
including cash if present, not necessarily account equity.

Override the directory for this process (fish):

```fish
env SCHWAB_DATA_DIR=/some/other/path cargo leptos watch
```

Run using only the included synthetic fixture:

```fish
cd /home/dylan/git_repo/epic-platform
fish_add_path --path "$PWD/target/tools/bin"
env SCHWAB_DATA_DIR="$PWD/tests/fixtures/schwab" cargo leptos watch
```

The fixture contains four holdings, including cash and a short option, totaling
USD 1,552.75. It contains no real account data. If loading fails, the web server
still starts and the Portfolio page explains why. Fix the directory/files and
press **Reload local data**. A failed reload keeps the last good snapshot and
marks it stale. Restarting loses memory; startup attempts to load the files again.

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

For a built server using synthetic data:

```fish
env SCHWAB_DATA_DIR="$PWD/tests/fixtures/schwab" ./target/debug/epic-platform
```

In another terminal:

```fish
curl --fail http://127.0.0.1:3000/health
# Expected: ok

# Inspect startup status and holdings.
curl --fail-with-body http://127.0.0.1:3000/api/portfolio

# Request a reload: no file, path or request body.
curl --fail-with-body -X POST http://127.0.0.1:3000/api/portfolio/reload

# Read the updated snapshot.
curl --fail-with-body http://127.0.0.1:3000/api/portfolio
```

GET returns a snapshot even when loading failed. POST returns 200 on success or
422 with the failed snapshot and error. Decimal amounts are JSON strings;
missing optional values are `null`. Last-success times use UTC RFC 3339.

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
cargo test --locked --features ssr
cargo leptos build
```

Build the `ssr` and `hydrate` features separately: they represent different
execution environments. Plain `cargo run` does not build the browser assets;
use cargo-leptos and run its built executable. The manifest passes `--locked`
to both Cargo builds. Node, npm, Trunk, and handwritten JavaScript are not needed.

Read [Stage 2 architecture](docs/architecture-stage-02.md) for the implemented
request/data flow, parsing rules, state lifetime and module responsibilities.
[Stage 1 architecture](docs/architecture-stage-01.md) records the original skeleton.
Tests use synthetic fixtures and temporary directories, never the default data
directory. No database, external retrieval, authentication, scheduling, AI,
or Review workflow is implemented.
