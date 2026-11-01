//! Conformance against Power BI Desktop itself.
//!
//! The golden fixture (tests/golden) is exported as a PBIP project, with every golden
//! measure in every golden visual shape written as the DAX query a Power BI visual
//! sends (`conformance/queries.json`). `conformance/run.ps1`, run on Windows with the
//! project open in Power BI Desktop, records Desktop's answers in
//! `conformance/expected.json`, and the test below compares charted's with them.
//!
//! - `CHARTED_UPDATE_CONFORMANCE=1 bin/heavy cargo test -p charted-core --test conformance`
//!   rewrites the project and the queries after the fixture or a measure changes.
//! - A query whose hash no longer matches expected.json is reported as needing a re-run
//!   on Windows, not compared.
//! - Known differences are listed in `conformance/known.json` (id → reason). They
//!   don't fail, but one that has started to agree does, so the list only shrinks.

mod golden;

use charted_compiler::interp::Value;
use charted_compiler::steps::{Query, Source};
use charted_compiler::visual::{self, OrderBy, OrderTarget, VisualQuery};
use charted_model::*;
use golden::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../conformance")
}

fn fnv(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Each fixture table as an *Enter data* query, so the project carries its own data.
fn queries(f: &Fixture) -> Vec<Query> {
    f.model
        .tables
        .iter()
        .map(|t| {
            let data = &f.data.tables[&t.name];
            let columns: Vec<(String, DataType)> = t
                .columns
                .iter()
                .map(|c| (c.name.clone(), c.data_type))
                .collect();
            let rows = data
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|v| match v {
                            Value::Blank => None,
                            Value::Num(x) => Some(format!("{x}")),
                            Value::Text(s) => Some(s.clone()),
                            Value::Bool(b) => Some(b.to_string()),
                        })
                        .collect()
                })
                .collect();
            Query {
                id: format!("q:{}", t.name),
                name: t.name.clone(),
                table: t.source.clone(),
                source: Source::Table { columns, rows },
                steps: Vec::new(),
                step_names: Default::default(),
                no_load: false,
            }
        })
        .collect()
}

fn table_ref(t: &str) -> String {
    format!("'{}'", t.replace('\'', "''"))
}

fn column_ref(m: &Model, field: &FieldRef) -> (String, DataType) {
    let FieldRef::Column { table, column } = field else {
        panic!("shapes group and filter by columns");
    };
    let t = m.table(table).unwrap();
    let c = t.column(column).unwrap();
    (
        format!("{}[{}]", table_ref(&t.name), c.name.replace(']', "]]")),
        c.data_type,
    )
}

fn literal(v: &Option<String>, ty: DataType) -> String {
    let Some(v) = v else {
        return "BLANK()".into();
    };
    match ty {
        DataType::Int64 | DataType::Double | DataType::Decimal => v.clone(),
        DataType::Boolean => v.to_uppercase() + "()",
        DataType::Date | DataType::DateTime => {
            let p: Vec<&str> = v[..10].split('-').collect();
            format!(
                "DATE({}, {}, {})",
                p[0],
                p[1].trim_start_matches('0'),
                p[2].trim_start_matches('0')
            )
        }
        _ => format!("\"{}\"", v.replace('"', "\"\"")),
    }
}

/// The query a Power BI visual sends for `measure` in shape `q`.
fn dax_query(m: &Model, measure: &str, q: &VisualQuery) -> String {
    let mut out = String::new();
    let mut args: Vec<String> = q.groups.iter().map(|g| column_ref(m, &g.field).0).collect();
    if !q.filters.is_empty() {
        out.push_str("DEFINE\n");
        for (i, f) in q.filters.iter().enumerate() {
            let (col, ty) = column_ref(m, &f.field.field);
            let FilterKind::Basic {
                values,
                exclude: false,
            } = &f.filter
            else {
                panic!("shapes filter by lists of values");
            };
            let vals: Vec<String> = values.iter().map(|v| literal(v, ty)).collect();
            let var = format!(
                "__DS0FilterTable{}",
                if i == 0 {
                    String::new()
                } else {
                    (i + 1).to_string()
                }
            );
            out.push_str(&format!(
                "\tVAR {var} = TREATAS({{{}}}, {col})\n",
                vals.join(", ")
            ));
            args.push(var);
        }
        out.push('\n');
    }
    args.push(format!("\"M\", [{}]", measure.replace(']', "]]")));
    out.push_str(&format!(
        "EVALUATE\n\tSUMMARIZECOLUMNS({})\n",
        args.join(", ")
    ));
    out
}

/// Measures whose answer depends on when or how often they run.
fn volatile(expr: &str) -> bool {
    let e = expr.to_uppercase();
    ["NOW (", "NOW(", "UTCNOW", "RAND"]
        .iter()
        .any(|f| e.contains(f))
}

/// A measure's expression with those of the measures it references, transitively.
fn measure_text(name: &str) -> String {
    let mut seen = vec![name.to_string()];
    let mut i = 0;
    let mut out = String::new();
    while i < seen.len() {
        let (_, expr) = MEASURES.iter().find(|(n, _)| *n == seen[i]).unwrap();
        out.push_str(&format!("{} = {expr}\n", seen[i]));
        for (n, _) in MEASURES {
            if expr.contains(&format!("[{n}]")) && !seen.iter().any(|s| s == n) {
                seen.push(n.to_string());
            }
        }
        i += 1;
    }
    out
}

struct Case {
    id: String,
    measure: &'static str,
    shape: &'static str,
    query: VisualQuery,
    dax: String,
    hash: String,
}

fn cases(f: &Fixture) -> Vec<Case> {
    let mut out = Vec::new();
    for (name, expr) in MEASURES {
        if volatile(expr) {
            continue;
        }
        let value = Projection {
            field: FieldRef::Measure {
                table: "Sales".into(),
                measure: format!("m:{name}"),
            },
            aggregation: None,
            display_name: None,
        };
        for (shape, base) in shapes(&f.model) {
            let dax = dax_query(&f.model, name, &base);
            let query = VisualQuery {
                values: vec![value.clone()],
                order: (0..base.groups.len())
                    .map(|i| OrderBy {
                        target: OrderTarget::Group(i),
                        descending: false,
                    })
                    .collect(),
                ..base
            };
            out.push(Case {
                id: format!("{name} / {shape}"),
                measure: name,
                shape,
                hash: fnv(&format!("{dax}\n{}", measure_text(name))),
                query,
                dax,
            });
        }
    }
    out
}

/// The hash of the data and the model without its measures.
fn fixture_hash(f: &Fixture) -> String {
    let mut bare = f.model.clone();
    for t in &mut bare.tables {
        t.measures.clear();
    }
    let files = charted_tmdl::pbip("golden", &bare, &queries(f), "");
    fnv(&files
        .iter()
        .map(|(p, t)| format!("{p}\n{t}"))
        .collect::<String>())
}

#[test]
fn conformance_files_are_current() {
    let f = fixture();
    let mut files: Vec<(PathBuf, String)> =
        charted_tmdl::pbip("golden", &f.model, &queries(&f), "")
            .into_iter()
            .map(|(p, t)| (root().join("golden").join(p), t))
            .collect();
    let cases = cases(&f);
    let doc = json!({
        "fixture": fixture_hash(&f),
        "queries": cases.iter().map(|c| json!({
            "id": c.id, "measure": c.measure, "shape": c.shape, "hash": c.hash, "dax": c.dax,
        })).collect::<Vec<_>>(),
    });
    files.push((
        root().join("queries.json"),
        serde_json::to_string_pretty(&doc).unwrap() + "\n",
    ));
    if std::env::var_os("CHARTED_UPDATE_CONFORMANCE").is_some() {
        let _ = std::fs::remove_dir_all(root().join("golden"));
        for (p, t) in &files {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, t).unwrap();
        }
        return;
    }
    let stale: Vec<String> = files
        .iter()
        .filter(|(p, t)| std::fs::read_to_string(p).ok().as_deref() != Some(t.as_str()))
        .map(|(p, _)| p.display().to_string())
        .collect();
    assert!(
        stale.is_empty(),
        "the conformance project is out of date ({}); run CHARTED_UPDATE_CONFORMANCE=1 bin/heavy cargo test -p charted-core --test conformance",
        stale.join(", ")
    );
}

/// A cell as run.ps1 writes it: JSON scalars, dates as ISO text, and non-finite
/// numbers as {"num": "Infinity" | "-Infinity" | "NaN"}.
fn cell(j: &Json) -> Value {
    match j {
        Json::Null => Value::Blank,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(x) => Value::Num(x.as_f64().unwrap()),
        Json::String(s) => Value::Text(s.clone()),
        Json::Object(o) => Value::Num(match o.get("num").and_then(Json::as_str) {
            Some("Infinity") => f64::INFINITY,
            Some("-Infinity") => f64::NEG_INFINITY,
            _ => f64::NAN,
        }),
        Json::Array(_) => panic!("a cell can't be a list"),
    }
}

/// Dates and date-times written either way compare equal.
fn date_text(s: &str) -> String {
    let s = s.replace(' ', "T");
    s.strip_suffix("T00:00:00").unwrap_or(&s).to_string()
}

fn agree(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => {
            (x.is_nan() && y.is_nan())
                || x == y
                || (x - y).abs() <= 1e-9 * 1f64.max(x.abs()).max(y.abs())
        }
        (Value::Bool(x), Value::Num(y)) | (Value::Num(y), Value::Bool(x)) => {
            f64::from(u8::from(*x)) == *y
        }
        (Value::Text(x), Value::Text(y)) => x == y || date_text(x) == date_text(y),
        _ => a == b,
    }
}

fn show(rows: &[Vec<Value>]) -> String {
    rows.iter()
        .map(|r| {
            r.iter()
                .map(|v| match v {
                    Value::Blank => "∅".into(),
                    Value::Num(x) => format!("{x}"),
                    Value::Text(s) => format!("{s:?}"),
                    Value::Bool(b) => b.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect::<Vec<_>>()
        .join("\n    ")
}

#[test]
fn charted_agrees_with_power_bi() {
    let Ok(text) = std::fs::read_to_string(root().join("expected.json")) else {
        eprintln!("conformance/expected.json is missing: run conformance/run.ps1 on Windows");
        return;
    };
    let expected: Json = serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
    let f = fixture();
    if expected["fixture"].as_str() != Some(fixture_hash(&f).as_str()) {
        eprintln!(
            "the fixture changed since expected.json was recorded: re-run conformance/run.ps1"
        );
        return;
    }
    let known: BTreeMap<String, String> = std::fs::read_to_string(root().join("known.json"))
        .ok()
        .map(|t| serde_json::from_str(&t).unwrap())
        .unwrap_or_default();
    let schema = |source: &str, column: &str| -> Option<DataType> {
        let t = f.model.tables.iter().find(|t| t.source == source)?;
        t.columns
            .iter()
            .find(|c| matches!(&c.kind, ColumnKind::Data { source_column } if source_column == column))
            .map(|c| c.data_type)
    };
    let (mut rerun, mut differ, mut fixed, mut agreed) = (Vec::new(), Vec::new(), Vec::new(), 0);
    for c in cases(&f) {
        let e = &expected["results"][&c.id];
        if e["hash"].as_str() != Some(c.hash.as_str()) {
            rerun.push(c.id);
            continue;
        }
        let ours = visual::compile(&f.model, &schema, &c.query)
            .map_err(|e| e.to_string())
            .and_then(|q| f.db.arrow(&q.sql).map_err(|e| e.to_string()))
            .map(|b| canonical(arrow_values(b)));
        let ok = match (&ours, e.get("error")) {
            (Err(_), Some(_)) => true,
            (Ok(_), Some(_)) | (Err(_), None) => false,
            (Ok(got), None) => {
                let mut want: Vec<Vec<Value>> = e["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_array().unwrap().iter().map(cell).collect())
                    .collect();
                // A card whose measure is blank gets no row from Power BI.
                if c.query.groups.is_empty() && want.is_empty() {
                    want.push(vec![Value::Blank]);
                }
                let want = canonical(want);
                got.len() == want.len()
                    && got.iter().zip(&want).all(|(a, b)| {
                        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| agree(x, y))
                    })
            }
        };
        match (ok, known.contains_key(&c.id)) {
            (true, false) => agreed += 1,
            (true, true) => fixed.push(c.id),
            (false, true) => {}
            (false, false) => differ.push(format!(
                "{}:\n  Power BI:\n    {}\n  charted:\n    {}",
                c.id,
                match e.get("error") {
                    Some(err) => format!("error: {err}"),
                    None => show(&canonical(
                        e["rows"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|r| r.as_array().unwrap().iter().map(cell).collect())
                            .collect(),
                    )),
                },
                match &ours {
                    Ok(rows) => show(rows),
                    Err(err) => format!("error: {err}"),
                }
            )),
        }
    }
    eprintln!(
        "{agreed} agree with Power BI {}; {} known differences; {} need a re-run on Windows",
        expected["powerbi"].as_str().unwrap_or("?"),
        known.len(),
        rerun.len()
    );
    assert!(
        fixed.is_empty(),
        "these now agree with Power BI; remove them from conformance/known.json: {fixed:?}"
    );
    assert!(
        differ.is_empty(),
        "{} differ from Power BI:\n\n{}",
        differ.len(),
        differ.join("\n\n")
    );
}
