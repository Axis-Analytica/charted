//! Times typical report visual queries: `visualbench PROJECT [RUNS]`.

use charted_compiler::visual::{QueryFilter, VisualQuery};
use charted_core::{Session, visualq};
use charted_model::{FieldRef, FilterKind, Model, Projection};
use std::time::Instant;

fn col(m: &Model, table: &str, column: &str) -> Projection {
    let t = m.table_by_name(table).expect("table");
    Projection {
        field: FieldRef::Column {
            table: t.id.clone(),
            column: t.column_by_name(column).expect("column").id.clone(),
        },
        aggregation: None,
        display_name: None,
    }
}

fn measure(m: &Model, name: &str) -> Projection {
    let (t, me) = m
        .tables
        .iter()
        .find_map(|t| t.measures.iter().find(|x| x.name == name).map(|x| (t, x)))
        .expect("measure");
    Projection {
        field: FieldRef::Measure {
            table: t.id.clone(),
            measure: me.id.clone(),
        },
        aggregation: None,
        display_name: None,
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: visualbench PROJECT [RUNS]");
    let runs: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(7);
    let s = Session::open(std::path::Path::new(&path)).expect("open");
    let m = &s.project.model;
    let year = QueryFilter {
        field: col(m, "Date", "Year"),
        filter: FilterKind::Basic {
            values: vec![Some("2025".into())],
            exclude: false,
        },
    };
    let cases: Vec<(&str, VisualQuery)> = vec![
        (
            "card Total Sales",
            VisualQuery {
                values: vec![measure(m, "Total Sales")],
                ..Default::default()
            },
        ),
        (
            "Total Sales by Category",
            VisualQuery {
                groups: vec![col(m, "Product", "Category")],
                values: vec![measure(m, "Total Sales")],
                ..Default::default()
            },
        ),
        (
            "Total Sales by YearMonth",
            VisualQuery {
                groups: vec![col(m, "Date", "YearMonth")],
                values: vec![measure(m, "Total Sales")],
                ..Default::default()
            },
        ),
        (
            "Sales, Cost by Region (2025)",
            VisualQuery {
                groups: vec![col(m, "Store", "Region")],
                values: vec![measure(m, "Total Sales"), measure(m, "Total Cost")],
                filters: vec![year.clone()],
                ..Default::default()
            },
        ),
        (
            "Sales and Target by Year",
            VisualQuery {
                groups: vec![col(m, "Date", "Year")],
                values: vec![measure(m, "Total Sales"), measure(m, "Target")],
                ..Default::default()
            },
        ),
        (
            "Orders by Product (top 1000)",
            VisualQuery {
                groups: vec![col(m, "Product", "Product")],
                values: vec![measure(m, "Orders")],
                limit: Some(1000),
                ..Default::default()
            },
        ),
    ];
    let time = |name: &str, sql: &str| {
        let mut times = Vec::new();
        for _ in 0..runs {
            let t = Instant::now();
            s.database().arrow(sql).expect("run");
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "{name:<40} p50 {:>7.1} ms   p95 {:>7.1} ms",
            times[times.len() / 2],
            times[(times.len() * 95 / 100).min(times.len() - 1)]
        );
    };
    for (name, q) in cases {
        let c = visualq::compile(m, s.schema(), &q).expect("compile");
        time(name, &c.sql);
    }

    // Report visuals (on a scratch copy of the model): a matrix with subtotals, a
    // highlighted column chart, a cross-filtered line chart and a slicer.
    let mut model = m.clone();
    let visual =
        |id: &str, kind: &str, roles: Vec<(&str, Vec<Projection>)>| charted_model::Visual {
            id: id.into(),
            visual_type: kind.into(),
            position: charted_model::Position {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
                z: 0,
            },
            roles: roles.into_iter().map(|(r, p)| (r.to_string(), p)).collect(),
            filters: vec![],
            sort: None,
            objects: Default::default(),
            selection: None,
            drill: Vec::new(),
        };
    let sales = measure(m, "Total Sales");
    let page = charted_model::Page {
        id: "bench".into(),
        settings: charted_model::PageSettings::named("Bench"),
        visuals: vec![
            visual(
                "matrix",
                "matrix",
                vec![
                    (
                        "rows",
                        vec![
                            col(m, "Product", "Category"),
                            col(m, "Product", "Subcategory"),
                        ],
                    ),
                    ("columns", vec![col(m, "Date", "Year")]),
                    ("values", vec![sales.clone()]),
                ],
            ),
            visual(
                "column",
                "columnChart",
                vec![
                    ("category", vec![col(m, "Product", "Category")]),
                    ("values", vec![sales.clone()]),
                ],
            ),
            visual(
                "line",
                "lineChart",
                vec![
                    ("category", vec![col(m, "Date", "YearMonth")]),
                    ("values", vec![sales.clone()]),
                ],
            ),
            visual(
                "slicer",
                "slicer",
                vec![("field", vec![col(m, "Store", "Region")])],
            ),
        ],
    };
    model.report.pages.push(page);
    let region = QueryFilter {
        field: col(m, "Store", "Region"),
        filter: FilterKind::Basic {
            values: vec![Some("Europe".into())],
            exclude: false,
        },
    };
    let report = |id: &str, filters: &[QueryFilter], highlight: &[QueryFilter]| {
        visualq::report_visual_sql(
            &model,
            s.schema(),
            id,
            &visualq::VisualSqlOptions {
                filters,
                highlight,
                ..Default::default()
            },
        )
        .expect("compile")
    };
    time(
        "matrix Category/Subcategory x Year",
        &report("matrix", &[], &[]),
    );
    time(
        "column by Category, highlighted",
        &report("column", &[], std::slice::from_ref(&region)),
    );
    time(
        "line by YearMonth, cross-filtered",
        &report("line", std::slice::from_ref(&region), &[]),
    );
    time("slicer Region", &report("slicer", &[], &[]));
}
