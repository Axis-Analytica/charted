//! Data-loading queries: a source plus a list of applied steps (Power Query's model),
//! compiled into a single SELECT that DuckDB optimises as a whole, so filters and column
//! choices are pushed down into the file readers. Each step also has its M equivalent,
//! for Power BI export and for showing alongside the SQL in the query editor.
//!
//! Steps never depend on the data when compiled: anything Power Query would look up at
//! run time (the header row's names, the values a pivot spreads into columns, how many
//! parts a split makes) is resolved when the step is added and stored in it, as Power
//! Query also bakes such names into later steps.

use charted_model::{Condition, DataType, FilterOp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

use crate::mformula;
use crate::quote;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Query {
    pub id: String,
    /// Display name, shown in the query editor.
    pub name: String,
    /// The DuckDB table the query loads into (the model table's `source`).
    pub table: String,
    pub source: Source,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Display names given to steps, by index (Power Query's renamed steps).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub step_names: BTreeMap<usize, String>,
    /// A query that only feeds others (merges, appends) and loads no table.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_load: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Source {
    Csv {
        /// Path relative to the project folder, or absolute.
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delimiter: Option<String>,
        #[serde(default = "yes")]
        header: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<String>,
    },
    Parquet {
        path: String,
    },
    Json {
        path: String,
    },
    /// One sheet of an Excel workbook. charted reads it into a cached CSV first (no
    /// DuckDB extension, no network).
    Excel {
        path: String,
        sheet: String,
        #[serde(default = "yes")]
        header: bool,
    },
    /// Every CSV file in a folder matching `pattern`, combined by column name, with the
    /// file each row came from in `Source.Name`.
    Folder {
        path: String,
        #[serde(default = "csv_pattern")]
        pattern: String,
        #[serde(default = "yes")]
        header: bool,
    },
    /// A table of a SQLite database (DuckDB's sqlite extension, installed with consent).
    Sqlite {
        path: String,
        table: String,
    },
    /// A table of a PostgreSQL database (DuckDB's postgres extension, installed with
    /// consent). The password is kept in the desktop keyring, never in the project.
    Postgres {
        host: String,
        #[serde(default = "pg_port")]
        port: u16,
        database: String,
        user: String,
        #[serde(default = "public")]
        schema: String,
        table: String,
    },
    /// A generated date table: one row per day from `start` to `end` (ISO dates), with
    /// year, quarter, month, weekday and fiscal-year columns. The fiscal year starts in
    /// `fiscal_start` (a month, 1 for calendar years) and is named by the year it ends.
    Calendar {
        start: String,
        end: String,
        #[serde(default = "january")]
        #[ts(type = "number")]
        fiscal_start: u32,
    },
}

/// A calendar bound: an ISO date (YYYY-MM-DD).
fn calendar_date(d: &str) -> Result<&str, StepError> {
    let ok = d.len() == 10
        && d.char_indices().all(|(i, c)| if i == 4 || i == 7 { c == '-' } else { c.is_ascii_digit() });
    if ok {
        Ok(d)
    } else {
        Err(StepError::Invalid(format!("\"{d}\" isn't a date (YYYY-MM-DD)")))
    }
}

fn january() -> u32 {
    1
}

fn yes() -> bool {
    true
}

fn csv_pattern() -> String {
    "*.csv".into()
}

fn pg_port() -> u16 {
    5432
}

fn public() -> String {
    "public".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SplitMode {
    /// At the first delimiter.
    Left,
    /// At the last delimiter.
    Right,
    /// At every delimiter.
    Each,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AggOp {
    Sum,
    Average,
    Min,
    Max,
    Count,
    CountDistinct,
    /// Rows in the group (no column).
    CountRows,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Aggregate {
    pub name: String,
    pub op: AggOp,
    #[serde(default)]
    pub column: Option<String>,
}

/// One condition of a conditional column: when `column` `op` `value`, the output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Rule {
    pub column: String,
    pub op: FilterOp,
    #[serde(default)]
    pub value: Option<String>,
    pub output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TextOp {
    Lower,
    Upper,
    Trim,
    Clean,
    Capitalize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum Extract {
    Length,
    First { count: u32 },
    Last { count: u32 },
    Range { start: u32, count: u32 },
    BeforeDelimiter { delimiter: String },
    AfterDelimiter { delimiter: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum JoinKind {
    LeftOuter,
    RightOuter,
    FullOuter,
    Inner,
    LeftAnti,
}

/// One applied step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum Step {
    RemoveTopRows {
        count: u32,
    },
    KeepTopRows {
        count: u32,
    },
    /// Uses the first row as column names (`names`, resolved when the step was added).
    PromoteHeaders {
        names: Vec<String>,
    },
    /// Values that don't convert become blank (Power Query shows them as errors).
    ChangeTypes {
        columns: Vec<(String, DataType)>,
    },
    SelectColumns {
        columns: Vec<String>,
    },
    RemoveColumns {
        columns: Vec<String>,
    },
    RenameColumns {
        renames: Vec<(String, String)>,
    },
    FilterRows {
        column: String,
        conditions: Vec<Condition>,
        #[serde(default)]
        or: bool,
    },
    /// Keeps the first row of each combination of `columns` (all columns when empty).
    RemoveDuplicates {
        #[serde(default)]
        columns: Vec<String>,
    },
    /// Removes rows where every column is blank.
    RemoveBlankRows,
    ReplaceValues {
        columns: Vec<String>,
        find: String,
        replace: String,
        /// Match the whole cell (otherwise every occurrence inside text is replaced).
        #[serde(default)]
        whole: bool,
    },
    SplitColumn {
        column: String,
        delimiter: String,
        mode: SplitMode,
        /// The new columns' names (their number is the number of parts).
        names: Vec<String>,
    },
    MergeColumns {
        columns: Vec<String>,
        #[serde(default)]
        separator: String,
        name: String,
    },
    /// A column computed by a formula (see `mformula`).
    CustomColumn {
        name: String,
        formula: String,
    },
    ConditionalColumn {
        name: String,
        rules: Vec<Rule>,
        #[serde(default)]
        otherwise: Option<String>,
    },
    GroupBy {
        keys: Vec<String>,
        aggregates: Vec<Aggregate>,
    },
    /// Turns `columns` into attribute/value rows.
    Unpivot {
        columns: Vec<String>,
        #[serde(default = "attribute")]
        attribute: String,
        #[serde(default = "value")]
        value: String,
    },
    /// Spreads the `values` of `column` into columns of `values_column`.
    Pivot {
        column: String,
        values_column: String,
        values: Vec<String>,
        aggregate: AggOp,
    },
    FillDown {
        columns: Vec<String>,
    },
    AddIndex {
        #[serde(default = "index")]
        name: String,
        #[serde(default)]
        #[ts(type = "number")]
        start: i64,
        #[serde(default = "one")]
        #[ts(type = "number")]
        step: i64,
    },
    /// Part of a text column: in place, or as a new column named `new_column`.
    ExtractText {
        column: String,
        extract: Extract,
        #[serde(default)]
        new_column: Option<String>,
    },
    FormatText {
        columns: Vec<String>,
        format: TextOp,
    },
    SortRows {
        /// Column and descending.
        columns: Vec<(String, bool)>,
    },
    /// Joins another query and brings in some of its columns.
    MergeQueries {
        query: String,
        left: Vec<String>,
        right: Vec<String>,
        kind: JoinKind,
        /// The other query's columns to add.
        columns: Vec<String>,
        /// Prefix for the added columns' names (Power Query uses the query name).
        #[serde(default)]
        prefix: Option<String>,
    },
    /// Adds the rows of other queries, matching columns by name.
    AppendQueries {
        queries: Vec<String>,
    },
}

fn attribute() -> String {
    "Attribute".into()
}

fn value() -> String {
    "Value".into()
}

fn index() -> String {
    "Index".into()
}

fn one() -> i64 {
    1
}

impl Source {
    /// The file or folder read, or the database location.
    pub fn path(&self) -> String {
        match self {
            Source::Csv { path, .. }
            | Source::Parquet { path }
            | Source::Json { path }
            | Source::Excel { path, .. }
            | Source::Folder { path, .. }
            | Source::Sqlite { path, .. } => path.clone(),
            Source::Postgres {
                host,
                port,
                database,
                ..
            } => format!("postgresql://{host}:{port}/{database}"),
            Source::Calendar { .. } => String::new(),
        }
    }

    /// Whether the source is a local file or folder (watched for changes).
    pub fn is_file(&self) -> bool {
        !matches!(self, Source::Postgres { .. } | Source::Calendar { .. })
    }

    /// The DuckDB extension the source needs, if any.
    pub fn extension(&self) -> Option<&'static str> {
        match self {
            Source::Sqlite { .. } => Some("sqlite"),
            Source::Postgres { .. } => Some("postgres"),
            _ => None,
        }
    }
}

impl Step {
    /// Power Query's name for the step.
    pub fn default_name(&self) -> &'static str {
        match self {
            Step::RemoveTopRows { .. } => "Removed Top Rows",
            Step::KeepTopRows { .. } => "Kept First Rows",
            Step::PromoteHeaders { .. } => "Promoted Headers",
            Step::ChangeTypes { .. } => "Changed Type",
            Step::SelectColumns { .. } => "Removed Other Columns",
            Step::RemoveColumns { .. } => "Removed Columns",
            Step::RenameColumns { .. } => "Renamed Columns",
            Step::FilterRows { .. } => "Filtered Rows",
            Step::RemoveDuplicates { .. } => "Removed Duplicates",
            Step::RemoveBlankRows => "Removed Blank Rows",
            Step::ReplaceValues { .. } => "Replaced Value",
            Step::SplitColumn { .. } => "Split Column by Delimiter",
            Step::MergeColumns { .. } => "Merged Columns",
            Step::CustomColumn { .. } => "Added Custom",
            Step::ConditionalColumn { .. } => "Added Conditional Column",
            Step::GroupBy { .. } => "Grouped Rows",
            Step::Unpivot { .. } => "Unpivoted Columns",
            Step::Pivot { .. } => "Pivoted Column",
            Step::FillDown { .. } => "Filled Down",
            Step::AddIndex { .. } => "Added Index",
            Step::ExtractText { .. } => "Extracted Text",
            Step::FormatText { .. } => "Formatted Text",
            Step::SortRows { .. } => "Sorted Rows",
            Step::MergeQueries { .. } => "Merged Queries",
            Step::AppendQueries { .. } => "Appended Query",
        }
    }

    /// Other queries the step reads.
    pub fn references(&self) -> Vec<&str> {
        match self {
            Step::MergeQueries { query, .. } => vec![query.as_str()],
            Step::AppendQueries { queries } => queries.iter().map(String::as_str).collect(),
            _ => vec![],
        }
    }
}

impl Query {
    /// Display names of the source and each step, unique as Power Query makes them
    /// ("Changed Type", "Changed Type 2", …).
    pub fn step_display_names(&self) -> Vec<String> {
        let mut out = vec!["Source".to_string()];
        for (i, s) in self.steps.iter().enumerate() {
            let base = self
                .step_names
                .get(&i)
                .cloned()
                .unwrap_or_else(|| s.default_name().to_string());
            let mut name = base.clone();
            let mut k = 2;
            while out.contains(&name) {
                name = format!("{base} {k}");
                k += 1;
            }
            out.push(name);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum StepError {
    #[error("{0}")]
    Invalid(String),
    #[error("the queries {0} refer to each other in a loop")]
    Cycle(String),
    #[error("there is no query called \"{0}\"")]
    UnknownQuery(String),
}

/// What compiling needs besides the query: the project folder, the other queries
/// (merges and appends), and the cached CSV copy of each Excel sheet.
pub struct Ctx<'a> {
    pub project_dir: &'a str,
    pub queries: &'a [Query],
    /// By `sheet_key(path, sheet)`: the cached CSV's absolute path.
    pub sheets: &'a BTreeMap<String, String>,
}

/// The key of an Excel sheet in `Ctx::sheets`.
pub fn sheet_key(path: &str, sheet: &str) -> String {
    format!("{path}\u{1}{sheet}")
}

/// The DuckDB name a PostgreSQL connection is attached as.
pub fn postgres_alias(host: &str, port: u16, database: &str, user: &str) -> String {
    // FNV-1a: stable across runs and platforms.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in format!("{host}\u{1}{port}\u{1}{database}\u{1}{user}").bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("pg_{:012x}", h & 0xffff_ffff_ffff)
}

/// Resolves a source path against the project folder (both as plain strings).
pub fn resolve_path(project_dir: &str, path: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", project_dir.trim_end_matches('/'), path)
    }
}

fn source_sql(source: &Source, ctx: &Ctx) -> Result<String, StepError> {
    let file = |p: &str| quote::string(&resolve_path(ctx.project_dir, p));
    Ok(match source {
        Source::Csv {
            path,
            delimiter,
            header,
            encoding,
        } => {
            let mut opts = vec![format!("header = {header}")];
            if let Some(d) = delimiter {
                opts.push(format!("delim = {}", quote::string(d)));
            }
            if let Some(e) = encoding {
                opts.push(format!("encoding = {}", quote::string(e)));
            }
            format!(
                "SELECT * FROM read_csv({}, {})",
                file(path),
                opts.join(", ")
            )
        }
        Source::Calendar {
            start,
            end,
            fiscal_start,
        } => {
            let (a, b) = (calendar_date(start)?, calendar_date(end)?);
            if a > b {
                return Err(StepError::Invalid("the date table ends before it starts".into()));
            }
            let fs = (*fiscal_start).clamp(1, 12);
            let next = if fs == 1 {
                "0".to_string()
            } else {
                format!("CASE WHEN month(d) >= {fs} THEN 1 ELSE 0 END")
            };
            format!(
                "SELECT CAST(d AS DATE) AS \"Date\", year(d) AS \"Year\", 'Q' || quarter(d) AS \"Quarter\", \
                 month(d) AS \"MonthNumber\", monthname(d) AS \"MonthName\", strftime(d, '%Y-%m') AS \"YearMonth\", \
                 dayname(d) AS \"Weekday\", isodow(d) AS \"WeekdayNumber\", 'FY' || (year(d) + {next}) AS \"FiscalYear\" \
                 FROM generate_series(DATE '{a}', DATE '{b}', INTERVAL 1 DAY) AS t(d)"
            )
        }
        Source::Parquet { path } => format!("SELECT * FROM read_parquet({})", file(path)),
        Source::Json { path } => format!("SELECT * FROM read_json_auto({})", file(path)),
        Source::Excel {
            path,
            sheet,
            header,
        } => {
            let cached = ctx.sheets.get(&sheet_key(path, sheet)).ok_or_else(|| {
                StepError::Invalid(format!("the sheet \"{sheet}\" of {path} hasn't been read"))
            })?;
            format!(
                "SELECT * FROM read_csv({}, header = {header}, all_varchar = {})",
                quote::string(cached),
                !header
            )
        }
        Source::Folder {
            path,
            pattern,
            header,
        } => {
            let glob = format!(
                "{}/{}",
                resolve_path(ctx.project_dir, path).trim_end_matches('/'),
                pattern
            );
            format!(
                "SELECT * EXCLUDE (filename), parse_filename(filename) AS \"Source.Name\" FROM read_csv({}, header = {header}, union_by_name = true, filename = true)",
                quote::string(&glob)
            )
        }
        Source::Sqlite { path, table } => format!(
            "SELECT * FROM sqlite_scan({}, {})",
            file(path),
            quote::string(table)
        ),
        Source::Postgres {
            host,
            port,
            database,
            user,
            schema,
            table,
        } => format!(
            "SELECT * FROM {}.{}.{}",
            quote::ident(&postgres_alias(host, *port, database, user)),
            quote::ident(schema),
            quote::ident(table)
        ),
    })
}

/// The DuckDB type for a model type (Power Query's fixed decimal has four places).
pub fn duckdb_type(t: DataType) -> &'static str {
    match t {
        DataType::String | DataType::Unknown => "VARCHAR",
        DataType::Int64 => "BIGINT",
        DataType::Double => "DOUBLE",
        DataType::Decimal => "DECIMAL(19, 4)",
        DataType::Boolean => "BOOLEAN",
        DataType::Date => "DATE",
        DataType::Time => "TIME",
        DataType::DateTime => "TIMESTAMP",
        DataType::DateTimeTz => "TIMESTAMPTZ",
        DataType::Binary => "BLOB",
    }
}

fn idents(cols: &[String]) -> String {
    cols.iter()
        .map(|c| quote::ident(c))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A condition on a column. Values are text; DuckDB converts them to the column's type
/// for comparisons.
fn condition_sql(col: &str, c: &Condition) -> Option<String> {
    let e = quote::ident(col);
    let text = format!("CAST({e} AS VARCHAR)");
    let v = c.value.as_deref().map(quote::string);
    let like = |pattern: String| format!("{text} LIKE {pattern}");
    let esc = |s: &str| {
        s.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    };
    let raw = c.value.as_deref().unwrap_or("");
    Some(match c.op {
        FilterOp::Equals => format!("{e} = {}", v?),
        FilterOp::NotEquals => format!("{e} IS DISTINCT FROM {}", v?),
        FilterOp::Contains => like(quote::string(&format!("%{}%", esc(raw)))) + " ESCAPE '\\'",
        FilterOp::NotContains => format!(
            "NOT coalesce({} ESCAPE '\\', FALSE)",
            like(quote::string(&format!("%{}%", esc(raw))))
        ),
        FilterOp::StartsWith => like(quote::string(&format!("{}%", esc(raw)))) + " ESCAPE '\\'",
        FilterOp::EndsWith => like(quote::string(&format!("%{}", esc(raw)))) + " ESCAPE '\\'",
        FilterOp::Less => format!("{e} < {}", v?),
        FilterOp::LessOrEqual => format!("{e} <= {}", v?),
        FilterOp::Greater => format!("{e} > {}", v?),
        FilterOp::GreaterOrEqual => format!("{e} >= {}", v?),
        FilterOp::Between => format!(
            "{e} BETWEEN {} AND {}",
            v?,
            c.value2.as_deref().map(quote::string)?
        ),
        FilterOp::IsBlank => format!("({e} IS NULL OR {text} = '')"),
        FilterOp::IsNotBlank => format!("({e} IS NOT NULL AND {text} <> '')"),
    })
}

fn agg_sql(op: AggOp, col: Option<&str>) -> String {
    let c = col.map(quote::ident).unwrap_or_else(|| "*".into());
    match op {
        AggOp::Sum => format!("sum({c})"),
        AggOp::Average => format!("avg({c})"),
        AggOp::Min => format!("min({c})"),
        AggOp::Max => format!("max({c})"),
        AggOp::Count => format!("count({c})"),
        AggOp::CountDistinct => format!("count(DISTINCT {c})"),
        AggOp::CountRows => "count(*)".into(),
    }
}

fn step_sql(
    step: &Step,
    input: &str,
    ctx: &Ctx,
    seen: &mut Vec<String>,
) -> Result<String, StepError> {
    Ok(match step {
        Step::RemoveTopRows { count } => format!(
            "SELECT * EXCLUDE (\"__row\") FROM (SELECT *, row_number() OVER () AS \"__row\" FROM {input}) WHERE \"__row\" > {count}"
        ),
        Step::KeepTopRows { count } => format!("SELECT * FROM {input} LIMIT {count}"),
        Step::PromoteHeaders { names } => {
            if names.is_empty() {
                return Err(StepError::Invalid(
                    "there is no row to use as headers".into(),
                ));
            }
            // Positional columns take the first row's names; the first row goes.
            let cols: Vec<String> = names
                .iter()
                .enumerate()
                .map(|(i, n)| format!("#{} AS {}", i + 1, quote::ident(n)))
                .collect();
            format!(
                "SELECT {} FROM (SELECT * FROM (SELECT *, row_number() OVER () AS \"__row\" FROM {input}) WHERE \"__row\" > 1)",
                cols.join(", ")
            )
        }
        Step::ChangeTypes { columns } => {
            let cols: Vec<String> = columns
                .iter()
                .map(|(c, t)| {
                    let e = quote::ident(c);
                    let cast = match t {
                        // Text like "1,234.50" or "$12" still reads as a number.
                        DataType::Int64 | DataType::Double | DataType::Decimal => format!(
                            "coalesce(TRY_CAST({e} AS {ty}), TRY_CAST(regexp_replace(CAST({e} AS VARCHAR), '[,$€£\\s]', '', 'g') AS {ty}))",
                            ty = duckdb_type(*t)
                        ),
                        DataType::String => format!("CAST({e} AS VARCHAR)"),
                        _ => format!("TRY_CAST({e} AS {})", duckdb_type(*t)),
                    };
                    format!("{cast} AS {e}")
                })
                .collect();
            format!("SELECT * REPLACE ({}) FROM {input}", cols.join(", "))
        }
        Step::SelectColumns { columns } => format!("SELECT {} FROM {input}", idents(columns)),
        Step::RemoveColumns { columns } => {
            format!("SELECT * EXCLUDE ({}) FROM {input}", idents(columns))
        }
        Step::RenameColumns { renames } => {
            let cols: Vec<String> = renames
                .iter()
                .map(|(from, to)| format!("{} AS {}", quote::ident(from), quote::ident(to)))
                .collect();
            format!("SELECT * RENAME ({}) FROM {input}", cols.join(", "))
        }
        Step::FilterRows {
            column,
            conditions,
            or,
        } => {
            let parts: Vec<String> = conditions
                .iter()
                .filter_map(|c| condition_sql(column, c))
                .collect();
            if parts.is_empty() {
                format!("SELECT * FROM {input}")
            } else {
                format!(
                    "SELECT * FROM {input} WHERE {}",
                    parts
                        .iter()
                        .map(|p| format!("({p})"))
                        .collect::<Vec<_>>()
                        .join(if *or { " OR " } else { " AND " })
                )
            }
        }
        Step::RemoveDuplicates { columns } => {
            // Keeps the first row of each group, in order; blanks match each other.
            let numbered = format!("(SELECT *, row_number() OVER () AS \"__row\" FROM {input})");
            let firsts = if columns.is_empty() {
                format!(
                    "SELECT min(\"__row\") AS m, * EXCLUDE (\"__row\") FROM {numbered} GROUP BY ALL"
                )
            } else {
                format!(
                    "SELECT min(\"__row\") AS m FROM {numbered} GROUP BY {}",
                    idents(columns)
                )
            };
            format!(
                "SELECT * EXCLUDE (\"__row\") FROM {numbered} WHERE \"__row\" IN (SELECT m FROM ({firsts})) ORDER BY \"__row\""
            )
        }
        // Keeps rows where some column has a value.
        Step::RemoveBlankRows => {
            format!("SELECT * FROM {input} WHERE trim(concat_ws('', *COLUMNS(*))) <> ''")
        }
        Step::ReplaceValues {
            columns,
            find,
            replace,
            whole,
        } => {
            let cols: Vec<String> = columns
                .iter()
                .map(|c| {
                    let e = quote::ident(c);
                    let r = quote::string(replace);
                    if *whole {
                        if find.is_empty() {
                            format!("CASE WHEN {e} IS NULL OR CAST({e} AS VARCHAR) = '' THEN {r} ELSE CAST({e} AS VARCHAR) END AS {e}")
                        } else {
                            format!("CASE WHEN CAST({e} AS VARCHAR) = {} THEN {r} ELSE CAST({e} AS VARCHAR) END AS {e}", quote::string(find))
                        }
                    } else {
                        format!("replace(CAST({e} AS VARCHAR), {}, {r}) AS {e}", quote::string(find))
                    }
                })
                .collect();
            format!("SELECT * REPLACE ({}) FROM {input}", cols.join(", "))
        }
        Step::SplitColumn {
            column,
            delimiter,
            mode,
            names,
        } => {
            let e = format!("CAST({} AS VARCHAR)", quote::ident(column));
            let d = quote::string(delimiter);
            let parts: Vec<String> = match mode {
                SplitMode::Each => names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| {
                        format!(
                            "nullif(split_part({e}, {d}, {}), '') AS {}",
                            i + 1,
                            quote::ident(n)
                        )
                    })
                    .collect(),
                SplitMode::Left | SplitMode::Right => {
                    let pos = if *mode == SplitMode::Left {
                        format!("strpos({e}, {d})")
                    } else {
                        format!(
                            "(length({e}) - strpos(reverse({e}), reverse({d})) - length({d}) + 2)"
                        )
                    };
                    let first = format!(
                        "CASE WHEN strpos({e}, {d}) = 0 THEN {e} ELSE left({e}, {pos} - 1) END"
                    );
                    let second = format!(
                        "CASE WHEN strpos({e}, {d}) = 0 THEN NULL ELSE substr({e}, {pos} + length({d})) END"
                    );
                    let mut v = vec![format!("{first} AS {}", quote::ident(&names[0]))];
                    if let Some(n) = names.get(1) {
                        v.push(format!("{second} AS {}", quote::ident(n)));
                    }
                    v
                }
            };
            format!(
                "SELECT * EXCLUDE ({c}), {} FROM {input}",
                parts.join(", "),
                c = quote::ident(column)
            )
        }
        Step::MergeColumns {
            columns,
            separator,
            name,
        } => format!(
            "SELECT * EXCLUDE ({cols}), concat_ws({}, {}) AS {} FROM {input}",
            quote::string(separator),
            columns
                .iter()
                .map(|c| format!("coalesce(CAST({} AS VARCHAR), '')", quote::ident(c)))
                .collect::<Vec<_>>()
                .join(", "),
            quote::ident(name),
            cols = idents(columns)
        ),
        Step::CustomColumn { name, formula } => {
            let e = mformula::parse(formula, &[]).map_err(|e| StepError::Invalid(e.0))?;
            format!(
                "SELECT *, {} AS {} FROM {input}",
                mformula::to_sql(&e),
                quote::ident(name)
            )
        }
        Step::ConditionalColumn {
            name,
            rules,
            otherwise,
        } => {
            let whens: Vec<String> = rules
                .iter()
                .filter_map(|r| {
                    let c = Condition {
                        op: r.op,
                        value: r.value.clone(),
                        value2: None,
                    };
                    condition_sql(&r.column, &c)
                        .map(|p| format!("WHEN {p} THEN {}", quote::string(&r.output)))
                })
                .collect();
            let other = otherwise
                .as_deref()
                .map(quote::string)
                .unwrap_or_else(|| "NULL".into());
            if whens.is_empty() {
                format!("SELECT *, {other} AS {} FROM {input}", quote::ident(name))
            } else {
                format!(
                    "SELECT *, CASE {} ELSE {other} END AS {} FROM {input}",
                    whens.join(" "),
                    quote::ident(name)
                )
            }
        }
        Step::GroupBy { keys, aggregates } => {
            let mut cols: Vec<String> = keys.iter().map(|k| quote::ident(k)).collect();
            for a in aggregates {
                cols.push(format!(
                    "{} AS {}",
                    agg_sql(a.op, a.column.as_deref()),
                    quote::ident(&a.name)
                ));
            }
            if keys.is_empty() {
                format!("SELECT {} FROM {input}", cols.join(", "))
            } else {
                format!(
                    "SELECT {} FROM {input} GROUP BY {} ORDER BY {}",
                    cols.join(", "),
                    idents(keys),
                    idents(keys)
                )
            }
        }
        Step::Unpivot {
            columns,
            attribute,
            value,
        } => format!(
            "SELECT * FROM (UNPIVOT {input} ON {} INTO NAME {} VALUE {})",
            columns
                .iter()
                .map(|c| format!(
                    "CAST({} AS VARCHAR) AS {}",
                    quote::ident(c),
                    quote::ident(c)
                ))
                .collect::<Vec<_>>()
                .join(", "),
            quote::ident(attribute),
            quote::ident(value)
        ),
        Step::Pivot {
            column,
            values_column,
            values,
            aggregate,
        } => format!(
            "SELECT * FROM (PIVOT {input} ON {} IN ({}) USING {})",
            quote::ident(column),
            values
                .iter()
                .map(|v| quote::string(v))
                .collect::<Vec<_>>()
                .join(", "),
            agg_sql(*aggregate, Some(values_column))
        ),
        Step::FillDown { columns } => {
            let cols: Vec<String> = columns
                .iter()
                .map(|c| {
                    let e = quote::ident(c);
                    format!("last_value({e} IGNORE NULLS) OVER (ORDER BY \"__row\" ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS {e}")
                })
                .collect();
            format!(
                "SELECT * EXCLUDE (\"__row\") REPLACE ({}) FROM (SELECT *, row_number() OVER () AS \"__row\" FROM {input}) ORDER BY \"__row\"",
                cols.join(", ")
            )
        }
        Step::AddIndex { name, start, step } => format!(
            "SELECT *, {start} + (row_number() OVER () - 1) * {step} AS {} FROM {input}",
            quote::ident(name)
        ),
        Step::ExtractText {
            column,
            extract,
            new_column,
        } => {
            let e = format!("CAST({} AS VARCHAR)", quote::ident(column));
            let x = match extract {
                Extract::Length => format!("length({e})"),
                Extract::First { count } => format!("left({e}, {count})"),
                Extract::Last { count } => format!("right({e}, {count})"),
                Extract::Range { start, count } => format!("substr({e}, {}, {count})", start + 1),
                Extract::BeforeDelimiter { delimiter } => {
                    let d = quote::string(delimiter);
                    format!(
                        "CASE WHEN strpos({e}, {d}) = 0 THEN {e} ELSE left({e}, strpos({e}, {d}) - 1) END"
                    )
                }
                Extract::AfterDelimiter { delimiter } => {
                    let d = quote::string(delimiter);
                    format!(
                        "CASE WHEN strpos({e}, {d}) = 0 THEN '' ELSE substr({e}, strpos({e}, {d}) + length({d})) END"
                    )
                }
            };
            match new_column {
                Some(n) => format!("SELECT *, {x} AS {} FROM {input}", quote::ident(n)),
                None => format!(
                    "SELECT * REPLACE ({x} AS {}) FROM {input}",
                    quote::ident(column)
                ),
            }
        }
        Step::FormatText { columns, format } => {
            let cols: Vec<String> = columns
                .iter()
                .map(|c| {
                    let e = format!("CAST({} AS VARCHAR)", quote::ident(c));
                    let x = match format {
                        TextOp::Lower => format!("lower({e})"),
                        TextOp::Upper => format!("upper({e})"),
                        TextOp::Trim => format!("trim({e})"),
                        TextOp::Clean => format!("regexp_replace({e}, '[\\x00-\\x1F\\x7F]', '', 'g')"),
                        TextOp::Capitalize => format!(
                            "array_to_string(list_transform(string_split(lower({e}), ' '), lambda w: upper(left(w, 1)) || substr(w, 2)), ' ')"
                        ),
                    };
                    format!("{x} AS {}", quote::ident(c))
                })
                .collect();
            format!("SELECT * REPLACE ({}) FROM {input}", cols.join(", "))
        }
        Step::SortRows { columns } => format!(
            "SELECT * FROM {input} ORDER BY {}",
            columns
                .iter()
                .map(|(c, d)| format!("{} {}", quote::ident(c), if *d { "DESC" } else { "ASC" }))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Step::MergeQueries {
            query,
            left,
            right,
            kind,
            columns,
            prefix,
        } => {
            if left.len() != right.len() || left.is_empty() {
                return Err(StepError::Invalid(
                    "choose the same number of matching columns in both queries".into(),
                ));
            }
            let other = query_sql(lookup(ctx, query)?, ctx, seen)?;
            let on: Vec<String> = left
                .iter()
                .zip(right)
                .map(|(l, r)| format!("a.{} = b.{}", quote::ident(l), quote::ident(r)))
                .collect();
            let added: Vec<String> = columns
                .iter()
                .map(|c| {
                    let n = match prefix {
                        Some(p) if !p.is_empty() => format!("{p}.{c}"),
                        _ => c.clone(),
                    };
                    format!("b.{} AS {}", quote::ident(c), quote::ident(&n))
                })
                .collect();
            let select = std::iter::once("a.*".to_string())
                .chain(added)
                .collect::<Vec<_>>()
                .join(", ");
            match kind {
                JoinKind::LeftAnti => format!(
                    "SELECT a.* FROM {input} AS a ANTI JOIN ({other}) AS b ON {}",
                    on.join(" AND ")
                ),
                _ => {
                    let j = match kind {
                        JoinKind::LeftOuter => "LEFT JOIN",
                        JoinKind::RightOuter => "RIGHT JOIN",
                        JoinKind::FullOuter => "FULL JOIN",
                        _ => "JOIN",
                    };
                    format!(
                        "SELECT {select} FROM {input} AS a {j} ({other}) AS b ON {}",
                        on.join(" AND ")
                    )
                }
            }
        }
        Step::AppendQueries { queries } => {
            let mut parts = vec![format!("SELECT * FROM {input}")];
            for q in queries {
                parts.push(format!(
                    "SELECT * FROM ({})",
                    query_sql(lookup(ctx, q)?, ctx, seen)?
                ));
            }
            parts.join(" UNION ALL BY NAME ")
        }
    })
}

fn lookup<'a>(ctx: &'a Ctx, id: &str) -> Result<&'a Query, StepError> {
    ctx.queries
        .iter()
        .find(|q| q.id == id)
        .ok_or_else(|| StepError::UnknownQuery(id.to_string()))
}

fn query_sql(query: &Query, ctx: &Ctx, seen: &mut Vec<String>) -> Result<String, StepError> {
    sql_upto(query, query.steps.len(), ctx, seen)
}

fn sql_upto(
    query: &Query,
    upto: usize,
    ctx: &Ctx,
    seen: &mut Vec<String>,
) -> Result<String, StepError> {
    if seen.contains(&query.id) {
        let names: Vec<String> = seen
            .iter()
            .filter_map(|id| {
                ctx.queries
                    .iter()
                    .find(|q| &q.id == id)
                    .map(|q| format!("\"{}\"", q.name))
            })
            .collect();
        return Err(StepError::Cycle(names.join(", ")));
    }
    seen.push(query.id.clone());
    let source = source_sql(&query.source, ctx)?;
    let n = upto.min(query.steps.len());
    let out = if n == 0 {
        source
    } else {
        let mut ctes = vec![format!("s0 AS ({source})")];
        for (i, step) in query.steps.iter().take(n).enumerate() {
            ctes.push(format!(
                "s{} AS ({})",
                i + 1,
                step_sql(step, &format!("s{i}"), ctx, seen)?
            ));
        }
        format!("WITH {}\nSELECT * FROM s{n}", ctes.join(",\n"))
    };
    seen.pop();
    Ok(out)
}

/// Compiles a query to one SELECT statement, with each step as a CTE.
pub fn to_sql(query: &Query, ctx: &Ctx) -> Result<String, StepError> {
    query_sql(query, ctx, &mut Vec::new())
}

/// The SQL of the query's source and its first `upto` steps (the query editor's
/// preview of a selected step).
pub fn to_sql_upto(query: &Query, upto: usize, ctx: &Ctx) -> Result<String, StepError> {
    sql_upto(query, upto, ctx, &mut Vec::new())
}

/// An M string literal.
pub fn m_string(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// An M identifier: `#"Changed Type"` when it needs quoting.
pub fn m_ident(s: &str) -> String {
    if !s.is_empty()
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit())
    {
        s.to_string()
    } else {
        format!("#{}", m_string(s))
    }
}

pub fn m_type(t: DataType) -> &'static str {
    match t {
        DataType::String => "type text",
        DataType::Int64 => "Int64.Type",
        DataType::Double => "type number",
        DataType::Decimal => "Currency.Type",
        DataType::Boolean => "type logical",
        DataType::Date => "type date",
        DataType::Time => "type time",
        DataType::DateTime => "type datetime",
        DataType::DateTimeTz => "type datetimezone",
        DataType::Binary | DataType::Unknown => "type any",
    }
}

/// A literal in M: numbers and true/false as such, everything else as text.
fn m_literal(v: &str) -> String {
    if v.parse::<f64>().is_ok() && !v.trim().is_empty() {
        v.trim().to_string()
    } else if v == "true" || v == "false" {
        v.to_string()
    } else {
        m_string(v)
    }
}

fn m_list(xs: &[String]) -> String {
    format!(
        "{{{}}}",
        xs.iter()
            .map(|x| m_string(x))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn m_condition(col: &str, c: &Condition) -> String {
    let x = format!("[{col}]");
    let v = c.value.as_deref().unwrap_or("");
    let lit = m_literal(v);
    match c.op {
        FilterOp::Equals => format!("{x} = {lit}"),
        FilterOp::NotEquals => format!("{x} <> {lit}"),
        FilterOp::Contains => format!("Text.Contains({x}, {})", m_string(v)),
        FilterOp::NotContains => format!("not Text.Contains({x}, {})", m_string(v)),
        FilterOp::StartsWith => format!("Text.StartsWith({x}, {})", m_string(v)),
        FilterOp::EndsWith => format!("Text.EndsWith({x}, {})", m_string(v)),
        FilterOp::Less => format!("{x} < {lit}"),
        FilterOp::LessOrEqual => format!("{x} <= {lit}"),
        FilterOp::Greater => format!("{x} > {lit}"),
        FilterOp::GreaterOrEqual => format!("{x} >= {lit}"),
        FilterOp::Between => format!(
            "{x} >= {lit} and {x} <= {}",
            m_literal(c.value2.as_deref().unwrap_or(""))
        ),
        FilterOp::IsBlank => format!("({x} = null or {x} = \"\")"),
        FilterOp::IsNotBlank => format!("({x} <> null and {x} <> \"\")"),
    }
}

fn m_agg(a: &Aggregate) -> String {
    let c = a
        .column
        .as_deref()
        .map(|c| format!("[{c}]"))
        .unwrap_or_default();
    let f = match a.op {
        AggOp::Sum => format!("List.Sum({c})"),
        AggOp::Average => format!("List.Average({c})"),
        AggOp::Min => format!("List.Min({c})"),
        AggOp::Max => format!("List.Max({c})"),
        AggOp::Count => format!("List.NonNullCount({c})"),
        AggOp::CountDistinct => format!("List.Count(List.Distinct({c}))"),
        AggOp::CountRows => "Table.RowCount(_)".into(),
    };
    format!("{{{}, each {f}}}", m_string(&a.name))
}

/// The query's M steps: (step name, expression), the source steps first. Files are
/// read relative to the `SourceFolder` parameter unless absolute; other queries are
/// referred to by name.
pub fn m_steps(q: &Query, queries: &[Query]) -> Vec<(String, String)> {
    let mut steps: Vec<(String, String)> = Vec::new();
    let file = |p: &str| {
        if p.starts_with('/') {
            format!("File.Contents({})", m_string(p))
        } else {
            format!("File.Contents(SourceFolder & {})", m_string(p))
        }
    };
    let promote = |prev: &str| {
        format!(
            "Table.PromoteHeaders({}, [PromoteAllScalars=true])",
            m_ident(prev)
        )
    };
    match &q.source {
        Source::Csv {
            path,
            delimiter,
            header,
            ..
        } => {
            let delim = delimiter.clone().unwrap_or_else(|| ",".into());
            steps.push((
                "Source".into(),
                format!(
                    "Csv.Document({}, [Delimiter={}, Encoding=65001, QuoteStyle=QuoteStyle.Csv])",
                    file(path),
                    m_string(&delim)
                ),
            ));
            if *header {
                steps.push(("Promoted Headers".into(), promote("Source")));
            }
        }
        Source::Calendar {
            start,
            end,
            fiscal_start,
        } => {
            let m_date = |d: &str| -> String {
                let p: Vec<&str> = d.split('-').collect();
                let n = |x: &str| x.trim_start_matches('0').to_string();
                format!("#date({}, {}, {})", p[0], n(p[1]), n(p[2]))
            };
            let (a, b) = (m_date(start), m_date(end));
            steps.push((
                "Source".into(),
                format!("List.Dates({a}, Duration.Days({b} - {a}) + 1, #duration(1, 0, 0, 0))"),
            ));
            steps.push((
                "To Table".into(),
                "Table.FromList(Source, Splitter.SplitByNothing(), type table [Date = date])".into(),
            ));
            let fs = (*fiscal_start).clamp(1, 12);
            let fiscal = if fs == 1 {
                "\"FY\" & Text.From(Date.Year([Date]))".to_string()
            } else {
                format!("\"FY\" & Text.From(Date.Year([Date]) + (if Date.Month([Date]) >= {fs} then 1 else 0))")
            };
            let mut prev = "To Table".to_string();
            for (name, expr, ty) in [
                ("Year", "Date.Year([Date])".to_string(), "Int64.Type"),
                ("Quarter", "\"Q\" & Text.From(Date.QuarterOfYear([Date]))".into(), "type text"),
                ("MonthNumber", "Date.Month([Date])".into(), "Int64.Type"),
                ("MonthName", "Date.MonthName([Date], \"en-US\")".into(), "type text"),
                ("YearMonth", "Date.ToText([Date], \"yyyy-MM\")".into(), "type text"),
                ("Weekday", "Date.DayOfWeekName([Date], \"en-US\")".into(), "type text"),
                ("WeekdayNumber", "Date.DayOfWeek([Date], Day.Monday) + 1".into(), "Int64.Type"),
                ("FiscalYear", fiscal.clone(), "type text"),
            ] {
                let step = format!("Added {name}");
                steps.push((
                    step.clone(),
                    format!("Table.AddColumn({}, {}, each {expr}, {ty})", m_ident(&prev), m_string(name)),
                ));
                prev = step;
            }
        }
        Source::Parquet { path } => {
            steps.push(("Source".into(), format!("Parquet.Document({})", file(path))))
        }
        Source::Json { path } => {
            steps.push(("Source".into(), format!("Json.Document({})", file(path))));
            steps.push(("To Table".into(), "Table.FromRecords(Source)".into()));
        }
        Source::Excel {
            path,
            sheet,
            header,
        } => {
            steps.push((
                "Source".into(),
                format!("Excel.Workbook({}, null, true)", file(path)),
            ));
            steps.push((
                "Sheet".into(),
                format!("Source{{[Item={},Kind=\"Sheet\"]}}[Data]", m_string(sheet)),
            ));
            if *header {
                steps.push(("Promoted Headers".into(), promote("Sheet")));
            }
        }
        Source::Folder {
            path,
            pattern,
            header,
        } => {
            let folder = if path.starts_with('/') {
                m_string(path)
            } else {
                format!("SourceFolder & {}", m_string(path))
            };
            let ext = pattern.trim_start_matches('*');
            steps.push(("Source".into(), format!("Folder.Files({folder})")));
            steps.push((
                "Filtered Files".into(),
                format!(
                    "Table.SelectRows(Source, each Text.EndsWith([Name], {}))",
                    m_string(ext)
                ),
            ));
            let read = if *header {
                "Table.PromoteHeaders(Csv.Document([Content]), [PromoteAllScalars=true])"
            } else {
                "Csv.Document([Content])"
            };
            steps.push((
                "Read Files".into(),
                format!("Table.AddColumn(#\"Filtered Files\", \"Data\", each {read})"),
            ));
            steps.push((
                "Combined".into(),
                "Table.Combine(Table.TransformRows(#\"Read Files\", each Table.AddColumn([Data], \"Source.Name\", (r) => [Name])))".into(),
            ));
        }
        Source::Sqlite { path, table } => {
            steps.push((
                "Source".into(),
                format!(
                    "Odbc.DataSource(\"database=\" & {})",
                    if path.starts_with('/') {
                        m_string(path)
                    } else {
                        format!("SourceFolder & {}", m_string(path))
                    }
                ),
            ));
            steps.push((
                "Table".into(),
                format!("Source{{[Name={}]}}[Data]", m_string(table)),
            ));
        }
        Source::Postgres {
            host,
            port,
            database,
            schema,
            table,
            ..
        } => {
            steps.push((
                "Source".into(),
                format!(
                    "PostgreSQL.Database({}, {})",
                    m_string(&format!("{host}:{port}")),
                    m_string(database)
                ),
            ));
            steps.push((
                "Table".into(),
                format!(
                    "Source{{[Schema={},Item={}]}}[Data]",
                    m_string(schema),
                    m_string(table)
                ),
            ));
        }
    }
    let name_of = |id: &str| {
        queries
            .iter()
            .find(|x| x.id == id)
            .map(|x| m_ident(&x.name))
            .unwrap_or_else(|| m_ident(id))
    };
    let names = q.step_display_names();
    for (i, s) in q.steps.iter().enumerate() {
        let prev = m_ident(&steps.last().unwrap().0);
        let e = match s {
            Step::RemoveTopRows { count } => format!("Table.Skip({prev}, {count})"),
            Step::KeepTopRows { count } => format!("Table.FirstN({prev}, {count})"),
            Step::PromoteHeaders { .. } => {
                format!("Table.PromoteHeaders({prev}, [PromoteAllScalars=true])")
            }
            Step::ChangeTypes { columns } => format!(
                "Table.TransformColumnTypes({prev}, {{{}}})",
                columns
                    .iter()
                    .map(|(c, t)| format!("{{{}, {}}}", m_string(c), m_type(*t)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Step::SelectColumns { columns } => {
                format!("Table.SelectColumns({prev}, {})", m_list(columns))
            }
            Step::RemoveColumns { columns } => {
                format!("Table.RemoveColumns({prev}, {})", m_list(columns))
            }
            Step::RenameColumns { renames } => format!(
                "Table.RenameColumns({prev}, {{{}}})",
                renames
                    .iter()
                    .map(|(a, b)| format!("{{{}, {}}}", m_string(a), m_string(b)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Step::FilterRows {
                column,
                conditions,
                or,
            } => {
                let parts: Vec<String> =
                    conditions.iter().map(|c| m_condition(column, c)).collect();
                format!(
                    "Table.SelectRows({prev}, each {})",
                    if parts.is_empty() {
                        "true".into()
                    } else {
                        parts.join(if *or { " or " } else { " and " })
                    }
                )
            }
            Step::RemoveDuplicates { columns } => {
                if columns.is_empty() {
                    format!("Table.Distinct({prev})")
                } else {
                    format!("Table.Distinct({prev}, {})", m_list(columns))
                }
            }
            Step::RemoveBlankRows => format!(
                "Table.SelectRows({prev}, each not List.IsEmpty(List.RemoveMatchingItems(Record.FieldValues(_), {{\"\", null}})))"
            ),
            Step::ReplaceValues {
                columns,
                find,
                replace,
                whole,
            } => format!(
                "Table.ReplaceValue({prev}, {}, {}, {}, {})",
                if *whole && find.is_empty() {
                    "null".into()
                } else {
                    m_string(find)
                },
                m_string(replace),
                if *whole {
                    "Replacer.ReplaceValue"
                } else {
                    "Replacer.ReplaceText"
                },
                m_list(columns)
            ),
            Step::SplitColumn {
                column,
                delimiter,
                mode,
                names,
            } => {
                let splitter = match mode {
                    SplitMode::Each => format!(
                        "Splitter.SplitTextByDelimiter({}, QuoteStyle.Csv)",
                        m_string(delimiter)
                    ),
                    SplitMode::Left => format!(
                        "Splitter.SplitTextByEachDelimiter({{{}}}, QuoteStyle.Csv, false)",
                        m_string(delimiter)
                    ),
                    SplitMode::Right => format!(
                        "Splitter.SplitTextByEachDelimiter({{{}}}, QuoteStyle.Csv, true)",
                        m_string(delimiter)
                    ),
                };
                format!(
                    "Table.SplitColumn({prev}, {}, {splitter}, {})",
                    m_string(column),
                    m_list(names)
                )
            }
            Step::MergeColumns {
                columns,
                separator,
                name,
            } => format!(
                "Table.CombineColumns(Table.TransformColumnTypes({prev}, {{{}}}), {}, Combiner.CombineTextByDelimiter({}, QuoteStyle.None), {})",
                columns
                    .iter()
                    .map(|c| format!("{{{}, type text}}", m_string(c)))
                    .collect::<Vec<_>>()
                    .join(", "),
                m_list(columns),
                m_string(separator),
                m_string(name)
            ),
            Step::CustomColumn { name, formula } => {
                let body = mformula::parse(formula, &[])
                    .map(|e| mformula::to_m(&e))
                    .unwrap_or_else(|_| formula.clone());
                format!("Table.AddColumn({prev}, {}, each {body})", m_string(name))
            }
            Step::ConditionalColumn {
                name,
                rules,
                otherwise,
            } => {
                let mut body = otherwise
                    .as_deref()
                    .map(m_literal)
                    .unwrap_or_else(|| "null".into());
                for r in rules.iter().rev() {
                    let c = Condition {
                        op: r.op,
                        value: r.value.clone(),
                        value2: None,
                    };
                    body = format!(
                        "if {} then {} else {body}",
                        m_condition(&r.column, &c),
                        m_literal(&r.output)
                    );
                }
                format!("Table.AddColumn({prev}, {}, each {body})", m_string(name))
            }
            Step::GroupBy { keys, aggregates } => format!(
                "Table.Group({prev}, {}, {{{}}})",
                m_list(keys),
                aggregates.iter().map(m_agg).collect::<Vec<_>>().join(", ")
            ),
            Step::Unpivot {
                columns,
                attribute,
                value,
            } => format!(
                "Table.Unpivot({prev}, {}, {}, {})",
                m_list(columns),
                m_string(attribute),
                m_string(value)
            ),
            Step::Pivot {
                column,
                values_column,
                values,
                aggregate,
            } => {
                let f = match aggregate {
                    AggOp::Sum => "List.Sum",
                    AggOp::Average => "List.Average",
                    AggOp::Min => "List.Min",
                    AggOp::Max => "List.Max",
                    AggOp::Count | AggOp::CountRows => "List.NonNullCount",
                    AggOp::CountDistinct => "each List.Count(List.Distinct(_))",
                };
                format!(
                    "Table.Pivot({prev}, {}, {}, {}, {f})",
                    m_list(values),
                    m_string(column),
                    m_string(values_column)
                )
            }
            Step::FillDown { columns } => format!("Table.FillDown({prev}, {})", m_list(columns)),
            Step::AddIndex { name, start, step } => {
                format!(
                    "Table.AddIndexColumn({prev}, {}, {start}, {step}, Int64.Type)",
                    m_string(name)
                )
            }
            Step::ExtractText {
                column,
                extract,
                new_column,
            } => {
                let f = match extract {
                    Extract::Length => "Text.Length".to_string(),
                    Extract::First { count } => format!("each Text.Start(_, {count})"),
                    Extract::Last { count } => format!("each Text.End(_, {count})"),
                    Extract::Range { start, count } => {
                        format!("each Text.Middle(_, {start}, {count})")
                    }
                    Extract::BeforeDelimiter { delimiter } => {
                        format!("each Text.BeforeDelimiter(_, {})", m_string(delimiter))
                    }
                    Extract::AfterDelimiter { delimiter } => {
                        format!("each Text.AfterDelimiter(_, {})", m_string(delimiter))
                    }
                };
                match new_column {
                    Some(n) => {
                        let body = f
                            .strip_prefix("each ")
                            .map(|b| b.replace('_', &format!("[{column}]")))
                            .unwrap_or_else(|| format!("{f}([{column}])"));
                        format!("Table.AddColumn({prev}, {}, each {body})", m_string(n))
                    }
                    None => format!(
                        "Table.TransformColumns({prev}, {{{{{}, {f}, type text}}}})",
                        m_string(column)
                    ),
                }
            }
            Step::FormatText { columns, format } => {
                let f = match format {
                    TextOp::Lower => "Text.Lower",
                    TextOp::Upper => "Text.Upper",
                    TextOp::Trim => "Text.Trim",
                    TextOp::Clean => "Text.Clean",
                    TextOp::Capitalize => "Text.Proper",
                };
                format!(
                    "Table.TransformColumns({prev}, {{{}}})",
                    columns
                        .iter()
                        .map(|c| format!("{{{}, {f}, type text}}", m_string(c)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Step::SortRows { columns } => format!(
                "Table.Sort({prev}, {{{}}})",
                columns
                    .iter()
                    .map(|(c, d)| format!(
                        "{{{}, {}}}",
                        m_string(c),
                        if *d {
                            "Order.Descending"
                        } else {
                            "Order.Ascending"
                        }
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Step::MergeQueries {
                query,
                left,
                right,
                kind,
                columns,
                prefix,
            } => {
                let other = name_of(query);
                let k = match kind {
                    JoinKind::LeftOuter => "JoinKind.LeftOuter",
                    JoinKind::RightOuter => "JoinKind.RightOuter",
                    JoinKind::FullOuter => "JoinKind.FullOuter",
                    JoinKind::Inner => "JoinKind.Inner",
                    JoinKind::LeftAnti => "JoinKind.LeftAnti",
                };
                let nested = format!(
                    "Table.NestedJoin({prev}, {}, {other}, {}, \"__merged\", {k})",
                    m_list(left),
                    m_list(right)
                );
                if *kind == JoinKind::LeftAnti {
                    format!("Table.RemoveColumns({nested}, {{\"__merged\"}})")
                } else {
                    let new_names: Vec<String> = columns
                        .iter()
                        .map(|c| match prefix {
                            Some(p) if !p.is_empty() => format!("{p}.{c}"),
                            _ => c.clone(),
                        })
                        .collect();
                    format!(
                        "Table.ExpandTableColumn({nested}, \"__merged\", {}, {})",
                        m_list(columns),
                        m_list(&new_names)
                    )
                }
            }
            Step::AppendQueries { queries: others } => format!(
                "Table.Combine({{{prev}, {}}})",
                others
                    .iter()
                    .map(|o| name_of(o))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        let mut name = names[i + 1].clone();
        let mut k = 2;
        while steps.iter().any(|(x, _)| *x == name) {
            name = format!("{} {k}", names[i + 1]);
            k += 1;
        }
        steps.push((name, e));
    }
    steps
}

/// The query as an M `let … in` expression.
pub fn to_m(q: &Query, queries: &[Query]) -> String {
    m_document(&m_steps(q, queries))
}

pub fn m_document(steps: &[(String, String)]) -> String {
    let mut out = String::from("let\n");
    for (i, (n, e)) in steps.iter().enumerate() {
        let comma = if i + 1 < steps.len() { "," } else { "" };
        out.push_str(&format!("    {} = {e}{comma}\n", m_ident(n)));
    }
    out.push_str(&format!(
        "in\n    {}",
        m_ident(&steps.last().map(|s| s.0.clone()).unwrap_or_default())
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(steps: Vec<Step>) -> Query {
        Query {
            id: "q1".into(),
            name: "Stores".into(),
            table: "store".into(),
            source: Source::Csv {
                path: "data/it's.csv".into(),
                delimiter: Some(";".into()),
                header: true,
                encoding: None,
            },
            steps,
            step_names: Default::default(),
            no_load: false,
        }
    }

    fn ctx<'a>(queries: &'a [Query], sheets: &'a BTreeMap<String, String>) -> Ctx<'a> {
        Ctx {
            project_dir: "/home/me/Sales",
            queries,
            sheets,
        }
    }

    #[test]
    fn compiles_sources_and_steps() {
        let query = q(vec![
            Step::RemoveColumns {
                columns: vec!["junk".into()],
            },
            Step::RenameColumns {
                renames: vec![("City Name".into(), "City".into())],
            },
        ]);
        let sheets = BTreeMap::new();
        insta::assert_snapshot!(to_sql(&query, &ctx(&[], &sheets)).unwrap(), @r#"
        WITH s0 AS (SELECT * FROM read_csv('/home/me/Sales/data/it''s.csv', header = true, delim = ';')),
        s1 AS (SELECT * EXCLUDE ("junk") FROM s0),
        s2 AS (SELECT * RENAME ("City Name" AS "City") FROM s1)
        SELECT * FROM s2
        "#);
        let p = Query {
            steps: vec![],
            source: Source::Parquet {
                path: "/abs/sales.parquet".into(),
            },
            ..query.clone()
        };
        assert_eq!(
            to_sql(&p, &ctx(&[], &sheets)).unwrap(),
            "SELECT * FROM read_parquet('/abs/sales.parquet')"
        );
        assert_eq!(
            to_sql_upto(&query, 1, &ctx(&[], &sheets))
                .unwrap()
                .lines()
                .count(),
            3
        );
    }

    #[test]
    fn merges_and_appends_inline_other_queries_and_catch_loops() {
        let store = Query {
            id: "store".into(),
            name: "Store".into(),
            ..q(vec![])
        };
        let mut raw = q(vec![Step::MergeQueries {
            query: "store".into(),
            left: vec!["Code".into()],
            right: vec!["StoreCode".into()],
            kind: JoinKind::LeftOuter,
            columns: vec!["Region".into()],
            prefix: Some("Store".into()),
        }]);
        raw.id = "raw".into();
        let all = vec![store.clone(), raw.clone()];
        let sheets = BTreeMap::new();
        let sql = to_sql(&raw, &ctx(&all, &sheets)).unwrap();
        assert!(
            sql.contains(r#"LEFT JOIN (SELECT * FROM read_csv"#),
            "{sql}"
        );
        assert!(sql.contains(r#"b."Region" AS "Store.Region""#), "{sql}");
        let m = to_m(&raw, &all);
        assert!(
            m.contains(
                "Table.NestedJoin(#\"Promoted Headers\", {\"Code\"}, Store, {\"StoreCode\"}"
            ),
            "{m}"
        );
        // A loop is an error, not a stack overflow.
        let looped = Query {
            steps: vec![Step::AppendQueries {
                queries: vec!["raw".into()],
            }],
            ..store
        };
        let all = vec![looped, raw.clone()];
        assert!(matches!(
            to_sql(&raw, &ctx(&all, &sheets)),
            Err(StepError::Cycle(_))
        ));
    }

    #[test]
    fn step_names_follow_power_query() {
        let mut query = q(vec![
            Step::ChangeTypes {
                columns: vec![("A".into(), DataType::Int64)],
            },
            Step::ChangeTypes {
                columns: vec![("B".into(), DataType::Date)],
            },
            Step::RemoveBlankRows,
        ]);
        query.step_names.insert(2, "Drop empties".into());
        assert_eq!(
            query.step_display_names(),
            ["Source", "Changed Type", "Changed Type 2", "Drop empties"]
        );
        let m = to_m(&query, &[]);
        assert!(m.contains("#\"Changed Type 2\" = Table.TransformColumnTypes(#\"Changed Type\", {{\"B\", type date}})"), "{m}");
        assert!(m.ends_with("in\n    #\"Drop empties\""), "{m}");
    }
}
