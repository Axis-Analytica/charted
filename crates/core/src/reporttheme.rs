//! Power BI theme files (`.json`): imported whole into the report, so properties
//! charted doesn't use survive a round trip. The UI reads the colours, text classes
//! and the visual styles it has format objects for (ui/src/report/themeStyles.ts).

use charted_model::ReportTheme;
use serde_json::Value;
use std::path::Path;

use crate::session::CoreError;

fn is_color(v: &Value) -> bool {
    v.as_str().is_some_and(|s| {
        let h = s.trim_start_matches('#');
        s.starts_with('#')
            && matches!(h.len(), 3 | 6 | 8)
            && h.chars().all(|c| c.is_ascii_hexdigit())
    })
}

/// Reads and checks a Power BI theme file: a JSON object with at least one of
/// `dataColors`, `background` or `foreground`, every one of them a hex colour.
pub fn read(path: &Path) -> Result<ReportTheme, CoreError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| CoreError::Invalid(format!("couldn't read {}: {e}", path.display())))?;
    parse(
        &text,
        path.file_stem().and_then(|s| s.to_str()).unwrap_or("Theme"),
    )
}

/// Writes `theme` as a Power BI theme file, named after it.
pub fn write(path: &Path, theme: &ReportTheme) -> Result<(), CoreError> {
    let mut json = theme.json.clone();
    if let Some(o) = json.as_object_mut() {
        o.insert("name".into(), Value::String(theme.name.clone()));
    }
    let mut text = serde_json::to_string_pretty(&json).unwrap_or_default();
    text.push('\n');
    crate::atomic::write(path, text.as_bytes())
        .map_err(|e| CoreError::Invalid(format!("couldn't write {}: {e}", path.display())))
}

pub fn parse(text: &str, fallback_name: &str) -> Result<ReportTheme, CoreError> {
    let json: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| CoreError::Invalid(format!("this isn't a theme file: {e}")))?;
    let Some(o) = json.as_object() else {
        return Err(CoreError::Invalid(
            "this isn't a theme file: it isn't a JSON object".into(),
        ));
    };
    let colors = o.get("dataColors");
    if let Some(c) = colors {
        let ok = c
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(is_color));
        if !ok {
            return Err(CoreError::Invalid(
                "the theme's dataColors must be a list of colours such as \"#118DFF\"".into(),
            ));
        }
    }
    for key in ["background", "foreground", "tableAccent"] {
        if let Some(v) = o.get(key)
            && !is_color(v)
        {
            return Err(CoreError::Invalid(format!(
                "the theme's {key} must be a colour such as \"#FFFFFF\""
            )));
        }
    }
    if colors.is_none() && !o.contains_key("background") && !o.contains_key("foreground") {
        return Err(CoreError::Invalid(
            "this isn't a theme file: it has no dataColors, background or foreground".into(),
        ));
    }
    let name = o
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_name)
        .to_string();
    Ok(ReportTheme { name, json })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_power_bi_themes_and_explains_bad_ones() {
        let t = parse(
            r##"{"name":"Tidal","dataColors":["#094782","#0B72D7","#098BF5"],"background":"#FFFFFF","foreground":"#252423","tableAccent":"#094782","visualStyles":{"*":{"*":{"title":[{"show":true}]}}}}"##,
            "x",
        )
        .unwrap();
        assert_eq!(t.name, "Tidal");
        assert!(t.json["visualStyles"].is_object(), "kept whole");
        assert_eq!(
            parse(r##"{"dataColors":["#123456"]}"##, "file")
                .unwrap()
                .name,
            "file"
        );
        for bad in [
            "[]",
            "not json",
            r#"{"name":"x"}"#,
            r#"{"dataColors":["blue"]}"#,
            r##"{"dataColors":["#123456"],"background":"white"}"##,
        ] {
            assert!(parse(bad, "x").is_err(), "{bad}");
        }
    }

    #[test]
    fn written_themes_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.json");
        let theme = ReportTheme {
            name: "Edited".into(),
            json: serde_json::json!({"name": "Old", "dataColors": ["#123456"], "textClasses": {"title": {"fontSize": 16}}, "visualStyles": {"*": {"*": {"title": [{"show": false}]}}}}),
        };
        write(&path, &theme).unwrap();
        let back = read(&path).unwrap();
        assert_eq!(back.name, "Edited");
        assert_eq!(back.json["visualStyles"], theme.json["visualStyles"]);
        assert_eq!(back.json["textClasses"], theme.json["textClasses"]);
    }
}
