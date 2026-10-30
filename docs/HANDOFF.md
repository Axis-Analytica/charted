# Handoff: v1.0 candidate pushed; next session starts v1.1–v1.5

Read this first when picking up development. Last updated 2026-10-26 (end of the
pre-release session). Then read docs/ROADMAP.md, which holds the scope of every
release below.

## Where things stand

*Updated 2026-10-30.*

- **Code:** v1.0 (M0–M6 and the pre-release batch) was pushed at `332c6bb`. Since then
  `main` has about 55 local commits, **not pushed**, carrying most of v1.1–v1.5. The
  plan is `~/.claude/plans/sparkling-scribbling-canyon.md` (Tracks A–E).
  - **v1.4.0 is released** (2026-10-30; the first public release, tag `v1.4.0`,
    https://github.com/Axis-Analytica/charted/releases/tag/v1.4.0), with x86_64 and
    aarch64 tarballs; install with `pkgbuild/PKGBUILD-bin` (see README). The user
    skipped the VM checks for it. Not done: AUR (no AUR key on this machine) and the
    omarchy-pkgs PR; each needs the user's go-ahead, as does pushing. CI is green.
  - `bin/test` is green on `main` with everything merged (2026-10-30).
- **Done:**
  - Track A (DAX), steps 0–11: RowSet, ALLSELECTED, running totals, iterated date
    sets, RANKX, TOPN, CONCATENATEX, CALCULATETABLE, RELATEDTABLE, LOOKUPVALUE,
    ADDCOLUMNS/SELECTCOLUMNS/SUMMARIZE/SUMMARIZECOLUMNS, CALENDAR/GENERATESERIES,
    WEEKDAY, MOD, table variables, the calculated-item dependency graph
    (`crates/core/src/calcgraph.rs`), and calculated tables. Calculated tables work in
    the model, Ossie, TMDL/model.bim and the UI (*New table* in the Fields pane opens
    the formula bar). v1.5 aggregating calculated columns are also done.
  - Track B (v1.2 visuals) and Track C (v1.3 report building).
  - Track D: hierarchies, groups and bins, display folders, Power Query references
    and *Enter data*.
  - Track E (v1.4): roles with DAX row filters, *Manage roles*, *View as* (every
    report query carries the role filters), the theme editor, what-if parameters and
    field parameters (*New parameter…* in the Fields pane;
    `crates/core/src/fieldparams.rs` expands a visual's roles to the fields picked, and
    the snapshot's `expandedRoles` carries them to the UI), and calculation groups
    (*New calculation group…*; `Table.calculationGroup` beside a derived calculated
    table of Name/Ordinal rows; `crates/core/src/calcgroups.rs` rewrites a visual's
    measures to `SWITCH ( SELECTEDVALUE ( group[Name] ), item, … )` when the group is on
    an axis or one item is filtered; `charted_dax::calcitem` substitutes
    SELECTEDMEASURE and its kin). DAX table constructors `{ ( … ) }` and NAMEOF came
    with them. Demo script: `docs/demos/v1.4.md`.
- **Open, in order:**
  1. Release checks: the corpus rerun (baseline Contoso 7/7, sales-sample 32/33,
     AurumJet 65/96; `bin/corpus && bin/heavy cargo test -p charted-core --test corpus
     -- --ignored --nocapture`), checkpoints and demo notes per release, the 3000-case DAX
     golden run (`CHARTED_DAX_CASES=3000 bin/heavy cargo test -p charted-core --test
     dax_golden random`), and the Excel/Folder M check in the user's Windows VM. Every
     DAX feature gets golden cases; regenerate `docs/dax-support.md` with
     `CHARTED_UPDATE_DOCS=1 bin/heavy cargo test -p charted-dax`.
  2. Next milestone: M7, the in-app agent (design in docs/ROADMAP.md). Later: the
     Power BI Desktop check in the VM (`~/Work/charted-checkpoints/pbi-roundtrip/`),
     AUR and omarchy-pkgs on the go-ahead.
- **Step 12 (v1.5 speed) measured, rewrites not needed yet:** `calcbench` on the 10M
  sample puts a 20,000-row EARLIER rank at 564 ms (budget: 10,000 rows in 2 s); see
  docs/perf.md. Window-function rewrites for inequalities are the follow-up if tables of
  100,000+ rows need ranks.
- **Worktrees:** `.claude/worktrees/agent-*` (a035c, a0df, a8147) are all merged into
  `main`. They can be removed (`git worktree remove`); each holds its own `target/`.
- **e2e specs share one sample project.** A spec that changes it must undo the change,
  or later specs must avoid what it touches: v11pq renames North America to Americas
  in Store, so v14roles filters Asia Pacific.

## Session after: plan M7 and beyond

M7 is an agent inside charted that builds report pages. Its first design is in
docs/ROADMAP.md: a control socket in the running app, `charted mcp` as an MCP server,
and an Agent pane that runs the user's default Omarchy agent headless, with
undoable "Agent: …" commits and privacy defaults.

- The default agent comes from `omarchy-default-agent`; on this machine it's `claude`.
  `/usr/share/omarchy/bin/omarchy-agent` shows the per-agent flags.
- The user sees M7 as the feature that beats Power BI. Microsoft's Copilot needs
  capacity, and its MCP server can't touch report pages.
- Plan it fully then, including what comes after M7.

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
crates/core/examples/calcbench.rs     calculated columns that read other rows (EARLIER ranks)
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

M5 added:

```
crates/compiler/src/steps.rs  Query {source, steps, step_names, no_load}; Source: Csv, Parquet,
                              Json, Excel {sheet}, Folder {pattern}, Sqlite, Postgres; 25 Steps,
                              each with SQL (one CTE per step) and M (m_steps / to_m, also
                              used by the PBIP export); Ctx {project_dir, queries, sheets};
                              merges/appends inline other queries (cycles are errors)
crates/compiler/src/mformula.rs  custom column formulas: an M subset parsed to SQL and M
crates/core/src/sources.rs    Excel → cached CSV (calamine; dates by hand), extension
                              consent (state_dir/extensions.json; code "consent"), Postgres
                              ATTACH with a secret from the keyring (code "password"),
                              source stamps (cache/sources.json) and status
crates/core/src/queryedit.rs  preview (Arrow), info (columns, SQL, M), profile (quality,
                              distribution, type-change errors), analyses (first row, split
                              parts, distinct values, detect types)
crates/core/src/session.rs    open_with (consent), prepared(), apply_queries (staging tables,
                              swap, model_sync, History::forget), refresh, source_status
crates/samplegen              data/stores_raw.csv (messy) and data/targets.xlsx (sheets per year)
crates/core/tests/queries.rs  every step, the cleaning flow, Excel, merge, apply, refresh
ui/src/query/                 QueryEditor (ribbon menus, queries, SQL/M strip, preview, steps),
                              transforms.ts (every transform: fields + build; also step names),
                              StepDialog, PreviewTable (profiles), NewSource (Get Data, Excel
                              navigator, consent and password dialogs), SourceBanner (polls
                              SourceStatus every 3 s: Refresh / Locate…), state.ts (draft +
                              its own undo), data.ts
ui/e2e/m5.spec.ts             (fails on React render loops)
```

New requests: `ApplyQueries`, `Refresh`, `AllowExtension`, `SetPassword`; data requests
`QueryPreview`, `QueryInfo`, `QueryProfile`, `QueryAnalyze`, `ExcelSheets`,
`SourceStatus`. `ApiError` gained `detail` (the extension or connection a fix needs).
The snapshot's `queries[].definition` is the full query. View "query" (Ctrl+4).

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

M6 added:

```
crates/dax/src/bind.rs        TableExpr::Dates {table, column, op: DateOp} (Shift, Parallel, Previous,
                              Next, ToDate with a fiscal year end); TOTALxTD bind to CALCULATE;
                              FIRSTDATE/LASTDATE as MIN/MAX; YEAR/MONTH/DAY/QUARTER scalars
crates/compiler/src/visual.rs date_set: a value-set CTE of dates per outer key (joined to the
                              column's own dates); on a date table (marked, or joined on the
                              column) CALCULATE clears the table's other filters and keys
crates/compiler/src/interp.rs the same in the reference interpreter (mod cal: civil-day maths)
crates/model/src/report.rs    Visual.drill: Vec<DrillStep> (Into{keys}|Next|Expand), drill_role(),
                              Visual::drill_levels(); PageSettings.drillthrough + keep_all_filters;
                              Report.bookmarks: Vec<Bookmark{page, filters, visuals: VisualState}>;
                              Change::SetBookmarks
crates/core/src/visualq.rs    drilled hierarchies show levels start..=depth, filtered to members
crates/core/src/session.rs    create_date_table (Source::Calendar query, marked, sorted, related)
crates/tmdl/src/read.rs       TMDL tree parser + model.bim (TMSL) → Model, queries, Skipped list
crates/tmdl/src/mquery.rs     M recogniser: Csv/Excel/Parquet/Json + PromoteHeaders, TransformColumnTypes,
                              Remove/Select/RenameColumns, Skip, FirstN, Distinct; else reported
crates/tmdl/src/pbir.rs       PBIR export (schemas vendored in crates/tmdl/schema/pbir, validated in
                              tests) and import of PBIR and legacy report.json; chartedVisual /
                              chartedPage / chartedReport annotations make charted's exports exact
crates/core/src/pbimport.rs   import(): locate .pbip/.bim, read, rebase SourceFolder paths, find data
                              files next to the project, load, empty tables for the rest, DAX check
crates/core/src/bundle.rs     .chartedz zip (no .cache); OpenProject unpacks bundles, imports .pbip
crates/core/src/crash.rs      panic hook → crash-<t>.log; AppState.crash; prefilled issue URL
crates/core/tests/fixtures/Contoso   a Power BI Desktop-style PBIP with things charted skips
ui/src/state/drill.ts, drillthrough.ts; ui/src/report/BookmarksPane.tsx, VisualBoundary.tsx
ui/src/model/DateTableDialog.tsx; ui/src/app/ImportReport.tsx, CrashNotice.tsx, regions.ts (F6)
ui/src/dax/LazyDaxEditor.tsx  Monaco and the Data/Model/Query views load lazily (473 KB initial JS)
bin/build, bin/install, bin/release; pkgbuild/{PKGBUILD,PKGBUILD-bin,local/PKGBUILD,charted.desktop,
charted.xml,icons}; .github/workflows/release.yml (tag v* → draft release with tarballs)
```

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

- **Run builds and tests through `bin/heavy`.** On this 8 GB machine systemd-oomd kills the
  largest app.slice scope once RAM and swap both pass 90%. A cargo or Playwright run
  started from a terminal lives in that terminal's scope, so oomd killed the whole terminal
  (every Claude session in it) four times on 2026-10-27/29. `bin/heavy CMD` runs CMD in its
  own scope (MemoryHigh 3G, MemoryMax 4G, swap 2G; `CHARTED_HEAVY_HIGH/MAX/SWAP`), queued
  behind a machine-wide lock. bin/test, build, checkpoint, screenshots and wasm wrap
  themselves; a full `bin/test` peaks at about 3.1 GB plus 0.8 GB swap (7.5 min). Agents in
  worktrees use `/mnt/mac/charted/bin/heavy`. A local Claude hook blocks unwrapped runs.

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
- **The CLI binary is `target/debug/charted-headless`**; `target/debug/charted` is the
  GUI app (stale unless `cargo build -p charted`). Playwright starts charted-headless.
- **There is no formatter for the UI.** Its code is hand-wrapped at about 200 columns;
  don't run prettier on it (its defaults rewrap whole files). Rust uses `cargo fmt`.
- **Hyprland 0.56 has a Lua config:** `hyprctl dispatch` takes Lua (`hl.dispatch(...)`),
  so old-style dispatch strings fail. README screenshots are therefore taken in
  Chromium at 1600×1000 (`bin/screenshots`), not by resizing the real window.
- **CSV detection** (`sources::detect_csv`, stored in `Ctx::sheets` under `csv_key`):
  - UTF-16 is found by its BOM, Western European by invalid UTF-8.
  - The decimal comma is detected only in semicolon files with no decimal points.
  - Ragged bodies get `null_padding` and `parallel = false`. Padding every file breaks
    DuckDB's title-row skipping, and it can't combine with quoted line breaks when
    parallel.
  - Whole-file typing (`sample_size = -1`) is only a retry after a conversion error,
    because it is 20× slower on large files.
- DuckDB rounds `'2.5'::BIGINT` to 3: type detection checks a value is integral
  before calling it whole.
- **Web files** (`https://` paths) need DuckDB's httpfs extension, behind the same
  consent as sqlite/postgres. Import doesn't load them; the first Refresh asks.
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

- **A React hook keyed on an object built during render loops forever** if the object
  gets a fresh random id each render (the Excel navigator made its preview query with
  `newQuery()`): build such objects with `useMemo` and stable ids. m5.spec fails on
  "Maximum update depth exceeded".
- The snapshot's `path` is the project's `.charted` file, not its folder (`projectDir()`
  in ui/src/query/NewSource.tsx).
- DuckDB 2.0 rejects `x -> f(x)` lambdas: write `lambda x: f(x)`. `COLUMNS(*)` is not
  allowed in a window's PARTITION BY (remove duplicates uses `GROUP BY ALL` instead), and
  in WHERE it expands with AND (blank rows use `concat_ws('', *COLUMNS(*))`). `/` is
  float division; use `//` for integers.
- keyring (Secret Service) links libdbus: `libdbus-1-dev` on CI; `bin/setup` machines have it.
- Row order: steps that depend on it (remove top rows, promote headers, fill down, index,
  remove duplicates) number rows with `row_number() OVER ()`, which follows file order.

- (M6) e2e runs `target/debug/charted-headless serve`: rebuild it (`cargo build -p
  charted-cli`) after adding a Request variant, or the UI gets "unknown variant".
- (M6) Release builds need `--features custom-protocol` (bin/build does it), or the
  binary still loads the UI from the Vite dev URL. `charted-cli` is a dependency without
  default features there, so release builds have no WebSocket server.
- (M6) makepkg can't set permissions on /mnt/mac: bin/install builds in a temp dir
  (BUILDDIR/PKGDEST). Installing asks for the user's sudo password: they run it.
- (M6) The jsonschema crate can't resolve `#/definitions/DecomposedTree<…>` in the
  bookmark schema; the PBIR test renames that definition when loading.
- (M6) A zustand selector must return stable references (drillFilters returns a shared
  empty array), or React re-renders forever.

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

## Deferred from M5

- Type-change errors are counted only when the selected step is the type change; cells
  that didn't convert show as null (Power Query shows Error cells and has Keep/Remove
  errors). No "Detect data type" button (only on a new CSV), no locale for types.
- No query groups, parameters, or "Reference" (duplicate copies the steps); no fuzzy
  merge; Merge expands chosen columns directly (no nested-table column).
- SQLite tables are typed in, not listed; PostgreSQL isn't tested against a server
  (SQL, M and the consent/keyring flow are). Folder combines CSV only.
- The M for Excel and Folder sources is written to work in Power BI but is unverified
  there; SQLite exports through ODBC.
- Close & Apply shows one progress message, not per-table progress. Apply clears the
  undo history (the data can't be un-loaded), like Power BI.
- Source watching polls every 3 s while a project is open (no inotify on /mnt/mac).

## Deferred from M6

- Time intelligence: OPENING/CLOSINGBALANCE,
  STARTOF…/ENDOF…, SAMEPERIOD on DateTime columns with times; date sets only as
  CALCULATE filters (not iterated). PREVIOUSYEAR takes no year-end argument.
- Drill: charts' axis and pie legend only (the matrix keeps expand/collapse); drill state
  is saved with the visual and undoable (Power BI doesn't undo it). No cross-report
  drill-through; drill-through filters are view state.
- Bookmarks keep data state only (no visibility/spotlight, selection or "current page"
  options); showing one is an undoable edit.
- Import: perspectives, translations, measures' dynamic format strings and M beyond
  the file shapes in mquery.rs are reported, not imported. Calculation items apply to
  the measures a visual shows, not to measures those use (Power BI applies them to
  every measure reference); an item's format string applies only when it is the one
  item picked for the visual. Visual formatting from Power BI is
  limited to titles and text boxes; PBIR export writes `themeCollection: {}` unless the
  report has a custom theme. Power BI Desktop itself: the round-trip folder is ready
  (`~/Work/charted-checkpoints/pbi-roundtrip/`), awaiting the user's VM check.

## Deferred from the pre-release batch

- **Quick measures:** no running total. `FILTER(ALLSELECTED(col), col <= MAX(col))`
  as a CALCULATE filter isn't compiled yet (v1.1).
- **Power Query:**
  - Not yet: reference queries (`Source = #"Other query"`), *Enter data* tables
    (`#table`, compressed JSON), and M beyond the file shapes.
  - Excel from the web isn't read.
  - A CSV's decimal comma goes to Power BI as the `"de-DE"` culture on Changed Type,
    but isn't read back from M on import.
- **The .NET TOM check** (`TmdlSerializer`) on exports was blocked: installing the
  .NET SDK from a downloaded script needs the user's permission. Power BI Desktop in
  the VM is the check instead.
- Idle memory: 234 MB private in the charted process (DuckDB's cache) plus WebKit's own
  process; see docs/perf.md.

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

## Milestone 5: Query editor (done; kept for reference)

Plan: queries list, preview, applied steps (rename/reorder/delete/gear/insert), read-only
SQL strip, Close & Apply; ~22 transforms each with SQL + M + dialog; profiling bars;
Excel sheet picker, JSON, folder combine, consented SQLite/Postgres with keyring
credentials; source watching with Refresh / Locate…. Checkpoint: clean stores_raw.csv;
split + conditional column; merge with Store; reorder/delete/undo steps; Close & Apply
updates the report slicer; Get Data → Excel.

## Milestone 6: Interop, polish, release v1.0.0 (built; kept for reference)

From the plan: time intelligence (TOTALYTD, DATESYTD, SAMEPERIODLASTYEAR, DATEADD,
PARALLELPERIOD, a date table wizard); drill-down and drill-through; bookmarks;
PBIP/TMDL/model.bim/Ossie import with a skip report; full PBIR export (visuals, themes);
a `.chartedz` bundle; packaging (PKGBUILD, mime type, translated `.desktop`, icon,
`bin/install` with `makepkg -fsi` so it appears in Super+Space; README hero screenshot;
tag v1.0.0, then omarchy-pkgs PR and AUR `charted`/`charted-bin`).

Checkpoint: launch from the launcher; a Trends page with LY/YTD; drill and drill-through;
switch bookmarks with the keyboard; import a PBIP fixture and see the import report;
export → re-import with no differences; open a bundle.

The plan's quality bars apply at release (startup under 1 s, cross-filter under 250 ms,
idle memory under 300 MB, initial JS under 1.2 MB gzipped, F6 region cycling, per-visual
error boundaries, crash logs in `$XDG_STATE_HOME/charted/`): measure them in M6.
