//! `charted query --visual` prints a report visual's rows by field name.

use charted_core::{App, Request};
use charted_model::*;
use std::process::Command;

#[test]
fn query_visual_prints_rows_by_field_name() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p");
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(project.display().to_string()),
    })
    .unwrap();
    let m = app.session().unwrap().project.model.clone();
    let sales = m.table_by_name("Sales").unwrap();
    let product = m.table_by_name("Product").unwrap();
    let total = sales
        .measures
        .iter()
        .find(|x| x.name == "Total Sales")
        .unwrap();
    let visual = Visual {
        id: "v-1".into(),
        visual_type: "barChart".into(),
        position: Position {
            x: 0.0,
            y: 0.0,
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
        selection: None,
    };
    let page = m.report.pages[0].id.clone();
    app.handle(Request::Commit {
        label: "Add visual".into(),
        changes: vec![Change::AddVisual {
            page,
            visual,
            index: None,
        }],
    })
    .unwrap();
    app.handle(Request::Save).unwrap();
    // The CLI needs the project to itself.
    drop(app);

    let bin = env!("CARGO_BIN_EXE_charted-headless");
    let out = Command::new(bin)
        .args([
            "query",
            project.to_str().unwrap(),
            "--visual",
            "total sales by category",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows.len(), 6);
    assert!(rows[0]["Category"].is_string());
    let v: Vec<f64> = rows
        .iter()
        .map(|r| r["Total Sales"].as_f64().unwrap())
        .collect();
    assert!(v.windows(2).all(|w| w[0] >= w[1]), "ranked by value");

    let unknown = Command::new(bin)
        .args(["query", project.to_str().unwrap(), "--visual", "nope"])
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("Total Sales by Category (v-1"));
}
