# What Power BI has that charted doesn't (2026-11-01)

A gap analysis against Power BI Desktop as of its August 2026 release, from three
sources: an audit of charted's code (report side, and data/model/DAX side), Microsoft
Learn's DAX reference and monthly release notes, and the DAX the public projects in
`bin/corpus` actually use. The Power BI service (publishing, sharing, scheduled refresh,
apps) is left out on purpose: it is a separate, later goal.

The milestones at the end turn this into work. M10 (Mac and Windows, `docs/PLAN.md`)
comes first; these follow it.

## DAX

### Coverage by Microsoft's categories

Leaving out the financial (51) and INFO (71) functions, Power BI has about 341 DAX
functions. charted evaluates 104 and recognises 39 more (`docs/dax-support.md`).

| Category | Evaluated | Missing (not even recognised) |
|---|---|---|
| Aggregation | 14 of 22 | APPROXIMATEDISTINCTCOUNT, DISTINCTCOUNTNOBLANK |
| Date and time | 11 of 24 | CALENDARAUTO, NETWORKDAYS, SECOND, TIME, TIMEVALUE, UTCNOW, UTCTODAY |
| Filter | 11 of 14 | ALLCROSSFILTERED, ALLNOBLANKROW, EARLIEST |
| Window | 0 of 8 | INDEX, OFFSET, WINDOW, RANK, ROWNUMBER, ORDERBY, PARTITIONBY, MATCHBY |
| Visual calculations | 0 of 14 | RUNNINGSUM, MOVINGAVERAGE, PREVIOUS, NEXT, FIRST, LAST, LOOKUP, LOOKUPWITHTOTALS, RANGE, COLLAPSE(ALL), EXPAND(ALL), ISATLEVEL |
| Information | 9 of 45 | CONTAINS, CONTAINSROW, CONTAINSSTRING(EXACT), HASONEFILTER, ISCROSSFILTERED, ISEMPTY, ISSUBTOTAL, the IS-type tests (ISNUMBER, ISTEXT, ISEVEN…), USERNAME, USERPRINCIPALNAME, USERCULTURE, TABLEOF, NONVISUAL… |
| Logical | 8 of 15 | IFERROR (recognised), IF.EAGER, BITAND/OR/XOR/LSHIFT/RSHIFT |
| Math | 6 of 49 | 11 recognised (CEILING, FLOOR, ROUNDUP, ROUNDDOWN, TRUNC, SQRT, POWER, EXP, LN, LOG, SIGN), plus LOG10, MROUND, QUOTIENT, RAND, RANDBETWEEN, CONVERT, CURRENCY, EVEN, ODD, FACT, GCD, LCM, trigonometry |
| Other | 1 of 6 | ERROR, EVALUATEANDLOG, TOCSV, TOJSON, EXTERNALMEASURE |
| Parent-child | 0 of 5 | PATH, PATHCONTAINS, PATHITEM, PATHITEMREVERSE, PATHLENGTH |
| Relationship | 3 of 4 | CROSSFILTER (recognised) |
| Statistical | 1 of 43 | MEDIAN(X), PERCENTILE(X).INC/EXC, STDEV(X), VAR(X), GEOMEAN(X), RANK.EQ, NORM/T/CHISQ/BETA/POISSON/EXPON distributions, LINEST(X), SAMPLE, COMBIN, PERMUT, CONFIDENCE |
| Table | 8 of 30 | TREATAS and GENERATE (recognised), DATATABLE, ROW, UNION, INTERSECT, EXCEPT, CROSSJOIN, NATURALINNERJOIN, NATURALLEFTOUTERJOIN, GENERATEALL, GROUPBY, CURRENTGROUP, ADDMISSINGITEMS, ROLLUP…, FILTERS, DETAILROWS, SUBSTITUTEWITHINDEX |
| Text | 11 of 21 | FIND, SEARCH, REPLACE, VALUE (recognised), EXACT, REPT, FIXED, UNICHAR, UNICODE, COMBINEVALUES |
| Time intelligence | 21 of 41 | STARTOF/ENDOF Month/Quarter/Year/Week, OPENING/CLOSINGBALANCE…, the week family (DATESWTD, TOTALWTD, NEXTWEEK, PREVIOUSWEEK) |

By how much each function is used, coverage is far better than 104 of 341 suggests.
Of the ~44 functions practitioners use most, charted lacks only TREATAS, CALENDARAUTO
and USERPRINCIPALNAME. The corpus agrees: its misses are DATATABLE (10 uses),
PERCENTILEX.INC (4), WEEKNUM (3), UNICODE (3), HOUR (2), and UNION, TIME, RAND,
ISEMPTY, IFERROR and CEILING once each.

### Beyond single functions

- **FORMAT strings.** FORMAT is the most used function in the corpus (242 uses), and
  charted takes only a fixed set of format strings. Custom number formats (sections
  `#,0;(#,0);-`, `0.0,,"M"`), more date and time codes and named formats need to work.
- **Dynamic format strings** (GA October 2024): imported measures keep their static
  format.
- **Error handling:** IFERROR, ISERROR and ERROR. DAX errors are a value charted doesn't
  model yet.
- **Many-to-many relationships** are stored but don't filter (`compiler/src/graph.rs`).
- **Time intelligence edges:** DateTime columns that carry times, the year-end argument
  (PREVIOUSYEAR, DATESYTD fiscal years), FIRSTNONBLANK/LASTNONBLANK.
- **Window functions** (GA 2023): OFFSET, INDEX, WINDOW, RANK, ROWNUMBER. They map well
  onto DuckDB window functions.
- **Visual calculations** (GA May 2026): calculations on a visual's own grid
  (RUNNINGSUM, MOVINGAVERAGE, PREVIOUS…), with their own editor.
- **DAX user-defined functions** (GA June 2026): `FUNCTION Name = (p : Scalar) => …`,
  stored in `functions.tmdl`. Projects using them will start to fail to import.
- **Calendar-based time intelligence** (preview): named calendars, fiscal and 4-4-5
  weeks. Wait for GA.
- **DAX query view:** EVALUATE / DEFINE queries with a result grid. It's useful to power
  users, and to the agent for checking its work.
- **USERNAME / USERPRINCIPALNAME:** needed for dynamic row-level security (*View as* a
  user).

### Checked against Power BI itself

Every function is checked against charted's own reference interpreter, never against
Power BI. A conformance run would close that hole: export the golden measures as one
DAX query (`EVALUATE` over the fixture model), run it in Power BI Desktop's DAX query
view on the Windows VM, save the results as expected values, and fail the golden tests
where charted disagrees. Redo it whenever a function family is added.

## Report formatting (thin today)

A column chart's Format pane has about 15 settings in charted and well over 100 in
Power BI. `ui/src/report/registry.ts` has, for every chart:

- **Title:** show, text, size, font, alignment.
  - Missing: bold, italic and underline; colour; background; subtitle; divider;
    spacing.
- **Legend:** show, and three positions.
  - Missing: eight positions, title, font, colour and size.
- **Axes:** show, title, gridlines.
  - Missing: range min/max, log scale, display units, decimal places, label font,
    colour and rotation, continuous or categorical type, invert, concatenate labels,
    and gridline colour, width and style.
- **Data labels:** show and size.
  - Missing: position, display units, decimals, colour, font, background, series name
    and detail, and per-series settings.
- **Colours:** one default fill.
  - Missing: colour per series and per data point; conditional colour (fx) by rules,
    gradient or field value; spacing, border and transparency.
- **Effects:** background and border on/off.
  - Missing: background colour and transparency; border colour, radius and width;
    shadow; padding.
- **Missing everywhere:** tooltip styling, visual header icons, alt text on data
  visuals, the zoom slider, responsive layout, and conditional formatting on titles.

Each setting is mostly a `PropDef` plus the chart reading it, and should come across in
PBIR import and export at the same time (Power BI files with these settings lose them on
import today).

## Visuals and interactions

- **Missing visuals:**
  - Area and stacked area. Import turns them into line charts.
  - Ribbon chart.
  - Maps. Offline, a filled or shape map from bundled GeoJSON is possible; tile maps
    need the network.
  - Decomposition tree.
  - Key influencers.
  - Smart narrative (the agent can write it).
  - Scatter play axis.
  - R and Python visuals; custom visuals.
- **Matrix:**
  - Tabular and outline layouts.
  - Drill on rows.
  - Saved expand/collapse state.
  - Intermediate column subtotals.
  - Custom totals (GA May 2026).
  - Column resize.
  - Web URL and image cells.
- **Slicers:**
  - Hierarchy slicer.
  - Tile and button slicers with images.
  - List slicer.
  - Date-picker slicer (GA August 2026).
  - Input (text) slicer (GA February 2026).
  - Hiding values with no data across tables.
- **Analytics pane:**
  - Median and percentile lines.
  - Forecast.
  - Error bars.
  - Anomalies.
  - Symmetry shading.
  - X-axis constant lines.
  - Lines on scatter and on 100% charts.
- **Filters pane:**
  - Relative date and relative time filters in the pane itself.
  - Locking and hiding filters.
  - Formatting the pane.
- **Bookmarks:**
  - Visibility, spotlight and the current page (they keep only data state now).
  - A bookmark navigator.
- **Other:**
  - Grouping visuals.
  - Spotlight.
  - See records.
  - Report-page tooltips on scatter, treemap and the other chart types.
  - Cross-report drill-through.
  - Mobile layout.
- **Export:** PowerPoint and Excel (CSV and PDF exist).

## Data and model

- **Connectors:**
  - SQL Server, MySQL, ODBC, OData, SharePoint and REST.
  - Excel from the web.
  - Folder of Excel or Parquet files (only CSV now).
  - PostgreSQL has never been tested against a real server.
- **Power Query:**
  - M parameters.
  - Query groups.
  - Custom functions.
  - An editable Advanced Editor (M is read-only).
  - Error cells, with keep and remove errors (type errors become nulls now).
  - Fill up.
  - Transpose.
  - Keep bottom and remove alternate rows.
  - Date and time transforms.
  - Column from examples (the agent could do it).
  - Fuzzy merge.
  - Locale for types.
- **Model:**
  - Auto date/time.
  - Object-level security.
  - Perspectives.
  - Translations.
  - Assume referential integrity.
  - A TMDL view (a text editor over the model's TMDL, with apply).
  - Several diagram layouts.
- **Out of scope** (service, or a different engine): incremental refresh, aggregations,
  DirectQuery and composite models, Direct Lake, scheduled refresh, translytical task
  flows.

## Milestones (after M10)

The order puts the most-used things first. Each item is measured by what a real project
needs, and checked against Power BI where it can be.

1. **M11: DAX breadth.**
   - The 39 recognised functions, plus the cheap families: math (including
     trigonometry and MROUND/QUOTIENT/RAND), text (FIND, SEARCH, REPLACE, VALUE, EXACT,
     REPT, UNICHAR, UNICODE, FIXED, CONTAINSSTRING), date and time (TIME, HOUR, MINUTE,
     SECOND, NOW, UTC…, DATEVALUE, TIMEVALUE, WEEKNUM, YEARFRAC, NETWORKDAYS,
     CALENDARAUTO), the IS-type tests, and BIT functions.
   - Statistics: MEDIAN(X), PERCENTILE(X), STDEV(X), VAR(X), GEOMEAN(X), PRODUCT(X),
     the "A" aggregations, DISTINCTCOUNTNOBLANK.
   - IFERROR, ISERROR and ERROR, with DAX errors modelled.
   - DATATABLE and ROW.
   - The full FORMAT string language, and dynamic format strings.
   - The conformance run against Power BI Desktop.
   - The aim: about 104 → 230 evaluated, and every function the corpus uses.
2. **M12: formatting depth.**
   - The Format pane settings above for every chart, plus per-series and per-point
     colours and conditional colour (fx).
   - Area and stacked area charts.
   - All of it through PBIR in both directions.
3. **M13: DAX semantics.**
   - TREATAS, CROSSFILTER, many-to-many filtering, HASONEFILTER, ISCROSSFILTERED,
     ALLCROSSFILTERED, ALLNOBLANKROW, CONTAINS(ROW), ISEMPTY, FILTERS.
   - UNION, INTERSECT, EXCEPT, CROSSJOIN, the natural joins, GENERATE(ALL), GROUPBY.
   - FIRSTNONBLANK and LASTNONBLANK.
   - The rest of time intelligence (STARTOF/ENDOF, balances, weeks, fiscal year ends,
     DateTime columns).
   - The window functions.
   - USERPRINCIPALNAME, for dynamic RLS.
   - PATH functions.
4. **M14: report depth.**
   - Matrix layouts, drill and custom totals.
   - Hierarchy, tile, list, date-picker and input slicers.
   - The rest of the analytics pane.
   - Relative date filters in the pane.
   - Lock and hide filters.
   - Full bookmarks.
   - Grouping.
   - A filled map from bundled shapes.
   - PowerPoint and Excel export.
5. **M15: modern DAX.**
   - DAX user-defined functions, imported from `functions.tmdl`.
   - Visual calculations.
   - A DAX query view.
   - A TMDL view.
6. **M16: data reach.**
   - SQL Server and MySQL (through DuckDB extensions where they exist), and a
     verified PostgreSQL.
   - Excel and Parquet folders.
   - M parameters and query groups.
   - Editable M.
   - Error cells.
   - The missing transforms.

Then the Power BI service equivalent, as its own project.
