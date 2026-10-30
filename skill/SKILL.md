---
name: charted
description: Inspect and query charted business-intelligence projects (a local Power BI Desktop alternative for Omarchy). Use when asked about a charted project, its semantic model, measures or data, or to validate or query one from the command line.
---

# charted

charted projects are folders containing a `<Name>.charted` manifest, a semantic model
in `model.ossie.yaml` (an Apache Ossie document; measures are DAX, each with a generated
`OSSIE_SQL_2026` equivalent), `queries/*.json` describing how each table is loaded, and
`report/` (pages and visuals, one JSON file each; fields are referenced by model id).
Loaded data is cached in `.cache/data.duckdb`. While the app has a project open, the
command line cannot open it (DuckDB allows one writer); close it in the app first.

Commands (all accept `--json`, exit 0 on success and 1 on failure, errors on stderr):

- `charted check <project>`: validate the model, queries and source files.
- `charted query <project> "<sql>" [--json|--csv] [--limit N]`: run DuckDB SQL against
  the loaded tables. Table names are the model tables' `source` values (lowercase).
- `charted query <project> --visual "<title or id>" [--json|--csv] [--show-sql]`: the rows
  a report visual shows, by field name, with its report, page and visual filters applied
  (measures evaluated with DAX semantics). An unknown title lists the visuals.
- `charted sample create <dir> [--scale demo|1m|10m]`: create the sample project.
- `charted open [<project>]`: open the desktop app.

To change a project that is open in the app, use the app's Agent pane (Ctrl+I in the
report view): it runs Claude Code or Codex with charted's own tools (served by
`charted mcp` over a socket in the running app), so changes show live and undo in one
step. With the project closed, edit `model.ossie.yaml` and `queries/*.json` directly;
the app reads them when it opens the project. Keep the Apache Ossie structure valid and
run `charted check` after.
