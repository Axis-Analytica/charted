//! Groups and bins: the DAX the group dialog writes (ui/src/report/groups.ts, checked
//! there by groups.test.ts) evaluates as calculated columns on the sample.

use charted_core::{App, DataRequest, Request};
use charted_model::*;
use serde_json::Value;

fn distinct(app: &App, table: &str, column: &str) -> Vec<Value> {
    let bytes = app
        .data(DataRequest::Distinct {
            view: charted_core::dataview::ViewSpec {
                table: table.into(),
                sort: vec![],
                filters: vec![],
            },
            column: column.into(),
            search: None,
            limit: 1000,
        })
        .unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["value"].clone())
        .collect()
}

#[test]
fn group_and_bin_columns_evaluate() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(dir.path().join("p").display().to_string()),
    })
    .unwrap();
    let m = app.session().unwrap().project.model.clone();
    let id = |t: &str| m.table_by_name(t).unwrap().id.clone();
    let cases: [(&str, &str, DataType); 6] = [
        (
            "Sales",
            "IF ( ISBLANK ( 'Sales'[Amount] ), BLANK (), INT ( 'Sales'[Amount] / 500 ) * 500 )",
            DataType::Double,
        ),
        (
            "Sales",
            "IF ( ISBLANK ( 'Sales'[Discount] ), BLANK (), ROUND ( INT ( 'Sales'[Discount] / 0.05 ) * 0.05, 2 ) )",
            DataType::Double,
        ),
        (
            "Date",
            "IF ( ISBLANK ( 'Date'[Date] ), BLANK (), DATE ( INT ( YEAR ( 'Date'[Date] ) / 2 ) * 2, 1, 1 ) )",
            DataType::Date,
        ),
        (
            "Date",
            "IF ( ISBLANK ( 'Date'[Date] ), BLANK (), DATE ( 1900, INT ( ( ( YEAR ( 'Date'[Date] ) - 1900 ) * 12 + MONTH ( 'Date'[Date] ) - 1 ) / 3 ) * 3 + 1, 1 ) )",
            DataType::Date,
        ),
        (
            "Date",
            "IF ( ISBLANK ( 'Date'[Date] ), BLANK (), DATE ( 1900, 1, 1 + INT ( DATEDIFF ( DATE ( 1900, 1, 1 ), 'Date'[Date], DAY ) / 7 ) * 7 ) )",
            DataType::Date,
        ),
        (
            "Product",
            "SWITCH (\n    TRUE (),\n    'Product'[Category] IN { \"Camping\", \"Packs\" }, \"Outdoor\",\n    'Product'[Category] IN { \"Footwear\" }, \"Say \"\"hi\"\"\",\n    \"Other\"\n)",
            DataType::String,
        ),
    ];
    for (i, (table, expression, ty)) in cases.iter().enumerate() {
        let column = Column {
            id: format!("c-group-{i}"),
            name: format!("Group {i}"),
            data_type: DataType::Unknown,
            kind: ColumnKind::Calculated {
                expression: expression.to_string(),
                language: ExpressionLanguage::Dax,
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
        };
        app.handle(Request::Commit {
            label: "New group".into(),
            changes: vec![Change::AddColumn {
                table: id(table),
                column,
                index: None,
            }],
        })
        .unwrap_or_else(|e| panic!("{expression}: {e:?}"));
        let model = app.session().unwrap().project.model.clone();
        let col = model
            .table(&id(table))
            .unwrap()
            .column(&format!("c-group-{i}"))
            .unwrap();
        assert_eq!(col.data_type, *ty, "{expression}");
    }
    let amounts = distinct(&app, &id("Sales"), "c-group-0");
    assert!(amounts.iter().all(|v| v.as_f64().unwrap() % 500.0 == 0.0));
    assert!(amounts.len() > 3, "{amounts:?}");
    let discounts = distinct(&app, &id("Sales"), "c-group-1");
    assert!(discounts.contains(&serde_json::json!(0.1)), "{discounts:?}");
    let years = distinct(&app, &id("Date"), "c-group-2");
    assert!(
        years
            .iter()
            .all(|v| v.as_str().unwrap().ends_with("-01-01")),
        "{years:?}"
    );
    let quarters = distinct(&app, &id("Date"), "c-group-3");
    assert!(
        quarters.contains(&serde_json::json!("2023-04-01")),
        "{quarters:?}"
    );
    assert!(quarters.iter().all(|v| {
        let month: u32 = v.as_str().unwrap()[5..7].parse().unwrap();
        month % 3 == 1
    }));
    // 1900-01-01 was a Monday: weeks start on Mondays.
    let weeks = distinct(&app, &id("Date"), "c-group-4");
    assert!(
        weeks.contains(&serde_json::json!("2022-01-03")),
        "{weeks:?}"
    );
    let groups = distinct(&app, &id("Product"), "c-group-5");
    assert_eq!(
        groups,
        [
            serde_json::json!("Other"),
            serde_json::json!("Outdoor"),
            serde_json::json!("Say \"hi\"")
        ]
    );
}
