# Handoff: milestone 1 (Data view) built; checkpoint review, then milestone 2

Read this first when picking up development. Last updated 2026-10-25, end of M1 build.
**Status:** M1 is implemented and `bin/test` is green. The user has not yet reviewed
the M1 checkpoint (`docs/demos/m1.md`, screenshots in `~/Work/charted-checkpoints/m1/`).
Address their feedback before starting M2.
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
docs/             perf.md (WebKitGTK spike numbers), demos/m0.md, demos/m1.md, this file
.github/workflows/ci.yml   x86_64 + arm64 Rust, UI + Playwright e2e
```

M1 added:

```
crates/core/src/dataview.rs   ViewSpec {table, sort, filters} -> View: rows (Arrow), count,
                              distinct values, column stats, find; type-change and sort-by
                              checks against the data (Session::commit runs them)
crates/core/tests/dataview.rs integration tests on the demo sample
crates/engine/examples/sqltime.rs   `sqltime DB "SQL"…` timing tool (release build;
                              LD_LIBRARY_PATH=target/duckdb-download/<arch>/v2.0.0)
ui/src/state/dataview.ts      view state per table (sort, filters, widths, selection);
                              reset when the project path changes
ui/src/views/data/            DataView (orchestrator, copy, shortcuts), DataGrid (Glide,
                              custom header drawing, Lucide header sprites), blocks.ts
                              (block LRU), FilterMenu, ColumnProperties, StatsDrawer,
                              FindBar, data.css
ui/src/format/                hardened engine + format.test.ts (Vitest, `pnpm test`)
ui/src/app/FrameMeter.tsx     FrameHud (Ctrl+Alt+P): fps, p50/p95, first-rows time
ui/e2e/m1.spec.ts
```

**Paging (deviation from the plan, better):** no OFFSET and no keyset paging. An
unsorted, unfiltered table is read by rowid range (rowids of a materialised table are
dense; checked once per table). Any sort or filter materialises the row order once as
an "index" table of source rowids in an attached in-memory database `charted_view`
(LRU of 8); a page is the index's rowid range joined back to the source. Measured on
9.8M rows (aarch64 VM): index build 60–360 ms, any page ~10 ms at any offset; in the
UI first rows paint in ~100 ms unsorted, ~170 ms after a one-key sort, 60 fps scroll.
`DataRequest` jobs are resolved under the `App` lock and run without it
(`App::data_job` → `DataJob::run`), so pages load in parallel. Non-`Rows` data requests
reply with JSON bytes (`dataJson` in the UI).

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

- **React StrictMode double-mounts in dev**: effects run cleanup then run again with the
  same memoised objects. Anything closed in a cleanup must be reopenable (BlockCache.open).
- Glide's offscreen accessibility table is not rendered here, so e2e tests cannot read
  grid cells from the DOM. Read values through Copy (clipboard permission granted in
  m1.spec.ts) and open column menus with `Alt+↓` (selected column) instead of clicking
  canvas coordinates.
- Keyboard handlers that `preventDefault` on plain arrows (the Tables tree) swallow
  global shortcuts such as `Alt+ArrowDown`: let modified keys through.
- The sample's ids are deterministic, so two sample projects share table ids. Key any
  per-table UI by project path too (DataView is keyed `${project.path}\n${table.id}`).
- The project lock is per process: a running `charted-headless serve` that opened a
  project makes `charted open` of the same project fail with "already open". Kill it
  with `fuser -k 7420/tcp` before WebKit screenshots.
- Commit labels (`commit("Change format", …)`) are not in the i18n catalogs: the string
  checker only extracts `t("…")` and `label: "…"`, and unknown catalog keys fail the
  check. Translating undo labels needs a small extractor change.

## Deferred from M0 (do these when relevant)

- Visual query IR → SQL with many-to-one star joins (`crates/compiler`): start of M2.
- `tauri-plugin-single-instance` (one window per project; the lock file already prevents
  double editing).
- tauri-driver smoke test in CI; nightly jobs (Ossie schema drift, converter cross-check).
- Live rounding is verified manually only (watch.rs polls Hyprland's socket every 1.5 s).
- CI: green on the M0 push (Rust on x86_64 and arm64, UI + e2e). Check `gh run list
  -R Axis-Analytica/charted` after each push.

## Deferred from M1

- Calculated columns are skipped by the Data view (they arrive with the DAX engine, M3).
- Find compares raw values (as text), not formatted text.
- Column order changes (drag) and freezing are not implemented; widths are view state
  and not saved with the project.
- Copy caps at 1,000,000 rows; formatted with the column's format string.
- Server-side query cancellation: stale page requests are dropped before sending, but a
  sent query runs to completion (DuckDB `interrupt_handle` exists if needed).

## Milestone 2: First report (after the M1 review)

From the plan: canvas with pages, snap/multi-select/align, inserts under the pointer;
Fields → role wells (drag and keyboard); capability registry; generated format pane;
visuals card, clustered bar/column, line, table; **the DAX core parser starts here**
(aggregates, arithmetic, DIVIDE, IF; stored as DAX + generated OSSIE_SQL; quick and
implicit measures); filters pane (visual/page/report; basic/advanced/Top N);
PBIR-lite report files; `charted query --visual … --json`. Starts with the deferred
visual query IR → SQL with many-to-one star joins in `crates/compiler`.

Checkpoint: build the Overview page (card, column, line, table); create `Margin %`
with `DIVIDE`; add a page filter Year=2025; align; save, reopen, identical.
Write `docs/demos/m2.md` and `e2e/m2.spec.ts`, keep `bin/test` green, then
`CHARTED_CHECKPOINT_NO_OPEN=1 bin/checkpoint m2`.
