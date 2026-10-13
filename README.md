# IRONDB-client

> **IRONDB** — a cross-platform, multi-engine data workbench: query, export, and serve dozens of database engines from one headless CLI. This repo distributes the prebuilt **Windows x86_64** binary plus full usage docs. No install, no runtime, no admin rights — download and run.

- **Binary:** [`irondb-headless-windows-x86_64.exe`](./irondb-headless-windows-x86_64.exe) (~48 MB, self-contained, v0.1.0)
- **Upstream source:** Rust workspace (`crates/irondb-*` + Tauri desktop shell) — see [Building from source](#8-building-from-source)
- **License:** see upstream `LICENSE`/`NOTICE`

---

## Table of contents

1. [What is IRONDB?](#1-what-is-irondb)
2. [Quickstart (60 seconds)](#2-quickstart-60-seconds)
3. [Installation](#3-installation)
4. [Core concepts](#4-core-concepts)
5. [CLI reference](#5-cli-reference)
6. [HTTP server mode](#6-http-server-mode)
7. [Connection management](#7-connection-management)
8. [Building from source](#8-building-from-source)
9. [Supported engines](#9-supported-engines)
10. [Output formats](#10-output-formats)
11. [Security model](#11-security-model)
12. [Troubleshooting](#12-troubleshooting)
13. [FAQ](#13-faq)

---

## 1. What is IRONDB?

IRONDB is the Rust rebrand of the GoNavi data workbench. It has two faces:

| Face | Binary | What it is |
|------|--------|------------|
| **Desktop app** | `irondb-tauri` (Tauri + web UI) | Full workbench: query editor, data grid, AI assist, sync/migration |
| **Headless CLI** (this repo) | `irondb-headless` | Everything *except* the window: run SQL, manage connections, export data, serve an HTTP API — from scripts, CI, containers, SSH |

The headless CLI speaks to the same driver registry and facade as the desktop app, so queries behave identically in both. A **differential gate** in CI replays a frozen golden-query corpus against the original Go implementation to guarantee parity.

**Typical uses**

- Ad-hoc SQL from the terminal against any supported engine (`query`)
- Nightly CSV/JSONL dumps for pipelines (`export`)
- A local HTTP API for query + export + cancel (`serve`) — e.g. sidecar next to a dashboard
- Connection profiles shared between CLI, desktop, and server (`connection`)

---

## 2. Quickstart (60 seconds)

```powershell
# 1. Download irondb-headless-windows-x86_64.exe from this repo (above)

# 2. Check it runs
.\irondb-headless-windows-x86_64.exe --version
# irondb-headless 0.1.0

# 3. Run a query against SQLite (in-memory demo via the fake driver)
.\irondb-headless-windows-x86_64.exe query --conn "sqlite::memory:" --sql "SELECT 1 AS one"
# {"one":1}

# 4. Export the same result to CSV
.\irondb-headless-windows-x86_64.exe export --conn "sqlite::memory:" --sql "SELECT 1 AS one" --output one.csv
# -> one.csv (use --force to overwrite an existing file)

# 5. Start the HTTP API (new terminal)
.\irondb-headless-windows-x86_64.exe serve --bind 127.0.0.1:18789
curl http://127.0.0.1:18789/healthz
# {"ok":true}
```

> Exact DSN shapes are driver-dependent (`engine:key=value …` style strings — see [§7](#7-connection-management)). SQLite needs no server and is the fastest way to verify your setup.

---

## 3. Installation

**Option A — prebuilt binary (recommended, Windows x86_64)**

1. Download `irondb-headless-windows-x86_64.exe` from this repo.
2. (Optional) Rename to `irondb.exe` and put it on `PATH`:
   ```powershell
   Rename-Item .\irondb-headless-windows-x86_64.exe irondb.exe
   $env:Path += ";$PWD"   # current session; persist via System settings for permanent
   ```
3. Verify: `irondb --version` → `irondb-headless 0.1.0`.

No installer, no dependencies (fully static-ish release build), no admin rights. Windows Defender SmartScreen may prompt on first run for an unsigned binary — choose *More info → Run anyway*.

**Option B — build from source** (any OS with a Rust 1.82+ toolchain): see [§8](#8-building-from-source).

**Option C — desktop app**: the Tauri GUI (`irondb-tauri`) is built from the upstream workspace; it embeds this same headless binary as a sidecar (`externalBin`) for background work.

---

## 4. Core concepts

- **Connection (`--conn`)** — a DSN string identifying engine + target, e.g. `sqlite:/path/to/app.db`. Pass inline per command, or store named profiles (see `connection` subcommand).
- **Data root (`--data-root`)** — directory where IRONDB keeps connection profiles and state. Default: `%TEMP%\irondb-headless` (ephemeral!). **Set `--data-root` explicitly** (or back it up) if you store named connections — temp dirs get wiped.
- **Read-only by default** — write statements are rejected unless `--allow-write` is passed. Exports never write to the source.
- **Query timeout (`--query-timeout`, seconds, `0` = none)** — long-running queries can also be cancelled via the HTTP API (`POST /api/cancel/:id`).

---

## 5. CLI reference

Global: `--data-root <DIR>` (state dir), `-h/--help`, `-V/--version`.

### `query` — run SQL, stream rows

```
irondb-headless query [OPTIONS]
      --conn <CONN>                         DSN string
      --connection-file <CONNECTION_FILE>   read DSN from a file (avoids secrets in shell history)
      --database <DATABASE>                 database/catalog override
      --sql <SQL>                           SQL text
      --sql-file <SQL_FILE>                 read SQL from a file
      --format <FORMAT>                     [default: jsonl] (see §10)
      --allow-write                         permit INSERT/UPDATE/DDL
      --query-timeout <QT>                  seconds, 0 = no timeout
```

Examples:

```powershell
# JSONL to stdout (default)
.\irondb-headless.exe query --conn "sqlite:C:\data\app.db" --sql "SELECT id, name FROM users LIMIT 5"

# SQL from file, DSN from file (no secrets on the command line)
.\irondb-headless.exe query --connection-file .\prod.dsn --sql-file .\report.sql --format csv

# Write query (explicit opt-in)
.\irondb-headless.exe query --conn "sqlite:C:\data\app.db" --allow-write --sql "CREATE INDEX idx_users_name ON users(name)"
```

### `export` — run SQL, write a file

```
irondb-headless export [OPTIONS]
      (same connection/sql options as query, plus:)
      --output <OUTPUT>    destination file (required)
      --format <FORMAT>    [default: csv]
      --force              overwrite an existing file
```

```powershell
.\irondb-headless.exe export --conn "sqlite:C:\data\app.db" --sql "SELECT * FROM orders" --output orders.csv --force
```

### `connection` — named connection profiles

```
irondb-headless connection <COMMAND> [--data-root DIR]
  list              list stored profiles
  add <ID>          store the DSN given via --conn/--connection-file under name <ID>
  import            import profiles from a file
```

```powershell
# store once (use --data-root somewhere durable!)
.\irondb-headless.exe --data-root C:\irondb-data connection add prod --conn "postgres:host=db.internal dbname=app"
.\irondb-headless.exe --data-root C:\irondb-data connection list
```

### `serve` — HTTP API mode

```
irondb-headless serve [--bind 127.0.0.1:18789] [--data-root DIR]
```

Starts the API described in [§6](#6-http-server-mode). Binds localhost by default — pass `--bind 0.0.0.0:18789` deliberately if you need LAN/container access (and read [§11](#11-security-model) first).

### `version` — build info

```powershell
.\irondb-headless.exe version
```

---

## 6. HTTP server mode

```powershell
.\irondb-headless.exe serve --bind 127.0.0.1:18789
```

| Method & path | Auth | Purpose |
|---------------|------|---------|
| `GET /healthz` | none | Liveness: `{"ok":true}` — use for load-balancer/health checks |
| `GET /api/connections` | bearer | List stored connection profiles |
| `POST /api/query` | bearer | Run SQL, JSON in/out |
| `POST /api/export` | bearer | Run SQL, get export back |
| `POST /api/cancel/:id` | bearer | Cancel a running query by id |

`POST /api/query` body (JSON):

```json
{ "conn": "sqlite:C:\\data\\app.db", "sql": "SELECT id FROM users LIMIT 5", "format": "jsonl" }
```

Notes:

- All `/api/*` routes require a bearer token (see [§11](#11-security-model)); `/healthz` is intentionally open.
- The same binary also serves the desktop app's web bridge under `/api/web/*` (`invoke`, `transfer/status`, `compact/register`, `bridge/ping`) — primarily for the Tauri frontend; the table above is the stable public surface.
- Behind a reverse proxy, terminate TLS there and keep the `--bind` on localhost.

---

## 7. Connection management

- **Inline DSN** (`--conn`) is best for one-offs and CI secrets (inject via env var).
- **DSN file** (`--connection-file`) keeps passwords out of shell history and process lists.
- **Named profiles** (`connection add/list`) are best for interactive reuse — but remember they live under `--data-root`, which defaults to the OS temp dir. For anything durable:

  ```powershell
  # durable state dir, e.g. %APPDATA%\irondb
  $env:IRONDB_DATA = "$env:APPDATA\irondb"
  .\irondb-headless.exe --data-root $env:IRONDB_DATA connection add warehouse --conn "postgres:host=…"
  .\irondb-headless.exe --data-root $env:IRONDB_DATA query --conn warehouse --sql "SELECT …"
  ```

  > The stored profile name (`warehouse`) can then be passed as `--conn` in later commands sharing the same `--data-root`.

---

## 8. Building from source

Prerequisites: **Rust 1.82+** (`rustup`), C compiler for native deps, Node 18+ + Tauri CLI only if you also build the desktop app.

```bash
git clone <upstream-IRONDB-workspace> && cd IRONDB
cargo build -p irondb-headless --release --locked
# -> target/release/irondb-headless[.exe]

# verify the workspace (needs the release sidecar present for the Tauri crate check)
cp target/release/irondb-headless target/release/irondb-headless-$(rustc -vV | sed -n 's/host: //p')  # sidecar naming convention
cargo check --workspace --locked
cargo test -p irondb-signoff --test differential --locked   # golden-query parity gate (live-oracle cases skip without a Go toolchain)
```

Workspace layout (upstream): `crates/irondb-{contracts,store,drivers,datasources,domain,facade,sidecar,headless,signoff}`, `src-tauri/` (desktop shell), `frontend-vanilla/` (web UI), `contracts/golden/` (frozen parity corpus), `tests/signoff/`.

---

## 9. Supported engines

Registered out of the box (via `irondb-datasources` + `irondb-facade`):

| Engine id | Notes |
|-----------|-------|
| `sqlite` | File or `:memory:`; in-memory capable |
| `postgres` | Pure-Rust client |
| `mysql` | Pure-Rust client |
| `sqlserver` | TDS client |
| `mongodb` | Document queries |
| `redis` | Key inspection / scan |
| `elasticsearch` | Console-style execution |
| `fake` / `fake-readonly` | Deterministic in-memory driver for demos + tests |
| long-tail (`gbase8a`, `gbase8s`, …) | Domestic/enterprise engines, capability-gated |

Capability flags per engine (in-memory, console-execute, …) are exposed through the driver registry; unknown engine ids fail fast with `UnknownEngine`.

---

## 10. Output formats

- `query --format`: default **`jsonl`** (one JSON object per line — ideal for `jq`, streaming, log pipelines). `csv` also supported.
- `export --format`: default **`csv`** (spreadsheets, `pandas.read_csv`, `COPY` ingestion).
- `--output` requires `--force` to clobber an existing file — pipelines should pass `--force` explicitly after their own existence checks.

---

## 11. Security model

- **Deny-write by default.** `query` rejects mutating statements unless `--allow-write` is passed per invocation. There is no config to make writes the default — every write is a conscious flag.
- **Secrets hygiene.** Prefer `--connection-file` over `--conn` for password-bearing DSNs; DSNs never get logged (redaction in the facade layer).
- **Server auth.** All `/api/*` routes are bearer-authenticated; only `/healthz` is open. Bind to localhost unless a reverse proxy provides TLS + auth in front.
- **Timeouts + cancel.** `--query-timeout` bounds runaway queries; `POST /api/cancel/:id` kills them server-side.
- **Unsigned binary notice.** This EXE is built from the pinned (`--locked`) workspace but is *not* code-signed. Verify provenance by rebuilding from source (§8) and comparing `version` output if your policy requires it.

---

## 12. Troubleshooting

| Symptom | Likely cause → fix |
|---------|-------------------|
| `UnknownEngine(foo)` | Typo in engine id or engine not compiled in → check [§9](#9-supported-engines) |
| Write rejected (`SELECT` works, `INSERT` fails) | Expected — add `--allow-write` |
| Named connection "vanished" | Default `--data-root` is `%TEMP%` (wiped on reboot/cleanup) → use a durable `--data-root` (§7) |
| `curl /healthz` → connection refused | `serve` binds `127.0.0.1:18789` by default; confirm the bind address/port and that the process is still running |
| `401` on `/api/*` | Missing/wrong bearer token; `/healthz` is the only open route |
| SmartScreen warning on first run | Unsigned binary → *More info → Run anyway*, or build from source (§8) |
| `cargo check -p irondb-tauri` fails on `resources/**` | Fixed upstream (line removed); `cargo check --workspace` is green. If you see the old error, pull latest |
| Tauri build complains about missing `irondb-headless-<triple>.exe` | Copy the release binary to the triple-suffixed name (see §8) — Tauri `externalBin` convention |

---

## 13. FAQ

**Is this the full IRONDB?**
This repo ships the headless CLI only. The full workbench (desktop GUI, sync/migration studio, AI assist) lives in the upstream workspace and shares the same drivers.

**Why is the EXE ~48 MB?**
It's a self-contained release build: async runtime, all drivers, HTTP server, no external DLLs required (beyond stock Windows libs + WebView2 only for the desktop app, not this CLI).

**Windows only?**
This repo's binary is Windows x86_64. The source builds on macOS/Linux too (`cargo build -p irondb-headless --release`).

**How do I update?**
Download the newer EXE from this repo (check `version` output), replace the file. Named connections persist as long as your `--data-root` is outside temp.

**Where do I report issues?**
Open an issue in this repo with: `version` output, engine id, `--format`, redacted command line, and the first 20 lines of output.
