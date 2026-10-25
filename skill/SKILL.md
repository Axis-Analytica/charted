---
name: charted
description: Inspect and query charted business-intelligence projects (a local Power BI Desktop alternative for Omarchy). Use when asked about a charted project, its semantic model, measures or data, or to validate or query one from the command line.
---

# charted

charted projects are folders containing a `<Name>.charted` manifest, a semantic model
in `model.ossie.yaml` (an Apache Ossie document; measures are DAX), and `queries/*.json`
describing how each table is loaded. Loaded data is cached in `.cache/data.duckdb`.

Commands (all accept `--json`, exit 0 on success and 1 on failure, errors on stderr):

- `charted check <project>`: validate the model, queries and source files.
- `charted query <project> "<sql>" [--json|--csv] [--limit N]`: run DuckDB SQL against
  the loaded tables. Table names are the model tables' `source` values (lowercase).
- `charted sample create <dir> [--scale demo|1m|10m]`: create the sample project.
- `charted open [<project>]`: open the desktop app.

Edit `model.ossie.yaml` and `queries/*.json` directly when asked to change a model; the
app reloads them. Keep the Apache Ossie structure valid and run `charted check` after.
