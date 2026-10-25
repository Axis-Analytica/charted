# Handoff: starting milestone 1 (Data view)

Read this first when picking up development. Last updated 2026-10-25, end of M0.
The approved plan lives in `~/.claude/plans/then-get-to-planning-imperative-wave.md`
(seven milestones, each ending in a visual checkpoint the user reviews before the next
one starts). Background research: `~/Work/pbi-re/notes/00-blueprint.md` and `01–04`
(Power BI Desktop architecture; build from these notes, never from decompiled code).

## Decisions already made (do not re-litigate)

- Name `charted`, Apache-2.0, repo `github.com/Axis-Analytica/charted` (public, `main`).
- Tauri 2 + Rust core + React 19 UI rendered by WebKitGTK. DuckDB 2.0 embedded.
- Semantic model on disk is Apache Ossie YAML (`model.ossie.yaml`, spec `0.2.0.dev0`,
  schema vendored at `crates/ossie/schema/` with the upstream commit in `UPSTREAM`).
  Power BI properties go in `POWER_BI` `_v:2` blobs exactly as the official converter
  (`apache/ossie-converters/microsoft`) writes them; measures are root-level metrics
  with the home table in the blob (the converter ignores dataset-scoped metrics).
  charted-only data goes in a `CHARTED` vendor blob; unknown content is preserved in
  each model object's `extras`.
- Report pages follow the Omarchy theme live by default.
- Network: only opt-in, consented DuckDB extension downloads (Postgres/SQLite, M5).
  No telemetry ever.
- Distribution: omarchy-pkgs + AUR (`charted`, `charted-bin`) + GitHub release tarballs.
- Push only when the user asks. Commit messages: short, imperative, with the
  `Co-Authored-By` / `Claude-Session` trailer lines.

## What exists (M0)

```
crates/model      types, typed Change + exact inverse, History (undo/redo, saved marker)  [wasm-clean]
crates/ossie      Ossie document types, model<->Ossie mapping, POWER_BI/CHARTED stash, schema validation [wasm-clean]
crates/compiler   steps.rs: Query{source, steps} -> one SELECT with CTEs; quote.rs     [wasm-clean]
crates/engine     DuckDB Database wrapper, Arrow IPC encoding, json/csv output, sql quoting
crates/samplegen  deterministic "Harbor & Pine" star schema (ChaCha8 seed 42), CC0
crates/core       project folder format, Session (history, recovery, lock), App + Request API,
                  env/ (Omarchy theme, Hyprland rounding/animations, language, live watcher), check, sample
crates/cli        clap CLI (check/query/sample/skill/serve) + `charted-headless` bin; devserver.rs (WebSocket)
src-tauri         the `charted` binary: commands rpc/data/env/pick/initial_path/quit, portal file picker
                  (ashpd), env-changed events, close guard, `open --screenshot`
ui/               Vite + React: bridge/ (Transport: Tauri | WebSocket), design/ (tokens from Omarchy),
                  i18n/, kit/, app/ (App, shortcuts registry, overlays, kit gallery), views/ (Welcome,
                  ProjectView, TablesPanel, TablePreview, StatusBar), format/ (format strings), spike/
i18n/             da.json, ar.json (English-keyed); tools/i18n-strings.mjs extracts/checks strings
bin/              env, setup, dev, test, checkpoint
docs/             perf.md (WebKitGTK spike numbers), demos/m0.md, this file
.github/workflows/ci.yml   x86_64 + arm64 Rust, UI + Playwright e2e
```

Request flow: UI → `transport.rpc(Request)` → `App::handle` → returns full `AppState`
(Rust is the source of truth; the UI replaces its mirror). Data (Arrow IPC bytes) goes
through `transport.data(DataRequest)` separately. TypeScript types in
`ui/src/bridge/generated/` are written by ts-rs when `cargo test` runs; commit them.

## Commands

```sh
. bin/env                 # puts mise tools and ~/.cargo/bin on PATH (needed in every shell)
bin/test                  # fmt, clippy -D warnings, tests, wasm32 check, i18n, tsc, e2e
bin/dev                   # vite + debug app
bin/checkpoint m1         # screenshots to ~/Work/charted-checkpoints/m1 + live demo
CHARTED_CHECKPOINT_NO_OPEN=1 bin/checkpoint m1   # non-interactive (for the agent)
target/debug/charted open <project> --screenshot out.png   # WebKitGTK snapshot; delay via CHARTED_SCREENSHOT_DELAY_MS
target/debug/charted-headless serve --port 7420 --state-dir <dir>   # dev WebSocket server
```

UI in a browser against the real core: start the dev server, start Vite, open
`http://localhost:1420/?ws=ws://127.0.0.1:7420&open=<url-encoded project path>`.
Playwright (`ui/playwright.config.ts`) does this automatically on ports 7421/1421 with
a throwaway `XDG_DOCUMENTS_DIR`; use `CHROMIUM_PATH=/usr/bin/chromium` locally.

## Gotchas learned the hard way

- **`~/Work` is `/mnt/mac`, a host shared folder with no inotify events.** Vite needs
  `CHARTED_POLL=1` there (bin/dev and bin/checkpoint set it automatically). Any future
  file watching of project files (M5 source refresh, external-edit reload) needs a
  `notify::PollWatcher` fallback on such filesystems.
- **Never `pkill -f`/`pgrep -f` with a pattern that appears in your own command line**;
  it kills the running shell (exit 144). Use pid files, `pgrep -x <name>`, or
  `fuser -k <port>/tcp`.
- **DuckDB is linked as the official prebuilt library** (`charted-engine` feature
  `duckdb-download`, the default). The bundled C++ build takes hours on this 7 GB
  aarch64 machine and its plain `bundled` feature omits Parquet/JSON. Binaries find
  `libduckdb.so` via rpaths in `.cargo/config.toml` (`$ORIGIN/../duckdb-download/...`
  for dev, `$ORIGIN/../lib/charted` for the package). Packaging must install
  `libduckdb.so` to `/usr/lib/charted/`.
- `rand` 0.10: methods live on `RngExt` (import `rand::RngExt`).
- ts-rs: annotate `u64` fields `#[ts(type = "number")]` or they become `bigint`; enums
  with struct variants need `rename_all_fields = "camelCase"`.
- Monaco imports use the export map: `monaco-editor/editor/editor.api`,
  `monaco-editor/editor/editor.worker?worker`.
- Glide Data Grid 6.0.3 declares React ≤18 as a peer; it works with React 19 (verified).
- notify watcher: watch stable parent dirs once; only re-arm the replaced theme dir.
  Re-watching the same path races with inotify's IN_IGNORED and drops later events.
  Ignore `Access` events or your own reads cause an event loop.
- Layout: use flex columns (not grid row templates) for containers with optional
  children (banners), or rows shift and the main area collapses.
- Arrow JS decodes DECIMAL poorly; `Session::preview` casts DECIMAL/HUGEINT to DOUBLE
  for display only. Do the same for any display query.
- WebKit's first load can race Vite's dependency optimisation; screenshots need a
  warm Vite (curl `/src/main.tsx` first) and a delay of 5–7 s.

## Deferred from M0 (do these when relevant)

- Visual query IR → SQL with many-to-one star joins (`crates/compiler`): start of M2.
- `tauri-plugin-single-instance` (one window per project; the lock file already prevents
  double editing).
- tauri-driver smoke test in CI; nightly jobs (Ossie schema drift, converter cross-check).
- Live rounding is verified manually only (watch.rs polls Hyprland's socket every 1.5 s).
- CI: green on the M0 push (Rust on x86_64 and arm64, UI + e2e). Check `gh run list
  -R Axis-Analytica/charted` after each push.

## Milestone 1: Data view (next)

Goal: a fast, beautiful data grid that beats Power BI's. Deliverables (from the plan):

1. Glide grid backed by the core: `DataRequest::Rows { table, columns, offset, limit,
   sort, filters }` → Arrow; 200-row blocks, LRU of 64 blocks, `AbortController` on fast
   scroll; stable order `ORDER BY …, rowid`; keyset paging beyond offset 1M. Row count
   request for the scroll height.
2. Header type icons, multi-column sort (Shift-click), per-column filter menu (value
   checklist over the top 1,000 distinct values with search; text/number/date
   conditions), clear filters. Filters/sort are view state, not undoable model changes.
3. Status bar: rows, filtered rows, distinct count of the selected column.
4. Column properties panel: name, data type, format string with live preview,
   summarize-by, data category, sort-by-column (verify 1:1 with COUNT DISTINCT), hidden,
   description; each a typed `Change` (`SetColumn`/`RenameColumn`) → undoable, persisted
   to Ossie.
5. Statistics drawer (Ctrl+I): count, distinct, nulls, min/max/mean/median/stddev/
   p25/p75, zeros, date span, mini histogram (one aggregate query + binning).
6. Copy cell/selection/column as TSV; Ctrl+F find value.
7. Format-string engine hardening (`ui/src/format/`) with Vitest tables (add Vitest).
8. Rail gets a Data view (Ctrl+2); M0's preview becomes the Data view.

Checkpoint (docs/demos/m1.md to write): fast scroll with a frame HUD (Ctrl+Alt+P),
multi-sort, filter, stats drawer, format change + undo, TSV copy, and the 10M-row
sample (`charted sample create <dir> --scale 10m`) painting in under 300 ms.
Add `e2e/m1.spec.ts`, keep `bin/test` green, then `CHARTED_CHECKPOINT_NO_OPEN=1
bin/checkpoint m1` and show the user the screenshots before moving on.
