//! Visual queries against the Harbor & Pine sample, checked against hand-written SQL.

use arrow_ipc::reader::StreamReader;
use charted_compiler::visual::{OrderBy, OrderTarget, QueryFilter, VisualQuery};
use charted_core::visualq::DaxCheck;
use charted_core::{App, DataRequest, Request};
use charted_model::*;
use std::sync::{Mutex, MutexGuard, OnceLock};

struct Fixture {
    app: App,
    _dir: tempfile::TempDir,
}

fn fixture() -> MutexGuard<'static, Fixture> {
    static F: OnceLock<Mutex<Fixture>> = OnceLock::new();
    F.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"));
        app.handle(Request::OpenSample {
            dir: Some(dir.path().join("sample").display().to_string()),
        })
        .unwrap();
        Mutex::new(Fixture { app, _dir: dir })
    })
    .lock()
    .unwrap_or_else(|p| p.into_inner())
}

fn model(f: &Fixture) -> Model {
    f.app.session().unwrap().project.model.clone()
}

fn col(f: &Fixture, table: &str, column: &str) -> Projection {
    let m = model(f);
    let t = m.table_by_name(table).unwrap();
    Projection {
        field: FieldRef::Column {
            table: t.id.clone(),
            column: t.column_by_name(column).unwrap().id.clone(),
        },
        aggregation: None,
        display_name: None,
    }
}

fn measure(f: &Fixture, name: &str) -> Projection {
    let m = model(f);
    let (t, me) = m
        .tables
        .iter()
        .find_map(|t| t.measures.iter().find(|x| x.name == name).map(|x| (t, x)))
        .unwrap_or_else(|| panic!("no measure {name}"));
    Projection {
        field: FieldRef::Measure {
            table: t.id.clone(),
            measure: me.id.clone(),
        },
        aggregation: None,
        display_name: None,
    }
}

/// Rows of the result as text cells.
fn run(f: &Fixture, query: VisualQuery) -> Vec<Vec<String>> {
    arrow_rows(
        f.app
            .data(DataRequest::Visual { query })
            .unwrap_or_else(|e| panic!("{}", e.message)),
    )
}

fn arrow_rows(bytes: Vec<u8>) -> Vec<Vec<String>> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).unwrap())
            .collect();
        for r in 0..batch.num_rows() {
            rows.push(fmts.iter().map(|f| f.value(r).to_string()).collect());
        }
    }
    rows
}

/// Rows of hand-written SQL, as text cells in column order, numbers rounded to cents.
fn sql(f: &Fixture, q: &str) -> Vec<Vec<String>> {
    raw_sql(f, q)
        .into_iter()
        .map(|r| r.into_iter().map(|c| cents(&c)).collect())
        .collect()
}

fn raw_sql(f: &Fixture, q: &str) -> Vec<Vec<String>> {
    arrow_rows(f.app.session().unwrap().database().arrow(q).unwrap())
}

fn cents(s: &str) -> String {
    s.parse::<f64>()
        .map(|x| format!("{:.2}", (x * 100.0).round() / 100.0))
        .unwrap_or_else(|_| s.to_string())
}

fn rounded(rows: Vec<Vec<String>>) -> Vec<Vec<String>> {
    rows.into_iter()
        .map(|r| r.into_iter().map(|c| cents(&c)).collect())
        .collect()
}

fn q(groups: Vec<Projection>, values: Vec<Projection>) -> VisualQuery {
    VisualQuery {
        groups,
        values,
        ..Default::default()
    }
}

#[test]
fn card_and_grouped_measure_match_sql() {
    let f = fixture();
    let total = run(&f, q(vec![], vec![measure(&f, "Total Sales")]));
    assert_eq!(rounded(total), sql(&f, "SELECT sum(Amount) FROM sales"));

    let by_cat = run(
        &f,
        q(
            vec![col(&f, "Product", "Category")],
            vec![measure(&f, "Total Sales")],
        ),
    );
    assert_eq!(
        rounded(by_cat),
        sql(
            &f,
            "SELECT p.Category, sum(s.Amount) FROM sales s LEFT JOIN product p ON s.ProductKey = p.ProductKey GROUP BY 1 ORDER BY 1 NULLS FIRST"
        )
    );
}

#[test]
fn measures_from_two_fact_tables_share_the_date_axis() {
    let f = fixture();
    let rows = run(
        &f,
        q(
            vec![col(&f, "Date", "Year")],
            vec![measure(&f, "Total Sales"), measure(&f, "Target")],
        ),
    );
    let expected = sql(
        &f,
        "WITH s AS (SELECT d.Year, sum(s.Amount) a FROM sales s LEFT JOIN date d ON s.OrderDateKey = d.DateKey GROUP BY 1),
              t AS (SELECT d.Year, sum(t.TargetAmount) b FROM targets t LEFT JOIN date d ON t.DateKey = d.DateKey GROUP BY 1)
         SELECT coalesce(s.Year, t.Year) y, a, b FROM s FULL JOIN t ON s.Year = t.Year ORDER BY 1",
    );
    assert_eq!(rounded(rows), expected);
}

#[test]
fn unrelated_groups_repeat_the_total() {
    let f = fixture();
    let rows = run(
        &f,
        q(
            vec![col(&f, "Product", "Category")],
            vec![measure(&f, "Target")],
        ),
    );
    let total = sql(&f, "SELECT sum(TargetAmount) FROM targets")[0][0].clone();
    let cats = sql(&f, "SELECT DISTINCT Category FROM product ORDER BY 1");
    assert_eq!(rows.len(), cats.len());
    for (r, c) in rows.iter().zip(&cats) {
        assert_eq!(r[0], c[0]);
        assert_eq!(cents(&r[1]), total);
    }
}

#[test]
fn new_measure_with_divide_filters_topn_and_sort_by() {
    let mut f = fixture();
    let sales = model(&f).table_by_name("Sales").unwrap().id.clone();
    f.app
        .handle(Request::Commit {
            label: "New measure".into(),
            changes: vec![Change::AddMeasure {
                table: sales,
                measure: Measure {
                    id: "m-margin".into(),
                    name: "Margin %".into(),
                    expression: "DIVIDE ( [Total Sales] - [Total Cost], [Total Sales] )".into(),
                    language: ExpressionLanguage::Dax,
                    description: None,
                    format_string: Some("0.0%".into()),
                    display_folder: None,
                    hidden: false,
                    extras: Default::default(),
                },
                index: None,
            }],
        })
        .unwrap();

    // Page filter Year = 2025.
    let year = QueryFilter {
        field: col(&f, "Date", "Year"),
        filter: FilterKind::Basic {
            values: vec![Some("2025".into())],
            exclude: false,
        },
    };
    let mut query = q(
        vec![col(&f, "Store", "Region")],
        vec![measure(&f, "Margin %")],
    );
    query.filters = vec![year.clone()];
    let rows = run(&f, query);
    let expected = raw_sql(
        &f,
        "SELECT st.Region, ((sum(s.Amount) - sum(s.Cost)) / sum(s.Amount))::DOUBLE FROM sales s
         LEFT JOIN store st ON s.StoreKey = st.StoreKey LEFT JOIN date d ON s.OrderDateKey = d.DateKey
         WHERE d.Year = 2025 GROUP BY 1 ORDER BY 1",
    );
    let r = |rows: Vec<Vec<String>>| -> Vec<Vec<String>> {
        rows.into_iter()
            .map(|r| {
                r.into_iter()
                    .map(|c| c.parse::<f64>().map(|x| format!("{x:.4}")).unwrap_or(c))
                    .collect()
            })
            .collect()
    };
    let expected_4: Vec<Vec<String>> = db_rows4(&f, &expected);
    assert_eq!(r(rows), expected_4);

    // Top 3 categories by Total Sales, ordered by it.
    let mut top = q(
        vec![col(&f, "Product", "Category")],
        vec![measure(&f, "Total Sales")],
    );
    top.filters = vec![QueryFilter {
        field: col(&f, "Product", "Category"),
        filter: FilterKind::TopN {
            count: 3,
            bottom: false,
            by: measure(&f, "Total Sales"),
        },
    }];
    top.order = vec![OrderBy {
        target: OrderTarget::Value(0),
        descending: true,
    }];
    let rows = run(&f, top);
    let expected = sql(
        &f,
        "SELECT p.Category, sum(s.Amount) a FROM sales s LEFT JOIN product p ON s.ProductKey = p.ProductKey GROUP BY 1 ORDER BY 2 DESC LIMIT 3",
    );
    assert_eq!(rounded(rows), expected);

    // MonthName follows MonthNumber.
    let rows = run(
        &f,
        q(
            vec![col(&f, "Date", "MonthName")],
            vec![measure(&f, "Units")],
        ),
    );
    assert_eq!(rows[0][0], "January");
    assert_eq!(rows[11][0], "December");

    // An implicit measure and a value filter.
    let mut qty = col(&f, "Sales", "Quantity");
    qty.aggregation = Some(Aggregation::Sum);
    let mut vq = q(vec![col(&f, "Store", "Region")], vec![qty.clone()]);
    vq.filters = vec![QueryFilter {
        field: qty,
        filter: FilterKind::Advanced {
            conditions: vec![Condition {
                op: FilterOp::Greater,
                value: Some("0".into()),
                value2: None,
            }],
            or: false,
        },
    }];
    let rows = run(&f, vq);
    assert!(!rows.is_empty() && rows.iter().all(|r| r[1].parse::<f64>().unwrap() > 0.0));

    f.app.handle(Request::Undo).unwrap();
}

fn db_rows4(_f: &Fixture, rows: &[Vec<String>]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|c| {
                    c.parse::<f64>()
                        .map(|x| format!("{x:.4}"))
                        .unwrap_or(c.clone())
                })
                .collect()
        })
        .collect()
}

#[test]
fn dax_checks_report_errors_and_cycles() {
    let mut f = fixture();
    let check = |f: &Fixture, e: &str, m: Option<&str>| -> DaxCheck {
        serde_json::from_slice(
            &f.app
                .data(DataRequest::CheckDax {
                    expression: e.into(),
                    measure: m.map(str::to_string),
                })
                .unwrap(),
        )
        .unwrap()
    };
    let ok = check(&f, "DIVIDE ( [Total Sales], [Units] )", None);
    assert!(ok.diagnostics.is_empty() && ok.sql.is_some());
    let bad = check(&f, "SUM ( Sales[Nope] )", None);
    assert!(
        bad.diagnostics[0]
            .message
            .contains("no column named [Nope]")
    );

    // Units = [Orders] + 1 while Orders = [Units] would be circular.
    let units = model(&f)
        .tables
        .iter()
        .flat_map(|t| t.measures.iter())
        .find(|m| m.name == "Units")
        .unwrap()
        .id
        .clone();
    let orders = model(&f)
        .tables
        .iter()
        .flat_map(|t| t.measures.iter())
        .find(|m| m.name == "Orders")
        .unwrap()
        .clone();
    let sales = model(&f).table_by_name("Sales").unwrap().id.clone();
    f.app
        .handle(Request::Commit {
            label: "Edit".into(),
            changes: vec![Change::UpdateMeasure {
                table: sales,
                measure: Measure {
                    expression: "[Units] + 1".into(),
                    ..orders
                },
            }],
        })
        .unwrap();
    let cyc = check(&f, "[Orders] * 2", Some(&units));
    assert!(
        cyc.diagnostics[0].message.contains("refers to itself"),
        "{:?}",
        cyc.diagnostics
    );
    f.app.handle(Request::Undo).unwrap();
}

#[test]
fn field_values_for_filter_cards() {
    let f = fixture();
    let v: serde_json::Value = serde_json::from_slice(
        &f.app
            .data(DataRequest::FieldValues {
                field: col(&f, "Date", "Year"),
                filters: vec![],
                search: None,
                limit: 100,
            })
            .unwrap(),
    )
    .unwrap();
    let keys: Vec<&str> = v["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["2022", "2023", "2024", "2025"]);
}

#[test]
fn report_visuals_query_by_role_and_survive_save_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p");
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(project.display().to_string()),
    })
    .unwrap();
    let m = app.session().unwrap().project.model.clone();
    let page = m.report.pages[0].id.clone();
    let sales = m.table_by_name("Sales").unwrap();
    let product = m.table_by_name("Product").unwrap();
    let total = sales
        .measures
        .iter()
        .find(|x| x.name == "Total Sales")
        .unwrap();
    let visual = Visual {
        id: "v-col".into(),
        visual_type: "columnChart".into(),
        position: Position {
            x: 20.0,
            y: 20.0,
            width: 400.0,
            height: 300.0,
            z: 0,
        },
        roles: [
            (
                "category".to_string(),
                vec![Projection {
                    field: FieldRef::Column {
                        table: product.id.clone(),
                        column: product.column_by_name("Category").unwrap().id.clone(),
                    },
                    aggregation: None,
                    display_name: None,
                }],
            ),
            (
                "values".to_string(),
                vec![Projection {
                    field: FieldRef::Measure {
                        table: sales.id.clone(),
                        measure: total.id.clone(),
                    },
                    aggregation: None,
                    display_name: None,
                }],
            ),
        ]
        .into(),
        filters: vec![],
        sort: None,
        objects: Default::default(),
    };
    app.handle(Request::Commit {
        label: "Add visual".into(),
        changes: vec![Change::AddVisual {
            page: page.clone(),
            visual,
            index: None,
        }],
    })
    .unwrap();
    let data = |app: &App| {
        arrow_rows(
            app.data(DataRequest::ReportVisual {
                visual: "v-col".into(),
                totals: false,
                filters: vec![],
            })
            .unwrap(),
        )
    };
    let before = data(&app);
    assert_eq!(before.len(), 6);
    // Ranked by value, largest first.
    let v: Vec<f64> = before.iter().map(|r| r[1].parse().unwrap()).collect();
    assert!(v.windows(2).all(|w| w[0] >= w[1]));
    let total_row = arrow_rows(
        app.data(DataRequest::ReportVisual {
            visual: "v-col".into(),
            totals: true,
            filters: vec![],
        })
        .unwrap(),
    );
    assert_eq!(total_row.len(), 1);

    let report_before = app.session().unwrap().project.model.report.clone();
    app.handle(Request::Save).unwrap();
    app.handle(Request::CloseProject).unwrap();
    app.handle(Request::OpenProject {
        path: project.display().to_string(),
    })
    .unwrap();
    assert_eq!(app.session().unwrap().project.model.report, report_before);
    assert_eq!(data(&app), before);
}
