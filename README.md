# EPIC Platform

A local-first personal investment platform, built gradually in Rust on Linux.
Stage 3 adds an internal research module with manual company refresh and durable
SQLite earnings snapshots. This is one workspace, one Linux process, one Axum
server and one Leptos app. **ConsensX does not need to run.** Its minimal public
source adapter and snapshot ideas are adapted inside EPIC; no iframe or second
service is involved. Portfolio holdings still come from local Schwab CSVs and
remain in memory. Review remains the original placeholder.

## Run (Linux / fish)

Prerequisites: Rust 1.94 or newer via rustup, a C linker/build toolchain, and fish.
For example, on Arch/CachyOS install build tools with `sudo pacman -S --needed base-devel cmake`;
on Debian/Ubuntu use `sudo apt install build-essential cmake pkg-config libssl-dev`.
The first build needs internet access to download dependencies and build tools.
The built application uses local UI assets. Only an explicit company refresh
requires internet access to Stock Analysis; reading saved research works offline.

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

Research uses `RESEARCH_DB_PATH`, default `data/research.db` relative to the
server's working directory. Missing parent directories and the database are
created; versioned migrations run at startup. For explicit paths in fish:

```fish
cd /home/dylan/git_repo/epic-platform
set -gx SCHWAB_DATA_DIR /home/dylan/schwab-data
set -gx RESEARCH_DB_PATH "$PWD/data/research.db"
fish_add_path --path "$PWD/target/tools/bin"
cargo leptos watch
```

No research API key, token, source configuration, or `CONSENSX_BASE_URL` is needed.
The source is ConsensX's actual public endpoint:
`https://stockanalysis.com/stocks/{lowercase-symbol}/forecast/__data.json`.
Only public fields are read. The adapter has a 25-second request timeout, a 2 MB
response cap, and no retries. Provider changes or access failures are displayed
without removing saved snapshots.

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

To exercise research with invented holdings, use the separate Stage 3 fixture
(the original Stage 2 fixture and tests are preserved):

```fish
cd /home/dylan/git_repo/epic-platform
fish_add_path --path "$PWD/target/tools/bin"
cargo leptos build
env SCHWAB_DATA_DIR="$PWD/tests/fixtures/research" RESEARCH_DB_PATH="$PWD/data/research-demo.db" ./target/debug/epic-platform
```

Open <http://127.0.0.1:3000/research>. NVDA and MSFT initially show **Never
refreshed**. Click **Refresh company** for NVDA to fetch and save its reported
quarter, revenue, diluted EPS, source timestamp, retrieval time, and source link.
Only that company's card changes. ZZZZ is unmatched; ETF, option and cash rows
are explicitly unsupported. The registry initially covers NVDA, MSFT, GOOGL,
NET, AMD, ARM, SPCX and MU, matching the inspected ConsensX registry. Unknown
symbols do not cause implicit company discovery.

In another terminal (fish):

```fish
curl --fail http://127.0.0.1:3000/health
curl --fail-with-body -X POST http://127.0.0.1:3000/api/portfolio/reload
curl --fail-with-body http://127.0.0.1:3000/api/research/holdings
curl --fail-with-body -X POST http://127.0.0.1:3000/api/research/companies/NVDA/refresh
curl --fail-with-body http://127.0.0.1:3000/api/research/companies/NVDA
```

Stop EPIC with **Ctrl+C**, then run the exact same `env ... ./target/debug/epic-platform`
command above with the same database path. Call the company GET again: the saved
snapshot, retrieval date, and source links survive restart without another
network request. Portfolio holdings are still reconstructed from the CSV at
startup; they are never saved in the research database.

A same-period refresh returns `unchanged` and preserves the first snapshot's
facts and retrieval time. New earnings periods add immutable history. Latest
means greatest period-end date, so an older provider response cannot roll the
display back. Last-attempt status and errors are process-local and reset at
restart; they are separate from the saved snapshot dates.

To simulate source failure while retaining your demo research, stop EPIC, then
restart with a deliberately unreachable HTTPS proxy (assuming local port 9 is unused):

```fish
env SCHWAB_DATA_DIR="$PWD/tests/fixtures/research" RESEARCH_DB_PATH="$PWD/data/research-demo.db" HTTPS_PROXY=http://127.0.0.1:9 NO_PROXY= ./target/debug/epic-platform
```

The company GET and Research page still show the saved snapshot. Click
**Refresh company**, or run the refresh POST above: it returns a typed error
alongside the old snapshot (HTTP 502 for connection failure). Portfolio, local
reload, `/health`, and Review continue working. Stop and restart without the
proxy override to restore normal source access. The proxy is a standard HTTP
client environment setting, not another research provider or service.

Research holdings GET always returns HTTP 200 with per-row status and any
repository error. Company GET returns 200 for saved or never-refreshed research,
400 for invalid symbols, 404 for symbols outside the registry, or 503 for a
repository failure. Refresh returns 200 (`saved` or `unchanged`), 409 if another
refresh is active, 502 for upstream/invalid-source errors, 504 for timeout, or
503 for database failure. Its JSON includes `outcome` and `company`, including
`refresh_error` and any older snapshot. Research initialization/migration errors
are logged and shown on Research; they do not stop the rest of EPIC.

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

Read [Stage 3 architecture](docs/architecture-stage-03.md) for module boundaries,
source normalization, transactions, immutable history and failure behavior.
Read [Stage 2 architecture](docs/architecture-stage-02.md) for the implemented
request/data flow, parsing rules, state lifetime and module responsibilities.
[Stage 1 architecture](docs/architecture-stage-01.md) records the original skeleton.
Tests use synthetic fixtures and temporary directories, never the default data
directory. Research tests use temporary databases and local mock HTTP servers;
they never require internet access, ConsensX, or a production database. There is
no authentication, scheduling, automatic refresh, AI analysis, or Review workflow.
Old ConsensX database import is deferred; do not point `RESEARCH_DB_PATH` at it.
