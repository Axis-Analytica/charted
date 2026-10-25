# Handoff: milestone 4 (interactivity) built; checkpoint review, then milestone 5

Read this first when picking up development. Last updated 2026-10-25, end of M4 build.
**Status:** M0–M2 are approved; the user liked M2 ("looking outstanding") and has a
standalone M2 build (`~/.local/opt/charted-m2`, launcher "charted (M2 preview)"). M3 was
delivered and the user said to move on without separate feedback. M4 is implemented,
committed locally (not pushed) and `bin/test` is green. The user has not yet reviewed
the M4 checkpoint (`docs/demos/m4.md`, screenshots in
`~/Work/charted-checkpoints/m4/`). Address their feedback before starting M5.

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
bin/              env, setup, dev, test, checkpoint, wasm (M3)
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

M2 added:

```
crates/model/src/report.rs    Report > Page > Visual (roles of Projections by model id,
                              filters, sort, format objects), ReportFilter/FilterKind;
                              report Changes (Add/Remove/Set/MovePage, Add/Remove/Update
                              Visual, SetReportFilters). The report lives in Model.report
                              (one undo history); the Ossie mapping ignores it.
crates/dax                    lexer, error-tolerant Pratt parser, binder (names → ids,
                              supported-function checks with plain-English diagnostics)
crates/compiler/src/graph.rs  active many-to-one reachability, ambiguity detection
crates/compiler/src/visual.rs VisualQuery → SQL (see the module doc: one CTE per
                              aggregated table, null-safe key joins, DAX blank rules,
                              Top N via subquery, sort-by columns as hidden keys)
crates/compiler/src/ossie.rs  OSSIE_SQL_2026 generation (written on save)
crates/core/src/visualq.rs    Schema (physical types), report visual → query (roles →
                              groups/values, report+page+visual filters, default sort and
                              limits by type), field values, DAX checks, titles
crates/core/src/reportfile.rs report/report.json, pages/<id>/page.json, visuals/<id>.json
crates/core/examples/visualbench.rs   visual query timings on a project
crates/cli                    `charted query --visual <title|id>` (+ tests/query_visual.rs)
ui/src/report/                ReportView (toolbar, page tabs, shortcuts), Canvas
                              (Moveable + Selecto), VisualContainer, registry.ts
                              (capabilities: roles, format objects, sizes), data.ts
                              (per-visual query cache), visuals/ (Card, Cartesian via
                              ECharts, TableVisual), FieldsPane, VisualizationsPane (wells,
                              generated format pane, page settings), FiltersPane,
                              FormulaBar (+ quick measures), report.css
ui/src/state/report.ts        view state (page, selection, panes, formula bar, pointer)
                              and helpers that build Changes; `edit()` in state/app.ts
```

Measured (aarch64 VM, release): visual queries p95 17–43 ms on the 10M sample (a
distinct count by Product: 386 ms), at most 43 ms on 1M. Budgets: 800 / 200 ms.

M3 added:

```
crates/dax/src/bind.rs        full binder: Bound (scalar), TableExpr, FilterArg, VAR/RETURN,
                              SWITCH, IN, info functions, row contexts; bind_in for
                              calculated columns (row context on their table)
crates/dax/src/catalog.rs     signatures + descriptions of 129 functions (42 evaluated);
                              generates docs/dax-support.md (a test keeps it current)
crates/dax/src/ide.rs         completion, signature help, hover, formatter (keeps comments)
crates/wasm                   wasm-bindgen wrapper (UTF-16 offsets, JSON results); built by
                              bin/wasm into ui/src/dax/pkg (gitignored). 170 KB gz
crates/compiler/src/visual.rs filter context per aggregate (keys, filters, USERELATIONSHIP),
                              one CTE per context; frames for context transition; VALUES
                              sets; blank member; key domains for auto-exist; Both hops
                              counted once per base row; calculated_column() SQL
crates/compiler/src/interp.rs reference interpreter (test oracle)
crates/compiler/src/graph.rs  Both-direction hops (flagged `expands`), ambiguities()
crates/tmdl                   PBIP/TMDL writer: tables, measures, calculated columns,
                              relationships, M partitions from steps, SourceFolder parameter
crates/core/src/interop.rs    export PBIP / Ossie, import Ossie, model diff
crates/core/src/relcheck.rs   key counts + suggested cardinality for a relationship
crates/core/src/session.rs    commit: prepare (infer calculated column types), check_model
                              (no new ambiguous paths; calculated columns compile);
                              refresh_calculated() materialises `__calc_<id>` columns
                              (ALTER + UPDATE … FROM by rowid) on open/commit/undo/redo
crates/core/tests/dax_golden.rs  71 measures × 7 shapes golden snapshot, random
                              differential tests (CHARTED_DAX_CASES, CHARTED_DAX_SEED),
                              Both-direction differential test
ui/src/model/                 ModelView (pan/zoom canvas, cards, SVG relationships,
                              drag-to-relate, card drag → SetDiagram), layout.ts (elkjs
                              in its own worker, star arrangement), RelationshipDialog
ui/src/dax/                   DaxEditor (Monaco), monaco.ts (language, providers, theme
                              from tokens), service.ts + dax.worker.ts (WASM client)
ui/src/views/data/ColumnFormulaBar.tsx   calculated columns in the Data view
ui/src/app/FileMenu.tsx       File menu: export PBIP / Ossie, import Ossie with diff preview
ui/src/kit/Menu.tsx           menu button (react-aria)
```

M4 added:

```
crates/model/src/report.rs    Page.interactions (source → target: filter/highlight/none,
                              only where it differs from the default), Page::interaction,
                              Visual.selection (a slicer's choice, a FilterKind),
                              FilterKind::RelativeDate, Report.theme (Power BI theme JSON,
                              kept whole), Change::SetTheme
crates/compiler/src/visual.rs VisualQuery.keys (each group's value as text, `k0…`, as
                              filters compare it), relative_range() for relative dates
crates/core/src/visualq.rs    report_visual_sql(): slicer filters from the page (through its
                              interactions), `key:role:i` columns, highlight (`hl:role:i`:
                              the same query with the selection's filters, LEFT JOINed on
                              the groups, in one statement), matrix (one compiled query per
                              row × column level, UNION ALL with `level:rows`,
                              `level:columns`, `order`), slicer values or min/max,
                              `captions` names (CSV, CLI)
crates/core/src/reporttheme.rs  Power BI theme file checks
crates/core/src/api.rs        ReportVisual {highlight, tag}: a newer request with the same
                              tag interrupts the running one (DataJob::Tagged, `Running`;
                              the UI ignores code "cancelled"); ReadImage; ImportTheme,
                              ExportVisualCsv (written to `.part`, then renamed)
crates/core/tests/interactivity.rs  slicers + interactions, highlight, matrix subtotals,
                              relative dates, cancellation, themes, CSV
crates/cli/src/devserver.rs   data requests run on threads (like Tauri) and desktop theme
                              changes are pushed as `env` events (polled every 400 ms)
ui/src/state/selection.ts     the data selection (page, source visual, fields, tuples of
                              keys), Ctrl-click union, interaction defaults (mirror the
                              core), selection → filters or highlight per target
ui/src/report/visuals/        Cartesian (clustered/stacked column and bar, line; highlight
                              drawn as solid part + faded rest in one stack), Pie (pie and
                              donut), Scatter, Matrix (+ useExpansion view state), Slicer
                              (list, dropdown, between, relative date), Decor (text box,
                              image, shape), interact.ts
ui/src/report/themes.ts       built-in themes, Power BI theme JSON → page tokens
ui/src/report/VisualContainer.tsx  interaction picker (edit interactions), focus mode,
                              Show as a table (DataTable), Export data, matrix menu
ui/src/report/ReportView.tsx  Edit interactions toggle, Themes menu, FocusView
ui/e2e/m4.spec.ts
```

Measured (aarch64 VM, release, 10M rows): matrix Category/Subcategory × Year with all
subtotals 131 ms p50; a highlighted column chart 30 ms; a cross-filtered line 12 ms; a
Region slicer 94 ms (`cargo run --release -p charted-core --example visualbench`).

Report gains `diagram` (model view card positions; saved in report/report.json, kept by
an Ossie import). New Changes: `SetDiagram`, `ColumnProperty::Expression`. New
requests: `ExportPbip`, `ExportOssie`, `ImportOssie`; data requests `OssieDiff`,
`CheckRelationship`. CLI: `export pbip|ossie`, `import [--dry-run]`.

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
bin/wasm                  # builds the DAX language service into ui/src/dax/pkg (skips if current)
target/debug/charted open <project> --screenshot out.png   # WebKitGTK snapshot; delay via CHARTED_SCREENSHOT_DELAY_MS,
                          # CHARTED_SCREENSHOT_SCRIPT runs JS first (then CHARTED_SCREENSHOT_AFTER_MS)
target/debug/charted-headless export pbip|ossie <project> <out>; import <project> <file> [--dry-run]
CHARTED_DAX_CASES=3000 cargo test -p charted-core --test dax_golden random   # longer differential run
CHARTED_UPDATE_DOCS=1 cargo test -p charted-dax   # regenerate docs/dax-support.md
INSTA_UPDATE=always cargo test -p charted-core --test dax_golden   # then review the .snap diff
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
  `monaco-editor/editor/editor.worker?worker`. **The bare API has no editor features**:
  import `monaco-editor/features/<name>/register` for each (see ui/src/dax/monaco.ts),
  and the suggest widget needs `monaco-editor/editor/contrib/suggest/browser/suggestController`
  too (no feature module imports it). Create editors with `editContext: false`:
  WebKitGTK has no EditContext, and Chromium's EditContext input breaks Playwright `fill`.
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
  checker only extracts `t("…")`, `msg("…")` and `label: "…"`, and unknown catalog keys
  fail the check. Wrap undo labels in `msg()` to translate them.
- Strings shown through a variable (`t(role.label)`) must be marked where defined with
  `msg("…")` (ui/src/i18n) or `label: "…"`, or the extractor misses them.
- **Edits that depend on current state go through `useApp().edit(label, model => changes)`**,
  not `commit`. It queues behind earlier edits and builds from the latest model; with
  plain `commit`, ticking two fields quickly lost the first one.
- **Moveable must live inside the scaled `.page`** with `zoom={1 / scale}`, and needs
  `updateRect()` after positions change through the model. Outside the scaled element
  its offsets were wildly wrong; its `bounds` option misbehaves under the transform, so
  positions are clamped to the page when committed instead.
- Charts under the zoomed canvas render with `devicePixelRatio × scale` (remounted on
  zoom changes) to stay sharp.
- HTML5 drag and drop in Playwright: `locator.dragTo` works; drops land asynchronously
  (the commit round trip), so assert with auto-waiting expectations. Controlled
  checkboxes that commit to the core change state after a round trip: use `click()`,
  not `check()`.
- The command line cannot open a project the app has open (DuckDB's single-writer
  lock and charted's lock file). Tests that call the CLI close the `App` first.
- **elkjs:** don't import `elk.bundled.js` inside your own worker: it detects the worker
  scope and installs its own message handler. Use `elk-api.js` with
  `workerFactory: () => new Worker(elkWorkerUrl)` (`elk-worker.min.js?url`).
- **Pointer drags on canvases must `preventDefault` on pointerdown** (and the area needs
  `user-select: none`): otherwise a text selection left by an earlier drag turns the
  next drag into a native drag, which fires `pointercancel` and ends the pointer stream.
- wasm-bindgen-cli must match the `wasm-bindgen` crate version in Cargo.lock (0.2.129;
  `bin/setup` installs it). `bin/wasm` runs before vite (pnpm dev/build), tests and
  checkpoints; a fresh clone without it fails to import `ui/src/dax/pkg`.
- DAX semantics are pinned by the reference interpreter: change the compiler and the
  interpreter together, and run a long random differential run before committing.

- Playwright runs `target/debug/charted-headless` as built: after changing Rust, run
  `cargo build -p charted-cli` before running one spec on its own (`bin/test` builds it).
- e2e tests click chart marks through the chart itself: every ECharts container element
  (`.cartesian`) carries its instance as `.chart`, so a test can `convertToPixel` a data
  point (multiply by the canvas zoom: bounding width ÷ offsetWidth).
- e2e runs the dev server with `HOME=$CHARTED_E2E_TMP/home` holding a copy of the
  desktop's Omarchy theme, so a test can rewrite `colors.toml` and watch the UI follow.
- In a table or matrix, `.table-visual th button` styles apply to every button in a
  header cell; scope new buttons more specifically (`.table-visual th .matrix-toggle`).

- **Never let the process exit while a DuckDB query is running on another thread.**
  `exit()` runs libduckdb's static destructors, and a query still binding then calls
  into freed memory (SIGBUS/SIGSEGV in `TryGetMaxLogicalTypeInternal`, seen when a
  screenshot exited right after a click). The app calls `charted_core::api::shutdown`
  on `RunEvent::Exit`: no new data jobs, running ones interrupted and waited for. Any new
  binary that runs queries on threads must do the same before exiting.

## Deferred from M0 (do these when relevant)

- Visual query IR → SQL with many-to-one star joins (`crates/compiler`): start of M2.
- `tauri-plugin-single-instance` (one window per project; the lock file already prevents
  double editing).
- tauri-driver smoke test in CI; nightly jobs (Ossie schema drift, converter cross-check).
- Live rounding is verified manually only (watch.rs polls Hyprland's socket every 1.5 s).
- CI: green on the M0 push (Rust on x86_64 and arm64, UI + e2e). Check `gh run list
  -R Axis-Analytica/charted` after each push.

## Deferred from M1

- Find compares raw values (as text), not formatted text.
- Column order changes (drag) and freezing are not implemented; widths are view state
  and not saved with the project.
- Copy caps at 1,000,000 rows; formatted with the column's format string.
- Server-side query cancellation: stale page requests are dropped before sending, but a
  sent query runs to completion (DuckDB `interrupt_handle` exists if needed).

## Deferred from M2

- Visual-level data reduction beyond row limits (Power BI's top 60 series is applied
  client-side for the legend role; categories are capped by `LIMIT`).
- Keyboard reordering of chips in field wells (drag works; Enter/Space add fields).
- Copy/paste of visuals between pages; page reordering by drag (Alt+←/→ on a tab works).
- Display names for projections (`displayName` exists; no UI to rename yet).
- Measure properties beyond name/expression/format (description, folder, home table).
- Value filters are visual-level only; report/page filters take columns.
- Cross-filtering, highlighting, slicers, matrix and the other M4 visuals.

## Deferred from M3

- Calculated columns: row expressions with RELATED and scalar functions only (no
  aggregates, measures or CALCULATE; no EARLIER). Recomputed whole on change.
- FILTER over measures as a CALCULATE filter argument; many-to-many relationships do not
  filter; time intelligence functions (recognised, not evaluated).
- Measures that are never blank follow "rows with data" (Power BI shows all
  combinations for some).
- Relationship dialog: no "Assume referential integrity" or "Apply security filter in
  both directions" options yet.
- Model view: no keyboard moving of cards; relationships are editable from the list
  (Enter) and the diagram (double-click).
- PBIP export writes an empty report shell (pages only); visuals map to PBIR in M6.
- New measures get no format string (Power BI shows General too); undo labels are not
  translated (see the gotcha on commit labels).

## Deferred from M4

Deviations from the plan (better, keep them): highlights are the same compiled query
with the selection's filters added, LEFT JOINed to the full result on the groups, in
one statement, instead of `agg FILTER (WHERE sel)`, which only works for plain
aggregates, not for DAX measures with their own filter context. Matrix subtotals are one
compiled query per level stacked with UNION ALL, instead of GROUPING SETS, so every
level gets DAX semantics (a distinct count or a ratio is recomputed, never summed). The
checkpoint's "the line highlights": line charts are filtered, not highlighted, as in
Power BI.


- Selections across two fields from different tables (a stacked segment, Ctrl-clicked
  several times) filter by each field's values, so every combination of them is kept
  (Power BI uses the exact tuples). Exact for a single point and for one field.
- Slicers list a column's values without checking the facts (Power BI hides values with
  no data under the other slicers when they are on another table); range slicers do
  narrow. No hierarchy slicers, tiles, or "Sync slicers" across pages.
- The matrix shows leaf column members and the grand total column (no intermediate
  column subtotals); expansion is view state (not saved); no stepped/tabular layout
  switch, no conditional formatting, no drill up/down buttons.
- Highlights only on column/bar/pie/donut/scatter; line charts filter (as Power BI).
- Theme JSON: only dataColors, background, foreground and tableAccent are applied
  (`visualStyles`, text classes and the sentiment colours are kept but unused); no theme
  editor. PBIP export does not write the theme yet (M6).
- Text box is plain text (no rich text runs); images are stored as data URLs in the
  visual's objects (5 MB cap).
- Selection and expansion state is lost when switching pages (Power BI keeps them).

## Milestone 3: Real modeling (done; kept for reference)

From the plan: Model view (elkjs in a worker; drag to create relationships;
cardinality/cross-filter/active dialog; mark as date table); Monaco DAX editor powered
by WASM (crates/wasm wrapping crates/dax: completion, signatures, hover, diagnostics,
formatter); compiler: filter context and semi-join propagation, both directions where
set, ambiguity errors; CALCULATE/FILTER/ALL*/REMOVEFILTERS/KEEPFILTERS/VALUES/
SELECTEDVALUE/HASONEVALUE/ISINSCOPE/USERELATIONSHIP/RELATED, X iterators with context
transition, IF/SWITCH/VAR; one CTE per context-changing measure; explicit diagnostics
for unsupported functions linking a generated `docs/dax-support.md`; calculated
columns; Ossie export/import round trip; TMDL/PBIP export (CLI + File → Export).
The plan asks for ~150 DAX golden cases written before the compiler work and a
reference interpreter for differential tests.

Checkpoint: the star in the model view (ShipDate relationship dashed as inactive);
create the Targets relationship, set Both; autocomplete + error squiggle;
`USERELATIONSHIP` and `ALL` share measures in a table; a RELATED calculated column;
export PBIP and view the `.tmdl`; `export ossie → import` shows 0 differences.

## Milestone 4: Interactivity (done; kept for reference)

From the plan: cross-filter store (scope-identity tuples, Ctrl-click union, an
interaction matrix, cancellation of stale queries); highlight via
`agg FILTER (WHERE sel)` in the same query; edit-interactions mode; slicers
(list/dropdown/between/relative date); matrix with GROUPING SETS subtotals and
expand/collapse; pie/donut, stacked bar/column, scatter, text/image/shape; themes
(Omarchy live by default, Power BI theme JSON import, gallery); focus mode, show as
table, export CSV.

Checkpoint: click a column → the line highlights, the matrix filters, the card updates;
Ctrl-click; slicers; set an interaction to "none"; expand the matrix; switch themes,
including a live Omarchy switch; import a Power BI theme JSON.

## Milestone 5: Query editor (after the M4 review)

From the plan: Queries list, preview grid, applied-steps list (rename/drag-reorder/
delete/gear/insert), read-only SQL strip, Close & Apply with progress. About 22
transforms, each a typed step + SQL emitter + M emitter + dialog: promote headers / type
/ choose / remove / rename / filter / dedupe / blanks / replace / split / merge columns /
custom / conditional / group / unpivot / pivot / fill down / index / extract / merge
queries / append. Profiling bars (first 1,000 rows or the full table). Sources: Excel
sheet picker, JSON, folder combine, consented SQLite/Postgres with `keyring`
credentials. Source file watching with Refresh / Locate… (needs a `notify::PollWatcher`
fallback on `/mnt/mac`, see the gotchas). The sample's messy `stores_raw.csv` and
`targets.xlsx` are meant for this milestone.

Checkpoint: clean `stores_raw.csv` end to end; split + conditional column; merge with
Store; reorder/delete/undo steps; Close & Apply updates the report slicer; Get Data →
Excel.
