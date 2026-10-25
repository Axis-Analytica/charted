# charted

A lightweight, local-first Power BI Desktop alternative for [Omarchy](https://omarchy.org).

Load data, build a semantic model with relationships and DAX measures, explore it in a
fast data grid, and design interactive reports, all on your own machine. Models are
stored as [Apache Ossie](https://ossie.apache.org/) documents and export to Power BI
projects (PBIP/TMDL), so your work is never locked in.

> **Status:** in active development. Milestones 0–5 are built: you can get and shape
> data, model it, write DAX and build interactive reports. Still to come are Power BI
> import, time intelligence, packaging and a 1.0 release (M6); see the
> [roadmap](#roadmap). There is no packaged release yet.

## What works today

**The app**
- A native window for Omarchy that follows your theme, corner rounding, animations
  setting and language live (English, Danish and Arabic so far, right-to-left included).
- Projects as plain folders: the model in `model.ossie.yaml`, load queries in `queries/`,
  the report in `report/` (one small JSON file per page and visual, so changes diff
  well in git), data cached in DuckDB. Atomic saves, crash recovery, unlimited undo.
- No telemetry and no network access, except opt-in downloads of database connectors.

**Getting and shaping data**
- A query editor in the spirit of Power Query: applied steps you can select, rename,
  reorder, edit and delete, with each step's SQL and its Power Query M side by side.
- About 25 transforms: remove or keep rows, promote headers, change types, filter,
  replace values, split and merge columns, custom and conditional columns, group by,
  pivot and unpivot, fill down, merge and append queries, and more.
- Column profiles (quality and distribution) over the first 1,000 rows or all of them.
- Sources: CSV, Excel (with a sheet navigator), JSON, Parquet, a folder of files, and
  SQLite and PostgreSQL (their connectors are downloaded only after you agree;
  passwords stay in your desktop keyring). charted notices when a source file changes.

**Data view**
- A virtual grid that scrolls 10 million rows at 60 fps: sort, filter, find, copy.
- Column properties (type, format string, summarisation, sort-by column, data category)
  and column statistics.
- Calculated columns written in DAX, with `RELATED`.

**Model view**
- The tables laid out as a star, with relationship lines showing cardinality, filter
  direction and inactive relationships.
- Drag one column onto another to relate them; charted counts the keys and suggests the
  cardinality.
- Both-direction filtering, refused (as Power BI does) where it would create two paths.
- Mark a table as a date table.

**DAX**
- A Monaco editor with autocomplete, parameter hints, hover help, errors as you type and
  a formatter, running in WebAssembly.
- A compiler from DAX to DuckDB SQL covering filter context, `CALCULATE` and its filter
  functions, iterators with context transition, `USERELATIONSHIP`, variables and
  `SWITCH`. It is checked against an independent reference interpreter with hundreds of
  fixed cases and thousands of random ones. [docs/dax-support.md](docs/dax-support.md)
  lists what is supported.

**Reports**
- A page canvas with snapping, alignment, multiple pages, and report, page and visual
  filters (basic, advanced, top N).
- Visuals: card, table, matrix (with subtotals), clustered and stacked column and bar,
  line, pie, donut, scatter/bubble, slicer (list, dropdown, range, relative date), text
  box, image and shape.
- Cross-filtering and highlighting between visuals, Ctrl-click to add to a selection, and
  per-visual interaction settings (filter, highlight or none).
- Focus mode, show as a table, and export a visual's data to CSV.
- Themes: follow the Omarchy desktop theme live, choose a built-in theme, or import a
  Power BI theme file.

**Interop and scripting**
- Export to a Power BI project (PBIP with TMDL: tables, measures, calculated columns,
  relationships, Power Query partitions), and export or import Apache Ossie with a diff
  preview.
- A command line for scripts and agents, all with `--json`: `charted check`,
  `charted query` (including `--visual`), `charted export pbip|ossie`, `charted import`,
  `charted sample create` and `charted skill install`.
- The "Harbor & Pine Outfitters" sample: a deterministic retail star schema at any scale
  up to 10 million rows (`charted sample create <dir> [--scale 10m]`).

## Development

```sh
bin/setup          # one-time: Rust, pnpm, wasm-bindgen and UI dependencies via mise
bin/dev            # run the app with hot reload
bin/test           # full suite: Rust, DAX differential tests, WebAssembly, TypeScript, i18n, end-to-end
bin/checkpoint m4  # milestone review: screenshots plus a live demo (docs/demos/)
```

charted is a Tauri 2 app: a Rust core (DuckDB, the semantic model, the DAX compiler) and
a React UI rendered by WebKitGTK. See `docs/` for the milestone demos, performance notes
and the DAX support list, and `docs/HANDOFF.md` for the architecture.

## Roadmap

| Milestone | Scope | Status |
|---|---|---|
| M0 | Spine: shell, project format, Ossie I/O, sample data, Omarchy integration, CLI | Done |
| M1 | Data view: virtual grid, sort/filter, column properties, statistics | Done |
| M2 | First report: canvas, visuals, measures, filters | Done |
| M3 | Real modeling: relationships, DAX language service and compiler, Power BI export | Done |
| M4 | Interactivity: cross-filter/highlight, slicers, matrix, themes | Done |
| M5 | Query editor: applied steps, transforms, profiling, more sources | In review |
| M6 | Interop and release: time intelligence, drill, bookmarks, PBIP import, packaging | |

## License

Apache-2.0. See [LICENSE](LICENSE). The sample data is synthetic and released under CC0.
