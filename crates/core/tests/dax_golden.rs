//! DAX golden cases: every measure below is evaluated in several visual shapes by the
//! SQL compiler (on DuckDB) and by the reference interpreter, and the results must
//! agree. A snapshot of the interpreter's answers (tests/snapshots) records them so
//! changes in semantics show up in review.

mod golden;

use charted_compiler::interp::{self, Value};
use charted_compiler::visual::{self, OrderBy, OrderTarget, VisualQuery};
use charted_model::*;
use golden::*;
use std::collections::HashMap;

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
                // A measure that errors (as in Power BI) must fail the SQL too.
                Err(e) => {
                    if shape == "card" || shape == "by category" {
                        golden.push_str(&format!("## {name} — {shape}\nerror: {e}\n\n"));
                    }
                    let sql = visual::compile(&f.model, &schema, &q)
                        .map_err(|e| e.to_string())
                        .and_then(|c| f.db.arrow(&c.sql).map_err(|e| e.to_string()));
                    if sql.is_ok() {
                        failures.push(format!(
                            "{name} / {shape}: the interpreter failed ({e}) but the SQL didn't"
                        ));
                    }
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
        "INT ( [Sales Amount] / 7 )",
        "IF ( [Orders] > 0, LEN ( SELECTEDVALUE ( Product[Category] ) ) )",
        "DATEDIFF ( MIN ( Date[Date] ), MAX ( Date[Date] ), MONTH )",
        "MIN ( [Quantity], 20 )",
        "MAX ( [Orders], [Target] / 100 )",
        "YEAR ( EOMONTH ( MAX ( Date[Date] ), 2 ) )",
        "MONTH ( EDATE ( MIN ( Date[Date] ), 5 ) )",
        "LEN ( FORMAT ( [Sales Amount], \"#,0\" ) )",
        "RANKX ( ALL ( Product[Category] ), [Quantity] )",
        "LEN ( CONCATENATEX ( VALUES ( Store[Region] ), Store[Region], \",\" ) )",
        "RANKX ( ALLSELECTED ( Store[Region] ), [Orders], , ASC, DENSE )",
        "MEDIAN ( Sales[Qty] )",
        "PERCENTILEX.INC ( VALUES ( Product[Product] ), [Quantity], 0.25 )",
        "STDEV.P ( Sales[Price] )",
        "VARX.S ( VALUES ( Store[Region] ), [Orders] )",
        "GEOMEAN ( Sales[Price] )",
        "DISTINCTCOUNTNOBLANK ( Sales[OrderedAt] )",
        "ROUNDUP ( [Sales Amount] / 13, 1 )",
        "SQRT ( [Quantity] )",
        "MROUND ( [Orders], 3 )",
        "LOG ( [Sales Amount] + 1, 2 )",
        "LEN ( FIXED ( [Sales Amount], 1 ) )",
        "WEEKNUM ( MAX ( Date[Date] ) )",
        "NETWORKDAYS ( MIN ( Date[Date] ), MAX ( Date[Date] ) )",
        "SEARCH ( \"e\", SELECTEDVALUE ( Product[Category] ) & \"x\", 1, 0 )",
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
        "DATESINPERIOD ( Date[Date], MAX ( Date[Date] ), -3, MONTH )",
        "DATESBETWEEN ( Date[Date], DATE ( 2024, 6, 1 ), LASTDATE ( Date[Date] ) )",
        "REMOVEFILTERS ()",
        "FILTER ( ALLSELECTED ( Date[Date] ), Date[Date] <= MAX ( Date[Date] ) )",
        "FILTER ( VALUES ( Product[Category] ), [Orders] > 10 )",
        "FILTER ( ALL ( Store[Region] ), [Quantity] > 30 )",
        "DATESINPERIOD ( Date[Date], EOMONTH ( MAX ( Date[Date] ), -1 ), -2, MONTH )",
        "TOPN ( 3, ALL ( Product[Product] ), [Sales Amount] )",
        "CALCULATETABLE ( VALUES ( Product[Category] ), Store[Region] <> \"South\" )",
    ];
    const ITERS: &[&str] = &[
        "SUMX ( VALUES ( Product[Category] ), {} )",
        "AVERAGEX ( VALUES ( Date[Year] ), {} )",
        "MAXX ( VALUES ( Store[Region] ), {} )",
        "SUMX ( FILTER ( VALUES ( Product[Color] ), [Quantity] > 5 ), {} )",
        "MINX ( VALUES ( Date[Quarter] ), {} )",
        "SUMX ( ALLSELECTED ( Product[Color] ), {} )",
        "MAXX ( ALLSELECTED ( Date[MonthNumber] ), {} )",
        "AVERAGEX ( DATESINPERIOD ( Date[Date], MAX ( Date[Date] ), -3, MONTH ), {} )",
        "SUMX ( TOPN ( 2, VALUES ( Product[Product] ), [Quantity] ), {} )",
        "MAXX ( SUMMARIZE ( Sales, Product[Category], Store[Region] ), {} )",
        "SUMX ( CALCULATETABLE ( VALUES ( Store[Region] ), Product[Category] = \"Camping\" ), {} )",
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
                // Both fail: the measure errors, as in Power BI.
                (Err(_), Err(_)) => {}
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

/// Agreement isn't correctness: running totals must accumulate. By year and month,
/// the running total never falls and ends at the grand total; the one over month
/// numbers starts again each year.
#[test]
fn running_totals_accumulate() {
    let f = fixture();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let col = |t: &str, c: &str| Projection {
        field: FieldRef::Column {
            table: t.into(),
            column: format!("{t}.{c}"),
        },
        aggregation: None,
        display_name: None,
    };
    let m = |name: &str| Projection {
        field: FieldRef::Measure {
            table: "Sales".into(),
            measure: format!("m:{name}"),
        },
        aggregation: None,
        display_name: None,
    };
    let q = VisualQuery {
        groups: vec![col("Date", "Year"), col("Date", "MonthNumber")],
        values: vec![m("Quantity"), m("Running Qty"), m("Running Qty In Year")],
        order: vec![
            OrderBy {
                target: OrderTarget::Group(0),
                descending: false,
            },
            OrderBy {
                target: OrderTarget::Group(1),
                descending: false,
            },
        ],
        ..Default::default()
    };
    let compiled = visual::compile(&f.model, &schema, &q).unwrap();
    let rows = arrow_values(f.db.arrow(&compiled.sql).unwrap());
    let num = |v: &Value| match v {
        Value::Num(x) => *x,
        _ => 0.0,
    };
    let (mut total, mut year_total, mut last_year) = (0.0, 0.0, 0.0);
    for r in &rows {
        if num(&r[0]) != last_year {
            year_total = 0.0;
            last_year = num(&r[0]);
        }
        total += num(&r[2]);
        year_total += num(&r[2]);
        assert_eq!(num(&r[3]), total, "running total at {:?}", &r[..2]);
        assert_eq!(
            num(&r[4]),
            year_total,
            "running total in year at {:?}",
            &r[..2]
        );
    }
    assert_eq!(total, 269.0);
}

/// LOOKUPVALUE that finds several different values is an error in both engines, unless
/// an alternate result is given.
#[test]
fn lookupvalue_with_several_values_is_an_error() {
    let mut f = fixture();
    for (id, expr) in [
        (
            "m:clash",
            "LOOKUPVALUE ( Product[Product], Product[Color], \"Green\" )",
        ),
        (
            "m:clash-alt",
            "LOOKUPVALUE ( Product[Product], Product[Color], \"Green\", \"several\" )",
        ),
    ] {
        f.model.table_mut("Sales").unwrap().measures.push(Measure {
            id: id.into(),
            name: id.into(),
            expression: expr.into(),
            language: ExpressionLanguage::Dax,
            description: None,
            format_string: None,
            display_folder: None,
            hidden: false,
            extras: Default::default(),
        });
    }
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let card = |id: &str| VisualQuery {
        values: vec![Projection {
            field: FieldRef::Measure {
                table: "Sales".into(),
                measure: id.into(),
            },
            aggregation: None,
            display_name: None,
        }],
        ..Default::default()
    };
    let q = card("m:clash");
    assert!(
        interp::run(&f.model, &f.data, &q)
            .unwrap_err()
            .contains("more than one value")
    );
    let sql = visual::compile(&f.model, &schema, &q).unwrap().sql;
    assert!(
        f.db.arrow(&sql)
            .unwrap_err()
            .to_string()
            .contains("more than one value")
    );
    let q = card("m:clash-alt");
    assert_eq!(
        interp::run(&f.model, &f.data, &q).unwrap(),
        vec![vec![t("several")]]
    );
    let sql = visual::compile(&f.model, &schema, &q).unwrap().sql;
    assert_eq!(
        arrow_values(f.db.arrow(&sql).unwrap()),
        vec![vec![t("several")]]
    );
}

/// Row-level security: with role filters on Store and Product, every measure agrees,
/// and ALL inside a measure doesn't see past them.
#[test]
fn security_filters_agree_with_the_reference_interpreter() {
    let f = fixture();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let security = vec![
        visual::RowFilter {
            table: "Store".into(),
            expression: "Store[Region] <> \"North\"".into(),
        },
        visual::RowFilter {
            table: "Product".into(),
            expression: "Product[Category] <> \"Apparel\"".into(),
        },
    ];
    let mut failures = Vec::new();
    for (name, _) in MEASURES {
        for (shape, base) in shapes(&f.model) {
            let q = VisualQuery {
                values: vec![Projection {
                    field: FieldRef::Measure {
                        table: "Sales".into(),
                        measure: format!("m:{name}"),
                    },
                    aggregation: None,
                    display_name: None,
                }],
                security: security.clone(),
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
                // Both fail: the measure errors, as in Power BI.
                (Err(_), Err(_)) => {}
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
    // All products, as the role sees them: no Apparel.
    let q = VisualQuery {
        values: vec![Projection {
            field: FieldRef::Measure {
                table: "Sales".into(),
                measure: "m:Product Count".into(),
            },
            aggregation: None,
            display_name: None,
        }],
        security,
        ..Default::default()
    };
    assert_eq!(
        interp::run(&f.model, &f.data, &q).unwrap(),
        vec![vec![n(5.0)]]
    );
}

/// Calculated columns that aggregate (v1.5): fixed cases on every kind of table.
const COLUMNS: &[(&str, &str)] = &[
    // Date/time arithmetic in a row.
    ("Sales", "Sales[OrderedAt] + 3"),
    ("Sales", "RELATED ( 'Date'[Date] ) - 0.25"),
    (
        "Sales",
        "IF ( Sales[OrderedAt] < RELATED ( 'Date'[Date] ) - 14, 1, 0 )",
    ),
    ("Sales", "Sales[OrderedAt] - RELATED ( 'Date'[Date] )"),
    (
        "Sales",
        "DATEDIFF ( Sales[OrderedAt], RELATED ( 'Date'[Date] ), DAY )",
    ),
    // Parent-child paths.
    (
        "Account",
        "PATH ( Account[AccountKey], Account[ParentKey] )",
    ),
    (
        "Account",
        "PATHITEM ( PATH ( Account[AccountKey], Account[ParentKey] ), 2, INTEGER )",
    ),
    (
        "Account",
        "PATHLENGTH ( PATH ( Account[AccountKey], Account[ParentKey] ) )",
    ),
    (
        "Account",
        "LOOKUPVALUE ( Account[Name], Account[AccountKey], PATHITEM ( PATH ( Account[AccountKey], Account[ParentKey] ), 1, INTEGER ) )",
    ),
    (
        "Account",
        "PATHCONTAINS ( PATH ( Account[AccountKey], Account[ParentKey] ), 2 )",
    ),
    (
        "Account",
        "PATHITEMREVERSE ( PATH ( Account[AccountKey], Account[ParentKey] ), 2 )",
    ),
    // Context transition: the row becomes a filter.
    ("Product", "CALCULATE ( SUM ( Sales[Amount] ) )"),
    ("Product", "COUNTROWS ( RELATEDTABLE ( Sales ) )"),
    (
        "Product",
        "SUMX ( RELATEDTABLE ( Sales ), Sales[Qty] * Sales[Price] )",
    ),
    ("Product", "[Quantity]"),
    ("Product", "[Avg Per Category]"),
    ("Product", "MEDIANX ( RELATEDTABLE ( Sales ), Sales[Qty] )"),
    (
        "Product",
        "PERCENTILEX.INC ( RELATEDTABLE ( Sales ), Sales[Price], 0.8 )",
    ),
    ("Product", "STDEVX.P ( RELATEDTABLE ( Sales ), Sales[Qty] )"),
    ("Sales", "CEILING ( Sales[Price] * 1.07, 0.05 )"),
    ("Product", "CONTAINSSTRING ( Product[Product], \"e\" )"),
    (
        "Sales",
        "HOUR ( Sales[OrderedAt] ) + MINUTE ( Sales[OrderedAt] ) / 100",
    ),
    ("Sales", "WEEKNUM ( RELATED ( 'Date'[Date] ), 2 )"),
    (
        "Sales",
        "NETWORKDAYS ( RELATED ( 'Date'[Date] ), Sales[OrderedAt] + 10 )",
    ),
    (
        "Product",
        "REPLACE ( Product[Product], 2, 1, \"_\" ) & EXACT ( Product[Color], \"Red\" )",
    ),
    (
        "Product",
        "SEARCH ( \"a\", Product[Product], 1, 0 ) + FIND ( \"e\", Product[Product], 1, 0 )",
    ),
    ("Sales", "POWER ( Sales[Qty], 2 ) + SIGN ( Sales[Qty] - 3 )"),
    // No transition: the whole table.
    ("Product", "SUM ( Sales[Amount] )"),
    ("Product", "COUNTROWS ( Product )"),
    (
        "Product",
        "DIVIDE ( CALCULATE ( SUM ( Sales[Amount] ) ), CALCULATE ( SUM ( Sales[Amount] ), ALLEXCEPT ( Product, Product[Category] ) ) )",
    ),
    // Earlier rows.
    (
        "Product",
        "COUNTROWS ( FILTER ( Product, Product[ListPrice] > EARLIER ( Product[ListPrice] ) ) ) + 1",
    ),
    ("Product", "RANKX ( ALL ( Product ), Product[ListPrice] )"),
    (
        "Product",
        "VAR p = Product[ListPrice] RETURN COUNTROWS ( FILTER ( ALL ( Product ), Product[ListPrice] <= p ) )",
    ),
    (
        "Product",
        "SUMX ( FILTER ( Product, Product[Category] = EARLIER ( Product[Category] ) && Product[ListPrice] <= EARLIER ( Product[ListPrice] ) ), Product[ListPrice] )",
    ),
    (
        "Product",
        "LOOKUPVALUE ( Product[Product], Product[ListPrice], Product[ListPrice] + 0 )",
    ),
    // A fact table without a key: the row's columns are the filter.
    ("Sales", "[Quantity]"),
    (
        "Sales",
        "DIVIDE ( Sales[Amount], CALCULATE ( SUM ( Sales[Amount] ), ALLEXCEPT ( Sales, Sales[ProductKey] ) ) )",
    ),
    (
        "Sales",
        "CALCULATE ( COUNTROWS ( Sales ), ALLEXCEPT ( Sales, Sales[ProductKey] ) )",
    ),
    (
        "Sales",
        "Sales[Qty] * RELATED ( Product[ListPrice] ) - SUM ( Sales[Qty] )",
    ),
    (
        "Sales",
        "COUNTROWS ( FILTER ( Sales, Sales[ProductKey] = EARLIER ( Sales[ProductKey] ) && Sales[OrderId] <= EARLIER ( Sales[OrderId] ) ) )",
    ),
    ("Targets", "CALCULATE ( COUNTROWS ( Targets ) )"),
    ("Store", "CALCULATE ( [Sales Amount] )"),
    ("Store", "RANKX ( ALL ( Store ), CALCULATE ( [Quantity] ) )"),
    ("Date", "CALCULATE ( [Quantity] )"),
    (
        "Date",
        "CALCULATE ( [Quantity], FILTER ( ALL ( Date ), Date[Date] <= EARLIER ( Date[Date] ) ) )",
    ),
    ("Date", "CALCULATE ( [Quantity], DATESYTD ( Date[Date] ) )"),
];

#[test]
fn calculated_columns_agree_with_the_reference_interpreter() {
    let f = fixture();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let mut failures = Vec::new();
    for (table, expr) in COLUMNS {
        let expected = match interp::calculated_column(&f.model, &f.data, table, expr) {
            Ok(v) => v,
            Err(e) => {
                failures.push(format!("{table}: {expr}: the interpreter failed: {e}"));
                continue;
            }
        };
        let got = visual::calculated_column(&f.model, &schema, table, expr)
            .map_err(|e| e.to_string())
            .and_then(|(sql, _)| {
                f.db.arrow(&format!("SELECT v FROM ({sql}) ORDER BY rid"))
                    .map_err(|e| format!("{e}\n{sql}"))
            })
            .map(|b| {
                arrow_values(b)
                    .into_iter()
                    .map(|mut r| r.remove(0))
                    .collect::<Vec<_>>()
            });
        match got {
            Ok(g)
                if g.len() == expected.len()
                    && g.iter().zip(&expected).all(|(a, b)| same(a, b)) => {}
            Ok(g) => failures.push(format!(
                "{table}: {expr}:\n  expected: {expected:?}\n  got:      {g:?}"
            )),
            Err(e) => failures.push(format!("{table}: {expr}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} columns differ:\n\n{}",
        failures.len(),
        COLUMNS.len(),
        failures.join("\n\n")
    );
}

/// Random calculated columns: the random measures evaluated per row, with context
/// transition (CALCULATE) and without, plus row and earlier-row pieces.
#[test]
fn random_calculated_columns_agree_with_the_reference_interpreter() {
    let cases: usize = std::env::var("CHARTED_DAX_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(|n: usize| n / 4)
        .unwrap_or(64);
    let f = fixture();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let tables: &[(&str, &[&str])] = &[
        (
            "Product",
            &[
                "Product[ListPrice]",
                "COUNTROWS ( FILTER ( Product, Product[ListPrice] < EARLIER ( Product[ListPrice] ) ) )",
                "CALCULATE ( SUM ( Sales[Qty] ), ALLEXCEPT ( Product, Product[Category] ) )",
                "MAXX ( RELATEDTABLE ( Sales ), Sales[Price] )",
            ],
        ),
        (
            "Store",
            &[
                "LEN ( Store[Region] )",
                "COUNTROWS ( RELATEDTABLE ( Targets ) )",
            ],
        ),
        (
            "Date",
            &[
                "Date[MonthNumber]",
                "CALCULATE ( [Orders], FILTER ( ALL ( Date ), Date[Date] <= EARLIER ( Date[Date] ) ) )",
            ],
        ),
    ];
    let mut r = Rng(0x05EE_DC01);
    let mut failures = Vec::new();
    for i in 0..cases {
        let (table, pieces) = tables[r.below(tables.len())];
        let measure = random_expr(&mut r, 2);
        let expr = match r.below(4) {
            0 => format!("CALCULATE ( {measure} )"),
            1 => measure,
            2 => format!("{} + CALCULATE ( {measure} )", r.pick(pieces)),
            _ => format!(
                "IF ( {} > 2, {}, CALCULATE ( {measure} ) )",
                r.pick(pieces),
                r.pick(pieces)
            ),
        };
        let expected = interp::calculated_column(&f.model, &f.data, table, &expr);
        let got = visual::calculated_column(&f.model, &schema, table, &expr)
            .map_err(|e| e.to_string())
            .and_then(|(sql, _)| {
                f.db.arrow(&format!("SELECT v FROM ({sql}) ORDER BY rid"))
                    .map_err(|e| e.to_string())
            })
            .map(|b| {
                arrow_values(b)
                    .into_iter()
                    .map(|mut x| x.remove(0))
                    .collect::<Vec<_>>()
            });
        match (expected, got) {
            (Ok(e), Ok(g)) if g.len() == e.len() && g.iter().zip(&e).all(|(a, b)| same(a, b)) => {}
            (Ok(e), Ok(g)) => failures.push(format!(
                "#{i} {table}: {expr}\n  expected: {e:?}\n  got:      {g:?}"
            )),
            (Err(e), _) => {
                failures.push(format!("#{i} {table}: {expr}: the interpreter failed: {e}"))
            }
            (_, Err(e)) => failures.push(format!("#{i} {table}: {expr}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} random columns differ:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Calculated tables: the rows of table expressions evaluated with no filters.
const TABLES: &[&str] = &[
    "Product",
    "VALUES ( Product[Category] )",
    "ALL ( Date[Year], Date[Quarter] )",
    "FILTER ( Product, Product[ListPrice] > 100 )",
    "FILTER ( VALUES ( Product[Product] ), [Quantity] > 10 )",
    "TOPN ( 3, VALUES ( Product[Product] ), [Sales Amount] )",
    "CALCULATETABLE ( VALUES ( Store[Region] ), Product[Category] = \"Camping\" )",
    "CALCULATETABLE ( Targets, Store[Region] = \"North\" )",
    "ADDCOLUMNS ( VALUES ( Product[Category] ), \"Sales\", [Sales Amount] )",
    "SUMMARIZECOLUMNS ( Product[Category], Date[Year], \"Qty\", [Quantity] )",
    "SUMMARIZECOLUMNS ( Product[Category], Store[Region], FILTER ( ALL ( Date[Year] ), Date[Year] = 2025 ), \"Orders\", [Orders] )",
    "SUMMARIZE ( Sales, Product[Category], Store[Region] )",
    "SELECTCOLUMNS ( Product, \"Name\", Product[Product], \"Double Price\", Product[ListPrice] * 2 )",
    "CALENDAR ( DATE ( 2025, 1, 1 ), DATE ( 2025, 1, 31 ) )",
    "CALENDAR ( MIN ( Date[Date] ), MAX ( Date[Date] ) )",
    "GENERATESERIES ( 0, 1, 0.25 )",
    "{ 10, 20, 30 }",
    "{ ( \"Quantity\", NAMEOF ( [Quantity] ), 0 ), ( \"Category\", NAMEOF ( Product[Category] ), 1 ) }",
    "TOPN ( 2, ADDCOLUMNS ( VALUES ( Product[Category] ), \"Q\", [Quantity] ), [Q] )",
];

#[test]
fn calculated_tables_agree_with_the_reference_interpreter() {
    let f = fixture();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns.iter().find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column)).map(|c| c.data_type)
    };
    let mut failures = Vec::new();
    for expr in TABLES {
        let expected = match interp::calculated_table(&f.model, &f.data, expr) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{expr}: the interpreter failed: {e}"));
                continue;
            }
        };
        let got = visual::calculated_table(&f.model, &schema, expr)
            .map_err(|e| e.to_string())
            .and_then(|(sql, shape)| {
                f.db.arrow(&sql)
                    .map(|b| (shape, arrow_values(b)))
                    .map_err(|e| format!("{e}\n{sql}"))
            });
        match got {
            Ok((shape, rows)) => {
                let names: Vec<String> = shape.iter().map(|(n, _)| n.clone()).collect();
                if names != expected.columns {
                    failures.push(format!(
                        "{expr}: columns {names:?} vs {:?}",
                        expected.columns
                    ));
                }
                let (g, e) = (canonical(rows), canonical(expected.rows.clone()));
                let ok = g.len() == e.len()
                    && g.iter().zip(&e).all(|(a, b)| {
                        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y))
                    });
                if !ok {
                    failures.push(format!(
                        "{expr}:\n  expected:\n    {}\n  got:\n    {}",
                        show(&e).replace('\n', "\n    "),
                        show(&g).replace('\n', "\n    ")
                    ));
                }
            }
            Err(e) => failures.push(format!("{expr}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} tables differ:\n\n{}",
        failures.len(),
        TABLES.len(),
        failures.join("\n\n")
    );
}
