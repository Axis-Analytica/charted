//! The agent's tools on the sample project.

use charted_agent::tools::{self, Outcome, Turn};
use charted_core::api::{App, AppState, Request};
use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fake {
    group: String,
    data: bool,
    allow: bool,
    asked: Mutex<Vec<String>>,
    changes: AtomicUsize,
}

impl Fake {
    fn new(group: &str) -> Self {
        Self {
            group: group.into(),
            data: false,
            allow: true,
            asked: Mutex::new(vec![]),
            changes: AtomicUsize::new(0),
        }
    }
}

impl Turn for Fake {
    fn group(&self) -> &str {
        &self.group
    }
    fn label(&self) -> &str {
        "Agent: test"
    }
    fn data(&self) -> bool {
        self.data
    }
    fn confirm(&self, what: &str) -> bool {
        self.asked.lock().unwrap().push(what.into());
        self.allow
    }
    fn changed(&self, _: &AppState) {
        self.changes.fetch_add(1, Ordering::Relaxed);
    }
}

fn sample() -> (tempfile::TempDir, Mutex<App>) {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(dir.path().join("p").display().to_string()),
    })
    .unwrap();
    (dir, Mutex::new(app))
}

fn ok(app: &Mutex<App>, turn: &dyn Turn, name: &str, args: Value) -> Outcome {
    let o = tools::call(app, turn, name, &args);
    assert!(!o.is_error, "{name} {args}: {}", o.text);
    o
}

fn err(app: &Mutex<App>, turn: &dyn Turn, name: &str, args: Value) -> String {
    let o = tools::call(app, turn, name, &args);
    assert!(o.is_error, "{name} {args} should fail: {}", o.text);
    o.text
}

fn model(app: &Mutex<App>) -> charted_model::Model {
    app.lock().unwrap().session().unwrap().project.model.clone()
}

#[test]
fn every_tool_has_a_schema() {
    let list = tools::list();
    assert!(list.len() >= 20);
    for t in &list {
        assert_eq!(t.input_schema["type"], "object", "{}", t.name);
    }
}

#[test]
fn the_agent_builds_a_page_in_one_undo_step() {
    let (_d, app) = sample();
    let before = model(&app);
    let turn = Fake::new("g1");
    let described = ok(&app, &turn, "describe_model", json!({})).text;
    assert!(described.contains("'Sales'"), "{described}");
    assert!(described.contains("measure [Total Sales]"), "{described}");

    let page = ok(
        &app,
        &turn,
        "page",
        json!({ "action": "add", "name": "Sales overview" }),
    );
    assert_eq!(page.step.as_ref().unwrap().op, "Added page \"%1\"");
    ok(
        &app,
        &turn,
        "add_visual",
        json!({
            "page": "Sales overview", "type": "lineChart", "title": "Sales by month",
            "fields": { "category": ["'Date'[YearMonth]"], "values": ["[Total Sales]"] }
        }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({
            "page": "Sales overview", "type": "donutChart",
            "fields": { "legend": ["'Product'[Category]"], "values": ["[Total Sales]"] }
        }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Sales overview", "type": "slicer", "fields": { "field": ["'Store'[Region]"] } }),
    );
    let m = model(&app);
    let p = m
        .report
        .pages
        .iter()
        .find(|p| p.settings.name == "Sales overview")
        .unwrap();
    assert_eq!(p.visuals.len(), 3);
    for (i, a) in p.visuals.iter().enumerate() {
        for b in &p.visuals[i + 1..] {
            let (a, b) = (&a.position, &b.position);
            assert!(
                a.x + a.width <= b.x
                    || b.x + b.width <= a.x
                    || a.y + a.height <= b.y
                    || b.y + b.height <= a.y
            );
        }
    }

    // Totals come back without data access; a slicer's values don't.
    let rows = ok(
        &app,
        &turn,
        "query_visual",
        json!({ "visual": "Sales by month" }),
    )
    .text;
    assert!(rows.contains("Total Sales"), "{rows}");
    let slicer = p
        .visuals
        .iter()
        .find(|v| v.visual_type == "slicer")
        .unwrap()
        .id
        .clone();
    assert!(
        err(&app, &turn, "query_visual", json!({ "visual": slicer }))
            .contains("data access is off")
    );
    assert!(
        err(
            &app,
            &turn,
            "distinct_values",
            json!({ "field": "'Store'[Region]" })
        )
        .contains("data access")
    );
    let peek = Fake {
        data: true,
        ..Fake::new("g1")
    };
    assert!(
        ok(
            &app,
            &peek,
            "distinct_values",
            json!({ "field": "'Store'[Region]" })
        )
        .text
        .contains("values in all")
    );

    ok(
        &app,
        &turn,
        "arrange_page",
        json!({ "page": "Sales overview" }),
    );
    assert!(turn.changes.load(Ordering::Relaxed) >= 5);

    let mut a = app.lock().unwrap();
    let state = a.handle(Request::State).unwrap();
    assert_eq!(
        state.project.as_ref().unwrap().undo_group.as_deref(),
        Some("g1")
    );
    a.handle(Request::UndoGroup { group: "g1".into() }).unwrap();
    assert_eq!(a.session().unwrap().project.model, before);
}

#[test]
fn bad_names_and_dax_are_refused_with_suggestions() {
    let (_d, app) = sample();
    let turn = Fake::new("g");
    let msg = err(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Page 1", "type": "card", "fields": { "values": ["[Totl Sales]"] } }),
    );
    assert!(msg.contains("[Total Sales]"), "{msg}");
    let msg = err(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Nope", "type": "card" }),
    );
    assert!(msg.contains("no page"), "{msg}");
    let msg = err(
        &app,
        &turn,
        "measure",
        json!({ "action": "add", "table": "Sales", "name": "Bad", "expression": "SUMM ( Sales[Amount] )" }),
    );
    assert!(msg.contains("doesn't check"), "{msg}");
    let msg = err(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Page 1", "type": "lineChart", "fields": { "category": ["[Total Sales]"] } }),
    );
    assert!(msg.contains("groups by columns"), "{msg}");
}

#[test]
fn the_model_changes_and_undoes() {
    let (_d, app) = sample();
    let before = model(&app);
    let turn = Fake::new("m");
    ok(
        &app,
        &turn,
        "measure",
        json!({ "action": "add", "table": "Sales", "name": "Avg Sale", "expression": "AVERAGE ( Sales[Amount] )", "format": "$#,0.00" }),
    );
    ok(
        &app,
        &turn,
        "measure",
        json!({ "action": "update", "name": "Avg Sale", "expression": "DIVIDE ( [Total Sales], COUNTROWS ( Sales ) )" }),
    );
    ok(
        &app,
        &turn,
        "column",
        json!({ "action": "add_calculated", "table": "Sales", "name": "Big", "expression": "Sales[Amount] > 100" }),
    );
    ok(
        &app,
        &turn,
        "column",
        json!({ "action": "update", "table": "Sales", "name": "Big", "hidden": true }),
    );
    ok(
        &app,
        &turn,
        "table",
        json!({ "action": "add_calculated", "name": "Years", "expression": "DISTINCT ( 'Date'[Year] )" }),
    );
    ok(
        &app,
        &turn,
        "hierarchy",
        json!({ "action": "add", "table": "Date", "name": "Calendar", "levels": ["Year", "[MonthName]"] }),
    );
    ok(
        &app,
        &turn,
        "role",
        json!({ "action": "add", "name": "Europe", "filters": { "Store": "[Region] = \"Europe\"" } }),
    );
    ok(
        &app,
        &turn,
        "calculation_group",
        json!({ "action": "add", "name": "Scenario", "items": [
        { "name": "Actual", "expression": "SELECTEDMEASURE ()" },
        { "name": "Double", "expression": "SELECTEDMEASURE () * 2" }
    ] }),
    );
    ok(
        &app,
        &turn,
        "parameter",
        json!({ "kind": "field", "name": "Metric", "fields": ["[Total Sales]", "[Avg Sale]"] }),
    );
    ok(
        &app,
        &turn,
        "parameter",
        json!({ "kind": "numeric", "name": "Growth", "min": 0, "max": 10, "step": 1, "default": 2 }),
    );
    ok(&app, &turn, "set_theme", json!({ "builtin": "Daylight" }));
    ok(
        &app,
        &turn,
        "set_filter",
        json!({ "scope": "report", "field": "'Store'[Region]", "filter": { "kind": "basic", "values": ["Europe"] } }),
    );
    ok(
        &app,
        &turn,
        "bookmark",
        json!({ "action": "add", "name": "Start", "page": "Page 1" }),
    );
    let m = model(&app);
    assert!(
        m.table_by_name("Scenario")
            .unwrap()
            .calculation_group
            .is_some()
    );
    assert_eq!(m.roles.last().unwrap().name, "Europe");
    let described = ok(&app, &turn, "describe_model", json!({ "table": "Date" })).text;
    assert!(
        described.contains("hierarchy [Calendar] = [Year] > [MonthName]"),
        "{described}"
    );

    // Deleting something in use asks; a no leaves it.
    let no = Fake {
        allow: false,
        ..Fake::new("m")
    };
    err(
        &app,
        &no,
        "measure",
        json!({ "action": "remove", "name": "Total Sales" }),
    );
    assert_eq!(no.asked.lock().unwrap().len(), 1);
    assert!(
        model(&app)
            .tables
            .iter()
            .any(|t| t.measures.iter().any(|x| x.name == "Total Sales"))
    );
    // An unused one goes without asking.
    ok(
        &app,
        &no,
        "column",
        json!({ "action": "remove", "table": "Sales", "name": "Big" }),
    );
    assert_eq!(no.asked.lock().unwrap().len(), 1);

    let mut a = app.lock().unwrap();
    a.handle(Request::UndoGroup { group: "m".into() }).unwrap();
    assert_eq!(a.session().unwrap().project.model, before);
}
