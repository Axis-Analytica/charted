//! The user's font library, and the copy of each font a report uses in its project.

use charted_core::api::{App, DataRequest, Request};
use serde_json::json;

const FONT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fonts/NotoSansBassaVah-Regular.ttf");

#[test]
fn added_fonts_are_listed_and_travel_with_the_projects_that_use_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    let project = dir.path().join("p");
    app.handle(Request::OpenSample { dir: Some(project.display().to_string()) }).unwrap();
    let s = app.handle(Request::AddFont { path: FONT.into() }).unwrap();
    assert_eq!(s.fonts.len(), 1);
    let f = &s.fonts[0];
    assert_eq!(f.family, "Noto Sans Bassa Vah");
    assert_eq!((f.weight, f.italic, f.in_library, f.in_project), (400, false, true, false));
    assert_eq!(app.data(DataRequest::ReadFont { file: f.file.clone() }).unwrap().len(), std::fs::metadata(FONT).unwrap().len() as usize);
    assert!(app.data(DataRequest::ReadFont { file: "../state/fonts/x.ttf".into() }).is_err());
    assert!(app.handle(Request::AddFont { path: concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml").into() }).is_err());

    // Saving a report that doesn't use it leaves the project alone.
    app.handle(Request::Save).unwrap();
    assert!(!project.join("fonts").exists());
    // One that does gets its own copy, which stays when the library's goes.
    let report = json!({ "pages": [{ "visuals": [{ "objects": { "title": { "fontFamily": "'Noto Sans Bassa Vah', sans-serif" } } }] }] });
    charted_core::fonts::bundle(&dir.path().join("state"), &project, &report).unwrap();
    assert!(project.join("fonts/NotoSansBassaVah-Regular.ttf").is_file());
    let s = app.handle(Request::RemoveFont { file: "NotoSansBassaVah-Regular.ttf".into() }).unwrap();
    assert_eq!((s.fonts[0].in_library, s.fonts[0].in_project), (false, true));
    assert!(app.data(DataRequest::ReadFont { file: "NotoSansBassaVah-Regular.ttf".into() }).is_ok());
}

#[test]
fn the_families_a_report_names() {
    let r = json!({ "theme": { "textClasses": { "title": { "fontFace": "\"Segoe UI\", wf_segoe-ui_normal, sans-serif" } } },
                    "pages": [{ "visuals": [{ "objects": { "text": { "fontFamily": "Lora" } } }] }] });
    assert_eq!(charted_core::fonts::used(&r), ["Lora", "Segoe UI", "sans-serif", "wf_segoe-ui_normal"]);
}
