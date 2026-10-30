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
        "table LocalDateTable_2b7e1d3c-62a1-4c55-9f0e-71b8c3a5f2a4",
        "query Targets",
        "role Regional managers: Orders",
        "measure Orders[Avg Order]",
        "visual Overview: map",
    ] {
        assert!(
            skipped.iter().any(|s| s == expected),
            "{expected} not in {skipped:#?}"
        );
    }
    // The calendar, the field parameter and the calculation group come in.
    assert!(
        !skipped.iter().any(|s| {
            s == "table Date" || s == "table Metric Choice" || s == "table Time Calculations"
        }),
        "{skipped:#?}"
    );
    // RANKX is evaluated now.
    assert!(!skipped.iter().any(|s| s == "measure Orders[Sales Rank]"));
    // Time intelligence, DIVIDE, the calculated column and hierarchies are fine.
    assert!(
        !skipped
            .iter()
            .any(|s| s.contains("Sales LY") || s.contains("Big Order") || s.contains("Hierarchy")),
        "{skipped:#?}"
    );
    assert_eq!(
        (report.tables, report.loaded, report.relationships),
        (6, 2, 2)
    );
    // The KPI (v1.2) comes across; the map doesn't.
    assert!(!skipped.iter().any(|s| s.contains("kpi")), "{skipped:#?}");
    assert_eq!((report.pages, report.visuals), (2, 7));
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
        date.calculated.as_ref().map(|c| c.expression.as_str()),
        Some("CALENDAR ( DATE ( 2024, 1, 1 ), DATE ( 2025, 12, 31 ) )")
    );
    let snap = app.session().unwrap().snapshot().unwrap();
    assert!(snap.calc_errors.is_empty(), "{:?}", snap.calc_errors);
    assert_eq!(
        snap.tables.iter().find(|t| t.id == date.id).unwrap().rows,
        731,
        "two years of days"
    );
    assert_eq!(
        date.column_by_name("Date").unwrap().data_type,
        DataType::Date
    );
    // The calculation group has its two items.
    let group = m.table_by_name("Time Calculations").unwrap();
    let items: Vec<&str> = group
        .calculation_group
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(items, ["Current", "YTD"]);
    // The field parameter has its two choices.
    let metric = m.table_by_name("Metric Choice").unwrap();
    assert_eq!(
        snap.tables.iter().find(|t| t.id == metric.id).unwrap().rows,
        2
    );
    let items = charted_core::fieldparams::items(&m, metric).unwrap();
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Total Sales", "Sales LY"]);
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
    // Roles come in (this one's dynamic filter is kept for Power BI).
    let role = &m.roles[0];
    assert_eq!(role.name, "Regional managers");
    assert_eq!(role.filters[0].table, orders.id);
    assert_eq!(role.filters[0].expression, "[Region] = USERPRINCIPALNAME()");
    let product = m.table_by_name("Product").unwrap();
    let h = &product.hierarchies[0];
    assert_eq!(h.name, "Category Hierarchy");
    assert_eq!(h.id, "5b8e0d2f-1c5a-4f9d-8a3b-5c0e3f9b19c0");
    let levels: Vec<(&str, &str)> = h
        .levels
        .iter()
        .map(|l| {
            (
                l.name.as_str(),
                product.column(&l.column).unwrap().name.as_str(),
            )
        })
        .collect();
    assert_eq!(
        levels,
        [("Category", "Category"), ("Subcategory", "Subcategory")]
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
        options: None,
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
    // Nothing else is reported this time: every measure that came across evaluates
    // (the role's dynamic filter is noted again).
    assert!(
        report2.skipped.iter().all(|s| s.kind == "role"),
        "{:#?}",
        report2.skipped
    );
    assert_eq!(report2.loaded, 2);
    // The interop diff names a calculated table's changed expression.
    let mut edited = back.model.clone();
    let date_id = m.table_by_name("Date").unwrap().id.clone();
    edited.table_mut(&date_id).unwrap().calculated = Some(CalculatedTable {
        expression: "CALENDAR ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ) )".into(),
        language: ExpressionLanguage::Dax,
    });
    assert_eq!(
        charted_core::interop::diff(&m, &edited),
        ["Date: expression changed"]
    );
}
