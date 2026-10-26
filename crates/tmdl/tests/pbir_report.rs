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
