//! The DAX golden fixture, its measures and visual shapes, shared by the golden
//! tests (dax_golden.rs) and the Power BI conformance harness (conformance.rs).
#![allow(dead_code)]

use arrow_ipc::reader::StreamReader;
use charted_compiler::interp::{Data, TableData, Value};
use charted_compiler::visual::{QueryFilter, VisualQuery};
use charted_engine::{Database, DbOptions, sql};
use charted_model::*;
use std::collections::HashMap;

/// (name, table, column type, values) for each column of each table.
pub struct Fixture {
    pub model: Model,
    pub db: Database,
    pub data: Data,
}

pub fn col(table: &str, name: &str, ty: DataType) -> Column {
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

pub fn rel(id: &str, from: (&str, &str), to: (&str, &str), active: bool) -> Relationship {
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
pub type FixtureTable = (String, Vec<(String, DataType)>, Vec<Vec<Value>>);

pub fn n(x: f64) -> Value {
    Value::Num(x)
}
pub fn t(s: &str) -> Value {
    Value::Text(s.into())
}

/// A small, irregular star: blanks, a sale of an unknown product, months without sales,
/// an inactive ship-date relationship and a second fact table (targets).
pub fn fixture() -> Fixture {
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
                // A duplicate row (Targets has no key): iterators must keep both.
                if y == 2025 && m == 2 && s == 1 {
                    targets.push(vec![n(202502.0), n(1.0), n(1210.0)]);
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

    // A parent-child hierarchy (PATH): two trees, and a row whose parent isn't a row.
    tables.push((
        "Account".into(),
        vec![
            ("AccountKey".into(), DataType::Int64),
            ("ParentKey".into(), DataType::Int64),
            ("Name".into(), DataType::String),
        ],
        vec![
            vec![n(1.0), Value::Blank, t("Revenue")],
            vec![n(2.0), n(1.0), t("Sales")],
            vec![n(3.0), n(2.0), t("Retail")],
            vec![n(4.0), n(2.0), t("Online")],
            vec![n(5.0), Value::Blank, t("Costs")],
            vec![n(6.0), n(5.0), t("Staff")],
            vec![n(7.0), n(9.0), t("Orphan")],
        ],
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
            hierarchies: Vec::new(),
            is_date_table: name == "Date",
            calculation_group: None,
            calculated: None,
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
pub const MEASURES: &[(&str, &str)] = &[
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
    // Iterated rows: every column readable, duplicate rows kept, the whole row
    // transitioned, outer rows referenced from inner iterators.
    (
        "Listed Qty Value",
        "SUMX ( Product, Product[ListPrice] * [Quantity] )",
    ),
    ("Per Target Row", "SUMX ( Targets, [Orders] )"),
    // Division by zero is IEEE in Power BI: Infinity, NaN; blank only over a blank.
    ("Qty Over Zero", "[Quantity] / 0"),
    ("Zero Over Zero", "( [Quantity] - [Quantity] ) / 0"),
    ("Over Blank", "-[Quantity] / BLANK ()"),
    (
        "Infinite Texts",
        "\"\" & [Quantity] / 0 & \"|\" & -[Quantity] / 0 & \"|\" & ( [Quantity] - [Quantity] ) / 0",
    ),
    // TREATAS as a filter: values found in a table, a list, two columns at once.
    (
        "Targets On Order Months",
        "CALCULATE ( SUM ( Targets[Target] ), TREATAS ( VALUES ( Sales[OrderDateKey] ), Targets[DateKey] ) )",
    ),
    (
        "Qty Red Green",
        "CALCULATE ( [Quantity], TREATAS ( { \"Red\", \"Green\", \"Purple\" }, Product[Color] ) )",
    ),
    (
        "Targets On Order Stores",
        "CALCULATE ( SUM ( Targets[Target] ), TREATAS ( ALL ( Sales[OrderDateKey], Sales[StoreKey] ), Targets[DateKey], Targets[StoreKey] ) )",
    ),
    // Text to number and parent-child path text.
    (
        "Value Of Text",
        "VALUE ( \" 1,234 \" ) + VALUE ( \"$5\" ) + VALUE ( \"2024-01-02\" ) + [Quantity]",
    ),
    (
        "Path Pieces",
        "PATHITEM ( \"4|5|6\", MAX ( Store[StoreKey] ) ) & \"/\" & PATHITEMREVERSE ( \"4|5|6\", MIN ( Store[StoreKey] ), INTEGER ) & \"/\" & PATHLENGTH ( \"4|5|6\" ) & \"/\" & PATHCONTAINS ( \"4|5|6\", MAX ( Store[StoreKey] ) + 3 )",
    ),
    ("Target Rows", "SUMX ( Targets, [Orders] * 0 + 1 )"),
    ("All Products Qty", "SUMX ( ALL ( Product ), [Quantity] )"),
    (
        "Lines Per Product",
        "SUMX ( Product, COUNTROWS ( FILTER ( Sales, Sales[ProductKey] = Product[ProductKey] ) ) )",
    ),
    // ALLSELECTED as a table
    (
        "Selected Months",
        "COUNTROWS ( ALLSELECTED ( Date[MonthNumber] ) )",
    ),
    (
        "Selected Color Qty",
        "SUMX ( ALLSELECTED ( Product[Color] ), [Quantity] )",
    ),
    ("Selected Products", "COUNTROWS ( ALLSELECTED ( Product ) )"),
    (
        "Share Of Selected",
        "DIVIDE ( [Quantity], SUMX ( ALLSELECTED ( Product[Product] ), [Quantity] ) )",
    ),
    // FILTER over measures and outer values as CALCULATE filters
    (
        "Running Qty",
        "CALCULATE ( [Quantity], FILTER ( ALLSELECTED ( Date[Date] ), Date[Date] <= MAX ( Date[Date] ) ) )",
    ),
    (
        "Running Qty In Year",
        "CALCULATE ( [Quantity], FILTER ( ALLSELECTED ( Date[MonthNumber] ), Date[MonthNumber] <= MAX ( Date[MonthNumber] ) ) )",
    ),
    (
        "Running Qty All Time",
        "CALCULATE ( [Quantity], FILTER ( ALL ( Date[Date] ), Date[Date] <= MAX ( Date[Date] ) ) )",
    ),
    (
        "Busy Products Sales",
        "CALCULATE ( [Sales Amount], FILTER ( VALUES ( Product[Product] ), [Quantity] > 10 ) )",
    ),
    (
        "Busy Colors Kept",
        "CALCULATE ( [Sales Amount], KEEPFILTERS ( FILTER ( ALL ( Product[Color] ), [Quantity] > 5 ) ) )",
    ),
    (
        "Cheaper Or Equal",
        "CALCULATE ( [Quantity], FILTER ( ALL ( Product ), Product[ListPrice] <= MAX ( Product[ListPrice] ) ) )",
    ),
    // Date sets iterated, and ends computed from expressions.
    (
        "Avg Qty Last 3 Months",
        "AVERAGEX ( DATESINPERIOD ( Date[Date], MAX ( Date[Date] ), -3, MONTH ), [Quantity] )",
    ),
    (
        "Best Month YTD",
        "MAXX ( DATESYTD ( Date[Date] ), [Quantity] )",
    ),
    (
        "Months In Half Year",
        "COUNTROWS ( DATESINPERIOD ( Date[Date], MAX ( Date[Date] ), -6, MONTH ) )",
    ),
    (
        "Qty 3 Months To Last Month",
        "CALCULATE ( [Quantity], DATESINPERIOD ( Date[Date], EOMONTH ( MAX ( Date[Date] ), -1 ), -3, MONTH ) )",
    ),
    (
        "Qty Since Today",
        "CALCULATE ( [Quantity], DATESBETWEEN ( Date[Date], TODAY (), BLANK () ) )",
    ),
    (
        "Qty Until Today",
        "CALCULATE ( [Quantity], DATESBETWEEN ( Date[Date], BLANK (), TODAY () ) )",
    ),
    (
        "Qty From Mid 2024",
        "VAR mid2024 = DATE ( 2024, 6, 30 ) RETURN SUMX ( VALUES ( Store[Region] ), CALCULATE ( [Quantity], DATESBETWEEN ( Date[Date], mid2024, MAX ( Date[Date] ) ) ) )",
    ),
    // RANKX
    ("Qty Rank", "RANKX ( ALL ( Product[Product] ), [Quantity] )"),
    (
        "Qty Rank Asc Dense",
        "RANKX ( ALL ( Product[Product] ), [Quantity], , ASC, DENSE )",
    ),
    (
        "Selected Qty Rank",
        "RANKX ( ALLSELECTED ( Product[Product] ), [Quantity] )",
    ),
    (
        "Tens Rank Skip",
        "RANKX ( ALL ( Product[Product] ), ROUND ( [Quantity] / 10, 0 ) )",
    ),
    (
        "Tens Rank Dense",
        "RANKX ( ALL ( Product[Product] ), ROUND ( [Quantity] / 10, 0 ), , DESC, DENSE )",
    ),
    (
        "List Price Rank",
        "RANKX ( ALL ( Product ), Product[ListPrice], MAX ( Product[ListPrice] ) )",
    ),
    (
        "Category Rank",
        "RANKX ( ALL ( Product[Category] ), [Sales Amount] )",
    ),
    // TOPN and CONCATENATEX
    (
        "Top 3 Products Sales",
        "CALCULATE ( [Sales Amount], TOPN ( 3, ALL ( Product[Product] ), [Sales Amount] ) )",
    ),
    (
        "Top 2 Products Qty",
        "SUMX ( TOPN ( 2, VALUES ( Product[Product] ), [Quantity] ), [Quantity] )",
    ),
    (
        "Top Tens Count",
        "COUNTROWS ( TOPN ( 1, VALUES ( Product[Product] ), ROUND ( [Quantity] / 10, 0 ), DESC ) )",
    ),
    (
        "Cheapest Two",
        "SUMX ( TOPN ( 2, Product, Product[ListPrice], ASC ), Product[ListPrice] )",
    ),
    (
        "Colors List",
        "CONCATENATEX ( VALUES ( Product[Color] ), Product[Color], \", \" )",
    ),
    (
        "Products By Qty",
        "CONCATENATEX ( VALUES ( Product[Product] ), Product[Product], \", \", [Quantity], DESC )",
    ),
    (
        "Region Quantities",
        "CONCATENATEX ( VALUES ( Store[Region] ), [Quantity], \"|\" )",
    ),
    // CALCULATETABLE, RELATEDTABLE, LOOKUPVALUE
    (
        "Red Sales Rows",
        "COUNTROWS ( CALCULATETABLE ( Sales, Product[Color] = \"Red\" ) )",
    ),
    (
        "Lines Via Related Table",
        "SUMX ( Product, COUNTROWS ( RELATEDTABLE ( Sales ) ) )",
    ),
    (
        "Qty Of North Products",
        "SUMX ( CALCULATETABLE ( VALUES ( Product[Product] ), Store[Region] = \"North\" ), [Quantity] )",
    ),
    (
        "Qty For North Products",
        "CALCULATE ( [Quantity], CALCULATETABLE ( VALUES ( Product[Product] ), Store[Region] = \"North\" ) )",
    ),
    (
        "Tent List Price",
        "LOOKUPVALUE ( Product[ListPrice], Product[Product], \"Tent\" )",
    ),
    (
        "Price Of Top Key",
        "LOOKUPVALUE ( Product[ListPrice], Product[ProductKey], MAX ( Sales[ProductKey] ), -1 )",
    ),
    (
        "Looked Up Value",
        "SUMX ( Sales, Sales[Qty] * LOOKUPVALUE ( Product[ListPrice], Product[ProductKey], Sales[ProductKey] ) )",
    ),
    (
        "Green Category",
        "LOOKUPVALUE ( Product[Category], Product[Color], \"Green\" )",
    ),
    // Tables with added columns
    (
        "Best Category Sales",
        "MAXX ( ADDCOLUMNS ( VALUES ( Product[Category] ), \"s\", [Sales Amount] ), [s] )",
    ),
    (
        "Busy Categories",
        "COUNTROWS ( FILTER ( ADDCOLUMNS ( VALUES ( Product[Category] ), \"q\", [Quantity] ), [q] > 50 ) )",
    ),
    (
        "Sold Colors",
        "COUNTROWS ( SUMMARIZE ( Sales, Product[Color] ) )",
    ),
    (
        "Qty Over Category Regions",
        "SUMX ( SUMMARIZE ( Sales, Product[Category], Store[Region] ), [Quantity] )",
    ),
    (
        "Selected Prices",
        "SUMX ( SELECTCOLUMNS ( Product, \"p\", Product[ListPrice] ), [p] )",
    ),
    (
        "Best Category Qty",
        "MAXX ( SUMMARIZECOLUMNS ( Product[Category], \"q\", [Quantity] ), [q] )",
    ),
    (
        "Days In Q1 2024",
        "COUNTROWS ( CALENDAR ( DATE ( 2024, 1, 1 ), DATE ( 2024, 3, 31 ) ) )",
    ),
    ("Odd Sum", "SUMX ( GENERATESERIES ( 1, 10, 2 ), [Value] )"),
    (
        "Best Category Units",
        "MAXX ( SUMMARIZE ( Sales, Product[Category], \"q\", SUM ( Sales[Qty] ) ), [q] )",
    ),
    // Table variables, MOD, WEEKDAY
    (
        "Last 3 Months Via Var",
        "VAR p = DATESINPERIOD ( Date[Date], MAX ( Date[Date] ), -3, MONTH ) RETURN CALCULATE ( [Quantity], p )",
    ),
    (
        "Var Table Twice",
        "VAR t = ADDCOLUMNS ( VALUES ( Product[Category] ), \"q\", [Quantity] ) RETURN MAXX ( t, [q] ) + COUNTROWS ( t )",
    ),
    (
        "Average 12M Via Vars",
        "VAR d = MAX ( Date[Date] ) VAR p = DATESINPERIOD ( Date[Date], d, -12, MONTH ) RETURN CALCULATE ( AVERAGEX ( VALUES ( Date[Date] ), [Quantity] ), p )",
    ),
    ("Qty Mod 7", "MOD ( [Quantity], 7 )"),
    ("Negative Mod", "MOD ( -[Quantity], 4 )"),
    // Nested, so MOD's twice-used arguments go through the bind-once lambda.
    (
        "Nested Mod",
        "MOD ( MOD ( [Quantity] * 3, 17 ), -5 ) + ABS ( MOD ( BLANK (), 3 ) )",
    ),
    (
        "Latest Weekday",
        "MAXX ( VALUES ( Date[Date] ), WEEKDAY ( Date[Date], 2 ) )",
    ),
    (
        "Weekday Sum",
        "SUMX ( VALUES ( Date[Date] ), WEEKDAY ( Date[Date] ) + WEEKDAY ( Date[Date], 3 ) )",
    ),
    // Table constructors and NAMEOF (field parameters' tables).
    ("List Sum", "SUMX ( { 1, 2, [Quantity] }, [Value] )"),
    (
        "Rows Count",
        "COUNTROWS ( { ( \"a\", 1 ), ( \"b\", 2 ), ( \"c\", [Orders] ) } )",
    ),
    (
        "Rows Max",
        "MAXX ( { ( \"a\", 1 ), ( \"b\", [Quantity] ) }, [Value2] )",
    ),
    (
        "Rows Filtered",
        "SUMX ( FILTER ( { ( \"a\", 1 ), ( \"b\", 2 ), ( \"c\", 3 ) }, [Value1] <> \"b\" ), [Value2] )",
    ),
    (
        "Field Names",
        "CONCATENATEX ( { ( \"Q\", NAMEOF ( [Quantity] ), 0 ), ( \"C\", NAMEOF ( Product[Category] ), 1 ) }, [Value2], \", \", [Value3] )",
    ),
    // Calculation items as charted applies them (calcgroups.rs): a SWITCH on the item
    // picked, each branch the item with SELECTEDMEASURE () replaced by the measure.
    (
        "Calc Item Switch",
        "SWITCH ( SELECTEDVALUE ( Product[Category] ), \"Apparel\", CALCULATE ( ( 'Sales'[Quantity] ), SAMEPERIODLASTYEAR ( Date[Date] ) ), \"Camping\", ( 'Sales'[Quantity] ) * 2, 'Sales'[Quantity] )",
    ),
    (
        "Calc Items Nested",
        "SWITCH ( SELECTEDVALUE ( Date[Year] ), 2025, ( SWITCH ( SELECTEDVALUE ( Product[Category] ), \"Apparel\", DIVIDE ( ( 'Sales'[Sales Amount] ), CALCULATE ( ( 'Sales'[Sales Amount] ), ALL ( Product ) ) ), 'Sales'[Sales Amount] ) ) + 1, SWITCH ( SELECTEDVALUE ( Product[Category] ), \"Apparel\", DIVIDE ( ( 'Sales'[Sales Amount] ), CALCULATE ( ( 'Sales'[Sales Amount] ), ALL ( Product ) ) ), 'Sales'[Sales Amount] ) )",
    ),
    // Several filters on one column are ANDed.
    (
        "Neither Apparel Nor Camping",
        "CALCULATE ( [Quantity], Product[Category] <> \"Apparel\", Product[Category] <> \"Camping\" )",
    ),
    (
        "Camping If Busy",
        "CALCULATE ( [Sales Amount], Product[Category] = \"Camping\", FILTER ( VALUES ( Product[Category] ), [Orders] > 10 ) )",
    ),
    (
        "Since March 2025",
        "CALCULATE ( [Quantity], Date[Date] >= DATE ( 2025, 3, 1 ) )",
    ),
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
        "VAR everything = CALCULATE ( [Quantity], ALL ( Product ) ) RETURN DIVIDE ( [Quantity], everything )",
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
    ("Latest Year", "MAXX ( 'Date', YEAR ( 'Date'[Date] ) )"),
    (
        "Quarter Days",
        "SUMX ( VALUES ( 'Date'[Date] ), QUARTER ( 'Date'[Date] ) * 100 + MONTH ( 'Date'[Date] ) + DAY ( 'Date'[Date] ) )",
    ),
    (
        "Target LY",
        "CALCULATE ( [Target], SAMEPERIODLASTYEAR ( Date[Date] ) )",
    ),
    (
        "Scoped Share",
        "IF ( ISINSCOPE ( Product[Category] ), [Share Of Category], BLANK () )",
    ),
    // Rolling periods and date ranges
    (
        "Rolling 12M",
        "CALCULATE ( [Sales Amount], DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -12, MONTH ) )",
    ),
    (
        "Rolling 3M Var",
        "VAR last = LASTDATE ( 'Date'[Date] ) RETURN CALCULATE ( [Quantity], DATESINPERIOD ( 'Date'[Date], last, -3, MONTH ) )",
    ),
    (
        "Next 2 Quarters",
        "CALCULATE ( [Quantity], DATESINPERIOD ( 'Date'[Date], MIN ( 'Date'[Date] ), 2, QUARTER ) )",
    ),
    (
        "Last 45 Days",
        "CALCULATE ( [Quantity], DATESINPERIOD ( 'Date'[Date], LASTDATE ( 'Date'[Date] ), -45, DAY ) )",
    ),
    (
        "Year Back",
        "CALCULATE ( [Quantity], DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -1, YEAR ) )",
    ),
    (
        "H1 2025",
        "CALCULATE ( [Quantity], DATESBETWEEN ( 'Date'[Date], DATE ( 2025, 1, 1 ), DATE ( 2025, 6, 30 ) ) )",
    ),
    (
        "Running Qty To Date",
        "CALCULATE ( [Quantity], DATESBETWEEN ( 'Date'[Date], BLANK (), MAX ( 'Date'[Date] ) ) )",
    ),
    (
        "From First",
        "CALCULATE ( [Quantity], DATESBETWEEN ( 'Date'[Date], FIRSTDATE ( 'Date'[Date] ), BLANK () ) )",
    ),
    ("No Filters", "CALCULATE ( [Quantity], REMOVEFILTERS () )"),
    (
        "Share Of All",
        "DIVIDE ( [Quantity], CALCULATE ( [Quantity], ALL () ) )",
    ),
    // Math, text and date functions
    ("Int Price", "INT ( [Avg Price] )"),
    ("Int Negative", "INT ( -[Price Per Unit] )"),
    ("Int Flag", "INT ( [Avg Price] > 10 )"),
    ("Pi Qty", "ROUND ( PI () * [Quantity], 2 )"),
    ("Smaller", "MIN ( [Sales Amount], [Target] )"),
    ("At Least 10", "MAX ( [Quantity], 10 )"),
    ("Max With Blank", "MAX ( [Missing Amounts], -1 )"),
    (
        "Later Date",
        "MAX ( LASTDATE ( 'Date'[Date] ), DATE ( 2025, 6, 30 ) )",
    ),
    (
        "Upper Category",
        "IF ( [Quantity] > 0, UPPER ( SELECTEDVALUE ( Product[Category] ) ) )",
    ),
    (
        "Category Code",
        "IF ( [Quantity] > 0, LEFT ( SELECTEDVALUE ( Product[Category] ) & \"all\", 3 ) & \"-\" & RIGHT ( SELECTEDVALUE ( Product[Category] ), 2 ) & LEFT ( \"xyz\" ) )",
    ),
    (
        "Category Length",
        "IF ( [Quantity] > 0, LEN ( SELECTEDVALUE ( Product[Color] ) ) )",
    ),
    (
        "Mid Lower",
        "IF ( [Quantity] > 0, LOWER ( MID ( SELECTEDVALUE ( Product[Category] ) & \" Products\", 2, 4 ) ) )",
    ),
    (
        "Trimmed",
        "IF ( [Quantity] > 0, TRIM ( \"  big   \" & SELECTEDVALUE ( Product[Category] ) & \"  \" ) )",
    ),
    (
        "Substituted",
        "IF ( [Quantity] > 0, SUBSTITUTE ( SELECTEDVALUE ( Product[Category] ) & \" aaa\", \"a\", \"o\" ) )",
    ),
    (
        "Second A",
        "IF ( [Quantity] > 0, SUBSTITUTE ( \"banana \" & SELECTEDVALUE ( Product[Category] ), \"a\", \"_\", 2 ) )",
    ),
    (
        "Joined",
        "IF ( [Quantity] > 0, CONCATENATE ( SELECTEDVALUE ( Product[Category] ) & \"-\", FORMAT ( [Quantity], \"0\" ) ) )",
    ),
    (
        "Blank Arguments",
        "IF ( [Quantity] > 0, LEN ( BLANK () ) + LEN ( UPPER ( BLANK () ) & LEFT ( BLANK (), 2 ) ) + LEN ( FORMAT ( BLANK (), \"0\" ) ) + MIN ( BLANK (), 3 ) + LEN ( SUBSTITUTE ( BLANK (), BLANK (), \"x\" ) ) )",
    ),
    ("Amount Text", "FORMAT ( [Sales Amount], \"#,0.00\" )"),
    ("Share Text", "FORMAT ( [Share Of Category], \"0.0%\" )"),
    ("Price Text", "FORMAT ( [Avg Price], \"$#,##0.00\" )"),
    ("Gap Text", "FORMAT ( [Gap], \"#,0\" )"),
    (
        "General Text",
        "FORMAT ( [Price Per Unit], \"General Number\" )",
    ),
    (
        "Number Texts",
        "1 / 3 & \"|\" & 1 / 30000 & \"|\" & 1 / 3000 & \"|\" & 0.0000333333 & \"|\" & 1000000 * 1000000 * 1000 & \"|\" & 10 / 3 * 1000000000000000000 & \"|\" & -2 / 3 & \"|\" & 123456.5 & \"|\" & [Quantity] / 7",
    ),
    (
        "Last Month Name",
        "FORMAT ( LASTDATE ( 'Date'[Date] ), \"mmm yyyy\" )",
    ),
    (
        "Last Date Text",
        "FORMAT ( MAX ( 'Date'[Date] ), \"dddd dd/mm/yy\" )",
    ),
    (
        "First Month",
        "FORMAT ( MIN ( 'Date'[Date] ), \"yyyy-MM\" ) & \" \" & FORMAT ( MIN ( 'Date'[Date] ), \"MMMM d\" )",
    ),
    ("Month End", "EOMONTH ( MIN ( 'Date'[Date] ), 1 )"),
    ("Quarter Back", "EOMONTH ( MAX ( 'Date'[Date] ), -3 )"),
    ("Month Later", "EDATE ( MAX ( 'Date'[Date] ), 1 )"),
    ("Date Overflow", "DATE ( 2024, 14, 0 )"),
    (
        "Next Month Start",
        "DATE ( MAX ( 'Date'[Year] ), MAX ( 'Date'[MonthNumber] ) + 1, 1 )",
    ),
    (
        "Date Span",
        "DATEDIFF ( FIRSTDATE ( 'Date'[Date] ), LASTDATE ( 'Date'[Date] ), MONTH ) * 100 + DATEDIFF ( MIN ( 'Date'[Date] ), MAX ( 'Date'[Date] ), QUARTER ) * 10 + DATEDIFF ( MIN ( 'Date'[Date] ), MAX ( 'Date'[Date] ), YEAR )",
    ),
    (
        "Days Before",
        "DATEDIFF ( MAX ( 'Date'[Date] ), DATE ( 2024, 1, 1 ), DAY )",
    ),
];

/// The visual shapes every measure is evaluated in.
pub fn shapes(m: &Model) -> Vec<(&'static str, VisualQuery)> {
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
    let camping = QueryFilter {
        field: c("Product", "Category"),
        filter: FilterKind::Basic {
            values: vec![Some("Camping".into())],
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
        // Slicers: filters from outside the visual, which ALLSELECTED keeps.
        (
            "by month in 2025, camping",
            VisualQuery {
                groups: vec![c("Date", "MonthNumber")],
                filters: vec![year_2025.clone(), camping.clone()],
                ..Default::default()
            },
        ),
        (
            "by product, camping",
            VisualQuery {
                groups: vec![c("Product", "Product")],
                filters: vec![camping],
                ..Default::default()
            },
        ),
    ]
}

pub fn arrow_values(bytes: Vec<u8>) -> Vec<Vec<Value>> {
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
pub fn canonical(mut rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    rows.sort_by_key(|r| r.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>());
    rows
}

pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => {
            x == y
                || (x.is_nan() && y.is_nan())
                || (x - y).abs() <= 1e-6 * (1.0 + x.abs().max(y.abs()))
        }
        (Value::Bool(x), Value::Num(y)) | (Value::Num(y), Value::Bool(x)) => {
            (*x as i32 as f64 - y).abs() < 1e-9
        }
        _ => a == b,
    }
}

pub fn show(rows: &[Vec<Value>]) -> String {
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
