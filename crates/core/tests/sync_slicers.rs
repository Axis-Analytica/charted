//! Synced slicers on the Harbor & Pine sample. A slicer synced to other pages has a copy
//! on each (visible or hidden) in one sync group, carrying the same choice: each copy
//! filters its own page, hidden copies included, and pages without one aren't filtered.

use arrow_ipc::reader::StreamReader;
use charted_core::visualq::slicer_filters;
use charted_core::{App, DataRequest, Request};
use charted_model::*;
use serde_json::json;

fn card_value(app: &App, id: &str) -> f64 {
    let bytes = app
        .data(DataRequest::ReportVisual {
            visual: id.into(),
            totals: false,
            filters: vec![],
            highlight: vec![],
            tag: None,
            search: None,
            roles: vec![],
        })
        .unwrap_or_else(|e| panic!("{}", e.message));
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    if let Some(batch) = reader.into_iter().next() {
        let batch = batch.unwrap();
        let i = batch.schema().index_of("values:0").unwrap();
        let f = arrow_cast::display::ArrayFormatter::try_new(batch.column(i), &Default::default())
            .unwrap();
        return f.value(0).to_string().parse().unwrap();
    }
    panic!("no rows for {id}");
}

fn visual(id: &str, kind: &str, role: &str, p: Projection) -> Visual {
    Visual {
        id: id.into(),
        visual_type: kind.into(),
        position: Position {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
            z: 0,
        },
        roles: [(role.to_string(), vec![p])].into(),
        filters: vec![],
        sort: None,
        objects: Default::default(),
        selection: None,
        drill: vec![],
        hidden: false,
    }
}

#[test]
fn a_synced_slicer_filters_every_page_it_has_a_copy_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(dir.path().join("p").display().to_string()),
    })
    .unwrap();
    let model = app.session().unwrap().project.model.clone();
    let dates = model.table_by_name("Date").unwrap();
    let year = Projection {
        field: FieldRef::Column {
            table: dates.id.clone(),
            column: dates.column_by_name("Year").unwrap().id.clone(),
        },
        aggregation: None,
        display_name: None,
    };
    let (t, m) = model
        .tables
        .iter()
        .find_map(|t| {
            t.measures
                .iter()
                .find(|m| m.name == "Total Sales")
                .map(|m| (t, m))
        })
        .unwrap();
    let sales = Projection {
        field: FieldRef::Measure {
            table: t.id.clone(),
            measure: m.id.clone(),
        },
        aggregation: None,
        display_name: None,
    };

    // Page A shows the slicer, page B has a hidden copy, page C has none.
    let mut slicer = visual("slicer-a", "slicer", "field", year.clone());
    slicer.objects =
        serde_json::from_value(json!({ "syncGroup": { "groupName": "Year" } })).unwrap();
    slicer.selection = Some(FilterKind::Basic {
        values: vec![Some("2024".into())],
        exclude: false,
    });
    let mut copy = slicer.clone();
    copy.id = "slicer-b".into();
    copy.hidden = true;
    let page = |id: &str, visuals: Vec<Visual>| Change::AddPage {
        page: Page {
            id: id.into(),
            settings: PageSettings::named(id),
            visuals,
        },
        index: None,
    };
    app.handle(Request::Commit {
        label: "pages".into(),
        changes: vec![
            page(
                "A",
                vec![slicer, visual("card-a", "card", "values", sales.clone())],
            ),
            page(
                "B",
                vec![copy, visual("card-b", "card", "values", sales.clone())],
            ),
            page("C", vec![visual("card-c", "card", "values", sales.clone())]),
        ],
    })
    .unwrap_or_else(|e| panic!("{}", e.message));

    let a = card_value(&app, "card-a");
    let b = card_value(&app, "card-b");
    let c = card_value(&app, "card-c");
    assert!((a - b).abs() < 0.01, "{a} vs {b}");
    assert!(
        a < c * 0.5,
        "2024 alone ({a}) should be well under all years ({c})"
    );

    // The hidden copy is what filters page B.
    let report = &app.session().unwrap().project.model.report;
    let filters = slicer_filters(report.page("B").unwrap(), "card-b");
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].field.field, year.field);
    assert!(slicer_filters(report.page("C").unwrap(), "card-c").is_empty());

    // Clearing the choice on every copy (one edit, as the UI does) unfilters both pages.
    let changes = ["A", "B"]
        .iter()
        .map(|p| {
            let page = report.page(p).unwrap();
            let mut v = page.visuals.iter().find(|v| v.is_slicer()).unwrap().clone();
            v.selection = None;
            Change::UpdateVisual {
                page: p.to_string(),
                visual: v,
            }
        })
        .collect();
    app.handle(Request::Commit {
        label: "clear".into(),
        changes,
    })
    .unwrap();
    assert!((card_value(&app, "card-a") - c).abs() < 0.01);
    assert!((card_value(&app, "card-b") - c).abs() < 0.01);
}
