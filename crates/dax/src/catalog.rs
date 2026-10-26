//! Signatures and short descriptions of the DAX functions charted knows, for the
//! editor (completion, signature help, hover) and docs/dax-support.md.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FunctionDoc {
    pub name: &'static str,
    /// Parameter names; optional ones are written `[Name]`, repeating ones end in `…`.
    pub params: Vec<&'static str>,
    pub category: &'static str,
    pub description: &'static str,
    /// Whether charted evaluates it (otherwise it parses and is reported as unsupported).
    pub supported: bool,
}

impl FunctionDoc {
    pub fn signature(&self) -> String {
        format!("{} ( {} )", self.name, self.params.join(", "))
    }
}

/// `NAME | params; separated; by semicolons | category | description`.
const TABLE: &str = "\
ABS|Number|Math|The absolute value of a number.
ADDCOLUMNS|Table; Name; Expression; [Name; Expression]…|Table|Adds calculated columns to a table.
ALL|[TableOrColumn]; [Column]…|Filter|Removes filters from a table or columns. As a table, returns all rows or distinct values ignoring filters.
ALLEXCEPT|Table; Column; [Column]…|Filter|Removes all filters on a table except those on the listed columns.
ALLSELECTED|[TableOrColumn]; [Column]…|Filter|Removes filters from inside the visual, keeping the filters from outside it (slicers, page and report filters).
AND|Logical1; Logical2|Logical|TRUE when both arguments are TRUE.
AVERAGE|Column|Aggregation|The arithmetic mean of a column's numbers.
AVERAGEA|Column|Aggregation|The mean of a column, counting text as zero.
AVERAGEX|Table; Expression|Aggregation|The mean of an expression evaluated for each row of a table.
BLANK||Information|Returns blank.
CALCULATE|Expression; [Filter]…|Filter|Evaluates an expression in a filter context changed by the filters.
CALCULATETABLE|Table; [Filter]…|Filter|Evaluates a table in a filter context changed by the filters.
CEILING|Number; Significance|Math|Rounds up to the nearest multiple of significance.
COALESCE|Value1; Value2; [Value]…|Logical|The first argument that is not blank.
CONCATENATE|Text1; Text2|Text|Joins two text strings.
CONCATENATEX|Table; Expression; [Delimiter]; [OrderBy]; [Order]|Text|Joins an expression evaluated for each row.
COUNT|Column|Aggregation|The number of non-blank values in a column.
COUNTA|Column|Aggregation|The number of non-blank values in a column.
COUNTAX|Table; Expression|Aggregation|The number of rows where an expression is not blank.
COUNTBLANK|Column|Aggregation|The number of blank values in a column.
COUNTROWS|[Table]|Aggregation|The number of rows in a table.
COUNTX|Table; Expression|Aggregation|The number of rows for which an expression is not blank.
CROSSFILTER|Column1; Column2; Direction|Relationships|Changes a relationship's cross-filter direction within a CALCULATE.
DATE|Year; Month; Day|Date and time|A date from its parts.
DATEADD|Dates; NumberOfIntervals; Interval|Time intelligence|Dates shifted by a number of intervals.
DATEDIFF|Date1; Date2; Interval|Date and time|The number of interval boundaries between two dates.
DATESBETWEEN|Dates; StartDate; EndDate|Time intelligence|Dates between two dates.
DATESINPERIOD|Dates; StartDate; NumberOfIntervals; Interval|Time intelligence|Dates in a period starting at a date (ending at it when the number is negative).
DATESMTD|Dates|Time intelligence|Dates from the start of the month to the last date in context.
DATESQTD|Dates|Time intelligence|Dates from the start of the quarter to the last date in context.
DATESYTD|Dates; [YearEndDate]|Time intelligence|Dates from the start of the year to the last date in context.
DATEVALUE|DateText|Date and time|Converts text to a date.
DAY|Date|Date and time|The day of the month.
DISTINCT|ColumnOrTable|Table|The distinct values of a column (without the blank row), or the distinct rows of a table.
DISTINCTCOUNT|Column|Aggregation|The number of distinct values in a column, counting blank.
DIVIDE|Numerator; Denominator; [AlternateResult]|Math|Divides, returning the alternate result (blank by default) when the denominator is zero.
EARLIER|Column; [Number]|Filter|A column's value in an outer row context.
EDATE|StartDate; Months|Date and time|The date a number of months before or after.
EOMONTH|StartDate; Months|Date and time|The last day of the month a number of months away.
EXP|Number|Math|e raised to a power.
FALSE||Logical|The logical value FALSE.
FILTER|Table; FilterExpression|Filter|The rows of a table for which the expression is TRUE.
FIND|FindText; WithinText; [StartPosition]; [NotFoundValue]|Text|The position of text within text (case-sensitive).
FIRSTDATE|Dates|Time intelligence|The first date in context.
FIRSTNONBLANK|Column; Expression|Filter|The first value of a column for which the expression is not blank.
FLOOR|Number; Significance|Math|Rounds down to the nearest multiple of significance.
FORMAT|Value; FormatString|Text|Converts a value to text with a format string.
GENERATE|Table1; Table2|Table|The cross join of each row of Table1 with Table2 evaluated in its context.
HASONEVALUE|Column|Information|TRUE when the filter context leaves exactly one value of the column.
HOUR|DateTime|Date and time|The hour of a time.
IF|LogicalTest; ResultIfTrue; [ResultIfFalse]|Logical|Returns one value when a condition is TRUE and another when it is FALSE.
IFERROR|Value; ValueIfError|Logical|The value, or another when it is an error.
INT|Number|Math|Rounds down to an integer.
ISBLANK|Value|Information|TRUE when the value is blank.
ISERROR|Value|Information|TRUE when the value is an error.
ISFILTERED|TableOrColumn|Information|TRUE when the column (or any column of the table) is filtered directly.
ISINSCOPE|Column|Information|TRUE when the column is a grouping level of the current row in the visual.
KEEPFILTERS|Filter|Filter|Applies a CALCULATE filter on top of existing filters instead of replacing them.
LASTDATE|Dates|Time intelligence|The last date in context.
LASTNONBLANK|Column; Expression|Filter|The last value of a column for which the expression is not blank.
LEFT|Text; [NumberOfCharacters]|Text|Characters from the start of text.
LEN|Text|Text|The number of characters.
LN|Number|Math|The natural logarithm.
LOG|Number; [Base]|Math|The logarithm in a base (10 by default).
LOOKUPVALUE|ResultColumn; SearchColumn; SearchValue; [SearchColumn; SearchValue]…; [AlternateResult]|Information|The value of a column in the row matching the search values.
LOWER|Text|Text|Text in lower case.
MAX|ColumnOrValue; [Value2]|Aggregation|The largest value in a column, or the larger of two values.
MAXA|Column|Aggregation|The largest value, counting logical values.
MAXX|Table; Expression|Aggregation|The largest value of an expression evaluated for each row.
MEDIAN|Column|Aggregation|The median of a column's numbers.
MEDIANX|Table; Expression|Aggregation|The median of an expression evaluated for each row.
MID|Text; StartPosition; NumberOfCharacters|Text|Characters from the middle of text.
MIN|ColumnOrValue; [Value2]|Aggregation|The smallest value in a column, or the smaller of two values.
MINA|Column|Aggregation|The smallest value, counting logical values.
MINUTE|DateTime|Date and time|The minute of a time.
MINX|Table; Expression|Aggregation|The smallest value of an expression evaluated for each row.
MOD|Number; Divisor|Math|The remainder after division.
MONTH|Date|Date and time|The month, 1 to 12.
NEXTDAY|Dates|Time intelligence|The day after the last date.
NEXTMONTH|Dates|Time intelligence|The dates of the next month.
NEXTQUARTER|Dates|Time intelligence|The dates of the next quarter.
NEXTYEAR|Dates|Time intelligence|The dates of the next year.
NOT|Logical|Logical|Reverses TRUE and FALSE.
NOW||Date and time|The current date and time.
OR|Logical1; Logical2|Logical|TRUE when either argument is TRUE.
PARALLELPERIOD|Dates; NumberOfIntervals; Interval|Time intelligence|The whole period a number of intervals away.
PERCENTILE.INC|Column; K|Aggregation|The k-th percentile, inclusive.
PI||Math|The number π, 3.14159265358979.
POWER|Number; Power|Math|A number raised to a power.
PREVIOUSDAY|Dates|Time intelligence|The day before the first date.
PREVIOUSMONTH|Dates|Time intelligence|The dates of the previous month.
PREVIOUSQUARTER|Dates|Time intelligence|The dates of the previous quarter.
PREVIOUSYEAR|Dates|Time intelligence|The dates of the previous year.
PRODUCT|Column|Aggregation|The product of a column's numbers.
PRODUCTX|Table; Expression|Aggregation|The product of an expression evaluated for each row.
QUARTER|Date|Date and time|The quarter, 1 to 4.
RANKX|Table; Expression; [Value]; [Order]; [Ties]|Statistical|The rank of a value among the values of an expression over a table.
RELATED|Column|Relationships|The value of a column in the related table on the one side of a relationship.
RELATEDTABLE|Table|Relationships|The rows of a table on the many side related to the current row.
REMOVEFILTERS|[TableOrColumn]; [Column]…|Filter|Removes filters from tables or columns (in CALCULATE).
REPLACE|OldText; StartPosition; NumberOfCharacters; NewText|Text|Replaces part of text by position.
RIGHT|Text; [NumberOfCharacters]|Text|Characters from the end of text.
ROUND|Number; NumberOfDigits|Math|Rounds a number to the given number of digits.
ROUNDDOWN|Number; NumberOfDigits|Math|Rounds towards zero.
ROUNDUP|Number; NumberOfDigits|Math|Rounds away from zero.
SAMEPERIODLASTYEAR|Dates|Time intelligence|The dates shifted back one year.
SEARCH|FindText; WithinText; [StartPosition]; [NotFoundValue]|Text|The position of text within text (not case-sensitive).
SELECTCOLUMNS|Table; Name; Expression; [Name; Expression]…|Table|A table with the given columns.
SELECTEDVALUE|Column; [AlternateResult]|Information|The column's value when the filter context leaves exactly one, otherwise the alternate result.
SIGN|Number|Math|1, 0 or -1 for the sign of a number.
SQRT|Number|Math|The square root.
STDEV.P|Column|Statistical|The standard deviation of a population.
STDEV.S|Column|Statistical|The standard deviation of a sample.
SUBSTITUTE|Text; OldText; NewText; [InstanceNumber]|Text|Replaces text by matching.
SUM|Column|Aggregation|The sum of a column's numbers.
SUMMARIZE|Table; GroupByColumn…; [Name; Expression]…|Table|A table grouped by columns, with optional expressions.
SUMMARIZECOLUMNS|GroupByColumn…; [Filter]…; [Name; Expression]…|Table|A table grouped by columns with expressions, as visuals use.
SUMX|Table; Expression|Aggregation|The sum of an expression evaluated for each row of a table.
SWITCH|Expression; Value; Result; [Value; Result]…; [Else]|Logical|Returns the result for the first value that matches the expression.
TODAY||Date and time|The current date.
TOPN|N; Table; [OrderBy]; [Order]…|Table|The top N rows of a table by an expression.
TOTALMTD|Expression; Dates; [Filter]|Time intelligence|An expression over the month to date.
TOTALQTD|Expression; Dates; [Filter]|Time intelligence|An expression over the quarter to date.
TOTALYTD|Expression; Dates; [Filter]; [YearEndDate]|Time intelligence|An expression over the year to date.
TREATAS|Table; Column…|Filter|Applies the rows of a table as filters on unrelated columns.
TRIM|Text|Text|Removes spaces from both ends and repeated spaces between words.
TRUE||Logical|The logical value TRUE.
TRUNC|Number; [NumberOfDigits]|Math|Truncates to an integer or digits.
UPPER|Text|Text|Text in upper case.
USERELATIONSHIP|ColumnName1; ColumnName2|Relationships|Uses an inactive relationship for the duration of a CALCULATE.
VALUE|Text|Text|Converts text to a number.
VALUES|TableOrColumn|Table|The distinct values of a column (with the blank row when unmatched rows exist), or a table's rows.
WEEKDAY|Date; [ReturnType]|Date and time|The day of the week.
WEEKNUM|Date; [ReturnType]|Date and time|The week number in the year.
YEAR|Date|Date and time|The year of a date.
YEARFRAC|StartDate; EndDate; [Basis]|Date and time|The fraction of a year between two dates.";

/// Every function charted knows, in alphabetical order.
pub fn functions() -> Vec<FunctionDoc> {
    let mut out: Vec<FunctionDoc> = TABLE
        .lines()
        .filter_map(|l| {
            let mut p = l.splitn(4, '|');
            let name = p.next()?;
            let params = p.next()?;
            Some(FunctionDoc {
                name,
                params: params
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect(),
                category: p.next()?,
                description: p.next()?,
                supported: crate::bind::SUPPORTED.contains(&name),
            })
        })
        .collect();
    out.sort_by_key(|f| f.name);
    out
}

pub fn function(name: &str) -> Option<FunctionDoc> {
    functions()
        .into_iter()
        .find(|f| f.name.eq_ignore_ascii_case(name))
}

/// docs/dax-support.md: which functions charted evaluates, generated from the catalog.
pub fn support_markdown() -> String {
    use std::fmt::Write;
    let all = functions();
    let supported: Vec<&FunctionDoc> = all.iter().filter(|f| f.supported).collect();
    let mut out = String::from(
        "<!-- Generated from crates/dax/src/catalog.rs; run `CHARTED_UPDATE_DOCS=1 cargo test -p charted-dax` to refresh. -->\n\
         # DAX support\n\n\
         charted evaluates DAX by compiling each visual's measures to one SQL query for DuckDB. \
         Every function below is checked against a reference interpreter on a fixture model \
         (golden tests plus thousands of randomly generated measures).\n\n",
    );
    let _ = writeln!(out, "## Evaluated ({} functions)\n", supported.len());
    let mut cats: Vec<&str> = supported.iter().map(|f| f.category).collect();
    cats.sort();
    cats.dedup();
    for c in cats {
        let _ = writeln!(out, "### {c}\n\n| Function | Description |\n|---|---|");
        for f in supported.iter().filter(|f| f.category == c) {
            let _ = writeln!(out, "| `{}` | {} |", f.signature(), f.description);
        }
        out.push('\n');
    }
    out.push_str(
        "Also: `VAR … RETURN`, `IN { … }`, arithmetic, comparison, `&&`, `||` and `&` operators, \
         and blank semantics as in Power BI (blank + 1 = 1, blank = 0 is TRUE, DIVIDE by blank is blank).\n\n\
         ## Semantics and limits\n\n\
         - **Filter context.** Visual groupings, slicers and page/report filters, CALCULATE filter \
           arguments (Boolean, table, ALL/REMOVEFILTERS/ALLEXCEPT/ALLSELECTED, KEEPFILTERS, \
           VALUES as a filter) and context transition in iterators behave as in Power BI.\n\
         - **Relationships.** Active many-to-one and one-to-one relationships filter from the one \
           side to the many side; *Both* also filters the other way. USERELATIONSHIP switches an \
           inactive relationship on inside CALCULATE. Changes that would give filters two active \
           paths between tables are refused, as in Power BI. Many-to-many relationships are \
           stored and exported but do not filter yet.\n\
         - **Calculated columns** are evaluated per row with the table's columns, RELATED and \
           scalar functions, and stored with the data; aggregates, measures and CALCULATE in \
           calculated columns are not supported yet.\n\
         - **FILTER as a CALCULATE filter** may test measures and values from the outer \
           context, as running totals do: \
           `FILTER ( ALLSELECTED ( 'Date'[Date] ), 'Date'[Date] <= MAX ( 'Date'[Date] ) )`. \
           A filter on the date column of a marked date table (or on a date column used in a \
           relationship) replaces every filter on that table, as in Power BI. As a table, \
           ALLSELECTED returns the rows visible without the visual's own grouping.\n\
         - **DATESINPERIOD and DATESBETWEEN** take their dates as MAX, MIN, LASTDATE or \
           FIRSTDATE of the same date column, `DATE ( … )` of numbers, BLANK () for an open \
           end of DATESBETWEEN, or a variable defined as one of these just outside the \
           CALCULATE.\n\
         - **FORMAT** takes a format string written out as text: number formats `0`, `0.00`, \
           `#,0`, `#,##0.00`, `0%`, `0.0%` (optionally after `$`, `€`, `£` or `¥`), date \
           formats built from `yyyy`, `yy`, `mmmm`, `mmm`, `mm`, `m`, `dddd`, `ddd`, `dd`, `d` and \
           the separators `- / . ,` and space (month and day names in English), and the named \
           formats General Number, Fixed, Standard and Percent. Other format strings are \
           reported as unsupported.\n\
         - **Text functions** treat blank as empty text; MIN and MAX of two numbers treat \
           blank as zero unless both are blank.\n\
         - A measure that is never blank shows a row only where the visual's tables have data \
           (Power BI shows every combination for some such measures).\n\n",
    );
    let others: Vec<&FunctionDoc> = all.iter().filter(|f| !f.supported).collect();
    let _ = writeln!(
        out,
        "## Recognised, not yet evaluated ({})\n\nThese parse, autocomplete and show their signatures in the editor; using one in a measure reports that it is not supported yet.\n",
        others.len()
    );
    let _ = writeln!(
        out,
        "{}",
        others
            .iter()
            .map(|f| format!("`{}`", f.name))
            .collect::<Vec<_>>()
            .join(", ")
    );
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_known_function_is_documented() {
        let docs = super::functions();
        for name in crate::bind::SUPPORTED.iter().chain(crate::bind::KNOWN) {
            assert!(
                docs.iter().any(|d| d.name == *name),
                "{name} has no entry in the catalog"
            );
        }
        assert_eq!(
            docs.len(),
            crate::bind::SUPPORTED.len() + crate::bind::KNOWN.len()
        );
    }

    #[test]
    fn dax_support_doc_is_current() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/dax-support.md");
        let want = super::support_markdown();
        if std::env::var_os("CHARTED_UPDATE_DOCS").is_some() {
            std::fs::write(path, &want).unwrap();
        }
        let have = std::fs::read_to_string(path).unwrap_or_default();
        assert!(
            have == want,
            "docs/dax-support.md is out of date; run CHARTED_UPDATE_DOCS=1 cargo test -p charted-dax"
        );
    }
}
