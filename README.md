# EPIC Platform

A local-first personal investment platform, built gradually in Rust on Linux.

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
`/research` and `/review`. On Review, click **Create portfolio review**, then
select a history entry to open it. Navigation supports direct URLs, reload,
and browser back/forward.
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

## Durable portfolio review jobs (Linux / fish)

Review and Jobs share the execution SQLite database, configured by `REVIEW_DB_PATH` (default
`data/reviews.db`, relative to the working directory). Use a different path from
`RESEARCH_DB_PATH`: their versioned migrations and ownership are independent.
Jobs owns queue/attempt tables; Review owns immutable review records. One migration
sequence upgrades existing Stage 4 databases in place. Existing databases are not moved.
Review files contain saved holding values;
database and WAL/SHM files stay local and are ignored by Git. No raw CSV is stored.

Start the single application process from fish:

```fish
cd /home/dylan/git_repo/epic-platform
fish_add_path --path "$PWD/target/tools/bin"
cargo leptos build
env SCHWAB_DATA_DIR=/home/dylan/schwab-data RESEARCH_DB_PATH="$PWD/data/research.db" REVIEW_DB_PATH="$PWD/data/reviews.db" ./target/debug/epic-platform
```

For an entirely synthetic holdings demo, replace the startup command with:

```fish
env SCHWAB_DATA_DIR="$PWD/tests/fixtures/research" RESEARCH_DB_PATH="$PWD/data/research-demo.db" REVIEW_DB_PATH="$PWD/data/reviews-demo.db" ./target/debug/epic-platform
```

In a second fish terminal (Python 3 is used only to extract the returned ID):

```fish
curl --fail http://127.0.0.1:3000/health
curl --fail-with-body http://127.0.0.1:3000/ready
curl --fail-with-body -X POST http://127.0.0.1:3000/api/portfolio/reload
curl --fail-with-body http://127.0.0.1:3000/api/portfolio

# Optional: explicitly refresh research before creating the review.
curl --fail-with-body -X POST http://127.0.0.1:3000/api/research/companies/NVDA/refresh

# One new key per intentional action. Reuse this key if the response is lost.
set job_key (cat /proc/sys/kernel/random/uuid)
set job_id (curl --fail-with-body -X POST -H "Idempotency-Key: $job_key" http://127.0.0.1:3000/api/reviews | python3 -c 'import json,sys; print(json.load(sys.stdin)["job_id"])')
# Repeat the same submission: HTTP 202, same job ID, no second review.
curl --fail-with-body -i -X POST -H "Idempotency-Key: $job_key" http://127.0.0.1:3000/api/reviews
curl --fail-with-body "http://127.0.0.1:3000/api/jobs/$job_id"
curl --fail-with-body http://127.0.0.1:3000/api/jobs

# Poll until a terminal state (also what the page does every two seconds).
set job_status queued
while contains -- $job_status queued running
    sleep 2
    set job_status (curl --fail-with-body "http://127.0.0.1:3000/api/jobs/$job_id" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])')
    echo $job_status
end
# After succeeded:
set review_id (curl --fail-with-body "http://127.0.0.1:3000/api/jobs/$job_id" | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["id"])')
curl --fail-with-body http://127.0.0.1:3000/api/reviews
curl --fail-with-body "http://127.0.0.1:3000/api/reviews/$review_id"

# Only if the job is failed or interrupted, after fixing its cause:
curl --fail-with-body -i -X POST "http://127.0.0.1:3000/api/jobs/$job_id/retry"
```

Open <http://127.0.0.1:3000/review> to submit jobs or select saved reviews. The
page restores recent jobs after a browser refresh, polls queued/running jobs,
offers manual retry after failure/interruption, and opens completed reviews.
Inputs are captured when the worker runs, using current memory and already-saved
research. A queued job contains no frozen portfolio; a retry without a saved
result captures then-current inputs. Missing research
allows zero/partial coverage. Registry-supported equities define coverage;
unmatched stocks and unsupported instruments are shown separately. Position
weights use signed value divided by positive net total, while concentration and
coverage by value use absolute values. Undefined percentages display N/A.
Snapshot ages are saved in whole seconds at creation, without a stale/fresh label.

Stop the server with Ctrl+C, then repeat the same startup command with the same
database paths. In the second terminal, repeat the history and saved-ID GETs:

```fish
curl --fail-with-body http://127.0.0.1:3000/api/reviews
curl --fail-with-body "http://127.0.0.1:3000/api/reviews/$review_id"
curl --fail-with-body "http://127.0.0.1:3000/api/jobs/$job_id"
curl --fail-with-body http://127.0.0.1:3000/ready
```

The review's positions, source filename/load time, research facts, dates, links
and ages remain unchanged. Create a second review after manually loading a new
portfolio or refreshing research; selecting it shows differences from the
immediately previous saved review. Those differences are not investment returns.

POST now returns **202 Accepted**, `Location: /api/jobs/{id}`, and
`{job_id,status,status_url}`. `Idempotency-Key` is optional (1–128 ASCII letters,
digits, `.`, `_`, `-`); its uniqueness lasts for the life of this database.
Repeating a key returns the existing job, including its current terminal status.
Use a fresh key for a separate intentional review. The page generates UUIDs and
retains a submission key on a request error. `GET /api/jobs` returns at most 30
recent jobs. Retry returns 202 with the same ID; only failed/interrupted jobs
qualify (409 otherwise). Attempts count executions and increment on claim,
starting at zero while initially queued. Previous failure details are retained.

GET review history returns lightweight entries;
GET by ID returns `review`, `comparison`, and `comparison_error`. Errors contain
`error.kind` and `message`: 400 invalid review ID, 404 absent review, or 503
repository/init/migration failure. Comparison errors remain separate from the
selected saved review. Job errors use the same structured envelope:
400 invalid ID/key, 404 missing job, 409 no portfolio/invalid retry state, 422 invalid
portfolio, 503 queue/worker unavailable. Execution failures appear on the job.
Research-read errors are recorded in a successfully created
review as unavailable capture, distinct from no saved research. Old reviews need
only the Review database.

Queued jobs survive restart and execute after startup. Abandoned running jobs
become succeeded if their unique origin review exists, or interrupted otherwise.
Interrupted jobs wait for deliberate retry; there is no automatic retry. The
unique job-to-review link prevents duplicates if a process stops after saving a
review but before recording success. A graceful Ctrl+C/SIGTERM stops new claims
and gives active work up to five seconds to finish. Run only one application
process against a given execution database.

`/health` is liveness: the process answers HTTP. `/ready` returns 200 only when
Research/Review migrations completed, the Job repository can be queried, and
the worker is available; otherwise it returns 503 with diagnostic booleans.
Neither endpoint contacts the research provider. Missing portfolio data does
not make the process unready; submission explains that data must be loaded.

Job logs are structured JSON with job ID, kind, attempt, state/progress, elapsed
execution time and result review ID. For explicit logging in fish:

```fish
env RUST_LOG=epic_platform=info SCHWAB_DATA_DIR=/home/dylan/schwab-data RESEARCH_DB_PATH="$PWD/data/research.db" REVIEW_DB_PATH="$PWD/data/reviews.db" ./target/debug/epic-platform
# Deterministic offline recovery tests, including queued restart and crash-after-commit:
cargo test --locked --features ssr --test jobs
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

Read [Stage 5 architecture](docs/architecture-stage-05.md) for the durable queue,
atomic claims, polling, idempotency, recovery, shutdown and readiness.
Read [Stage 4 architecture](docs/architecture-stage-04.md) for review orchestration,
immutable inputs, calculation definitions, separate database ownership and API behavior.
Read [Stage 3 architecture](docs/architecture-stage-03.md) for module boundaries,
source normalization, transactions, immutable history and failure behavior.
Read [Stage 2 architecture](docs/architecture-stage-02.md) for the implemented
request/data flow, parsing rules, state lifetime and module responsibilities.
[Stage 1 architecture](docs/architecture-stage-01.md) records the original skeleton.
Tests use synthetic fixtures and temporary directories, never the default data
directory. Research tests use temporary databases and local mock HTTP servers;
they never require internet access, ConsensX, or a production database. There is
no authentication, scheduling, automatic refresh, automatic job retry, or AI analysis.
Old ConsensX database import is deferred; do not point `RESEARCH_DB_PATH` at it.
