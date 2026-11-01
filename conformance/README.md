# Conformance against Power BI Desktop

Charted's DAX is tested against its own reference interpreter
(`crates/core/tests/dax_golden.rs`). This folder checks both against Power BI itself.

| File | What it is | Made by |
| --- | --- | --- |
| `golden/golden.pbip` | The golden fixture as a Power BI project. Each table's data is *Enter data*, so it needs no files. | `CHARTED_UPDATE_CONFORMANCE=1 bin/heavy cargo test -p charted-core --test conformance` |
| `queries.json` | Every golden measure in every golden shape, as the DAX query a Power BI visual sends. | the same command |
| `expected.json` | Power BI Desktop's answers. | `run.ps1`, on Windows |
| `known.json` | Differences we know about: `{"<measure> / <shape>": "why"}`. | by hand; it should only shrink |

## Recording Power BI's answers (Windows)

1. Open `conformance\golden\golden.pbip` in Power BI Desktop and leave it open. It
   doesn't matter if the tables look empty.
2. From the repository folder, in Windows PowerShell:

   ```powershell
   powershell -ExecutionPolicy Bypass -File conformance\run.ps1
   ```

   It finds Desktop's local Analysis Services port, loads the data, runs about 2,000
   queries (a minute or two) and writes `conformance\expected.json`.
   - If the data won't load from the script, press **Refresh** in Desktop, then run it
     again with `-NoRefresh`.
   - With several Desktop windows open, pass `-Port <n>`. The port is in
     `msmdsrv.port.txt` under `%LOCALAPPDATA%\Microsoft\Power BI Desktop\AnalysisServicesWorkspaces`.
   - It uses Desktop's own ADOMD.NET client. With the Microsoft Store edition, it
     downloads the client from nuget.org instead.
3. Copy `expected.json` back and run
   `bin/heavy cargo test -p charted-core --test conformance -- --nocapture`.

## When things differ

- **Power BI disagrees with the reference interpreter:** fix the interpreter first (it
  is the oracle for the random tests), then the SQL compiler, until the golden tests
  pass again.
- **A difference that has to wait:** add it to `known.json` with the reason.
- **After a measure or the fixture changes:** regenerate the files. The test then
  reports the changed queries as needing a re-run on Windows, rather than failing them.
