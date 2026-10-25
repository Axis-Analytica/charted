<!-- Generated from crates/dax/src/catalog.rs; run `CHARTED_UPDATE_DOCS=1 cargo test -p charted-dax` to refresh. -->
# DAX support

charted evaluates DAX by compiling each visual's measures to one SQL query for DuckDB. Every function below is checked against a reference interpreter on a fixture model (golden tests plus thousands of randomly generated measures).

## Evaluated (61 functions)

### Aggregation

| Function | Description |
|---|---|
| `AVERAGE ( Column )` | The arithmetic mean of a column's numbers. |
| `AVERAGEX ( Table, Expression )` | The mean of an expression evaluated for each row of a table. |
| `COUNT ( Column )` | The number of non-blank values in a column. |
| `COUNTA ( Column )` | The number of non-blank values in a column. |
| `COUNTBLANK ( Column )` | The number of blank values in a column. |
| `COUNTROWS ( [Table] )` | The number of rows in a table. |
| `COUNTX ( Table, Expression )` | The number of rows for which an expression is not blank. |
| `DISTINCTCOUNT ( Column )` | The number of distinct values in a column, counting blank. |
| `MAX ( ColumnOrValue, [Value2] )` | The largest value in a column, or the larger of two values. |
| `MAXX ( Table, Expression )` | The largest value of an expression evaluated for each row. |
| `MIN ( ColumnOrValue, [Value2] )` | The smallest value in a column, or the smaller of two values. |
| `MINX ( Table, Expression )` | The smallest value of an expression evaluated for each row. |
| `SUM ( Column )` | The sum of a column's numbers. |
| `SUMX ( Table, Expression )` | The sum of an expression evaluated for each row of a table. |

### Filter

| Function | Description |
|---|---|
| `ALL ( [TableOrColumn], [Column]… )` | Removes filters from a table or columns. As a table, returns all rows or distinct values ignoring filters. |
| `ALLEXCEPT ( Table, Column, [Column]… )` | Removes all filters on a table except those on the listed columns. |
| `ALLSELECTED ( [TableOrColumn], [Column]… )` | Removes filters from inside the visual, keeping the filters from outside it (slicers, page and report filters). |
| `CALCULATE ( Expression, [Filter]… )` | Evaluates an expression in a filter context changed by the filters. |
| `FILTER ( Table, FilterExpression )` | The rows of a table for which the expression is TRUE. |
| `KEEPFILTERS ( Filter )` | Applies a CALCULATE filter on top of existing filters instead of replacing them. |
| `REMOVEFILTERS ( [TableOrColumn], [Column]… )` | Removes filters from tables or columns (in CALCULATE). |

### Information

| Function | Description |
|---|---|
| `BLANK (  )` | Returns blank. |
| `HASONEVALUE ( Column )` | TRUE when the filter context leaves exactly one value of the column. |
| `ISBLANK ( Value )` | TRUE when the value is blank. |
| `ISFILTERED ( TableOrColumn )` | TRUE when the column (or any column of the table) is filtered directly. |
| `ISINSCOPE ( Column )` | TRUE when the column is a grouping level of the current row in the visual. |
| `SELECTEDVALUE ( Column, [AlternateResult] )` | The column's value when the filter context leaves exactly one, otherwise the alternate result. |

### Logical

| Function | Description |
|---|---|
| `AND ( Logical1, Logical2 )` | TRUE when both arguments are TRUE. |
| `COALESCE ( Value1, Value2, [Value]… )` | The first argument that is not blank. |
| `FALSE (  )` | The logical value FALSE. |
| `IF ( LogicalTest, ResultIfTrue, [ResultIfFalse] )` | Returns one value when a condition is TRUE and another when it is FALSE. |
| `NOT ( Logical )` | Reverses TRUE and FALSE. |
| `OR ( Logical1, Logical2 )` | TRUE when either argument is TRUE. |
| `SWITCH ( Expression, Value, Result, [Value, Result]…, [Else] )` | Returns the result for the first value that matches the expression. |
| `TRUE (  )` | The logical value TRUE. |

### Math

| Function | Description |
|---|---|
| `ABS ( Number )` | The absolute value of a number. |
| `DIVIDE ( Numerator, Denominator, [AlternateResult] )` | Divides, returning the alternate result (blank by default) when the denominator is zero. |
| `ROUND ( Number, NumberOfDigits )` | Rounds a number to the given number of digits. |

### Relationships

| Function | Description |
|---|---|
| `RELATED ( Column )` | The value of a column in the related table on the one side of a relationship. |
| `USERELATIONSHIP ( ColumnName1, ColumnName2 )` | Uses an inactive relationship for the duration of a CALCULATE. |

### Table

| Function | Description |
|---|---|
| `DISTINCT ( ColumnOrTable )` | The distinct values of a column (without the blank row), or the distinct rows of a table. |
| `VALUES ( TableOrColumn )` | The distinct values of a column (with the blank row when unmatched rows exist), or a table's rows. |

### Time intelligence

| Function | Description |
|---|---|
| `DATEADD ( Dates, NumberOfIntervals, Interval )` | Dates shifted by a number of intervals. |
| `DATESMTD ( Dates )` | Dates from the start of the month to the last date in context. |
| `DATESQTD ( Dates )` | Dates from the start of the quarter to the last date in context. |
| `DATESYTD ( Dates, [YearEndDate] )` | Dates from the start of the year to the last date in context. |
| `FIRSTDATE ( Dates )` | The first date in context. |
| `LASTDATE ( Dates )` | The last date in context. |
| `NEXTDAY ( Dates )` | The day after the last date. |
| `NEXTMONTH ( Dates )` | The dates of the next month. |
| `NEXTQUARTER ( Dates )` | The dates of the next quarter. |
| `NEXTYEAR ( Dates )` | The dates of the next year. |
| `PARALLELPERIOD ( Dates, NumberOfIntervals, Interval )` | The whole period a number of intervals away. |
| `PREVIOUSDAY ( Dates )` | The day before the first date. |
| `PREVIOUSMONTH ( Dates )` | The dates of the previous month. |
| `PREVIOUSQUARTER ( Dates )` | The dates of the previous quarter. |
| `PREVIOUSYEAR ( Dates )` | The dates of the previous year. |
| `SAMEPERIODLASTYEAR ( Dates )` | The dates shifted back one year. |
| `TOTALMTD ( Expression, Dates, [Filter] )` | An expression over the month to date. |
| `TOTALQTD ( Expression, Dates, [Filter] )` | An expression over the quarter to date. |
| `TOTALYTD ( Expression, Dates, [Filter], [YearEndDate] )` | An expression over the year to date. |

Also: `VAR … RETURN`, `IN { … }`, arithmetic, comparison, `&&`, `||` and `&` operators, and blank semantics as in Power BI (blank + 1 = 1, blank = 0 is TRUE, DIVIDE by blank is blank).

## Semantics and limits

- **Filter context.** Visual groupings, slicers and page/report filters, CALCULATE filter arguments (Boolean, table, ALL/REMOVEFILTERS/ALLEXCEPT/ALLSELECTED, KEEPFILTERS, VALUES as a filter) and context transition in iterators behave as in Power BI.
- **Relationships.** Active many-to-one and one-to-one relationships filter from the one side to the many side; *Both* also filters the other way. USERELATIONSHIP switches an inactive relationship on inside CALCULATE. Changes that would give filters two active paths between tables are refused, as in Power BI. Many-to-many relationships are stored and exported but do not filter yet.
- **Calculated columns** are evaluated per row with the table's columns, RELATED and scalar functions, and stored with the data; aggregates, measures and CALCULATE in calculated columns are not supported yet.
- **FILTER over measures** used as a CALCULATE filter argument is not supported yet; FILTER over columns is.
- A measure that is never blank shows a row only where the visual's tables have data (Power BI shows every combination for some such measures).

## Recognised, not yet evaluated (74)

These parse, autocomplete and show their signatures in the editor; using one in a measure reports that it is not supported yet.

`ADDCOLUMNS`, `AVERAGEA`, `CALCULATETABLE`, `CEILING`, `CONCATENATE`, `CONCATENATEX`, `COUNTAX`, `CROSSFILTER`, `DATE`, `DATEDIFF`, `DATESBETWEEN`, `DATESINPERIOD`, `DATEVALUE`, `DAY`, `EARLIER`, `EDATE`, `EOMONTH`, `EXP`, `FIND`, `FIRSTNONBLANK`, `FLOOR`, `FORMAT`, `GENERATE`, `HOUR`, `IFERROR`, `INT`, `ISERROR`, `LASTNONBLANK`, `LEFT`, `LEN`, `LN`, `LOG`, `LOOKUPVALUE`, `LOWER`, `MAXA`, `MEDIAN`, `MEDIANX`, `MID`, `MINA`, `MINUTE`, `MOD`, `MONTH`, `NOW`, `PERCENTILE.INC`, `POWER`, `PRODUCT`, `PRODUCTX`, `QUARTER`, `RANKX`, `RELATEDTABLE`, `REPLACE`, `RIGHT`, `ROUNDDOWN`, `ROUNDUP`, `SEARCH`, `SELECTCOLUMNS`, `SIGN`, `SQRT`, `STDEV.P`, `STDEV.S`, `SUBSTITUTE`, `SUMMARIZE`, `SUMMARIZECOLUMNS`, `TODAY`, `TOPN`, `TREATAS`, `TRIM`, `TRUNC`, `UPPER`, `VALUE`, `WEEKDAY`, `WEEKNUM`, `YEAR`, `YEARFRAC`
