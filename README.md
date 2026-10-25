# charted

A lightweight, local-first Power BI Desktop alternative for [Omarchy](https://omarchy.org).

Load data, build a semantic model with relationships and DAX measures, explore it in a
fast data grid, and design interactive reports, all on your own machine. Models are
stored as [Apache Ossie](https://ossie.apache.org/) documents, so they can be exchanged
with other tools and converted to Power BI models.

> **Status:** early development. Milestone 0 (the application spine) is complete; see
> the [roadmap](#roadmap). Not yet ready for real work.

## What works today

- A native window for Omarchy that follows your theme, corner rounding, animations
  setting and language live.
- Projects as plain folders: the semantic model in `model.ossie.yaml`, load queries in
  `queries/`, data cached in DuckDB. Atomic saves, crash recovery, unlimited undo.
- The "Harbor & Pine Outfitters" sample: a deterministic retail star schema
  (`charted sample create <dir>`).
- Table previews over DuckDB with Power BI–style format strings.
- A command line for scripts and agents: `charted check`, `charted query`,
  `charted sample create`, `charted skill install`, all with `--json`.

## Development

```sh
bin/setup        # one-time: Rust, pnpm and UI dependencies via mise
bin/dev          # run the app with hot reload
bin/test         # full test suite (Rust, WebAssembly checks, TypeScript, i18n, end-to-end)
bin/checkpoint m0  # milestone review: screenshots plus a live demo (docs/demos/)
```

charted is a Tauri 2 app: a Rust core (DuckDB, the semantic model, the DAX compiler) and
a React UI rendered by WebKitGTK. See `docs/` for performance notes and demos.

## Roadmap

| Milestone | Scope | Status |
|---|---|---|
| M0 | Spine: shell, project format, Ossie I/O, sample data, Omarchy integration, CLI | Done |
| M1 | Data view: virtual grid, sort/filter, column properties, statistics | Done |
| M2 | First report: canvas, visuals, measures, filters | Done |
| M3 | Real modeling: relationships, DAX language service and compiler, Power BI export | Done |
| M4 | Interactivity: cross-filter/highlight, slicers, matrix, themes | In review |
| M5 | Query editor: applied steps, transforms, profiling, more sources | |
| M6 | Interop and release: time intelligence, drill, bookmarks, PBIP import, packaging | |

## License

Apache-2.0. See [LICENSE](LICENSE). The sample data is synthetic and released under CC0.
