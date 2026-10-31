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
    draft: Mutex<Option<Vec<charted_compiler::steps::Query>>>,
}

impl Fake {
    fn new(group: &str) -> Self {
        Self {
            group: group.into(),
            data: false,
            allow: true,
            asked: Mutex::new(vec![]),
            changes: AtomicUsize::new(0),
            draft: Mutex::new(None),
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
    fn draft(&self) -> Option<Vec<charted_compiler::steps::Query>> {
        self.draft.lock().unwrap().clone()
    }
    fn set_draft(&self, q: Option<Vec<charted_compiler::steps::Query>>) {
        *self.draft.lock().unwrap() = q;
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
    // It says how the rows are ordered (months in calendar order, or alphabetically).
    assert!(rows.starts_with("Sorted by 'Date'[YearMonth]"), "{rows}");
    let donut = ok(
        &app,
        &turn,
        "query_visual",
        json!({ "visual": "Total Sales by Category" }),
    )
    .text;
    assert!(
        donut.starts_with("Sorted by [Total Sales], largest first."),
        "{donut}"
    );
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

#[test]
fn query_changes_are_drafted_and_checked() {
    let (_d, app) = sample();
    let turn = Fake::new("q");
    let before = app
        .lock()
        .unwrap()
        .session()
        .unwrap()
        .project
        .queries
        .clone();
    let list = ok(&app, &turn, "describe_queries", json!({})).text;
    assert!(list.contains("query \"Store\""), "{list}");
    let one = ok(&app, &turn, "describe_queries", json!({ "query": "Store" })).text;
    assert!(one.contains("columns:") && one.contains("Region"), "{one}");
    let o = ok(
        &app,
        &turn,
        "draft_query",
        json!({ "action": "add_step", "query": "Store", "step": { "op": "renameColumns", "renames": [["Region", "Area"]] } }),
    );
    assert!(o.text.contains("Area"), "{}", o.text);
    // A step that doesn't fit is refused, and the draft keeps what came before.
    err(
        &app,
        &turn,
        "draft_query",
        json!({ "action": "add_step", "query": "Store", "step": { "op": "removeColumns", "columns": ["Nope"] } }),
    );
    assert_eq!(
        turn.draft()
            .unwrap()
            .iter()
            .find(|q| q.name == "Store")
            .unwrap()
            .steps
            .len(),
        before
            .iter()
            .find(|q| q.name == "Store")
            .unwrap()
            .steps
            .len()
            + 1
    );
    assert!(
        ok(&app, &turn, "describe_queries", json!({}))
            .text
            .contains("draft")
    );
    // Nothing changes until the user applies it.
    assert_eq!(
        app.lock().unwrap().session().unwrap().project.queries,
        before
    );
}

#[test]
fn the_agent_reshapes_the_layout_and_it_undoes_as_one() {
    let (_d, app) = sample();
    let turn = Fake::new("l");
    let before = app
        .lock()
        .unwrap()
        .layouts()
        .workspace
        .active()
        .layout
        .clone();
    ok(
        &app,
        &turn,
        "set_pane",
        json!({ "pane": "fields", "place": "float", "x": 40, "y": 60 }),
    );
    ok(&app, &turn, "split_view", json!({ "view": "model" }));
    // Buttons are known once the window lists them.
    assert!(
        ok(&app, &turn, "list_commands", json!({}))
            .text
            .contains("hasn't listed")
    );
    app.lock()
        .unwrap()
        .handle(Request::SetCommands {
            commands: vec![charted_core::workspace::CommandInfo {
                id: "align-left".into(),
                label: "Align left".into(),
                place: "report toolbar".into(),
                uses: 0,
            }],
        })
        .unwrap();
    assert!(
        ok(&app, &turn, "list_commands", json!({}))
            .text
            .contains("align-left \"Align left\" in the report toolbar: used 0 times")
    );
    let no = Fake {
        allow: false,
        ..Fake::new("l")
    };
    err(&app, &no, "hide_commands", json!({ "ids": ["align-left"] }));
    ok(
        &app,
        &turn,
        "hide_commands",
        json!({ "ids": ["align-left"] }),
    );
    let described = ok(&app, &turn, "describe_workspace", json!({})).text;
    assert!(
        described.contains("pane fields: float")
            && described.contains("second view: model")
            && described.contains("hidden buttons: align-left"),
        "{described}"
    );
    // The request's layout changes undo in one step.
    let mut a = app.lock().unwrap();
    a.handle(Request::UndoLayout).unwrap();
    assert_eq!(a.layouts().workspace.active().layout, before);
    drop(a);
    ok(
        &app,
        &turn,
        "profile",
        json!({ "action": "switch", "name": "Two report pages" }),
    );
    ok(
        &app,
        &turn,
        "profile",
        json!({ "action": "create", "name": "Mine" }),
    );
    assert_eq!(
        app.lock().unwrap().layouts().workspace.active().name,
        "Mine"
    );
}

#[test]
fn report_wide_formatting_is_one_step_and_skips_what_a_type_lacks() {
    let (_d, app) = sample();
    let turn = Fake::new("f1");
    ok(
        &app,
        &turn,
        "page",
        json!({ "action": "add", "name": "Fmt" }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Fmt", "type": "columnChart", "title": "Sales By Region YoY",
                "fields": { "category": ["'Store'[Region]"], "values": ["[Total Sales]"] } }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Fmt", "type": "slicer", "fields": { "field": ["'Store'[Region]"] } }),
    );
    let o = ok(
        &app,
        &turn,
        "format_visuals",
        json!({ "page": "Fmt", "title_case": "sentence", "format": { "title": { "fontSize": 14 }, "dataLabels": { "show": true } } }),
    );
    assert!(o.text.starts_with("Formatted 2 visuals."), "{}", o.text);
    assert!(o.text.contains("slicer has no dataLabels"), "{}", o.text);
    let m = model(&app);
    let p = m
        .report
        .pages
        .iter()
        .find(|p| p.settings.name == "Fmt")
        .unwrap();
    let chart = p
        .visuals
        .iter()
        .find(|v| v.visual_type == "columnChart")
        .unwrap();
    assert_eq!(chart.objects["title"]["text"], "Sales by region YoY");
    assert_eq!(chart.objects["title"]["fontSize"], 14);
    assert_eq!(chart.objects["dataLabels"]["show"], true);
    // Nothing left to change is an error, so the agent doesn't think it did something.
    err(
        &app,
        &turn,
        "format_visuals",
        json!({ "page": "Fmt", "title_case": "sentence", "format": { "title": { "fontSize": 14 } } }),
    );
}

#[test]
fn remember_adds_to_the_projects_instructions_when_the_user_agrees() {
    let (_d, app) = sample();
    let dir = app.lock().unwrap().session().unwrap().project.dir.clone();
    let turn = Fake::new("r1");
    ok(
        &app,
        &turn,
        "remember",
        json!({ "instruction": "Always use the Date table" }),
    );
    assert_eq!(
        charted_agent::settings::instructions(&dir),
        "- Always use the Date table\n"
    );
    assert!(turn.asked.lock().unwrap()[0].contains("Always use the Date table"));
    let no = Fake {
        allow: false,
        ..Fake::new("r2")
    };
    err(
        &app,
        &no,
        "remember",
        json!({ "instruction": "Titles in capitals" }),
    );
    assert!(!charted_agent::settings::instructions(&dir).contains("capitals"));
}

#[test]
fn a_slicer_is_named_in_its_header_and_layers_can_be_moved() {
    let (_d, app) = sample();
    let turn = Fake::new("s1");
    ok(
        &app,
        &turn,
        "page",
        json!({ "action": "add", "name": "Layers" }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Layers", "type": "slicer", "title": "Store", "fields": { "field": ["'Store'[Region]"] } }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Layers", "type": "card", "title": "Sales", "fields": { "values": ["[Total Sales]"] } }),
    );
    let page = |app: &Mutex<App>| {
        model(app)
            .report
            .pages
            .into_iter()
            .find(|p| p.settings.name == "Layers")
            .unwrap()
    };
    let p = page(&app);
    let slicer = p
        .visuals
        .iter()
        .find(|v| v.visual_type == "slicer")
        .unwrap();
    // Only the header names it: a title as well would show "Store Store".
    assert_eq!(slicer.objects["header"]["text"], "Store");
    assert!(slicer.objects.get("title").is_none());
    let card = p.visuals.iter().find(|v| v.visual_type == "card").unwrap();
    assert!(card.position.z > slicer.position.z);
    ok(
        &app,
        &turn,
        "update_visual",
        json!({ "visual": "Store", "layer": "front" }),
    );
    let p = page(&app);
    let z = |ty: &str| {
        p.visuals
            .iter()
            .find(|v| v.visual_type == ty)
            .unwrap()
            .position
            .z
    };
    assert!(z("slicer") > z("card"));
    let o = ok(&app, &turn, "describe_report", json!({ "page": "Layers" }));
    assert!(o.text.contains("layer "), "{}", o.text);
    // Arranging keeps slicers on top, so their dropdowns open over the cards.
    ok(
        &app,
        &turn,
        "update_visual",
        json!({ "visual": "Store", "layer": "back" }),
    );
    ok(&app, &turn, "arrange_page", json!({ "page": "Layers" }));
    let p = page(&app);
    // Its short band fits a dropdown, not a list.
    let slicer = p
        .visuals
        .iter()
        .find(|v| v.visual_type == "slicer")
        .unwrap();
    assert_eq!(slicer.objects["slicer"]["style"], "dropdown");
    let z = |ty: &str| {
        p.visuals
            .iter()
            .find(|v| v.visual_type == ty)
            .unwrap()
            .position
            .z
    };
    assert!(z("slicer") > z("card"));
}

#[test]
fn fonts_for_the_report_and_for_one_visual() {
    let (_d, app) = sample();
    let turn = Fake::new("f1");
    let o = ok(&app, &turn, "list_visual_types", json!({}));
    assert!(o.text.contains("Playfair Display"), "{}", o.text);
    // Following the desktop, fonts alone keep its colours.
    ok(&app, &turn, "set_theme", json!({ "builtin": "desktop" }));
    ok(
        &app,
        &turn,
        "set_theme",
        json!({ "fonts": { "title": "Helvetica", "label": "Inter" } }),
    );
    let t = model(&app).report.theme.unwrap();
    assert_eq!(t.json["followDesktop"], true);
    assert_eq!(t.json["textClasses"]["title"]["fontFace"], "Helvetica");
    assert_eq!(t.json["textClasses"]["label"]["fontFace"], "Inter");
    err(
        &app,
        &turn,
        "set_theme",
        json!({ "fonts": { "body": "Inter" } }),
    );
    ok(
        &app,
        &turn,
        "page",
        json!({ "action": "add", "name": "Fonts" }),
    );
    ok(
        &app,
        &turn,
        "add_visual",
        json!({ "page": "Fonts", "type": "textbox", "format": { "text": { "content": "# Overview", "fontFamily": "Lora" } } }),
    );
}
