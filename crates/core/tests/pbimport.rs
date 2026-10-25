//! Importing a Power BI project: the Contoso fixture (written the way Power BI Desktop
//! writes one, with things charted can't use), then charted's own export of it back
//! in, unchanged.

use arrow_ipc::reader::StreamReader;
use charted_compiler::visual::VisualQuery;
use charted_core::{App, DataRequest, Request};
use charted_model::*;
use std::path::Path;

fn rows(bytes: Vec<u8>) -> Vec<Vec<String>> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).unwrap())
            .collect();
        for r in 0..batch.num_rows() {
            out.push(fmts.iter().map(|f| f.value(r).to_string()).collect());
        }
    }
    out
}

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/Contoso")
}

#[test]
fn imports_contoso_with_a_report_of_what_it_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let (dir, report) =
        charted_core::pbimport::import(&fixture().join("Contoso.pbip"), tmp.path()).unwrap();
    assert_eq!(dir, tmp.path().join("Contoso"));
    let skipped: Vec<String> = report
        .skipped
        .iter()
        .map(|s| format!("{} {}", s.kind, s.name))
        .collect();
    for expected in [
        "table Time Calculations",
        "table LocalDateTable_2b7e1d3c-62a1-4c55-9f0e-71b8c3a5f2a4",
        "table Date",
        "query Targets",
        "hierarchy Product[Category Hierarchy]",
        "role Regional managers",
        "measure Orders[Avg Order]",
        "measure Orders[Sales Rank]",
        "visual Overview: map",
        "visual Trend: kpi",
    ] {
        assert!(
            skipped.iter().any(|s| s == expected),
            "{expected} not in {skipped:#?}"
        );
    }
    // Time intelligence, DIVIDE and the calculated column are fine.
    assert!(
        !skipped
            .iter()
            .any(|s| s.contains("Sales LY") || s.contains("Big Order")),
        "{skipped:#?}"
    );
    assert_eq!(
        (report.tables, report.loaded, report.relationships),
        (4, 2, 2)
    );
    assert_eq!((report.pages, report.visuals), (2, 6));
    assert!(
        report.notes.iter().any(|n| n.contains("orders.csv")),
        "{:?}",
        report.notes
    );

    let mut app = App::new(tmp.path().join("state"));
    app.handle(Request::OpenProject {
        path: dir.display().to_string(),
    })
    .unwrap();
    let m = app.session().unwrap().project.model.clone();
    let date = m.table_by_name("Date").unwrap();
    assert!(date.is_date_table);
    assert_eq!(
        date.column_by_name("Date").unwrap().data_type,
        DataType::Date
    );
    let orders = m.table_by_name("Orders").unwrap();
    assert_eq!(
        orders.column_by_name("OrderDate").unwrap().data_type,
        DataType::Date
    );
    assert_eq!(
        orders.column_by_name("Amount").unwrap().data_type,
        DataType::Decimal
    );
    let total = orders
        .measures
        .iter()
        .find(|x| x.name == "Total Sales")
        .unwrap();
    assert_eq!(total.display_folder.as_deref(), Some("Sales"));
    assert_eq!(
        total.description.as_deref(),
        Some("Sales in dollars, before returns.")
    );
    let overview = &m.report.pages[0];
    assert_eq!(overview.settings.name, "Overview");
    assert!(
        matches!(&overview.settings.filters[0].filter, FilterKind::Basic { values, .. } if values.len() == 2)
    );
    let types: Vec<&str> = overview
        .visuals
        .iter()
        .map(|v| v.visual_type.as_str())
        .collect();
    assert_eq!(types, ["card", "columnChart", "table", "slicer"]);

    // The data: Total Sales by category, as the CSV adds up.
    let product = m.table_by_name("Product").unwrap();
    let q = VisualQuery {
        groups: vec![Projection {
            field: FieldRef::Column {
                table: product.id.clone(),
                column: product.column_by_name("Category").unwrap().id.clone(),
            },
            aggregation: None,
            display_name: None,
        }],
        values: vec![Projection {
            field: FieldRef::Measure {
                table: orders.id.clone(),
                measure: total.id.clone(),
            },
            aggregation: None,
            display_name: None,
        }],
        ..Default::default()
    };
    let got = rows(app.data(DataRequest::Visual { query: q }).unwrap());
    let csv = std::fs::read_to_string(fixture().join("data/orders.csv")).unwrap();
    let all: f64 = csv
        .lines()
        .skip(1)
        .map(|l| l.rsplit(',').next().unwrap().parse::<f64>().unwrap())
        .sum();
    let sum: f64 = got.iter().map(|r| r[1].parse::<f64>().unwrap()).sum();
    assert_eq!(got.len(), 3);
    assert!((sum - all).abs() < 0.01, "{sum} vs {all}");

    // Export as a Power BI project and import that: nothing changes.
    let out = tmp.path().join("export");
    app.handle(Request::ExportPbip {
        dir: out.display().to_string(),
    })
    .unwrap();
    let (again, report2) =
        charted_core::pbimport::import(&out.join("Contoso.pbip"), tmp.path()).unwrap();
    assert_eq!(again, tmp.path().join("Contoso 2"));
    let back = charted_core::project::Project::open(&again).unwrap();
    assert_eq!(
        charted_core::interop::diff(&m, &back.model),
        Vec::<String>::new()
    );
    assert_eq!(back.model.report, m.report);
    // Only the measures charted can't evaluate are reported this time.
    let kinds: Vec<&str> = report2.skipped.iter().map(|s| s.kind.as_str()).collect();
    assert_eq!(kinds, ["measure"], "{:#?}", report2.skipped);
    assert_eq!(report2.loaded, 2);
}
