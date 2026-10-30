# charted and Power BI: what comes across

charted reads and writes Power BI projects (PBIP), so you can bring existing work in and
take it back out. What it can't use is never dropped silently: an import ends with an
**import report** listing each thing that didn't come across and why. You can see the
report again from the banner in the app, or on the command line with
`charted import-pbip <file> --json`.

Found something that should come across and doesn't? Please
[open an import issue](https://github.com/Axis-Analytica/charted/issues/new?template=import.yml).
Attach the import report, and the project if you can share it.

## Importing (Power BI → charted)

**Formats.** A `.pbip` folder with its semantic model in TMDL (a `definition/` folder)
or TMSL (`model.bim`), and its report in PBIR or the older `report.json`. You can also
import a `model.bim` on its own. Use *File → Import Power BI project…* in the app, or
`charted import-pbip <file>`.

| Comes across | Notes |
|---|---|
| Tables, columns (types, formats, summarisation, sort by, data categories, display folders, hidden) | |
| Measures and their DAX | Measures that use DAX charted doesn't evaluate yet still come across; the report lists them ([what's supported](dax-support.md)). |
| Calculated columns and tables | |
| Relationships (cardinality, cross-filter direction, active or inactive) | |
| Hierarchies | |
| Roles with DAX row filters | |
| Calculation groups, field parameters and what-if parameters | |
| Power Query that loads files: CSV, Excel, Parquet and JSON, on disk or on the web | Covers promoted headers, type changes, removed, selected and renamed columns, skipped and kept rows, and distinct rows. A file that has moved is reported; locate it in *Transform data*. Web files load when you *Refresh*, after asking. |
| Report pages and visuals of the types charted has, with their fields, position, sort, title and basic filters | |

| Doesn't come across yet | What happens |
|---|---|
| Other M (databases, APIs, custom functions, steps charted doesn't know) | The table comes in with its columns but no data. The report names the M function. |
| Visual types charted doesn't have (custom visuals, R and Python visuals, maps, and others) | Left out; the report lists each one by page. |
| Fields a visual uses that aren't in the model, or have no matching role in charted | Left out of that visual; the report counts them. |
| Dynamic format strings | The measure keeps its static format. |
| Perspectives and translations | Left out. |
| DirectQuery, composite models and live connections | charted is import-only (DuckDB). |

## Exporting (charted → Power BI)

Use *File → Export Power BI project…*, or `charted export pbip`. This writes a PBIP that
Power BI Desktop opens:

- **The model as TMDL:** tables, columns, measures, relationships, hierarchies, roles and
  calculation groups, with an M partition per table generated from its load steps.
- **The report as PBIR:** written to Microsoft's published schemas, which the tests
  check.

What PBIR has no place for is carried in annotations, so importing charted's own export
brings everything back exactly. That covers advanced and Top N filters, slicer choices,
drill state, bookmark states and charted's own formatting. Power BI ignores those
annotations, so those parts don't show there.

Optionally the export copies the data files and sets the folder Power BI reads them from,
for opening it on another machine.

**Not verified yet:** opening charted's exports in Power BI Desktop itself. The export
is checked against Microsoft's schemas and round-trips through charted, but the Desktop
check is still open (see docs/PLAN.md).

## DAX

charted evaluates DAX with its own engine. Every supported function is tested case by
case against a reference implementation. [dax-support.md](dax-support.md) lists what's
supported and what isn't.
