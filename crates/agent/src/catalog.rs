//! The visual types the agent can add, their roles and formatting, and the built-in
//! themes: `catalog.json`, generated from the UI's registry (ui/src/report/catalog.test.ts).

use serde::Deserialize;
use serde_json::{Map, Value};
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub visuals: Vec<VisualDef>,
    pub themes: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct VisualDef {
    #[serde(rename = "type")]
    pub ty: String,
    pub label: String,
    pub roles: Vec<RoleDef>,
    pub size: Size,
    pub decor: bool,
    pub objects: Vec<ObjectDef>,
}

#[derive(Debug, Deserialize)]
pub struct RoleDef {
    pub name: String,
    pub label: String,
    /// grouping, value or any.
    pub kind: String,
    pub max: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Deserialize)]
pub struct ObjectDef {
    pub name: String,
    pub props: Vec<PropDef>,
}

#[derive(Debug, Deserialize)]
pub struct PropDef {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub default: Value,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub options: Option<Vec<String>>,
}

pub fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| serde_json::from_str(include_str!("catalog.json")).expect("catalog.json is valid"))
}

pub fn visual(ty: &str) -> Option<&'static VisualDef> {
    catalog().visuals.iter().find(|v| v.ty.eq_ignore_ascii_case(ty))
}

pub fn types() -> String {
    catalog().visuals.iter().map(|v| v.ty.as_str()).collect::<Vec<_>>().join(", ")
}

impl VisualDef {
    pub fn role(&self, name: &str) -> Option<&RoleDef> {
        self.roles.iter().find(|r| r.name.eq_ignore_ascii_case(name))
    }

    /// The type described for the agent: roles, size and formatting.
    pub fn describe(&self, objects: bool) -> String {
        let roles = self
            .roles
            .iter()
            .map(|r| {
                let max = r.max.map(|m| format!(", max {m}")).unwrap_or_default();
                format!("{} ({}{max})", r.name, r.kind)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut out = format!(
            "{} — {}; roles: {}; default size {}×{}",
            self.ty,
            self.label,
            if roles.is_empty() { "none".into() } else { roles },
            self.size.width,
            self.size.height
        );
        if objects {
            for o in &self.objects {
                let props = o
                    .props
                    .iter()
                    .map(|p| match (&p.options, p.ty.as_str()) {
                        (Some(opts), _) => format!("{}: {}", p.name, opts.join("|")),
                        (None, "number") => format!(
                            "{}: number{}",
                            p.name,
                            match (p.min, p.max) {
                                (Some(a), Some(b)) => format!(" {a}–{b}"),
                                _ => String::new(),
                            }
                        ),
                        (None, "color") => format!("{}: #rrggbb", p.name),
                        (None, t) => format!("{}: {t}", p.name),
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("\n  format.{} {{ {props} }}", o.name));
            }
        }
        out
    }

    /// Checks formatting given as `{object: {prop: value}}` and returns it as stored.
    pub fn check_format(&self, format: &Value) -> Result<Map<String, Value>, String> {
        let Some(objs) = format.as_object() else {
            return Err("format is an object of objects: {\"title\": {\"text\": \"…\"}}".into());
        };
        let mut out = Map::new();
        for (name, props) in objs {
            let Some(def) = self.objects.iter().find(|o| &o.name == name) else {
                return Err(format!(
                    "{} has no format object \"{name}\"; it has {}.",
                    self.ty,
                    self.objects.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(", ")
                ));
            };
            let Some(props) = props.as_object() else {
                return Err(format!("format.{name} is an object of properties."));
            };
            let mut checked = Map::new();
            for (p, v) in props {
                let Some(pd) = def.props.iter().find(|d| &d.name == p) else {
                    return Err(format!(
                        "format.{name} has no \"{p}\"; it has {}.",
                        def.props.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ")
                    ));
                };
                let ok = match pd.ty.as_str() {
                    "bool" => v.is_boolean(),
                    "number" => v.as_f64().is_some_and(|n| {
                        pd.min.is_none_or(|m| n >= m) && pd.max.is_none_or(|m| n <= m)
                    }),
                    "select" => v
                        .as_str()
                        .is_some_and(|s| pd.options.as_ref().is_some_and(|o| o.iter().any(|x| x == s))),
                    "color" => v.as_str().is_some_and(is_color),
                    _ => v.is_string(),
                };
                if !ok {
                    return Err(format!("format.{name}.{p}: {v} isn't a valid {}.", pd.ty));
                }
                checked.insert(p.clone(), v.clone());
            }
            out.insert(name.clone(), Value::Object(checked));
        }
        Ok(out)
    }
}

pub fn is_color(s: &str) -> bool {
    s.len() >= 4
        && s.len() <= 9
        && s.starts_with('#')
        && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_loads_and_checks_formatting() {
        let line = visual("lineChart").unwrap();
        assert!(line.role("category").is_some() || line.role("axis").is_some());
        assert!(line.check_format(&serde_json::json!({"title": {"text": "Sales", "show": true}})).is_ok());
        assert!(line.check_format(&serde_json::json!({"title": {"colour": 1}})).is_err());
        assert!(line.check_format(&serde_json::json!({"nope": {}})).is_err());
        assert_eq!(catalog().themes.len(), 3);
    }
}
