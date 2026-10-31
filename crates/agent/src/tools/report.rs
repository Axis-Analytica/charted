//! Changing the report: pages, visuals (fields by name, placed automatically),
//! arrangements, filters, themes and bookmarks.

use charted_core::visualq;
use charted_model::{
    Bookmark, Change, FieldRef, FilterKind, Model, Page, PageSettings, Position, Projection,
    ReportFilter, ReportTheme, Visual, VisualState,
};
use serde_json::{Map, Value, json};

use super::{
    Cx, Done, Fail, Res, Step, ToolDef, action, fail, opt_bool, opt_str, schema, str_arg, str_list,
};
use crate::builders::new_id;
use crate::catalog::{self, VisualDef};
use crate::layout;
use crate::names::{self, format_projection};

const FIELDS_DOC: &str = "Fields by role, e.g. {\"category\": [\"'Date'[Month]\"], \"values\": [\"[Total Sales]\"]}. A field is 'Table'[Column], [Measure], or SUM/AVERAGE/MIN/MAX/COUNT/DISTINCTCOUNT('Table'[Column]); or {\"field\": …, \"as\": \"Display name\"}. See list_visual_types for each type's roles.";

pub fn defs() -> Vec<ToolDef> {
    let field_map = json!({ "type": "object", "description": FIELDS_DOC, "additionalProperties": { "type": "array" } });
    let position = json!({
        "description": "\"auto\" (the first free spot; default) or {x, y, width, height} in pixels (pages are usually 1280×720)",
        "anyOf": [{ "type": "string", "enum": ["auto"] }, { "type": "object", "properties": { "x": {"type": "number"}, "y": {"type": "number"}, "width": {"type": "number"}, "height": {"type": "number"} } }]
    });
    let format = json!({ "type": "object", "description": "Formatting by object, e.g. {\"title\": {\"text\": \"Sales by month\"}, \"dataLabels\": {\"show\": true}}; list_visual_types with a type lists them" });
    vec![
        ToolDef {
            name: "page",
            description: "Adds, renames or resizes, hides, moves, duplicates or removes a page. add returns the new page's id; the user is taken to pages you change.".into(),
            input_schema: schema(
                json!({
                    "action": { "type": "string", "enum": ["add", "update", "remove", "move", "duplicate"] },
                    "page": { "type": "string", "description": "The page (name or id); not for add" },
                    "name": { "type": "string", "description": "add/duplicate: its name; update: a new name" },
                    "width": { "type": "integer" }, "height": { "type": "integer" },
                    "hidden": { "type": "boolean" },
                    "index": { "type": "integer", "description": "add/move: its place among the pages (0 first)" }
                }),
                &["action"],
            ),
        },
        ToolDef {
            name: "add_visual",
            description: format!(
                "Adds a visual to a page with its fields, title and formatting. Returns its id. Check the numbers with query_visual. Roles by type: {}.",
                catalog::roles_summary()
            ),
            input_schema: schema(
                json!({ "page": { "type": "string" }, "type": { "type": "string", "description": "lineChart, columnChart, barChart, donutChart, card, table, matrix, slicer … (list_visual_types)" }, "fields": field_map, "title": { "type": "string" }, "position": position, "format": format }),
                &["page", "type"],
            ),
        },
        ToolDef {
            name: "update_visual",
            description: "Changes a visual (id or title): its type, fields (fields replaces the roles given; add_fields appends; remove_fields takes fields out), title, position, formatting (merged), sort or visibility.".into(),
            input_schema: schema(
                json!({
                    "visual": { "type": "string" }, "type": { "type": "string" },
                    "fields": field_map, "add_fields": field_map,
                    "remove_fields": { "type": "array", "items": { "type": "string" } },
                    "title": { "type": "string" }, "position": position, "format": format,
                    "sort": { "type": "object", "description": "{\"field\": a field on the visual, \"descending\": true}", "properties": { "field": {"type": "string"}, "descending": {"type": "boolean"} } },
                    "hidden": { "type": "boolean" },
                    "layer": { "type": "string", "enum": ["front", "back", "forward", "backward"], "description": "Moves the visual in the page's layer order (what lies on top where visuals overlap)" }
                }),
                &["visual"],
            ),
        },
        ToolDef {
            name: "format_visuals",
            description: "Formats many visuals at once, for report-wide standards (\"all titles in sentence case at 14 px\"): format is merged into each (objects a type doesn't have are skipped), title_case recases the titles shown. Which visuals: page (else every page), types, or visuals (ids or titles). One step for the user.".into(),
            input_schema: schema(
                json!({
                    "page": { "type": "string" },
                    "types": { "type": "array", "items": { "type": "string" } },
                    "visuals": { "type": "array", "items": { "type": "string" } },
                    "format": format,
                    "title_case": { "type": "string", "enum": ["sentence", "title", "upper", "lower"] }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "remove_visual",
            description: "Removes visuals (ids or titles).".into(),
            input_schema: schema(
                json!({ "visuals": { "type": "array", "items": { "type": "string" } } }),
                &["visuals"],
            ),
        },
        ToolDef {
            name: "arrange_page",
            description: "Lays out every visual on a page: slicers across the top, then cards, then charts two or three to a row, then tables. order lists visuals (ids or titles) to place first within their band.".into(),
            input_schema: schema(
                json!({ "page": { "type": "string" }, "order": { "type": "array", "items": { "type": "string" } } }),
                &["page"],
            ),
        },
        ToolDef {
            name: "set_filter",
            description: "Sets (or with filter null, removes) a filter on a field for the report, a page or a visual. filter is {\"kind\": \"basic\", \"values\": [\"Europe\"], \"exclude\": false} | {\"kind\": \"advanced\", \"conditions\": [{\"op\": \"greater\", \"value\": \"100\"}], \"or\": false} (ops: equals, notEquals, contains, notContains, startsWith, endsWith, less, lessOrEqual, greater, greaterOrEqual, between (value2), isBlank, isNotBlank) | {\"kind\": \"topN\", \"count\": 5, \"bottom\": false, \"by\": \"[Total Sales]\"} | {\"kind\": \"relativeDate\", \"period\": \"last|this|next\", \"count\": 3, \"unit\": \"days|weeks|months|years\"}.".into(),
            input_schema: schema(
                json!({
                    "scope": { "type": "string", "enum": ["report", "page", "visual"] },
                    "target": { "type": "string", "description": "The page or visual (name, title or id)" },
                    "field": { "type": "string" },
                    "filter": { "type": ["object", "null"] }
                }),
                &["scope", "field"],
            ),
        },
        ToolDef {
            name: "set_theme",
            description: "Sets the report's theme: a built-in (Classic, Daylight, Nightfall), \"desktop\" to follow the Omarchy theme, or custom colours; and/or its fonts (alone, fonts change only the current theme's fonts; list_visual_types lists them).".into(),
            input_schema: schema(
                json!({
                    "builtin": { "type": "string" },
                    "custom": { "type": "object", "properties": { "name": {"type": "string"}, "dataColors": { "type": "array", "items": {"type": "string"} }, "background": {"type": "string"}, "foreground": {"type": "string"}, "tableAccent": {"type": "string"} } },
                    "fonts": { "type": "object", "description": "Font names for the report's text", "properties": { "title": {"type": "string", "description": "visual titles"}, "label": {"type": "string", "description": "labels and other text"}, "callout": {"type": "string", "description": "card values"} } }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "bookmark",
            description: "Adds a bookmark of a page as it is now (its filters and visuals' state), updates one to the page's current state, renames or removes one.".into(),
            input_schema: schema(
                json!({ "action": { "type": "string", "enum": ["add", "update", "rename", "remove"] }, "name": { "type": "string" }, "page": { "type": "string" }, "new_name": { "type": "string" } }),
                &["action", "name"],
            ),
        },
    ]
}

pub fn call(cx: &Cx, name: &str, args: &Value) -> Option<Res<Done>> {
    Some(match name {
        "page" => page(cx, args),
        "add_visual" => add_visual(cx, args),
        "update_visual" => update_visual(cx, args),
        "remove_visual" => remove_visual(cx, args),
        "format_visuals" => format_visuals(cx, args),
        "arrange_page" => arrange_page(cx, args),
        "set_filter" => set_filter(cx, args),
        "set_theme" => set_theme(cx, args),
        "bookmark" => bookmark(cx, args),
        _ => return None,
    })
}

fn page(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["add", "update", "remove", "move", "duplicate"])?;
    if act == "remove" {
        let (id, name) = cx.with_model(|m, _| {
            let p = names::find_page(m, str_arg(args, "page")?)?;
            Ok((p.id.clone(), p.settings.name.clone()))
        })?;
        if !cx.turn.confirm(&format!("Delete the page \"{name}\"")) {
            return fail(format!(
                "the user didn't allow deleting the page \"{name}\"; leave it"
            ));
        }
        return cx.change(|m, _| {
            if m.report.pages.len() <= 1 {
                return fail("a report keeps at least one page");
            }
            Ok((
                vec![Change::RemovePage { page: id.clone() }],
                Done {
                    text: format!("Removed the page \"{name}\"."),
                    step: Some(Step::new("Removed page \"%1\"", &[&name])),
                },
            ))
        });
    }
    cx.change(|m, _| {
        let index = args
            .get("index")
            .and_then(Value::as_u64)
            .map(|i| i as usize);
        match act {
            "add" => {
                let name = unique_page_name(m, opt_str(args, "name").unwrap_or("Page"));
                let id = new_id();
                let mut settings = default_settings(&name);
                if let Some(w) = args.get("width").and_then(Value::as_u64) {
                    settings.width = w as u32;
                }
                if let Some(h) = args.get("height").and_then(Value::as_u64) {
                    settings.height = h as u32;
                }
                settings.hidden = opt_bool(args, "hidden").unwrap_or(false);
                Ok((
                    vec![Change::AddPage {
                        page: Page {
                            id: id.clone(),
                            settings,
                            visuals: vec![],
                        },
                        index: index.map(|i| i.min(m.report.pages.len())),
                    }],
                    Done {
                        text: format!("Added the page \"{name}\" (id {id})."),
                        step: Some(Step::new("Added page \"%1\"", &[&name]).page(&id)),
                    },
                ))
            }
            "duplicate" => {
                let p = names::find_page(m, str_arg(args, "page")?)?;
                let name = unique_page_name(
                    m,
                    opt_str(args, "name").unwrap_or(&format!("{} (copy)", p.settings.name)),
                );
                let id = new_id();
                let mut settings = p.settings.clone();
                settings.name = name.clone();
                let visuals = p
                    .visuals
                    .iter()
                    .map(|v| Visual {
                        id: new_id(),
                        ..v.clone()
                    })
                    .collect();
                let at = m
                    .report
                    .pages
                    .iter()
                    .position(|x| x.id == p.id)
                    .map(|i| i + 1);
                Ok((
                    vec![Change::AddPage {
                        page: Page {
                            id: id.clone(),
                            settings,
                            visuals,
                        },
                        index: index.or(at),
                    }],
                    Done {
                        text: format!(
                            "Duplicated \"{}\" as \"{name}\" (id {id}).",
                            p.settings.name
                        ),
                        step: Some(Step::new("Added page \"%1\"", &[&name]).page(&id)),
                    },
                ))
            }
            "move" => {
                let p = names::find_page(m, str_arg(args, "page")?)?;
                let Some(i) = index else {
                    return fail("move needs index");
                };
                Ok((
                    vec![Change::MovePage {
                        page: p.id.clone(),
                        index: i.min(m.report.pages.len() - 1),
                    }],
                    Done {
                        text: format!("Moved \"{}\" to place {i}.", p.settings.name),
                        step: Some(Step::new("Moved page \"%1\"", &[&p.settings.name])),
                    },
                ))
            }
            _ => {
                let p = names::find_page(m, str_arg(args, "page")?)?;
                let mut s = p.settings.clone();
                if let Some(n) = opt_str(args, "name") {
                    if !n.eq_ignore_ascii_case(&p.settings.name)
                        && m.report
                            .pages
                            .iter()
                            .any(|x| x.settings.name.eq_ignore_ascii_case(n))
                    {
                        return fail(format!("there's a page \"{n}\" already"));
                    }
                    s.name = n.trim().into();
                }
                if let Some(w) = args.get("width").and_then(Value::as_u64) {
                    s.width = w as u32;
                }
                if let Some(h) = args.get("height").and_then(Value::as_u64) {
                    s.height = h as u32;
                }
                if let Some(h) = opt_bool(args, "hidden") {
                    s.hidden = h;
                }
                let name = s.name.clone();
                Ok((
                    vec![Change::SetPage {
                        page: p.id.clone(),
                        settings: s,
                    }],
                    Done {
                        text: format!("Updated the page \"{name}\"."),
                        step: Some(Step::new("Changed page \"%1\"", &[&name]).page(&p.id)),
                    },
                ))
            }
        }
    })
}

fn default_settings(name: &str) -> PageSettings {
    serde_json::from_value(json!({ "name": name })).expect("page settings have defaults")
}

fn unique_page_name(m: &Model, want: &str) -> String {
    let want = want.trim();
    let taken = |n: &str| {
        m.report
            .pages
            .iter()
            .any(|p| p.settings.name.eq_ignore_ascii_case(n))
    };
    if !taken(want) {
        return want.into();
    }
    (2..)
        .map(|i| format!("{want} {i}"))
        .find(|n| !taken(n))
        .expect("a free name")
}

/// One field given for a role, resolved, with the role's summarising rules (as the UI's
/// `projectionFor`).
fn projection_for(m: &Model, def: &VisualDef, role: &str, given: &Value) -> Res<Projection> {
    let (src, display) = match given {
        Value::String(s) => (s.as_str(), None),
        Value::Object(o) => (
            o.get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| Fail("a field object needs \"field\"".into()))?,
            o.get("as").and_then(Value::as_str).map(str::to_string),
        ),
        _ => return fail("a field is text ('Table'[Column]) or {\"field\", \"as\"}"),
    };
    let Some(r) = def.role(role) else {
        return fail(format!(
            "{} has no role \"{role}\"; its roles are {}",
            def.ty,
            def.roles
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    };
    let mut p = names::field(m, src)?;
    p.display_name = display;
    let col = match &p.field {
        FieldRef::Column { table, column } => m.table(table).and_then(|t| t.column(column)),
        _ => None,
    };
    let field_param = is_parameter_field(m, &p.field);
    match r.kind.as_str() {
        "grouping" => {
            if !matches!(p.field, FieldRef::Column { .. }) || p.aggregation.is_some() {
                return fail(format!(
                    "{}'s {} role groups by columns; {src} is a value (use a value role)",
                    def.ty, r.name
                ));
            }
        }
        "value" => {
            if let (Some(c), None, false) = (col, p.aggregation, field_param) {
                p.aggregation = Some(default_aggregation(c));
            }
        }
        _ => {
            if let (Some(c), None, false) = (col, p.aggregation, field_param)
                && c.data_type.is_numeric()
                && c.summarize_by != charted_model::Summarize::None
                && !c.is_key
                && c.sort_by.is_none()
            {
                p.aggregation = Some(default_aggregation(c));
            }
        }
    }
    Ok(p)
}

/// A field parameter's label or field column: it stands for the fields picked in it.
fn is_parameter_field(m: &Model, f: &FieldRef) -> bool {
    let FieldRef::Column { table, column } = f else {
        return false;
    };
    let Some(t) = m.table(table) else {
        return false;
    };
    matches!(
        t.column(column).map(|c| &c.kind),
        Some(charted_model::ColumnKind::Data { source_column }) if source_column == "Value1" || source_column == "Value2"
    ) && charted_core::fieldparams::items(m, t).is_some()
}

fn default_aggregation(c: &charted_model::Column) -> charted_model::Aggregation {
    use charted_model::{Aggregation as A, Summarize as S};
    match c.summarize_by {
        S::Sum => A::Sum,
        S::Average => A::Average,
        S::Min => A::Min,
        S::Max => A::Max,
        S::Count => A::Count,
        S::DistinctCount => A::DistinctCount,
        S::None | S::Default => {
            if c.data_type.is_numeric() {
                A::Sum
            } else {
                A::Count
            }
        }
    }
}

/// Puts `fields` (`{role: [field]}`) in `v`'s roles: replacing each role given, or adding.
fn set_fields(
    m: &Model,
    def: &VisualDef,
    v: &mut Visual,
    fields: &Value,
    add: bool,
) -> Res<Vec<String>> {
    let Some(map) = fields.as_object() else {
        return fail(format!("fields is an object of roles. {FIELDS_DOC}"));
    };
    let mut shown = Vec::new();
    for (role, list) in map {
        let list: Vec<Value> = match list {
            Value::Array(a) => a.clone(),
            other => vec![other.clone()],
        };
        let r = def
            .role(role)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| role.clone());
        let mut ps = Vec::new();
        for f in &list {
            ps.push(projection_for(m, def, role, f)?);
        }
        let slot = v.roles.entry(r.clone()).or_default();
        if add {
            slot.extend(ps);
        } else {
            *slot = ps;
        }
        if let Some(max) = def.role(&r).and_then(|d| d.max)
            && slot.len() > max
        {
            return fail(format!(
                "{}'s {r} role takes at most {max} field(s)",
                def.ty
            ));
        }
        shown.push(format!(
            "{r}: {}",
            slot.iter()
                .map(|p| format_projection(m, p))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    v.roles.retain(|_, ps| !ps.is_empty());
    Ok(shown)
}

/// A slicer names itself in its header (as in Power BI), so its title goes there and the
/// container title stays off: both on would show the name twice.
fn set_title(v: &mut Visual, title: &str) {
    let slicer = v.visual_type == "slicer";
    if slicer {
        if let Some(t) = v.objects.get_mut("title").and_then(Value::as_object_mut) {
            t.insert("show".into(), Value::Bool(false));
        }
    }
    let o = v
        .objects
        .entry(if slicer { "header" } else { "title" })
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(o) = o.as_object_mut() {
        o.insert("text".into(), Value::String(title.into()));
        o.insert("show".into(), Value::Bool(true));
    }
}

/// The name a visual shows above itself, if it shows one: a slicer's header, otherwise
/// the title.
fn shown_title(m: &Model, v: &Visual) -> Option<String> {
    let get = |o: &str, p: &str| v.objects.get(o).and_then(|t| t.get(p));
    if v.visual_type == "slicer" {
        let title_on = get("title", "show").and_then(Value::as_bool) == Some(true);
        if get("header", "show").and_then(Value::as_bool) == Some(false) && !title_on {
            return None;
        }
        return Some(match get("header", "text").and_then(Value::as_str) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => visualq::visual_title(m, v),
        });
    }
    (get("title", "show").and_then(Value::as_bool) != Some(false)).then(|| visualq::visual_title(m, v))
}

fn merge_format(v: &mut Visual, def: &VisualDef, format: &Value) -> Res<()> {
    for (obj, props) in def.check_format(format)? {
        let slot = v
            .objects
            .entry(obj)
            .or_insert_with(|| Value::Object(Map::new()));
        if let (Some(s), Value::Object(p)) = (slot.as_object_mut(), props) {
            s.extend(p);
        }
    }
    Ok(())
}

fn explicit_position(v: &Value) -> Option<(f64, f64, f64, f64)> {
    let o = v.as_object()?;
    let n = |k: &str| o.get(k).and_then(Value::as_f64);
    Some((n("x")?, n("y")?, n("width")?, n("height")?))
}

fn add_visual(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let p = names::find_page(m, str_arg(args, "page")?)?;
        let ty = str_arg(args, "type")?;
        let Some(def) = catalog::visual(ty) else {
            return fail(format!("there's no visual type {ty}; types: {}", catalog::types()));
        };
        let mut v: Visual = serde_json::from_value(json!({
            "id": new_id(), "visualType": def.ty,
            "position": { "x": 0, "y": 0, "width": def.size.width, "height": def.size.height, "z": 0 }
        }))?;
        let shown = match args.get("fields") {
            Some(f) if !f.is_null() => set_fields(m, def, &mut v, f, false)?,
            _ => vec![],
        };
        if let Some(t) = opt_str(args, "title") {
            set_title(&mut v, t);
        }
        if let Some(f) = args.get("format").filter(|f| !f.is_null()) {
            merge_format(&mut v, def, f)?;
        }
        let existing: Vec<Position> = p.visuals.iter().map(|v| v.position).collect();
        let z = existing.iter().map(|q| q.z).max().unwrap_or(0) + 1;
        let mut changes = vec![];
        if let Some((x, y, w, h)) = args.get("position").and_then(explicit_position) {
            v.position = Position { x, y, width: w, height: h, z };
        } else {
            let (at, need) = layout::place(p.settings.width as f64, p.settings.height as f64, &existing, def.size.width, def.size.height);
            v.position = at;
            if need > p.settings.height as f64 {
                let mut s = p.settings.clone();
                s.height = need.ceil() as u32;
                changes.push(Change::SetPage { page: p.id.clone(), settings: s });
            }
        }
        let title = visualq::visual_title(m, &v);
        let id = v.id.clone();
        let pos = v.position;
        changes.insert(0, Change::AddVisual { page: p.id.clone(), visual: v, index: None });
        Ok((
            changes,
            Done {
                text: format!(
                    "Added {} \"{title}\" (id {id}) on \"{}\" at x{} y{} w{} h{}.",
                    def.ty, p.settings.name, pos.x, pos.y, pos.width, pos.height
                ),
                step: Some(
                    Step::new("Added %1 \"%2\"", &[&def.label.to_lowercase(), &title])
                        .page(&p.id)
                        .visuals(vec![id])
                        .detail(shown.join("\n")),
                ),
            },
        ))
    })
}

fn update_visual(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let (p, old) = names::find_visual(m, None, str_arg(args, "visual")?)?;
        let mut v = old.clone();
        if let Some(t) = opt_str(args, "type") {
            let Some(def) = catalog::visual(t) else {
                return fail(format!(
                    "there's no visual type {t}; types: {}",
                    catalog::types()
                ));
            };
            v.visual_type = def.ty.clone();
        }
        let def = catalog::visual(&v.visual_type).ok_or_else(|| {
            Fail(format!(
                "{} visuals can't be changed by the agent",
                v.visual_type
            ))
        })?;
        let mut shown = Vec::new();
        if let Some(f) = args.get("fields").filter(|f| !f.is_null()) {
            shown.extend(set_fields(m, def, &mut v, f, false)?);
        }
        if let Some(f) = args.get("add_fields").filter(|f| !f.is_null()) {
            shown.extend(set_fields(m, def, &mut v, f, true)?);
        }
        for f in str_list(args, "remove_fields") {
            let want = names::field(m, &f)?;
            let before: usize = v.roles.values().map(Vec::len).sum();
            for ps in v.roles.values_mut() {
                ps.retain(|x| x.field != want.field);
            }
            if v.roles.values().map(Vec::len).sum::<usize>() == before {
                return fail(format!("{f} isn't on the visual"));
            }
            v.roles.retain(|_, ps| !ps.is_empty());
        }
        // Roles the new type doesn't have go.
        v.roles.retain(|r, _| def.role(r).is_some());
        if let Some(t) = opt_str(args, "title") {
            set_title(&mut v, t);
        }
        if let Some(f) = args.get("format").filter(|f| !f.is_null()) {
            merge_format(&mut v, def, f)?;
        }
        if let Some((x, y, w, h)) = args.get("position").and_then(explicit_position) {
            v.position = Position {
                x,
                y,
                width: w,
                height: h,
                z: v.position.z,
            };
        }
        if let Some(h) = opt_bool(args, "hidden") {
            v.hidden = h;
        }
        let mut relayered = vec![];
        if let Some(how) = opt_str(args, "layer") {
            relayered = layer(p, &v.id, how)?;
            if let Some((_, z)) = relayered.iter().find(|(id, _)| *id == v.id) {
                v.position.z = *z;
            }
        }
        if let Some(s) = args.get("sort").and_then(Value::as_object) {
            let f = s
                .get("field")
                .and_then(Value::as_str)
                .ok_or_else(|| Fail("sort needs field".into()))?;
            let want = names::field(m, f)?;
            let at = v
                .roles
                .iter()
                .find_map(|(r, ps)| {
                    ps.iter()
                        .position(|x| x.field == want.field)
                        .map(|i| (r.clone(), i))
                })
                .ok_or_else(|| {
                    Fail(format!(
                        "{f} isn't on the visual; sort by one of its fields"
                    ))
                })?;
            v.sort = Some(charted_model::VisualSort {
                role: at.0,
                index: at.1,
                descending: s.get("descending").and_then(Value::as_bool).unwrap_or(true),
            });
        }
        if v.sort
            .as_ref()
            .is_some_and(|s| v.roles.get(&s.role).is_none_or(|ps| s.index >= ps.len()))
        {
            v.sort = None;
        }
        let title = visualq::visual_title(m, &v);
        let id = v.id.clone();
        let mut changes = vec![Change::UpdateVisual {
            page: p.id.clone(),
            visual: v,
        }];
        for o in &p.visuals {
            match relayered.iter().find(|(id, _)| *id == o.id) {
                Some((_, z)) if o.id != id && *z != o.position.z => {
                    let mut o = o.clone();
                    o.position.z = *z;
                    changes.push(Change::UpdateVisual { page: p.id.clone(), visual: o });
                }
                _ => {}
            }
        }
        Ok((
            changes,
            Done {
                text: format!("Updated {} \"{title}\" (id {id}).", def.ty),
                step: Some(
                    Step::new("Changed %1 \"%2\"", &[&def.label.to_lowercase(), &title])
                        .page(&p.id)
                        .visuals(vec![id])
                        .detail(shown.join("\n")),
                ),
            },
        ))
    })
}

/// The page's layer order after moving one visual, as new z values (1 at the back),
/// like the Selection pane.
fn layer(p: &Page, id: &str, how: &str) -> Res<Vec<(String, i32)>> {
    let mut order: Vec<&Visual> = p.visuals.iter().collect();
    order.sort_by_key(|v| v.position.z);
    let at = order.iter().position(|v| v.id == id).ok_or_else(|| Fail("no such visual".into()))?;
    let v = order.remove(at);
    let to = match how {
        "front" => order.len(),
        "back" => 0,
        "forward" => (at + 1).min(order.len()),
        "backward" => at.saturating_sub(1),
        _ => return fail(format!("layer is front, back, forward or backward, not {how}")),
    };
    order.insert(to, v);
    Ok(order.iter().enumerate().map(|(i, v)| (v.id.clone(), i as i32 + 1)).collect())
}

/// Recases a title: sentence case keeps words that are already capitalised after the first
/// (names, acronyms like YoY) as they are.
fn recase(title: &str, how: &str) -> String {
    match how {
        "upper" => title.to_uppercase(),
        "lower" => title.to_lowercase(),
        "title" => title
            .split(' ')
            .map(|w| {
                let mut c = w.chars();
                c.next()
                    .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => {
            let mut out = String::new();
            for (i, w) in title.split(' ').enumerate() {
                if i > 0 {
                    out.push(' ');
                }
                let mut c = w.chars();
                match c.next() {
                    Some(f) if i == 0 => out.extend(f.to_uppercase().chain(c)),
                    // "Sales By Month" → "Sales by month"; "YoY" and "EMEA" stay.
                    Some(f) if f.is_uppercase() && c.clone().all(|x| !x.is_uppercase()) => {
                        out.extend(f.to_lowercase().chain(c))
                    }
                    _ => out.push_str(w),
                }
            }
            out
        }
    }
}

fn format_visuals(cx: &Cx, args: &Value) -> Res<Done> {
    let format = args.get("format").filter(|f| !f.is_null());
    let case = opt_str(args, "title_case");
    if format.is_none() && case.is_none() {
        return fail("give format, title_case or both");
    }
    let types = str_list(args, "types");
    let keys = str_list(args, "visuals");
    cx.change(|m, _| {
        let page = opt_str(args, "page")
            .map(|k| names::find_page(m, k))
            .transpose()?;
        let wanted: Vec<String> = keys
            .iter()
            .map(|k| names::find_visual(m, None, k).map(|(_, v)| v.id.clone()))
            .collect::<Result<_, _>>()?;
        let mut changes = vec![];
        let mut ids = vec![];
        let mut skipped = std::collections::BTreeSet::new();
        for p in &m.report.pages {
            if page.is_some_and(|x| x.id != p.id) {
                continue;
            }
            for v in &p.visuals {
                if (!wanted.is_empty() && !wanted.contains(&v.id))
                    || (!types.is_empty() && !types.contains(&v.visual_type))
                {
                    continue;
                }
                let Some(def) = catalog::visual(&v.visual_type) else {
                    continue;
                };
                let mut next = v.clone();
                if let Some(f) = format.and_then(Value::as_object) {
                    // Only the objects this type has.
                    let mine: Map<String, Value> = f
                        .iter()
                        .filter(|(k, _)| def.objects.iter().any(|o| &o.name == *k))
                        .map(|(k, x)| (k.clone(), x.clone()))
                        .collect();
                    for k in f.keys().filter(|k| !mine.contains_key(*k)) {
                        skipped.insert(format!("{} has no {k}", v.visual_type));
                    }
                    merge_format(&mut next, def, &Value::Object(mine))?;
                }
                let title = shown_title(m, &next)
                    .filter(|_| def.objects.iter().any(|o| o.name == "title"));
                if let (Some(how), Some(title)) = (case, title) {
                    let cased = recase(&title, how);
                    if cased != title {
                        set_title(&mut next, &cased);
                    }
                }
                if next != *v {
                    ids.push(next.id.clone());
                    changes.push(Change::UpdateVisual {
                        page: p.id.clone(),
                        visual: next,
                    });
                }
            }
        }
        if changes.is_empty() {
            return fail("no visual changed: none matched, or they already look like that");
        }
        let n = changes.len();
        let mut text = format!("Formatted {n} visuals.");
        if !skipped.is_empty() {
            text.push_str(&format!(
                " Skipped: {}.",
                skipped.into_iter().collect::<Vec<_>>().join("; ")
            ));
        }
        Ok((
            changes,
            Done {
                text,
                step: Some(
                    Step::new("Formatted %1 visuals", &[&n.to_string()])
                        .visuals(ids)
                        .detail(
                            serde_json::to_string_pretty(
                                &json!({ "format": format, "title_case": case }),
                            )
                            .unwrap_or_default(),
                        ),
                ),
            },
        ))
    })
}

fn remove_visual(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let keys = str_list(args, "visuals");
        if keys.is_empty() {
            return fail("visuals lists the visuals to remove");
        }
        let mut changes = vec![];
        let mut titles = vec![];
        let mut page = None;
        for k in &keys {
            let (p, v) = names::find_visual(m, None, k)?;
            changes.push(Change::RemoveVisual {
                page: p.id.clone(),
                visual: v.id.clone(),
            });
            titles.push(visualq::visual_title(m, v));
            page = Some(p.id.clone());
        }
        let list = titles
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let mut step = Step::new("Removed %1", &[&list]);
        step.page = page;
        Ok((
            changes,
            Done {
                text: format!("Removed {list}."),
                step: Some(step),
            },
        ))
    })
}

fn arrange_page(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let p = names::find_page(m, str_arg(args, "page")?)?;
        let mut order: Vec<&Visual> = Vec::new();
        for k in str_list(args, "order") {
            let (_, v) = names::find_visual(m, Some(p), &k)?;
            order.push(v);
        }
        for v in &p.visuals {
            if !order.iter().any(|o| o.id == v.id) && !v.hidden {
                order.push(v);
            }
        }
        let items: Vec<(String, &str)> = order
            .iter()
            .map(|v| (v.id.clone(), v.visual_type.as_str()))
            .collect();
        let (placed, need) =
            layout::arrange(p.settings.width as f64, p.settings.height as f64, &items);
        let mut changes: Vec<Change> = placed
            .into_iter()
            .filter_map(|(id, pos)| {
                let v = p.visual(&id)?;
                Some(Change::UpdateVisual {
                    page: p.id.clone(),
                    visual: Visual {
                        position: pos,
                        ..v.clone()
                    },
                })
            })
            .collect();
        if need > p.settings.height as f64 {
            let mut s = p.settings.clone();
            s.height = need.ceil() as u32;
            changes.push(Change::SetPage {
                page: p.id.clone(),
                settings: s,
            });
        }
        let ids = p.visuals.iter().map(|v| v.id.clone()).collect();
        Ok((
            changes,
            Done {
                text: format!(
                    "Arranged {} visuals on \"{}\".",
                    items.len(),
                    p.settings.name
                ),
                step: Some(
                    Step::new("Arranged page \"%1\"", &[&p.settings.name])
                        .page(&p.id)
                        .visuals(ids),
                ),
            },
        ))
    })
}

/// A filter as the agent writes it, with `by` (top N) as a field name.
fn filter_kind(m: &Model, f: &Value) -> Res<FilterKind> {
    let mut f = f.clone();
    if let Some(by) = f.get("by").and_then(Value::as_str) {
        let p = names::field(m, by)?;
        f["by"] = serde_json::to_value(p)?;
    }
    serde_json::from_value(f).map_err(|e| Fail(format!("filter: {e}")))
}

fn upsert(list: &mut Vec<ReportFilter>, field: Projection, filter: Option<FilterKind>) {
    list.retain(|x| x.field.field != field.field);
    if let Some(filter) = filter {
        list.push(ReportFilter {
            id: new_id(),
            field,
            filter,
        });
    }
}

fn set_filter(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let mut field = names::field(m, str_arg(args, "field")?)?;
        if matches!(field.field, FieldRef::Measure { .. }) && args["scope"] != "visual" {
            return fail("measures filter visuals only (scope visual); for pages and the report filter a column");
        }
        if field.aggregation.is_some() && args["scope"] != "visual" {
            field.aggregation = None;
        }
        let filter = match args.get("filter") {
            None | Some(Value::Null) => None,
            Some(f) => Some(filter_kind(m, f)?),
        };
        let shown = format_projection(m, &field);
        let what = match &filter {
            Some(f) => format!("{shown} {}", serde_json::to_string(f)?),
            None => format!("{shown} (removed)"),
        };
        let (changes, step) = match str_arg(args, "scope")? {
            "report" => {
                let mut filters = m.report.filters.clone();
                upsert(&mut filters, field, filter);
                (vec![Change::SetReportFilters { filters }], Step::new("Filtered the report: %1", &[&what]))
            }
            "page" => {
                let p = names::find_page(m, str_arg(args, "target")?)?;
                let mut s = p.settings.clone();
                upsert(&mut s.filters, field, filter);
                (
                    vec![Change::SetPage { page: p.id.clone(), settings: s }],
                    Step::new("Filtered page \"%1\": %2", &[&p.settings.name, &what]).page(&p.id),
                )
            }
            "visual" => {
                let (p, v) = names::find_visual(m, None, str_arg(args, "target")?)?;
                let mut v = v.clone();
                upsert(&mut v.filters, field, filter);
                let title = visualq::visual_title(m, &v);
                let id = v.id.clone();
                (
                    vec![Change::UpdateVisual { page: p.id.clone(), visual: v }],
                    Step::new("Filtered \"%1\": %2", &[&title, &what]).page(&p.id).visuals(vec![id]),
                )
            }
            s => return fail(format!("scope {s} isn't report, page or visual")),
        };
        Ok((changes, Done { text: format!("Set the filter: {what}."), step: Some(step) }))
    })
}

fn set_theme(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let fonts = args.get("fonts").and_then(Value::as_object).filter(|f| !f.is_empty());
        let (theme, name) = if let Some(b) = opt_str(args, "builtin") {
            if b.eq_ignore_ascii_case("desktop") || b.eq_ignore_ascii_case("omarchy") {
                (None, "the desktop theme".to_string())
            } else {
                let Some(json) = catalog::catalog()
                    .themes
                    .iter()
                    .find(|t| t.get("name").and_then(Value::as_str).is_some_and(|n| n.eq_ignore_ascii_case(b)))
                else {
                    return fail(format!("there's no built-in theme {b}; there are Classic, Daylight and Nightfall (or \"desktop\")"));
                };
                let name = json["name"].as_str().unwrap_or(b).to_string();
                (Some(ReportTheme { name: name.clone(), json: json.clone() }), name)
            }
        } else if let Some(c) = args.get("custom").and_then(Value::as_object) {
            for k in ["background", "foreground", "tableAccent"] {
                if let Some(v) = c.get(k)
                    && !v.as_str().is_some_and(catalog::is_color)
                {
                    return fail(format!("{k} is a colour like #1a2b3c"));
                }
            }
            if let Some(cs) = c.get("dataColors")
                && !cs.as_array().is_some_and(|a| !a.is_empty() && a.iter().all(|v| v.as_str().is_some_and(catalog::is_color)))
            {
                return fail("dataColors lists colours like #1a2b3c");
            }
            let name = c.get("name").and_then(Value::as_str).unwrap_or("Custom").to_string();
            let mut json = Value::Object(c.clone());
            json["name"] = Value::String(name.clone());
            (Some(ReportTheme { name: name.clone(), json }), name)
        } else if fonts.is_some() {
            let current = m.report.theme.clone();
            let name = current.as_ref().map_or("the desktop theme".to_string(), |t| t.name.clone());
            (current, name)
        } else {
            return fail("give builtin, custom or fonts");
        };
        let Some(fonts) = fonts else {
            return Ok((
                vec![Change::SetTheme { theme }],
                Done { text: format!("The report now uses {name}."), step: Some(Step::new("Set the theme to %1", &[&name])) },
            ));
        };
        // Fonts go in the theme's text classes; following the desktop, they start a
        // theme of their own that keeps the desktop's colours.
        let mut theme = theme.unwrap_or_else(|| ReportTheme { name: "Desktop fonts".into(), json: json!({ "name": "Desktop fonts", "followDesktop": true }) });
        let mut set = vec![];
        for (class, font) in fonts {
            let Some(font) = font.as_str().filter(|f| !f.trim().is_empty()) else { continue };
            if !["title", "label", "callout"].contains(&class.as_str()) {
                return fail(format!("fonts are title, label or callout, not {class}"));
            }
            theme.json["textClasses"][class.as_str()]["fontFace"] = json!(font);
            set.push(format!("{class} {font}"));
        }
        let list = set.join(", ");
        Ok((
            vec![Change::SetTheme { theme: Some(theme) }],
            Done { text: format!("The report's fonts: {list}."), step: Some(Step::new("Set the fonts: %1", &[&list])) },
        ))
    })
}

fn snapshot(
    m: &Model,
    p: &Page,
) -> (
    Vec<ReportFilter>,
    std::collections::BTreeMap<String, VisualState>,
) {
    let visuals = p
        .visuals
        .iter()
        .map(|v| {
            (
                v.id.clone(),
                VisualState {
                    filters: v.filters.clone(),
                    selection: v.selection.clone(),
                    sort: v.sort.clone(),
                    drill: v.drill.clone(),
                },
            )
        })
        .collect();
    let _ = m;
    (p.settings.filters.clone(), visuals)
}

fn bookmark(cx: &Cx, args: &Value) -> Res<Done> {
    cx.change(|m, _| {
        let act = action(args, &["add", "update", "rename", "remove"])?;
        let name = str_arg(args, "name")?.trim().to_string();
        let mut list = m.report.bookmarks.clone();
        let at = list.iter().position(|b| b.name.eq_ignore_ascii_case(&name));
        let text = match (act, at) {
            ("add", Some(_)) => {
                return fail(format!("there's a bookmark \"{name}\" already; use update"));
            }
            ("add", None) => {
                let p = names::find_page(m, str_arg(args, "page")?)?;
                let (page_filters, visuals) = snapshot(m, p);
                list.push(Bookmark {
                    id: new_id(),
                    name: name.clone(),
                    page: p.id.clone(),
                    report_filters: m.report.filters.clone(),
                    page_filters,
                    visuals,
                });
                "Added bookmark \"%1\""
            }
            (_, None) => return fail(format!("there's no bookmark \"{name}\"")),
            ("update", Some(i)) => {
                let p = match opt_str(args, "page") {
                    Some(k) => names::find_page(m, k)?,
                    None => names::find_page(m, &list[i].page)?,
                };
                let (page_filters, visuals) = snapshot(m, p);
                list[i] = Bookmark {
                    page: p.id.clone(),
                    report_filters: m.report.filters.clone(),
                    page_filters,
                    visuals,
                    ..list[i].clone()
                };
                "Updated bookmark \"%1\""
            }
            ("rename", Some(i)) => {
                list[i].name = str_arg(args, "new_name")?.trim().into();
                "Renamed bookmark \"%1\""
            }
            (_, Some(i)) => {
                list.remove(i);
                "Removed bookmark \"%1\""
            }
        };
        let step = Step::new(text, &[&name]);
        Ok((
            vec![Change::SetBookmarks { bookmarks: list }],
            Done {
                text: format!("{}.", step.text),
                step: Some(step),
            },
        ))
    })
}
