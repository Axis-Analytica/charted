//! Report building in PBIR: hidden visuals and pages, layer and tab order, buttons and
//! page navigators. Each is checked as Power BI reads it, with charted's own
//! annotations removed, so the mapping to Power BI's properties is what is tested.
use std::collections::BTreeMap;

use charted_model::*;
use serde_json::{Value, json};

fn visual(id: &str, visual_type: &str, z: i32) -> Visual {
    Visual {
        id: id.into(),
        visual_type: visual_type.into(),
        position: Position {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 100.0,
            z,
        },
        roles: Default::default(),
        filters: vec![],
        sort: None,
        objects: Default::default(),
        selection: None,
        drill: vec![],
        hidden: false,
    }
}

fn model(pages: Vec<Page>) -> Model {
    let mut m = Model::default();
    m.report.pages = pages;
    m
}

/// The report's files with charted's annotations taken out (as Power BI would leave a
/// file it wrote itself).
fn as_power_bi(model: &Model) -> BTreeMap<String, String> {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(o) => {
                o.remove("annotations");
                o.values_mut().for_each(strip);
            }
            Value::Array(a) => a.iter_mut().for_each(strip),
            _ => {}
        }
    }
    charted_tmdl::pbir::report_files(model)
        .into_iter()
        .map(|(p, t)| match serde_json::from_str::<Value>(&t) {
            Ok(mut v) if p.ends_with(".json") => {
                strip(&mut v);
                (p, v.to_string())
            }
            _ => (p, t),
        })
        .collect()
}

fn file(files: &BTreeMap<String, String>, suffix: &str) -> Value {
    let (_, t) = files
        .iter()
        .find(|(p, _)| p.ends_with(suffix))
        .unwrap_or_else(|| panic!("no file ending {suffix}"));
    serde_json::from_str(t).unwrap()
}

fn read(files: &BTreeMap<String, String>) -> (Report, Vec<charted_tmdl::read::Skipped>) {
    let mut skipped = Vec::new();
    let r = charted_tmdl::pbir::read_pbir(&Model::default(), files, &mut skipped).unwrap();
    (r, skipped)
}

#[test]
fn hidden_visuals_and_pages_and_the_layer_order() {
    let mut hidden = visual("shape1", "shape", 3);
    hidden.hidden = true;
    let mut tooltip = PageSettings::named("Tooltip");
    tooltip.hidden = true;
    let m = model(vec![
        Page {
            id: "p1".into(),
            settings: PageSettings::named("Overview"),
            visuals: vec![visual("text1", "textbox", 1), hidden],
        },
        Page {
            id: "p2".into(),
            settings: tooltip,
            visuals: vec![],
        },
    ]);
    let files = as_power_bi(&m);
    let v = file(&files, "shape1/visual.json");
    assert_eq!(v["isHidden"], json!(true));
    assert_eq!(v["position"]["z"], json!(3));
    assert_eq!(v["position"]["tabOrder"], json!(3));
    assert!(file(&files, "text1/visual.json").get("isHidden").is_none());
    assert_eq!(
        file(&files, "p2/page.json")["visibility"],
        json!("HiddenInViewMode")
    );
    assert!(file(&files, "p1/page.json").get("visibility").is_none());

    let (back, _) = read(&files);
    let p1 = &back.pages[0];
    assert!(!p1.settings.hidden);
    assert!(back.pages[1].settings.hidden);
    let shape = p1.visual("shape1").unwrap();
    assert!(shape.hidden);
    assert_eq!(shape.position.z, 3);
    assert!(!p1.visual("text1").unwrap().hidden);
}

#[test]
fn the_tab_order_stands_in_for_a_missing_layer_order() {
    let m = model(vec![Page {
        id: "p1".into(),
        settings: PageSettings::named("Overview"),
        visuals: vec![visual("text1", "textbox", 0)],
    }]);
    let mut files = as_power_bi(&m);
    let key = files
        .keys()
        .find(|k| k.ends_with("text1/visual.json"))
        .unwrap()
        .clone();
    let mut v: Value = serde_json::from_str(&files[&key]).unwrap();
    let pos = v["position"].as_object_mut().unwrap();
    pos.remove("z");
    pos.insert("tabOrder".into(), json!(7000));
    files.insert(key, v.to_string());
    let (back, _) = read(&files);
    assert_eq!(back.pages[0].visuals[0].position.z, 7000);
}

#[test]
fn synced_slicers_keep_their_group() {
    let mut a = visual("slicerA", "slicer", 1);
    a.objects = serde_json::from_value(json!({ "syncGroup": { "groupName": "Region" } })).unwrap();
    let mut b = a.clone();
    b.id = "slicerB".into();
    b.hidden = true;
    let m = model(vec![
        Page {
            id: "p1".into(),
            settings: PageSettings::named("One"),
            visuals: vec![a],
        },
        Page {
            id: "p2".into(),
            settings: PageSettings::named("Two"),
            visuals: vec![b],
        },
    ]);
    let files = as_power_bi(&m);
    assert_eq!(
        file(&files, "slicerB/visual.json")["visual"]["syncGroup"],
        json!({ "groupName": "Region", "fieldChanges": true, "filterChanges": true })
    );
    let (back, _) = read(&files);
    for p in &back.pages {
        assert_eq!(
            p.visuals[0].objects["syncGroup"],
            json!({ "groupName": "Region" })
        );
    }
    assert!(back.pages[1].visuals[0].hidden);
}

#[test]
fn buttons_and_page_navigators() {
    let button = |id: &str, objects: Value| {
        let mut v = visual(id, "actionButton", 1);
        v.objects = serde_json::from_value(objects).unwrap();
        v
    };
    let m = model(vec![
        Page {
            id: "p1".into(),
            settings: PageSettings::named("Overview"),
            visuals: vec![
                button(
                    "back1",
                    json!({ "button": { "kind": "back", "text": "Go back" } }),
                ),
                button(
                    "nav1",
                    json!({ "button": { "kind": "pageNavigation" }, "action": { "page": "p2", "tooltip": "To details" } }),
                ),
                button(
                    "bm1",
                    json!({ "button": { "kind": "blank", "text": "Q1" }, "action": { "type": "bookmark", "bookmark": "b-1" } }),
                ),
                visual("pages1", "pageNavigator", 2),
            ],
        },
        Page {
            id: "p2".into(),
            settings: PageSettings::named("Details"),
            visuals: vec![],
        },
    ]);
    let files = as_power_bi(&m);
    let back = file(&files, "back1/visual.json");
    assert_eq!(back["visual"]["visualType"], json!("actionButton"));
    assert_eq!(
        back["visual"]["objects"]["icon"][0]["properties"]["shapeType"]["expr"]["Literal"]["Value"],
        json!("'back'")
    );
    let link = &back["visual"]["visualContainerObjects"]["visualLink"][0]["properties"];
    assert_eq!(link["type"]["expr"]["Literal"]["Value"], json!("'Back'"));
    let nav = file(&files, "nav1/visual.json");
    let link = &nav["visual"]["visualContainerObjects"]["visualLink"][0]["properties"];
    assert_eq!(
        link["type"]["expr"]["Literal"]["Value"],
        json!("'PageNavigation'")
    );
    assert_eq!(
        link["navigationSection"]["expr"]["Literal"]["Value"],
        json!("'p2'")
    );
    assert_eq!(
        file(&files, "pages1/visual.json")["visual"]["visualType"],
        json!("pageNavigator")
    );

    let (r, skipped) = read(&files);
    assert!(skipped.is_empty(), "{skipped:?}");
    let p = &r.pages[0];
    let objects = |id: &str| Value::Object(p.visual(id).unwrap().objects.clone());
    assert_eq!(
        objects("back1"),
        json!({ "button": { "kind": "back", "text": "Go back" }, "action": { "type": "back" } })
    );
    assert_eq!(
        objects("nav1"),
        json!({ "button": { "kind": "pageNavigation" }, "action": { "type": "pageNavigation", "page": "p2", "tooltip": "To details" } })
    );
    assert_eq!(
        objects("bm1")["action"],
        json!({ "type": "bookmark", "bookmark": "b-1" })
    );
    assert_eq!(p.visual("pages1").unwrap().visual_type, "pageNavigator");
}

#[test]
fn a_slicers_header_comes_across() {
    let mut s = visual("slicerA", "slicer", 1);
    s.objects = serde_json::from_value(
        json!({ "header": { "show": true, "text": "Store", "fontSize": 14.0, "fontFamily": "Lora" }, "title": { "show": false } }),
    )
    .unwrap();
    let m = model(vec![Page {
        id: "p1".into(),
        settings: PageSettings::named("One"),
        visuals: vec![s],
    }]);
    let files = as_power_bi(&m);
    let header = &file(&files, "slicerA/visual.json")["visual"]["objects"]["header"][0]["properties"];
    assert_eq!(header["text"]["expr"]["Literal"]["Value"], "'Store'");
    assert_eq!(header["fontSize"]["expr"]["Literal"]["Value"], "14D");
    let (back, _) = read(&files);
    assert_eq!(
        back.pages[0].visuals[0].objects["header"],
        json!({ "show": true, "text": "Store", "fontSize": 14.0, "fontFamily": "Lora" })
    );
}

#[test]
fn a_text_boxs_markdown_becomes_styled_runs() {
    let mut t = visual("text1", "textbox", 1);
    t.objects = serde_json::from_value(
        json!({ "text": { "content": "# Sales\nUp **12%** on *last year*, see [notes](https://x.org)" } }),
    )
    .unwrap();
    let m = model(vec![Page {
        id: "p1".into(),
        settings: PageSettings::named("One"),
        visuals: vec![t],
    }]);
    let files = as_power_bi(&m);
    let ps = &file(&files, "text1/visual.json")["visual"]["objects"]["general"][0]["properties"]["paragraphs"];
    assert_eq!(ps[0]["textRuns"][0], json!({ "value": "Sales", "textStyle": { "fontWeight": "bold" } }));
    assert_eq!(ps[1]["textRuns"][1], json!({ "value": "12%", "textStyle": { "fontWeight": "bold" } }));
    assert_eq!(ps[1]["textRuns"][3], json!({ "value": "last year", "textStyle": { "fontStyle": "italic" } }));
    assert_eq!(ps[1]["textRuns"][5], json!({ "value": "notes", "url": "https://x.org" }));
    let (back, _) = read(&files);
    assert_eq!(
        back.pages[0].visuals[0].objects["text"]["content"],
        "**Sales**\nUp **12%** on *last year*, see [notes](https://x.org)"
    );
}
