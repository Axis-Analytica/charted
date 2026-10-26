# Roadmap after v1.0

What's next, in order. Each release ends like the milestones did, with a checkpoint
(`bin/checkpoint`) and a demo script. The order comes from three places: what analysts
use most in Power BI Desktop, what real public Power BI projects needed when imported
(`bin/corpus`, `crates/core/tests/corpus.rs`), and the commonest complaints about
Power BI.

## What importing real projects showed

The corpus is seven public PBIP repositories. Every project imports, its pages and
visuals come across, and charted's export of it imports again. The import reports point
at what to build next:

- **DAX:**
  - The commonest missing functions were SUBSTITUTE, FORMAT, DATE, EOMONTH, DATEDIFF,
    DATESINPERIOD and DATESBETWEEN. They're now evaluated, with other text functions.
  - Still missing: LOOKUPVALUE, CONCATENATEX, ADDCOLUMNS, CALCULATETABLE, TIME, and
    date sets iterated by AVERAGEX and similar.
- **Power Query:**
  - Queries that start from another query (`Source = #"RAW-Customer"`, Power BI's
    *Reference*).
  - Tables typed in with *Enter data* (`#table`, or compressed JSON).
  - Calendars built with `List.Dates` and `each` columns.
  - The others came in once files on the web were supported.
- **Model:** hierarchies, calculated tables, calculation groups, field parameters and
  row-level security roles appear in most projects.
- **Visuals:** the new card and the button slicer now map. Action buttons, KPIs, maps
  and SVG image measures don't yet.

## v1.1: modelling and DAX

- **DAX patterns:**
  - Running totals: `FILTER(ALLSELECTED(col), col <= MAX(col))` as a CALCULATE
    filter.
  - RANKX and TOPN.
  - Iterating date sets (`AVERAGEX(DATESINPERIOD(…), …)`), and DATESINPERIOD ends
    computed from other expressions (`EOMONTH(MAX(…), -1)`, `TODAY()`).
  - SQL and the reference interpreter disagree on the blank-member row for measures
    that are never blank (`UPPER(SELECTEDVALUE(…))`). Under a Both relationship, SQL is
    right.
  - LOOKUPVALUE, CONCATENATEX, ADDCOLUMNS, SUMMARIZECOLUMNS and CALCULATETABLE.
- **Query editor:**
  - Reference queries and *Enter data* tables, with the M to match.
  - A Calendar from `List.Dates`, recognised on import.
- **Field list:**
  - Sort by column chosen from the Fields pane.
  - Hierarchies: create, drill with them, import and export.
  - Display folders.
  - A measure's description and format string edited in the Fields pane.
- **Groups and bins.**
- **Calculated tables** (`SUMMARIZECOLUMNS`, `ADDCOLUMNS`, `CALENDAR`).

## v1.2: visuals

- **New chart types:**
  - Line and clustered column combo.
  - Waterfall, treemap, gauge, KPI and funnel.
  - 100% stacked bar and column.
  - The multi-row / new card.
- **Filtering:** a Top N filter.
- **Conditional formatting:** background, font colour, data bars and icons, from
  rules or a field's value.
- **Report-page tooltips.**
- **Small multiples.**
- **Analytics lines:** constant, average and trend.

## v1.3: report building

- **Formatting:**
  - A format painter that applies to several visuals at once.
  - Editing several selected visuals' shared options together.
- **Layout and pages:**
  - A selection pane (layers, visibility, tab order).
  - Copy and paste visuals between pages.
  - Sync slicers across pages.
  - Buttons and a page navigator.
- **Export to PDF.**

## v1.4: interop depth

- **Model features:** field parameters, calculation groups, roles with *View as*, and
  what-if parameters.
- **Themes:** a theme editor with a live preview, plus all of a theme file's visual
  styles.
- **Round trips:** Excel and Folder queries verified in Power BI Desktop, and CSV
  culture (`"de-DE"`) read on import.

## v1.5: aggregations in calculated columns

Calculated columns today are row expressions: arithmetic, scalar functions and
`RELATED`. v1.5 lets them aggregate, as in Power BI:

- **Aggregates over related rows:**
  - `CALCULATE ( SUM ( Sales[Amount] ) )` in a Customer column: context transition
    turns the row into a filter, so the column holds each customer's total sales.
  - `SUMX`, `COUNTROWS` and `RELATEDTABLE` over the rows related to this one.
- **Aggregates over the whole table:**
  - `SUM ( Sales[Amount] )` gives the grand total on every row.
  - `ALL`/`ALLEXCEPT` patterns, such as each row's share of its category.
- **Earlier-row patterns:** `EARLIER` and variables, for row ranks and running values
  within a table.
- **Measures in columns:** they evaluate per row with context transition.
- **How it's built:**
  - Computed once on load and refresh, as set-based DuckDB SQL (window functions and
    grouped joins), not row by row.
  - Checked against the reference interpreter like measures are.

## Better than Power BI

These are the things people complain about most in Power BI Desktop, and where charted
stands on each.

**Already in charted:**

- **Linux and offline.** Power BI Desktop is Windows-only; charted runs on Linux and
  sends nothing anywhere.
- **Search.** The visual gallery, slicers, fields and the command palette are all
  searchable.
- **Slicer search that behaves.** It's on by default, *Select all* selects the
  results, and it searches past the values listed.
- **One undo history** for the model and the report, kept across saves. The query
  editor's draft has its own.
- **Fast loading.** DuckDB loads 5M CSV rows in about 0.5 s, and Close & Apply reloads
  only the queries that changed.
- **Awkward files just work.**
  - UTF-16 and Western European encodings, semicolons and decimal commas.
  - Ragged rows, title lines and late type changes.
  - Each file's settings are visible and editable.
- **Version-tolerant PBIP.** charted reads PBIR and the legacy report, and TMDL and
  model.bim, and writes conservative schemas.

**To do:**

- A search box in the format pane.
- A test proving that renames don't break visuals (fields are bound by id, not by
  name) for every kind of field.
- The field list keeps what you expanded across views.
- Smart guides that can be turned off, and a grid size setting.

## M7: an agent that builds reports inside charted

Microsoft's Copilot can draft a report page only with paid capacity and an admin
switched on, and its modelling MCP server can't touch report pages. charted can let
the agent already on the desktop build pages, live, in the open window:

> Build a page with sales by month as a line, sales by category as a donut and a
> region slicer.

**How it fits together:**

1. **A control socket in the running app.**
   - It lives at `$XDG_RUNTIME_DIR/charted/<pid>.sock`, readable only by the user.
   - It speaks the dev server's protocol (`{id, method, params}`, `rpc` and `data`)
     against the window's own project.
   - Every change goes through `Commit`. The UI updates as it happens, and *Undo*
     takes back any step ("Agent: add donut chart").
2. **`charted mcp`, an MCP server over stdio.** It attaches to the socket, or opens a
   project without a window when none is running. Its tools:
   - `describe_model`
   - `list_pages`, `get_page`, `add_page`
   - `add_visual`, `update_visual`, `remove_visual`, with fields named
     `'Table'[Column]` and positions laid out automatically
   - `add_measure`, checked with charted's DAX checker first
   - `query_visual`, which returns the visual's rows so the agent can check its own
     work
   - `set_theme` and `arrange_page`

   The `charted skill` instructions describe the tools.
3. **An Agent pane in the report view.**
   - It runs the user's default Omarchy agent (`omarchy-default-agent`: Claude Code,
     OpenCode, Codex and others) without a terminal, with charted's MCP server added.
   - It shows the agent's steps as they happen, with a Stop button.
   - Agents charted can't run that way open in a terminal through
     `omarchy agent prompt`.
4. **Privacy as the rest of charted.**
   - The first use says what the agent's provider will see. By default that's the
     model's names and types, and the numbers on the page the agent built.
   - Sending data values needs *Let the agent see data*.
   - Nothing runs until the user types a request.
5. **Tests:**
   - MCP tool tests on the sample.
   - An end-to-end test where a scripted stand-in for the agent builds a page through
     the socket.
   - A checkpoint with Claude Code itself.
