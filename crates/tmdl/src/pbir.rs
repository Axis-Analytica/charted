//! The report as Power BI's enhanced report format (PBIR): a `definition/` folder with
//! the report, its pages and one file per visual, written to Microsoft's published
//! schemas (vendored in `schema/pbir`, checked in tests). Visuals carry their fields by
//! role, position, sort, title and basic filters, so Power BI shows the same report.
//!
//! What PBIR can't say the way charted does (advanced and Top N filters, slicer
//! choices, drill state, bookmarks' states, charted's formatting) travels in
//! annotations, so importing charted's own export brings the report back exactly.
//! Reports from Power BI are read from PBIR or from the older `report.json`; visual
//! types charted doesn't have are listed in the import report.

use charted_model::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::read::Skipped;

pub const REPORT_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/report/2.1.0/schema.json";
pub const PAGE_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/page/2.0.0/schema.json";
pub const PAGES_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/pagesMetadata/1.0.0/schema.json";
pub const VISUAL_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/visualContainer/2.1.0/schema.json";
pub const VERSION_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/versionMetadata/1.0.0/schema.json";
pub const BOOKMARK_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/bookmark/1.4.0/schema.json";
pub const BOOKMARKS_SCHEMA: &str = "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/bookmarksMetadata/1.0.0/schema.json";

/// (charted role, Power BI role) pairs.
type Roles = &'static [(&'static str, &'static str)];

/// charted visual types, their Power BI names and roles.
const TYPES: &[(&str, &str, Roles)] = &[
    ("card", "card", &[("values", "Values")]),
    ("columnChart", "clusteredColumnChart", CARTESIAN),
    ("barChart", "clusteredBarChart", CARTESIAN),
    ("stackedColumnChart", "columnChart", CARTESIAN),
    ("stackedBarChart", "barChart", CARTESIAN),
    ("lineChart", "lineChart", CARTESIAN),
    (
        "pieChart",
        "pieChart",
        &[("legend", "Category"), ("values", "Y")],
    ),
    (
        "donutChart",
        "donutChart",
        &[("legend", "Category"), ("values", "Y")],
    ),
    (
        "scatterChart",
        "scatterChart",
        &[
            ("details", "Category"),
            ("legend", "Series"),
            ("x", "X"),
            ("y", "Y"),
            ("size", "Size"),
        ],
    ),
    ("table", "tableEx", &[("columns", "Values")]),
    (
        "matrix",
        "pivotTable",
        &[
            ("rows", "Rows"),
            ("columns", "Columns"),
            ("values", "Values"),
        ],
    ),
    ("slicer", "slicer", &[("field", "Values")]),
    ("textbox", "textbox", &[]),
    ("image", "image", &[]),
    ("shape", "shape", &[]),
    (
        "hundredPercentStackedColumnChart",
        "hundredPercentStackedColumnChart",
        CARTESIAN,
    ),
    (
        "hundredPercentStackedBarChart",
        "hundredPercentStackedBarChart",
        CARTESIAN,
    ),
    (
        "lineClusteredColumnComboChart",
        "lineClusteredColumnComboChart",
        COMBO,
    ),
    (
        "lineStackedColumnComboChart",
        "lineStackedColumnComboChart",
        COMBO,
    ),
    (
        "waterfallChart",
        "waterfallChart",
        &[
            ("category", "Category"),
            ("breakdown", "Breakdown"),
            ("values", "Y"),
        ],
    ),
    (
        "funnel",
        "funnel",
        &[("category", "Category"), ("values", "Y")],
    ),
    (
        "treemap",
        "treemap",
        &[
            ("group", "Group"),
            ("details", "Details"),
            ("values", "Values"),
        ],
    ),
    (
        "gauge",
        "gauge",
        &[
            ("values", "Y"),
            ("minValue", "MinValue"),
            ("maxValue", "MaxValue"),
            ("targetValue", "TargetValue"),
        ],
    ),
    (
        "kpi",
        "kpi",
        &[
            ("indicator", "Indicator"),
            ("trend", "TrendLine"),
            ("goal", "Goal"),
        ],
    ),
    ("multiRowCard", "multiRowCard", &[("values", "Values")]),
];

/// The roles of 100% stacked charts (and, with `Rows`, small multiples).
const CARTESIAN: Roles = &[
    ("category", "Category"),
    ("series", "Series"),
    ("values", "Y"),
    ("smallMultiples", "Rows"),
];

/// Combo charts: column values (`Y`) and line values (`Y2`).
const COMBO: Roles = &[
    ("category", "Category"),
    ("series", "Series"),
    ("values", "Y"),
    ("lineValues", "Y2"),
];

/// Older or alternative Power BI names that map to the same charted visual.
const ALIASES: &[(&str, &str)] = &[
    ("cardVisual", "multiRowCard"),
    ("table", "table"),
    ("matrix", "matrix"),
    ("areaChart", "lineChart"),
    ("stackedAreaChart", "lineChart"),
    ("basicShape", "shape"),
    ("advancedSlicerVisual", "slicer"),
    ("listSlicer", "slicer"),
];

fn table_name(model: &Model, id: &str) -> String {
    model.table(id).map(|t| t.name.clone()).unwrap_or_default()
}

/// A field as a Power BI query expression, with its query reference.
fn field_expr(model: &Model, p: &Projection) -> Option<(Value, String, String)> {
    let source = |t: &str| json!({ "SourceRef": { "Entity": table_name(model, t) } });
    match &p.field {
        FieldRef::Column { table, column } => {
            let c = model.table(table)?.column(column)?;
            let col = json!({ "Column": { "Expression": source(table), "Property": c.name } });
            let tn = table_name(model, table);
            match p.aggregation {
                None => Some((col, format!("{tn}.{}", c.name), c.name.clone())),
                Some(a) => {
                    let (f, label) = match a {
                        Aggregation::Sum => (0, "Sum"),
                        Aggregation::Average => (1, "Avg"),
                        Aggregation::DistinctCount => (2, "Count"),
                        Aggregation::Min => (3, "Min"),
                        Aggregation::Max => (4, "Max"),
                        Aggregation::Count => (5, "CountNonNull"),
                    };
                    Some((
                        json!({ "Aggregation": { "Expression": col, "Function": f } }),
                        format!("{label}({tn}.{})", c.name),
                        format!(
                            "{} of {}",
                            label
                                .replace("CountNonNull", "Count")
                                .replace("Avg", "Average"),
                            c.name
                        ),
                    ))
                }
            }
        }
        FieldRef::Measure { table, measure } => {
            let t = model.table(table)?;
            let m = t.measures.iter().find(|m| m.id == *measure)?;
            Some((
                json!({ "Measure": { "Expression": source(table), "Property": m.name } }),
                format!("{}.{}", t.name, m.name),
                m.name.clone(),
            ))
        }
    }
}

/// A text literal in a query expression.
fn text_literal(s: &str) -> Value {
    json!({ "Literal": { "Value": format!("'{}'", s.replace('\'', "''")) } })
}

fn literal_for(model: &Model, p: &Projection, v: &Option<String>) -> Value {
    let Some(v) = v else {
        return json!({ "Literal": { "Value": "null" } });
    };
    let ty = match &p.field {
        FieldRef::Column { table, column } => model
            .table(table)
            .and_then(|t| t.column(column))
            .map(|c| c.data_type),
        _ => None,
    };
    match ty {
        Some(DataType::Int64) if v.parse::<i64>().is_ok() => {
            json!({ "Literal": { "Value": format!("{v}L") } })
        }
        Some(DataType::Double | DataType::Decimal) if v.parse::<f64>().is_ok() => {
            json!({ "Literal": { "Value": format!("{v}D") } })
        }
        Some(DataType::Boolean) => json!({ "Literal": { "Value": v.to_lowercase() } }),
        Some(DataType::Date | DataType::DateTime) if v.len() >= 10 => {
            json!({ "Literal": { "Value": format!("datetime'{}T00:00:00'", &v[..10]) } })
        }
        _ => text_literal(v),
    }
}

/// A filter as Power BI's filter container (basic filters with their values; other
/// kinds with their field only — the annotations keep them).
fn filter_container(model: &Model, f: &ReportFilter, how: Option<&str>) -> Option<Value> {
    let (field, _, _) = field_expr(
        model,
        &Projection {
            aggregation: None,
            ..f.field.clone()
        },
    )?;
    let mut out = json!({ "name": short(&f.id), "field": field.clone() });
    if let Some(h) = how {
        out["howCreated"] = json!(h);
    }
    if let FilterKind::Basic { values, exclude } = &f.filter {
        let FieldRef::Column { table, .. } = &f.field.field else {
            return Some(out);
        };
        let alias = "t";
        let col = json!({ "Column": { "Expression": { "SourceRef": { "Source": alias } }, "Property": field["Column"]["Property"].clone() } });
        let mut cond = json!({ "In": { "Expressions": [col], "Values": values.iter().map(|v| json!([literal_for(model, &f.field, v)])).collect::<Vec<_>>() } });
        if *exclude {
            cond = json!({ "Not": { "Expression": cond } });
        }
        out["type"] = json!("Categorical");
        out["filter"] = json!({
            "Version": 2,
            "From": [{ "Name": alias, "Entity": table_name(model, table), "Type": 0 }],
            "Where": [{ "Condition": cond }],
        });
    }
    Some(out)
}

/// PBIR names: letters, digits, `_` and `-` (charted's ids are UUIDs).
fn short(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect()
}

fn annotation(name: &str, value: &impl serde::Serialize) -> Value {
    json!({ "name": name, "value": serde_json::to_string(value).unwrap_or_default() })
}

fn visual_json(model: &Model, page: &Page, v: &Visual) -> Value {
    let (pbi_type, roles) = TYPES
        .iter()
        .find(|(t, _, _)| *t == v.visual_type)
        .map(|(_, p, r)| (*p, *r))
        .unwrap_or((v.visual_type.as_str(), &[]));
    let mut query_state = serde_json::Map::new();
    let hier = drill_role(&v.visual_type).and_then(|r| Some((r, v.roles.get(r)?.len())));
    for (ours, theirs) in roles {
        let Some(ps) = v.roles.get(*ours).filter(|ps| !ps.is_empty()) else {
            continue;
        };
        let shown = match hier {
            Some((r, n)) if r == *ours && n > 1 => {
                let (start, depth, _) = v.drill_levels(n);
                Some(start..=depth)
            }
            _ => None,
        };
        let projections: Vec<Value> = ps
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let (field, query_ref, native) = field_expr(model, p)?;
                let mut x =
                    json!({ "field": field, "queryRef": query_ref, "nativeQueryRef": native });
                if let Some(d) = &p.display_name {
                    x["displayName"] = json!(d);
                }
                if let Some(s) = &shown {
                    x["active"] = json!(s.contains(&i));
                }
                Some(x)
            })
            .collect();
        query_state.insert((*theirs).into(), json!({ "projections": projections }));
    }
    let mut visual = json!({ "visualType": pbi_type });
    if !roles.is_empty() {
        let mut query = json!({ "queryState": query_state });
        if let Some(s) = &v.sort
            && let Some(p) = v.roles.get(&s.role).and_then(|ps| ps.get(s.index))
            && let Some((field, _, _)) = field_expr(model, p)
        {
            query["sortDefinition"] = json!({ "sort": [{ "field": field, "direction": if s.descending { "Descending" } else { "Ascending" } }], "isDefaultSort": false });
        }
        visual["query"] = query;
        visual["drillFilterOtherVisuals"] = json!(true);
    }
    // The title, and a text box's text, in Power BI's own objects.
    let title = v.objects.get("title");
    let mut title_props = serde_json::Map::new();
    if let Some(show) = title.and_then(|t| t.get("show")).and_then(Value::as_bool) {
        title_props.insert(
            "show".into(),
            json!({ "expr": { "Literal": { "Value": show.to_string() } } }),
        );
    }
    if let Some(text) = title
        .and_then(|t| t.get("text"))
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
    {
        title_props.insert("text".into(), json!({ "expr": text_literal(text) }));
    }
    if !title_props.is_empty() {
        visual["visualContainerObjects"] = json!({ "title": [{ "properties": title_props }] });
    }
    if v.visual_type == "textbox"
        && let Some(text) = v
            .objects
            .get("text")
            .and_then(|t| t.get("content"))
            .and_then(Value::as_str)
    {
        let paragraphs: Vec<Value> = text
            .split('\n')
            .map(|l| json!({ "textRuns": [{ "value": l }] }))
            .collect();
        visual["objects"] = json!({ "general": [{ "properties": { "paragraphs": paragraphs } }] });
    }
    let mut out = json!({
        "$schema": VISUAL_SCHEMA,
        "name": short(&v.id),
        "position": { "x": v.position.x, "y": v.position.y, "z": v.position.z, "width": v.position.width, "height": v.position.height, "tabOrder": v.position.z },
        "visual": visual,
        "annotations": [annotation("chartedVisual", v)],
    });
    let filters: Vec<Value> = v
        .filters
        .iter()
        .filter_map(|f| filter_container(model, f, Some("User")))
        .collect();
    if !filters.is_empty() {
        out["filterConfig"] = json!({ "filters": filters });
    }
    let _ = page;
    out
}

/// The report's PBIR files, relative to the `<Name>.Report` folder.
pub fn report_files(model: &Model) -> Vec<(String, String)> {
    let r = &model.report;
    let mut files = Vec::new();
    let pretty = |v: &Value| {
        let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
        s.push('\n');
        s
    };
    files.push((
        "definition/version.json".into(),
        pretty(&json!({ "$schema": VERSION_SCHEMA, "version": "2.0.0" })),
    ));
    let mut report = json!({
        "$schema": REPORT_SCHEMA,
        "themeCollection": {},
        "annotations": [annotation("chartedReport", &json!({ "filters": r.filters, "bookmarks": r.bookmarks, "theme": r.theme }))],
    });
    let filters: Vec<Value> = r
        .filters
        .iter()
        .filter_map(|f| filter_container(model, f, Some("User")))
        .collect();
    if !filters.is_empty() {
        report["filterConfig"] = json!({ "filters": filters });
    }
    if let Some(theme) = &r.theme {
        let file = format!("{}.json", short(&theme.name.replace(' ', "_")));
        report["themeCollection"] = json!({ "customTheme": { "name": file, "reportVersionAtImport": "5.61", "type": "RegisteredResources" } });
        report["resourcePackages"] = json!([{ "name": "RegisteredResources", "type": "RegisteredResources", "items": [{ "name": file, "path": file, "type": "CustomTheme" }] }]);
        files.push((
            format!("StaticResources/RegisteredResources/{file}"),
            pretty(&theme.json),
        ));
    }
    files.push(("definition/report.json".into(), pretty(&report)));
    // A report has at least one page.
    let blank = Page {
        id: "Page1".into(),
        settings: PageSettings::named("Page 1"),
        visuals: Vec::new(),
    };
    let pages: Vec<&Page> = if r.pages.is_empty() {
        vec![&blank]
    } else {
        r.pages.iter().collect()
    };
    files.push((
        "definition/pages/pages.json".into(),
        pretty(&json!({ "$schema": PAGES_SCHEMA, "pageOrder": pages.iter().map(|p| short(&p.id)).collect::<Vec<_>>(), "activePageName": pages.first().map(|p| short(&p.id)).unwrap_or_default() })),
    ));
    for p in pages {
        let s = &p.settings;
        let mut page = json!({
            "$schema": PAGE_SCHEMA,
            "name": short(&p.id),
            "displayName": s.name,
            "displayOption": "FitToPage",
            "height": s.height,
            "width": s.width,
            "annotations": [annotation("chartedPage", s)],
        });
        let mut filters: Vec<Value> = s
            .filters
            .iter()
            .filter_map(|f| filter_container(model, f, Some("User")))
            .collect();
        if !s.drillthrough.is_empty() {
            let mut params = Vec::new();
            for (i, d) in s.drillthrough.iter().enumerate() {
                let f = ReportFilter {
                    id: format!("{}-drill{i}", short(&p.id)),
                    field: d.clone(),
                    filter: FilterKind::All,
                };
                if let Some(c) = filter_container(model, &f, Some("Drillthrough")) {
                    params.push(json!({ "name": format!("{}-param{i}", short(&p.id)), "boundFilter": f.id, "fieldExpr": c["field"].clone() }));
                    filters.push(c);
                }
            }
            page["pageBinding"] = json!({
                "name": format!("{}-binding", short(&p.id)),
                "type": "Drillthrough",
                "parameters": params,
                "acceptsFilterContext": if s.keep_all_filters { "Default" } else { "None" },
            });
        } else if s.tooltip {
            // A report-page tooltip, hidden in view mode as Power BI hides them.
            let mut params = Vec::new();
            for (i, d) in s.tooltip_fields.iter().enumerate() {
                let f = ReportFilter {
                    id: format!("{}-tip{i}", short(&p.id)),
                    field: d.clone(),
                    filter: FilterKind::All,
                };
                if let Some(c) = filter_container(model, &f, Some("Drillthrough")) {
                    params.push(json!({ "name": format!("{}-param{i}", short(&p.id)), "boundFilter": f.id, "fieldExpr": c["field"].clone() }));
                    filters.push(c);
                }
            }
            page["type"] = json!("Tooltip");
            page["visibility"] = json!("HiddenInViewMode");
            page["pageBinding"] = json!({
                "name": format!("{}-binding", short(&p.id)),
                "type": "Tooltip",
                "parameters": params,
            });
        }
        if !filters.is_empty() {
            page["filterConfig"] = json!({ "filters": filters });
        }
        let dir = format!("definition/pages/{}", short(&p.id));
        files.push((format!("{dir}/page.json"), pretty(&page)));
        for v in &p.visuals {
            files.push((
                format!("{dir}/visuals/{}/visual.json", short(&v.id)),
                pretty(&visual_json(model, p, v)),
            ));
        }
    }
    if !r.bookmarks.is_empty() {
        files.push((
            "definition/bookmarks/bookmarks.json".into(),
            pretty(&json!({ "$schema": BOOKMARKS_SCHEMA, "items": r.bookmarks.iter().map(|b| json!({ "name": short(&b.id) })).collect::<Vec<_>>() })),
        ));
        for b in &r.bookmarks {
            files.push((
                format!("definition/bookmarks/{}.bookmark.json", short(&b.id)),
                pretty(&json!({ "$schema": BOOKMARK_SCHEMA, "name": short(&b.id), "displayName": b.name, "explorationState": { "version": "1.3", "activeSection": short(&b.page), "sections": { short(&b.page): { "visualContainers": {} } } } })),
            ));
        }
    }
    files
}

// --- Import -------------------------------------------------------------------------

/// A Power BI field expression as a charted projection.
fn projection(
    model: &Model,
    field: &Value,
    aliases: &BTreeMap<String, String>,
) -> Option<Projection> {
    let entity = |e: &Value| -> Option<String> {
        let s = e.get("SourceRef")?;
        s.get("Entity")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                s.get("Source")
                    .and_then(Value::as_str)
                    .and_then(|a| aliases.get(a).cloned())
            })
    };
    let column = |c: &Value| -> Option<(Id, Id)> {
        let t = model.table_by_name(&entity(c.get("Expression")?)?)?;
        let col = t.column_by_name(c.get("Property")?.as_str()?)?;
        Some((t.id.clone(), col.id.clone()))
    };
    if let Some(c) = field.get("Column") {
        let (table, column) = column(c)?;
        return Some(Projection {
            field: FieldRef::Column { table, column },
            aggregation: None,
            display_name: None,
        });
    }
    if let Some(m) = field.get("Measure") {
        let t = model.table_by_name(&entity(m.get("Expression")?)?)?;
        let name = m.get("Property")?.as_str()?;
        let me = t.measures.iter().find(|x| x.name == name)?;
        return Some(Projection {
            field: FieldRef::Measure {
                table: t.id.clone(),
                measure: me.id.clone(),
            },
            aggregation: None,
            display_name: None,
        });
    }
    if let Some(a) = field.get("Aggregation") {
        let (table, column) = column(a.get("Expression")?.get("Column")?)?;
        let aggregation = match a.get("Function")?.as_u64()? {
            0 => Aggregation::Sum,
            1 => Aggregation::Average,
            2 => Aggregation::DistinctCount,
            3 => Aggregation::Min,
            4 => Aggregation::Max,
            _ => Aggregation::Count,
        };
        return Some(Projection {
            field: FieldRef::Column { table, column },
            aggregation: Some(aggregation),
            display_name: None,
        });
    }
    None
}

/// A literal's value as charted's filter text.
fn literal_text(v: &Value) -> Option<Option<String>> {
    let s = v.get("Literal")?.get("Value")?.as_str()?;
    if s == "null" {
        return Some(None);
    }
    if let Some(t) = s.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        return Some(Some(t.replace("''", "'")));
    }
    if let Some(d) = s.strip_prefix("datetime'") {
        return Some(Some(d.chars().take(10).collect()));
    }
    Some(Some(s.trim_end_matches(['L', 'D', 'M']).to_string()))
}

/// A Power BI filter container as a charted filter (categorical filters keep their
/// values; others filter nothing yet).
fn report_filter(model: &Model, f: &Value) -> Option<ReportFilter> {
    // PBIR names the field `field`; the older report.json `expression`.
    let field = projection(
        model,
        f.get("field").or(f.get("expression"))?,
        &BTreeMap::new(),
    )?;
    let mut kind = FilterKind::All;
    if let Some(def) = f.get("filter") {
        let aliases: BTreeMap<String, String> = def
            .get("From")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|s| {
                        Some((
                            s.get("Name")?.as_str()?.to_string(),
                            s.get("Entity")?.as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let _ = aliases;
        if let Some(cond) = def
            .get("Where")
            .and_then(|w| w.get(0))
            .and_then(|w| w.get("Condition"))
        {
            let (inner, exclude) = match cond.get("Not") {
                Some(n) => (n.get("Expression")?, true),
                None => (cond, false),
            };
            if let Some(values) = inner
                .get("In")
                .and_then(|i| i.get("Values"))
                .and_then(Value::as_array)
            {
                let values: Vec<Option<String>> = values
                    .iter()
                    .filter_map(|t| t.get(0).and_then(literal_text))
                    .collect();
                kind = FilterKind::Basic { values, exclude };
            }
        }
    }
    Some(ReportFilter {
        id: f
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_default(),
        field: Projection {
            aggregation: None,
            ..field
        },
        filter: kind,
    })
}

fn charted_type(pbi: &str) -> Option<&'static str> {
    TYPES
        .iter()
        .find(|(_, p, _)| *p == pbi)
        .map(|(c, _, _)| *c)
        .or_else(|| ALIASES.iter().find(|(p, _)| *p == pbi).map(|(_, c)| *c))
}

fn annotation_of<T: serde::de::DeserializeOwned>(v: &Value, name: &str) -> Option<T> {
    v.get("annotations")?
        .as_array()?
        .iter()
        .find(|a| a.get("name").and_then(Value::as_str) == Some(name))
        .and_then(|a| serde_json::from_str(a.get("value")?.as_str()?).ok())
}

/// A visual from its PBIR (or older `config`) description.
fn import_visual(
    model: &Model,
    v: &Value,
    page: &str,
    skipped: &mut Vec<Skipped>,
) -> Option<Visual> {
    if let Some(own) = annotation_of::<Visual>(v, "chartedVisual") {
        return Some(own);
    }
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("visual")
        .to_string();
    let pos = v.get("position").cloned().unwrap_or_default();
    let num = |k: &str| pos.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let vis = v.get("visual")?;
    let pbi_type = vis.get("visualType").and_then(Value::as_str).unwrap_or("");
    let Some(ours) = charted_type(pbi_type) else {
        skipped.push(Skipped {
            kind: "visual".into(),
            name: format!("{page}: {pbi_type}"),
            reason: "charted has no visual of this type".into(),
        });
        return None;
    };
    let roles_map = TYPES
        .iter()
        .find(|(c, _, _)| *c == ours)
        .map(|(_, _, r)| *r)
        .unwrap_or(&[]);
    let mut roles: BTreeMap<String, Vec<Projection>> = BTreeMap::new();
    let mut lost = 0;
    if let Some(state) = vis
        .get("query")
        .and_then(|q| q.get("queryState"))
        .and_then(Value::as_object)
    {
        for (theirs, st) in state {
            // The new card ("cardVisual") calls its values "Data".
            let theirs = if theirs == "Data" {
                "Values"
            } else {
                theirs.as_str()
            };
            let ours_role = roles_map
                .iter()
                .find(|(_, t)| *t == theirs)
                .map(|(o, _)| *o);
            for p in st
                .get("projections")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                match (
                    ours_role,
                    p.get("field")
                        .and_then(|f| projection(model, f, &BTreeMap::new())),
                ) {
                    (Some(r), Some(mut proj)) => {
                        proj.display_name = p
                            .get("displayName")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        roles.entry(r.into()).or_default().push(proj);
                    }
                    _ => lost += 1,
                }
            }
        }
    }
    if lost > 0 {
        skipped.push(Skipped {
            kind: "field".into(),
            name: format!("{page}: {pbi_type} {name}"),
            reason: format!(
                "{lost} of its fields aren't in the model or have no place in charted's {ours}"
            ),
        });
    }
    let mut objects = serde_json::Map::new();
    let lit = |x: &Value| x.get("expr").and_then(literal_text).flatten();
    if let Some(t) = vis
        .get("visualContainerObjects")
        .and_then(|o| o.get("title"))
        .and_then(|t| t.get(0))
        .and_then(|t| t.get("properties"))
    {
        let mut title = serde_json::Map::new();
        if let Some(show) = t.get("show").and_then(lit) {
            title.insert("show".into(), json!(show == "true"));
        }
        if let Some(text) = t.get("text").and_then(lit) {
            title.insert("text".into(), json!(text));
        }
        if !title.is_empty() {
            objects.insert("title".into(), Value::Object(title));
        }
    }
    if ours == "textbox"
        && let Some(ps) = vis
            .pointer("/objects/general/0/properties/paragraphs")
            .and_then(Value::as_array)
    {
        let text: Vec<String> = ps
            .iter()
            .map(|p| {
                p.get("textRuns")
                    .and_then(Value::as_array)
                    .map(|r| {
                        r.iter()
                            .filter_map(|x| x.get("value").and_then(Value::as_str))
                            .collect::<String>()
                    })
                    .unwrap_or_default()
            })
            .collect();
        objects.insert("text".into(), json!({ "content": text.join("\n") }));
    }
    let sort = vis.pointer("/query/sortDefinition/sort/0").and_then(|s| {
        let p = projection(model, s.get("field")?, &BTreeMap::new())?;
        let (role, index) = roles.iter().find_map(|(r, ps)| {
            ps.iter()
                .position(|x| x.field == p.field && x.aggregation == p.aggregation)
                .map(|i| (r.clone(), i))
        })?;
        Some(VisualSort {
            role,
            index,
            descending: s.get("direction").and_then(Value::as_str) == Some("Descending"),
        })
    });
    let filters = v
        .pointer("/filterConfig/filters")
        .and_then(Value::as_array)
        .map(|fs| fs.iter().filter_map(|f| report_filter(model, f)).collect())
        .unwrap_or_default();
    Some(Visual {
        id: name,
        visual_type: ours.into(),
        position: Position {
            x: num("x"),
            y: num("y"),
            width: num("width").max(20.0),
            height: num("height").max(20.0),
            z: num("z") as i32,
        },
        roles,
        filters,
        sort,
        objects,
        selection: None,
        drill: Vec::new(),
    })
}

/// Reads a PBIR report (files relative to the `<Name>.Report` folder).
pub fn read_pbir(
    model: &Model,
    files: &BTreeMap<String, String>,
    skipped: &mut Vec<Skipped>,
) -> Result<Report, String> {
    let json = |p: &str| -> Option<Value> {
        serde_json::from_str(files.get(p)?.trim_start_matches('\u{feff}')).ok()
    };
    let report =
        json("definition/report.json").ok_or("the report has no definition/report.json")?;
    let own: Option<Value> = annotation_of(&report, "chartedReport");
    let mut out = Report::default();
    match &own {
        Some(o) => {
            out.filters = serde_json::from_value(o["filters"].clone()).unwrap_or_default();
            out.bookmarks = serde_json::from_value(o["bookmarks"].clone()).unwrap_or_default();
            out.theme = serde_json::from_value(o["theme"].clone()).unwrap_or_default();
        }
        None => {
            out.filters = report
                .pointer("/filterConfig/filters")
                .and_then(Value::as_array)
                .map(|fs| fs.iter().filter_map(|f| report_filter(model, f)).collect())
                .unwrap_or_default();
            // A custom theme file.
            if let Some(name) = report
                .pointer("/themeCollection/customTheme/name")
                .and_then(Value::as_str)
                && let Some(theme) = files
                    .get(&format!("StaticResources/RegisteredResources/{name}"))
                    .and_then(|t| serde_json::from_str::<Value>(t).ok())
            {
                out.theme = Some(ReportTheme {
                    name: theme
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(name)
                        .to_string(),
                    json: theme,
                });
            }
        }
    }
    let order: Vec<String> = json("definition/pages/pages.json")
        .and_then(|p| p.get("pageOrder").cloned())
        .and_then(|o| serde_json::from_value(o).ok())
        .unwrap_or_else(|| {
            let mut names: Vec<String> = files
                .keys()
                .filter_map(|k| {
                    k.strip_prefix("definition/pages/")?
                        .strip_suffix("/page.json")
                        .map(str::to_string)
                })
                .collect();
            names.sort();
            names
        });
    for name in order {
        let Some(p) = json(&format!("definition/pages/{name}/page.json")) else {
            continue;
        };
        let settings = match annotation_of::<PageSettings>(&p, "chartedPage") {
            Some(s) => s,
            None => {
                let mut s = PageSettings::named(
                    p.get("displayName")
                        .and_then(Value::as_str)
                        .unwrap_or(&name),
                );
                s.width = p.get("width").and_then(Value::as_u64).unwrap_or(1280) as u32;
                s.height = p.get("height").and_then(Value::as_u64).unwrap_or(720) as u32;
                let tooltip = p.get("type").and_then(Value::as_str) == Some("Tooltip")
                    || p.pointer("/pageBinding/type").and_then(Value::as_str) == Some("Tooltip");
                s.tooltip = tooltip;
                for f in p
                    .pointer("/filterConfig/filters")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let drill = f.get("howCreated").and_then(Value::as_str) == Some("Drillthrough");
                    if let Some(rf) = report_filter(model, f) {
                        if drill && tooltip {
                            s.tooltip_fields.push(rf.field);
                        } else if drill {
                            s.drillthrough.push(rf.field);
                        } else {
                            s.filters.push(rf);
                        }
                    }
                }
                if p.pointer("/pageBinding/acceptsFilterContext")
                    .and_then(Value::as_str)
                    == Some("None")
                {
                    s.keep_all_filters = false;
                }
                s
            }
        };
        let prefix = format!("definition/pages/{name}/visuals/");
        let mut visuals: Vec<Visual> = files
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with("/visual.json"))
            .filter_map(|(_, t)| {
                serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok()
            })
            .filter_map(|v| import_visual(model, &v, &settings.name, skipped))
            .collect();
        visuals.sort_by_key(|v| v.position.z);
        out.pages.push(Page {
            id: name,
            settings,
            visuals,
        });
    }
    Ok(out)
}

/// Reads an older report (`report.json` with sections and visual containers whose
/// `config` is JSON in a string).
pub fn read_legacy(
    model: &Model,
    text: &str,
    skipped: &mut Vec<Skipped>,
) -> Result<Report, String> {
    let r: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("report.json isn't valid JSON: {e}"))?;
    let mut out = Report::default();
    let parse = |v: Option<&Value>| -> Value {
        v.and_then(Value::as_str)
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    };
    for f in parse(r.get("filters")).as_array().into_iter().flatten() {
        if let Some(rf) = report_filter(model, f) {
            out.filters.push(rf);
        }
    }
    for (i, s) in r
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = s
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("page{i}"));
        let mut settings = PageSettings::named(
            s.get("displayName")
                .and_then(Value::as_str)
                .unwrap_or(&name),
        );
        settings.width = s.get("width").and_then(Value::as_f64).unwrap_or(1280.0) as u32;
        settings.height = s.get("height").and_then(Value::as_f64).unwrap_or(720.0) as u32;
        for f in parse(s.get("filters")).as_array().into_iter().flatten() {
            if let Some(rf) = report_filter(model, f) {
                settings.filters.push(rf);
            }
        }
        let mut visuals = Vec::new();
        for c in s
            .get("visualContainers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let config = parse(c.get("config"));
            let single = config.get("singleVisual").cloned().unwrap_or_default();
            // The prototype query names each field (`queryRef`) and its table alias.
            let proto = single.get("prototypeQuery").cloned().unwrap_or_default();
            let aliases: BTreeMap<String, String> = proto
                .get("From")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|s| {
                            Some((
                                s.get("Name")?.as_str()?.to_string(),
                                s.get("Entity")?.as_str()?.to_string(),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut select: BTreeMap<String, Value> = BTreeMap::new();
            for sel in proto
                .get("Select")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(n) = sel.get("Name").and_then(Value::as_str) {
                    // Resolve aliases to entities, as PBIR writes them.
                    let resolved = resolve_aliases(sel, &aliases);
                    select.insert(n.to_string(), resolved);
                }
            }
            let mut query_state = serde_json::Map::new();
            for (role, ps) in single
                .get("projections")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
            {
                let projections: Vec<Value> = ps
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let q = p.get("queryRef")?.as_str()?;
                        let mut field = select.get(q)?.clone();
                        if let Some(o) = field.as_object_mut() {
                            o.remove("Name");
                            o.remove("NativeReferenceName");
                        }
                        Some(json!({ "field": field, "queryRef": q }))
                    })
                    .collect();
                query_state.insert(role.clone(), json!({ "projections": projections }));
            }
            let layout = config.pointer("/layouts/0/position").cloned().unwrap_or_else(|| json!({ "x": c.get("x"), "y": c.get("y"), "z": c.get("z"), "width": c.get("width"), "height": c.get("height") }));
            let v = json!({
                "name": config.get("name").cloned().unwrap_or(json!(format!("visual{}", visuals.len()))),
                "position": layout,
                "visual": { "visualType": single.get("visualType").cloned().unwrap_or(json!("")), "query": { "queryState": query_state }, "objects": single.get("objects").cloned().unwrap_or_default(), "visualContainerObjects": single.get("vcObjects").cloned().unwrap_or_default() },
            });
            if let Some(vis) = import_visual(model, &v, &settings.name, skipped) {
                visuals.push(vis);
            }
        }
        out.pages.push(Page {
            id: name,
            settings,
            visuals,
        });
    }
    Ok(out)
}

/// `SourceRef: {Source: alias}` → `SourceRef: {Entity: table}`, everywhere in `v`.
fn resolve_aliases(v: &Value, aliases: &BTreeMap<String, String>) -> Value {
    match v {
        Value::Object(o) => {
            if let Some(s) = o
                .get("SourceRef")
                .and_then(|s| s.get("Source"))
                .and_then(Value::as_str)
                && let Some(e) = aliases.get(s)
            {
                let mut o = o.clone();
                o.insert("SourceRef".into(), json!({ "Entity": e }));
                return Value::Object(o);
            }
            Value::Object(
                o.iter()
                    .map(|(k, x)| (k.clone(), resolve_aliases(x, aliases)))
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| resolve_aliases(x, aliases)).collect()),
        _ => v.clone(),
    }
}
