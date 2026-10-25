//! DAX golden cases: every measure below is evaluated in several visual shapes by the
//! SQL compiler (on DuckDB) and by the reference interpreter, and the results must
//! agree. A snapshot of the interpreter's answers (tests/snapshots) records them so
//! changes in semantics show up in review.

use arrow_ipc::reader::StreamReader;
use charted_compiler::interp::{self, Data, TableData, Value};
use charted_compiler::visual::{self, OrderBy, OrderTarget, QueryFilter, VisualQuery};
use charted_engine::{Database, DbOptions, sql};
use charted_model::*;
use std::collections::HashMap;

/// (name, table, column type, values) for each column of each table.
struct Fixture {
    model: Model,
    db: Database,
    data: Data,
}

fn col(table: &str, name: &str, ty: DataType) -> Column {
    Column {
        id: format!("{table}.{name}"),
        name: name.into(),
        data_type: ty,
        kind: ColumnKind::Data {
            source_column: name.into(),
        },
        description: None,
        format_string: None,
        summarize_by: Summarize::Default,
        data_category: None,
        sort_by: None,
        display_folder: None,
        hidden: false,
        is_key: false,
        extras: Default::default(),
    }
}

fn rel(id: &str, from: (&str, &str), to: (&str, &str), active: bool) -> Relationship {
    Relationship {
        id: id.into(),
        from_table: from.0.into(),
        from_column: format!("{}.{}", from.0, from.1),
        to_table: to.0.into(),
        to_column: format!("{}.{}", to.0, to.1),
        from_cardinality: Cardinality::Many,
        to_cardinality: Cardinality::One,
        cross_filter: CrossFilter::Single,
        active,
        extras: Default::default(),
    }
}

/// A table's name, its columns (name, type) and its rows.
type FixtureTable = (String, Vec<(String, DataType)>, Vec<Vec<Value>>);

fn n(x: f64) -> Value {
    Value::Num(x)
}
fn t(s: &str) -> Value {
    Value::Text(s.into())
}

/// A small, irregular star: blanks, a sale of an unknown product, months without sales,
/// an inactive ship-date relationship and a second fact table (targets).
fn fixture() -> Fixture {
    let mut tables: Vec<FixtureTable> = Vec::new();
    let products = [
        (1, "Tent", "Camping", "Green", 300.0),
        (2, "Stove", "Camping", "Black", 120.0),
        (3, "Lamp", "Camping", "Green", 40.0),
        (4, "Boots", "Footwear", "Brown", 150.0),
        (5, "Sandals", "Footwear", "Black", 60.0),
        (6, "Jacket", "Apparel", "Red", 200.0),
        (7, "Cap", "Apparel", "", 25.0),
        (8, "Scarf", "Apparel", "Red", 30.0),
    ];
    tables.push((
        "Product".into(),
        vec![
            ("ProductKey".into(), DataType::Int64),
            ("Product".into(), DataType::String),
            ("Category".into(), DataType::String),
            ("Color".into(), DataType::String),
            ("ListPrice".into(), DataType::Double),
        ],
        products
            .iter()
            .map(|p| {
                vec![
                    n(p.0 as f64),
                    t(p.1),
                    t(p.2),
                    if p.3.is_empty() { Value::Blank } else { t(p.3) },
                    n(p.4),
                ]
            })
            .collect(),
    ));
    let months = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut dates = Vec::new();
    // Each month's last day: time intelligence keeps whole months whole.
    let last_day = |y: i32, m: i32| match m {
        2 if y % 4 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    for y in [2024, 2025] {
        for m in 1..=12 {
            dates.push(vec![
                t(&format!("{y}-{m:02}-{:02}", last_day(y, m))),
                n((y * 100 + m) as f64),
                n(y as f64),
                n(m as f64),
                t(months[(m - 1) as usize]),
                n(((m - 1) / 3 + 1) as f64),
            ]);
        }
    }
    tables.push((
        "Date".into(),
        vec![
            ("Date".into(), DataType::Date),
            ("DateKey".into(), DataType::Int64),
            ("Year".into(), DataType::Int64),
            ("MonthNumber".into(), DataType::Int64),
            ("MonthName".into(), DataType::String),
            ("Quarter".into(), DataType::Int64),
        ],
        dates,
    ));
    tables.push((
        "Store".into(),
        vec![
            ("StoreKey".into(), DataType::Int64),
            ("Region".into(), DataType::String),
            ("Channel".into(), DataType::String),
        ],
        vec![
            vec![n(1.0), t("North"), t("Store")],
            vec![n(2.0), t("South"), t("Store")],
            vec![n(3.0), t("Online"), t("Web")],
        ],
    ));
    // Deterministic, irregular sales.
    let mut sales = Vec::new();
    let mut x: u64 = 7;
    let mut next = |m: u64| {
        x = x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (x >> 33) % m
    };
    for i in 0..90 {
        let y = if i % 3 == 0 { 2024 } else { 2025 };
        let m = 1 + next(10); // no sales in November and December
        let pk = if i == 17 { 99 } else { 1 + next(8) };
        let qty = 1 + next(5);
        let price = [300.0, 120.0, 40.0, 150.0, 60.0, 200.0, 25.0, 30.0][(pk as usize - 1).min(7)]
            * (0.8 + 0.1 * next(3) as f64);
        let amount = if i % 11 == 5 {
            Value::Blank
        } else {
            n((qty as f64 * price * 100.0).round() / 100.0)
        };
        let ship_m = (m + next(2)).min(12);
        sales.push(vec![
            n(i as f64 + 1.0),
            n(pk as f64),
            n((y * 100 + m) as f64),
            n((y * 100 + ship_m) as f64),
            n(1.0 + next(3) as f64),
            n(qty as f64),
            n(price),
            amount,
        ]);
    }
    tables.push((
        "Sales".into(),
        vec![
            ("OrderId".into(), DataType::Int64),
            ("ProductKey".into(), DataType::Int64),
            ("OrderDateKey".into(), DataType::Int64),
            ("ShipDateKey".into(), DataType::Int64),
            ("StoreKey".into(), DataType::Int64),
            ("Qty".into(), DataType::Int64),
            ("Price".into(), DataType::Double),
            ("Amount".into(), DataType::Double),
        ],
        sales,
    ));
    let mut targets = Vec::new();
    for y in [2024, 2025] {
        for m in 1..=12 {
            for s in 1..=3 {
                if (m + s) % 4 != 0 {
                    targets.push(vec![
                        n((y * 100 + m) as f64),
                        n(s as f64),
                        n((1000 + 100 * s + 10 * m) as f64),
                    ]);
                }
            }
        }
    }
    tables.push((
        "Targets".into(),
        vec![
            ("DateKey".into(), DataType::Int64),
            ("StoreKey".into(), DataType::Int64),
            ("Target".into(), DataType::Double),
        ],
        targets,
    ));

    let db = Database::open(None, &DbOptions::default()).unwrap();
    let mut model = Model {
        name: "golden".into(),
        ..Default::default()
    };
    let mut data = Data {
        tables: HashMap::new(),
    };
    for (name, cols, rows) in &tables {
        let ty = |d: &DataType| match d {
            DataType::Int64 => "BIGINT",
            DataType::Double => "DOUBLE",
            DataType::Date => "DATE",
            _ => "VARCHAR",
        };
        let defs: Vec<String> = cols
            .iter()
            .map(|(c, d)| format!("{} {}", sql::ident(c), ty(d)))
            .collect();
        db.execute(&format!(
            "CREATE TABLE {} ({})",
            sql::ident(&name.to_lowercase()),
            defs.join(", ")
        ))
        .unwrap();
        for r in rows {
            let vals: Vec<String> = r
                .iter()
                .map(|v| match v {
                    Value::Blank => "NULL".into(),
                    Value::Num(x) => format!("{x}"),
                    Value::Text(s) => sql::string(s),
                    Value::Bool(b) => b.to_string(),
                })
                .collect();
            db.execute(&format!(
                "INSERT INTO {} VALUES ({})",
                sql::ident(&name.to_lowercase()),
                vals.join(", ")
            ))
            .unwrap();
        }
        let columns: Vec<Column> = cols.iter().map(|(c, d)| col(name, c, *d)).collect();
        data.tables.insert(
            name.clone(),
            TableData {
                columns: columns.iter().map(|c| c.id.clone()).collect(),
                rows: rows.clone(),
            },
        );
        model.tables.push(Table {
            id: name.clone(),
            name: name.clone(),
            description: None,
            source: name.to_lowercase(),
            columns,
            measures: vec![],
            hidden: false,
            is_date_table: name == "Date",
            extras: Default::default(),
        });
    }
    let t = model.table_mut("Date").unwrap();
    t.column_mut("Date.MonthName").unwrap().sort_by = Some("Date.MonthNumber".into());
    model
        .table_mut("Product")
        .unwrap()
        .column_mut("Product.ProductKey")
        .unwrap()
        .is_key = true;
    model.relationships = vec![
        rel(
            "r-product",
            ("Sales", "ProductKey"),
            ("Product", "ProductKey"),
            true,
        ),
        rel(
            "r-order",
            ("Sales", "OrderDateKey"),
            ("Date", "DateKey"),
            true,
        ),
        rel(
            "r-ship",
            ("Sales", "ShipDateKey"),
            ("Date", "DateKey"),
            false,
        ),
        rel(
            "r-store",
            ("Sales", "StoreKey"),
            ("Store", "StoreKey"),
            true,
        ),
        rel("r-tdate", ("Targets", "DateKey"), ("Date", "DateKey"), true),
        rel(
            "r-tstore",
            ("Targets", "StoreKey"),
            ("Store", "StoreKey"),
            true,
        ),
    ];
    let sales = model.table_mut("Sales").unwrap();
    for (name, expr) in MEASURES {
        sales.measures.push(Measure {
            id: format!("m:{name}"),
            name: name.to_string(),
            expression: expr.to_string(),
            language: ExpressionLanguage::Dax,
            description: None,
            format_string: None,
            display_folder: None,
            hidden: false,
            extras: Default::default(),
        });
    }
    Fixture { model, db, data }
}

/// The golden measures. Earlier ones are referenced by later ones.
const MEASURES: &[(&str, &str)] = &[
    // Aggregates
    ("Sales Amount", "SUM ( Sales[Amount] )"),
    ("Quantity", "SUM ( Sales[Qty] )"),
    ("Orders", "COUNTROWS ( Sales )"),
    ("Avg Price", "AVERAGE ( Sales[Price] )"),
    ("Min Price", "MIN ( Sales[Price] )"),
    ("Max Price", "MAX ( Sales[Price] )"),
    ("Products Sold", "DISTINCTCOUNT ( Sales[ProductKey] )"),
    ("Amounts", "COUNT ( Sales[Amount] )"),
    ("Missing Amounts", "COUNTBLANK ( Sales[Amount] )"),
    ("Target", "SUM ( Targets[Target] )"),
    ("Product Count", "COUNTROWS ( Product )"),
    ("Colors", "DISTINCTCOUNT ( Product[Color] )"),
    ("Max List Price", "MAX ( Product[ListPrice] )"),
    // Arithmetic and blanks
    ("Gap", "[Sales Amount] - [Target]"),
    ("Price Per Unit", "DIVIDE ( [Sales Amount], [Quantity] )"),
    ("Vs Target", "DIVIDE ( [Sales Amount], [Target], 0 )"),
    ("Double Qty", "[Quantity] * 2"),
    ("Neg Qty", "-[Quantity]"),
    ("Qty Squared", "[Quantity] ^ 2"),
    ("Rounded PPU", "ROUND ( [Price Per Unit], 1 )"),
    ("Abs Gap", "ABS ( [Gap] )"),
    ("Sales Or Target", "COALESCE ( [Sales Amount], [Target] )"),
    ("Plus Blank", "[Sales Amount] + BLANK ()"),
    ("Big", "IF ( [Quantity] > 10, \"High\", \"Low\" )"),
    ("Big Or Blank", "IF ( [Quantity] > 10, [Quantity] )"),
    (
        "Tier",
        "SWITCH ( TRUE (), [Quantity] >= 20, \"A\", [Quantity] >= 8, \"B\", \"C\" )",
    ),
    ("Has Missing", "IF ( ISBLANK ( [Missing Amounts] ), 0, 1 )"),
    (
        "And Or",
        "IF ( [Quantity] > 5 && NOT ( [Orders] < 2 ) || [Quantity] = 1, 1, 0 )",
    ),
    (
        "In List",
        "IF ( [Orders] IN { 1, 2, 3 }, \"few\", \"many\" )",
    ),
    (
        "Var Ratio",
        "VAR s = [Sales Amount] VAR q = [Quantity] RETURN DIVIDE ( s, q ) * 100",
    ),
    // Row iterators
    ("Line Total", "SUMX ( Sales, Sales[Qty] * Sales[Price] )"),
    ("Avg Line", "AVERAGEX ( Sales, Sales[Qty] * Sales[Price] )"),
    ("Max Line", "MAXX ( Sales, Sales[Qty] * Sales[Price] )"),
    (
        "List Value",
        "SUMX ( Sales, Sales[Qty] * RELATED ( Product[ListPrice] ) )",
    ),
    ("Discount", "[List Value] - [Line Total]"),
    (
        "Big Lines",
        "COUNTROWS ( FILTER ( Sales, Sales[Qty] >= 4 ) )",
    ),
    (
        "Green Qty",
        "SUMX ( FILTER ( Sales, RELATED ( Product[Color] ) = \"Green\" ), Sales[Qty] )",
    ),
    ("Counted Lines", "COUNTX ( Sales, Sales[Amount] )"),
    // Context transition
    (
        "Avg Per Category",
        "AVERAGEX ( VALUES ( Product[Category] ), [Sales Amount] )",
    ),
    (
        "Best Product",
        "MAXX ( VALUES ( Product[Product] ), [Quantity] )",
    ),
    (
        "Months With Sales",
        "COUNTROWS ( FILTER ( VALUES ( Date[MonthNumber] ), [Orders] > 0 ) )",
    ),
    (
        "Sum Of Max",
        "SUMX ( VALUES ( Store[Region] ), [Max Price] )",
    ),
    ("Per Product", "SUMX ( Product, [Quantity] )"),
    // CALCULATE
    (
        "Camping Sales",
        "CALCULATE ( [Sales Amount], Product[Category] = \"Camping\" )",
    ),
    (
        "Not Camping",
        "CALCULATE ( [Sales Amount], Product[Category] <> \"Camping\" )",
    ),
    (
        "All Products",
        "CALCULATE ( [Sales Amount], ALL ( Product ) )",
    ),
    (
        "Share Of Category",
        "DIVIDE ( [Sales Amount], CALCULATE ( [Sales Amount], ALL ( Product[Category] ) ) )",
    ),
    (
        "All Sales",
        "CALCULATE ( [Sales Amount], REMOVEFILTERS ( Sales ) )",
    ),
    ("All Years", "CALCULATE ( [Quantity], ALL ( Date[Year] ) )"),
    ("Year 2024", "CALCULATE ( [Quantity], Date[Year] = 2024 )"),
    (
        "Keep 2024",
        "CALCULATE ( [Quantity], KEEPFILTERS ( Date[Year] = 2024 ) )",
    ),
    (
        "Green Or Red",
        "CALCULATE ( [Quantity], Product[Color] IN { \"Green\", \"Red\" } )",
    ),
    (
        "Two Filters",
        "CALCULATE ( [Quantity], Product[Category] = \"Apparel\", Store[Region] = \"Online\" )",
    ),
    (
        "Big Orders Sales",
        "CALCULATE ( [Sales Amount], Sales[Qty] >= 3 )",
    ),
    (
        "Filter Table",
        "CALCULATE ( [Quantity], FILTER ( ALL ( Product[ListPrice] ), Product[ListPrice] > 100 ) )",
    ),
    (
        "Filter Rows",
        "CALCULATE ( [Quantity], FILTER ( Sales, Sales[Price] > 100 ) )",
    ),
    (
        "Except Category",
        "CALCULATE ( [Quantity], ALLEXCEPT ( Product, Product[Category] ) )",
    ),
    (
        "By Ship Date",
        "CALCULATE ( [Quantity], USERELATIONSHIP ( Sales[ShipDateKey], Date[DateKey] ) )",
    ),
    (
        "Ship Orders",
        "CALCULATE ( [Orders], USERELATIONSHIP ( Sales[ShipDateKey], Date[DateKey] ) )",
    ),
    (
        "Restore Values",
        "CALCULATE ( [Quantity], ALL ( Product ), VALUES ( Product[Category] ) )",
    ),
    (
        "All Selected",
        "CALCULATE ( [Quantity], ALLSELECTED ( Product[Category] ) )",
    ),
    (
        "Nested Calc",
        "CALCULATE ( CALCULATE ( [Quantity], ALL ( Product ) ), Product[Category] = \"Footwear\" )",
    ),
    ("Target All Stores", "CALCULATE ( [Target], ALL ( Store ) )"),
    (
        "Calc Var",
        "VAR all = CALCULATE ( [Quantity], ALL ( Product ) ) RETURN DIVIDE ( [Quantity], all )",
    ),
    (
        "Transition Sum",
        "SUMX ( VALUES ( Product[Category] ), CALCULATE ( [Quantity] ) )",
    ),
    (
        "Iterate Calc",
        "SUMX ( VALUES ( Date[Year] ), CALCULATE ( [Sales Amount], ALL ( Product ) ) )",
    ),
    // Context facts
    (
        "Selected Category",
        "SELECTEDVALUE ( Product[Category], \"Many\" )",
    ),
    (
        "One Year",
        "IF ( HASONEVALUE ( Date[Year] ), \"one\", \"many\" )",
    ),
    (
        "Category In Scope",
        "IF ( ISINSCOPE ( Product[Category] ), 1, 0 )",
    ),
    ("Year Filtered", "IF ( ISFILTERED ( Date[Year] ), 1, 0 )"),
    // Time intelligence
    (
        "Sales LY",
        "CALCULATE ( [Sales Amount], SAMEPERIODLASTYEAR ( Date[Date] ) )",
    ),
    (
        "YoY %",
        "DIVIDE ( [Sales Amount] - [Sales LY], [Sales LY] )",
    ),
    (
        "Qty Prev Month",
        "CALCULATE ( [Quantity], DATEADD ( Date[Date], -1, MONTH ) )",
    ),
    (
        "Qty Next Quarter",
        "CALCULATE ( [Quantity], DATEADD ( Date[Date], 1, QUARTER ) )",
    ),
    ("Sales YTD", "TOTALYTD ( [Sales Amount], Date[Date] )"),
    ("Qty FYTD", "TOTALYTD ( [Quantity], Date[Date], \"6/30\" )"),
    ("Qty QTD", "TOTALQTD ( [Quantity], Date[Date] )"),
    (
        "Qty MTD",
        "CALCULATE ( [Quantity], DATESMTD ( Date[Date] ) )",
    ),
    (
        "Camping YTD",
        "TOTALYTD ( [Quantity], Date[Date], Product[Category] = \"Camping\" )",
    ),
    (
        "Qty Parallel",
        "CALCULATE ( [Quantity], PARALLELPERIOD ( Date[Date], -1, YEAR ) )",
    ),
    (
        "Qty Prev Year",
        "CALCULATE ( [Quantity], PREVIOUSYEAR ( Date[Date] ) )",
    ),
    (
        "Qty Prev Quarter",
        "CALCULATE ( [Quantity], PREVIOUSQUARTER ( Date[Date] ) )",
    ),
    (
        "Qty Next Month",
        "CALCULATE ( [Quantity], NEXTMONTH ( Date[Date] ) )",
    ),
    (
        "Keep LY",
        "CALCULATE ( [Quantity], KEEPFILTERS ( SAMEPERIODLASTYEAR ( Date[Date] ) ) )",
    ),
    (
        "Target LY",
        "CALCULATE ( [Target], SAMEPERIODLASTYEAR ( Date[Date] ) )",
    ),
    (
        "Scoped Share",
        "IF ( ISINSCOPE ( Product[Category] ), [Share Of Category], BLANK () )",
    ),
];

/// The visual shapes every measure is evaluated in.
fn shapes(m: &Model) -> Vec<(&'static str, VisualQuery)> {
    let c = |t: &str, col: &str| Projection {
        field: FieldRef::Column {
            table: t.into(),
            column: format!("{t}.{col}"),
        },
        aggregation: None,
        display_name: None,
    };
    let _ = m;
    let year_2025 = QueryFilter {
        field: c("Date", "Year"),
        filter: FilterKind::Basic {
            values: vec![Some("2025".into())],
            exclude: false,
        },
    };
    vec![
        ("card", VisualQuery::default()),
        (
            "by category",
            VisualQuery {
                groups: vec![c("Product", "Category")],
                ..Default::default()
            },
        ),
        (
            "by year",
            VisualQuery {
                groups: vec![c("Date", "Year")],
                ..Default::default()
            },
        ),
        (
            "by category and year",
            VisualQuery {
                groups: vec![c("Product", "Category"), c("Date", "Year")],
                ..Default::default()
            },
        ),
        (
            "by region in 2025",
            VisualQuery {
                groups: vec![c("Store", "Region")],
                filters: vec![year_2025.clone()],
                ..Default::default()
            },
        ),
        (
            "by year and month",
            VisualQuery {
                groups: vec![c("Date", "Year"), c("Date", "MonthNumber")],
                ..Default::default()
            },
        ),
        (
            "by month name",
            VisualQuery {
                groups: vec![c("Date", "MonthName")],
                ..Default::default()
            },
        ),
        (
            "by color, apparel and camping",
            VisualQuery {
                groups: vec![c("Product", "Color")],
                filters: vec![QueryFilter {
                    field: c("Product", "Category"),
                    filter: FilterKind::Basic {
                        values: vec![Some("Apparel".into()), Some("Camping".into())],
                        exclude: false,
                    },
                }],
                ..Default::default()
            },
        ),
    ]
}

fn arrow_values(bytes: Vec<u8>) -> Vec<Vec<Value>> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        for r in 0..batch.num_rows() {
            let mut row = Vec::new();
            for c in batch.columns() {
                if c.is_null(r) {
                    row.push(Value::Blank);
                    continue;
                }
                let s = arrow_cast::display::ArrayFormatter::try_new(c, &Default::default())
                    .unwrap()
                    .value(r)
                    .to_string();
                let ty = c.data_type().to_string();
                row.push(if ty.contains("Utf8") {
                    Value::Text(s)
                } else if ty == "Boolean" {
                    Value::Bool(s == "true")
                } else {
                    s.parse::<f64>().map(Value::Num).unwrap_or(Value::Text(s))
                });
            }
            out.push(row);
        }
    }
    out
}

/// Rows in a fixed order (ordering is tested elsewhere; sort-by columns differ here).
fn canonical(mut rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    rows.sort_by_key(|r| r.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>());
    rows
}

fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => (x - y).abs() <= 1e-6 * (1.0 + x.abs().max(y.abs())),
        (Value::Bool(x), Value::Num(y)) | (Value::Num(y), Value::Bool(x)) => {
            (*x as i32 as f64 - y).abs() < 1e-9
        }
        _ => a == b,
    }
}

fn show(rows: &[Vec<Value>]) -> String {
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|v| match v {
                    Value::Blank => "∅".to_string(),
                    Value::Num(x) => {
                        let s = format!("{:.4}", x);
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    }
                    Value::Text(s) => format!("{s:?}"),
                    Value::Bool(b) => b.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn compiler_agrees_with_the_reference_interpreter() {
    let f = fixture();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let mut failures = Vec::new();
    let mut golden = String::new();
    let mut cases = 0;
    for (name, _) in MEASURES {
        let value = Projection {
            field: FieldRef::Measure {
                table: "Sales".into(),
                measure: format!("m:{name}"),
            },
            aggregation: None,
            display_name: None,
        };
        for (shape, base) in shapes(&f.model) {
            cases += 1;
            let q = VisualQuery {
                values: vec![value.clone()],
                order: (0..base.groups.len())
                    .map(|i| OrderBy {
                        target: OrderTarget::Group(i),
                        descending: false,
                    })
                    .collect(),
                ..base
            };
            let expected = match interp::run(&f.model, &f.data, &q) {
                Ok(r) => canonical(r),
                Err(e) => {
                    failures.push(format!("{name} / {shape}: the interpreter failed: {e}"));
                    continue;
                }
            };
            if shape == "card" || shape == "by category" {
                golden.push_str(&format!("## {name} — {shape}\n{}\n\n", show(&expected)));
            }
            let compiled = match visual::compile(&f.model, &schema, &q) {
                Ok(c) => c,
                Err(e) => {
                    failures.push(format!("{name} / {shape}: compile error: {e}"));
                    continue;
                }
            };
            let got = match f.db.arrow(&compiled.sql) {
                Ok(b) => canonical(arrow_values(b)),
                Err(e) => {
                    failures.push(format!(
                        "{name} / {shape}: SQL error: {e}\n{}",
                        compiled.sql
                    ));
                    continue;
                }
            };
            let ok = got.len() == expected.len()
                && got
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y)));
            if !ok {
                failures.push(format!(
                    "{name} / {shape}:\n  expected:\n{}\n  got:\n{}",
                    show(&expected).replace('\n', "\n    "),
                    show(&got).replace('\n', "\n    ")
                ));
            }
        }
    }
    insta::assert_snapshot!("golden", golden);
    assert!(
        failures.is_empty(),
        "{} of {cases} cases differ:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// A small deterministic generator (no external crates needed).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// A random measure expression built from the constructs the compiler supports.
fn random_expr(r: &mut Rng, depth: usize) -> String {
    const AGGS: &[&str] = &[
        "SUM ( Sales[Amount] )",
        "SUM ( Sales[Qty] )",
        "COUNTROWS ( Sales )",
        "MAX ( Sales[Price] )",
        "DISTINCTCOUNT ( Sales[ProductKey] )",
        "SUM ( Targets[Target] )",
        "COUNTROWS ( Product )",
        "[Sales Amount]",
        "[Quantity]",
        "[Orders]",
        "[Target]",
    ];
    const FILTERS: &[&str] = &[
        "Product[Category] = \"Camping\"",
        "Product[Category] <> \"Apparel\"",
        "Date[Year] = 2024",
        "Date[MonthNumber] <= 6",
        "ALL ( Product )",
        "ALL ( Date[Year] )",
        "ALL ( Product[Category] )",
        "REMOVEFILTERS ( Store )",
        "KEEPFILTERS ( Product[Color] = \"Red\" )",
        "Store[Region] <> \"North\"",
        "USERELATIONSHIP ( Sales[ShipDateKey], Date[DateKey] )",
        "VALUES ( Product[Category] )",
        "ALLEXCEPT ( Product, Product[Category] )",
        "Sales[Qty] > 2",
        "FILTER ( ALL ( Product[ListPrice] ), Product[ListPrice] >= 100 )",
        "ALLSELECTED ( Date[Year] )",
        "SAMEPERIODLASTYEAR ( Date[Date] )",
        "DATESYTD ( Date[Date] )",
        "DATEADD ( Date[Date], -1, QUARTER )",
        "PREVIOUSMONTH ( Date[Date] )",
    ];
    const ITERS: &[&str] = &[
        "SUMX ( VALUES ( Product[Category] ), {} )",
        "AVERAGEX ( VALUES ( Date[Year] ), {} )",
        "MAXX ( VALUES ( Store[Region] ), {} )",
        "SUMX ( FILTER ( VALUES ( Product[Color] ), [Quantity] > 5 ), {} )",
        "MINX ( VALUES ( Date[Quarter] ), {} )",
    ];
    if depth == 0 {
        return r.pick(AGGS).to_string();
    }
    match r.below(7) {
        0 | 1 => r.pick(AGGS).to_string(),
        2 => {
            let mut args = vec![random_expr(r, depth - 1)];
            for _ in 0..=r.below(2) {
                args.push(r.pick(FILTERS).to_string());
            }
            format!("CALCULATE ( {} )", args.join(", "))
        }
        3 => r.pick(ITERS).replace("{}", &random_expr(r, depth - 1)),
        4 => {
            let op = r.pick(&["+", "-", "*"]);
            format!(
                "( {} {op} {} )",
                random_expr(r, depth - 1),
                random_expr(r, depth - 1)
            )
        }
        5 => format!(
            "DIVIDE ( {}, {} )",
            random_expr(r, depth - 1),
            random_expr(r, depth - 1)
        ),
        _ => format!(
            "IF ( {} > {}, {}, {} )",
            random_expr(r, depth - 1),
            r.below(500),
            random_expr(r, depth - 1),
            random_expr(r, depth - 1)
        ),
    }
}

#[test]
fn random_measures_agree_with_the_reference_interpreter() {
    let cases: usize = std::env::var("CHARTED_DAX_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(256);
    let mut f = fixture();
    let schema_types: HashMap<(String, String), DataType> = f
        .model
        .tables
        .iter()
        .flat_map(|t| {
            t.columns.iter().filter_map(move |c| match &c.kind {
                ColumnKind::Data { source_column } => {
                    Some(((t.source.clone(), source_column.clone()), c.data_type))
                }
                _ => None,
            })
        })
        .collect();
    let schema = |source: &str, column: &str| {
        schema_types
            .get(&(source.to_string(), column.to_string()))
            .copied()
    };
    let seed = std::env::var("CHARTED_DAX_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);
    let mut r = Rng(seed);
    let mut failures = Vec::new();
    for i in 0..cases {
        let expr = random_expr(&mut r, 3);
        let id = format!("m:random{i}");
        f.model.table_mut("Sales").unwrap().measures.push(Measure {
            id: id.clone(),
            name: format!("Random {i}"),
            expression: expr.clone(),
            language: ExpressionLanguage::Dax,
            description: None,
            format_string: None,
            display_folder: None,
            hidden: false,
            extras: Default::default(),
        });
        let all = shapes(&f.model);
        let (shape, base) = all[r.below(all.len())].clone();
        let q = VisualQuery {
            values: vec![Projection {
                field: FieldRef::Measure {
                    table: "Sales".into(),
                    measure: id,
                },
                aggregation: None,
                display_name: None,
            }],
            ..base
        };
        let expected = match interp::run(&f.model, &f.data, &q) {
            Ok(rows) => canonical(rows),
            Err(e) => {
                failures.push(format!("{expr} / {shape}: the interpreter failed: {e}"));
                continue;
            }
        };
        let got = visual::compile(&f.model, &schema, &q)
            .map_err(|e| e.to_string())
            .and_then(|c| f.db.arrow(&c.sql).map_err(|e| format!("{e}\n{}", c.sql)))
            .map(|b| canonical(arrow_values(b)));
        match got {
            Err(e) => failures.push(format!("{expr} / {shape}: {e}")),
            Ok(got) => {
                let ok = got.len() == expected.len()
                    && got
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.iter().zip(b).all(|(x, y)| same(x, y)));
                if !ok {
                    failures.push(format!(
                        "{expr} / {shape}:\n  expected:\n    {}\n  got:\n    {}",
                        show(&expected).replace('\n', "\n    "),
                        show(&got).replace('\n', "\n    ")
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} random cases differ:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

#[test]
fn both_directions_agree_with_the_reference_interpreter() {
    let mut f = fixture();
    // Sales filters Product too: products follow the sales a visual shows.
    f.model
        .relationships
        .iter_mut()
        .find(|r| r.id == "r-product")
        .unwrap()
        .cross_filter = CrossFilter::Both;
    assert!(charted_compiler::graph::ambiguities(&f.model).is_empty());
    let schema_types: HashMap<(String, String), DataType> = f
        .model
        .tables
        .iter()
        .flat_map(|t| {
            t.columns.iter().filter_map(move |c| match &c.kind {
                ColumnKind::Data { source_column } => {
                    Some(((t.source.clone(), source_column.clone()), c.data_type))
                }
                _ => None,
            })
        })
        .collect();
    let schema = |source: &str, column: &str| {
        schema_types
            .get(&(source.to_string(), column.to_string()))
            .copied()
    };
    let extra = [
        ("Products In View", "COUNTROWS ( Product )"),
        ("Colors In View", "DISTINCTCOUNT ( Product[Color] )"),
        ("Max List In View", "MAX ( Product[ListPrice] )"),
    ];
    let mut failures = Vec::new();
    let names: Vec<(String, String)> = MEASURES
        .iter()
        .map(|(n, _)| (n.to_string(), format!("m:{n}")))
        .chain(extra.iter().enumerate().map(|(i, (n, e))| {
            f.model.table_mut("Sales").unwrap().measures.push(Measure {
                id: format!("m:both{i}"),
                name: n.to_string(),
                expression: e.to_string(),
                language: ExpressionLanguage::Dax,
                description: None,
                format_string: None,
                display_folder: None,
                hidden: false,
                extras: Default::default(),
            });
            (n.to_string(), format!("m:both{i}"))
        }))
        .collect();
    for (name, id) in names {
        for (shape, base) in shapes(&f.model) {
            let q = VisualQuery {
                values: vec![Projection {
                    field: FieldRef::Measure {
                        table: "Sales".into(),
                        measure: id.clone(),
                    },
                    aggregation: None,
                    display_name: None,
                }],
                ..base
            };
            let expected = interp::run(&f.model, &f.data, &q).map(canonical);
            let got = visual::compile(&f.model, &schema, &q)
                .map_err(|e| e.to_string())
                .and_then(|c| f.db.arrow(&c.sql).map_err(|e| e.to_string()))
                .map(|b| canonical(arrow_values(b)));
            match (expected, got) {
                (Ok(e), Ok(g)) => {
                    let ok = g.len() == e.len()
                        && g.iter()
                            .zip(&e)
                            .all(|(a, b)| a.iter().zip(b).all(|(x, y)| same(x, y)));
                    if !ok {
                        failures.push(format!(
                            "{name} / {shape}:\n  expected:\n    {}\n  got:\n    {}",
                            show(&e).replace('\n', "\n    "),
                            show(&g).replace('\n', "\n    ")
                        ));
                    }
                }
                (e, g) => {
                    failures.push(format!("{name} / {shape}: {:?} vs {:?}", e.err(), g.err()))
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} cases differ:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
