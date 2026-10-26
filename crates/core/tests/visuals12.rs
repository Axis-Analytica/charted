//! v1.2 visual queries against the Harbor & Pine sample: combo charts' line values,
//! conditional formatting's hidden values, Top N filters on pages and the report, and
//! small multiples.

use arrow_ipc::reader::StreamReader;
use charted_compiler::visual::VisualQuery;
use charted_core::{App, DataRequest, Request};
use charted_model::*;
use std::collections::BTreeMap;

struct Sample {
    app: App,
    page: String,
    _dir: tempfile::TempDir,
}

fn sample() -> Sample {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(dir.path().join("sample").display().to_string()),
    })
    .unwrap();
    let page = app.session().unwrap().project.model.report.pages[0]
        .id
        .clone();
    Sample {
        app,
        page,
        _dir: dir,
    }
}

impl Sample {
    fn model(&self) -> Model {
        self.app.session().unwrap().project.model.clone()
    }

    fn col(&self, table: &str, column: &str) -> Projection {
        let m = self.model();
        let t = m.table_by_name(table).unwrap();
        Projection {
            field: FieldRef::Column {
                table: t.id.clone(),
                column: t.column_by_name(column).unwrap().id.clone(),
            },
            aggregation: None,
            display_name: None,
        }
    }

    fn measure(&self, name: &str) -> Projection {
        let m = self.model();
        let (t, me) = m
            .tables
            .iter()
            .find_map(|t| t.measures.iter().find(|x| x.name == name).map(|x| (t, x)))
            .unwrap();
        Projection {
            field: FieldRef::Measure {
                table: t.id.clone(),
                measure: me.id.clone(),
            },
            aggregation: None,
            display_name: None,
        }
    }

    fn commit(&mut self, changes: Vec<Change>) {
        self.app
            .handle(Request::Commit {
                label: "test".into(),
                changes,
            })
            .unwrap();
    }

    fn add(
        &mut self,
        id: &str,
        ty: &str,
        roles: Vec<(&str, Vec<Projection>)>,
        objects: serde_json::Value,
    ) {
        let visual = Visual {
            id: id.into(),
            visual_type: ty.into(),
            position: Position {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
                z: 0,
            },
            roles: roles
                .into_iter()
                .map(|(r, ps)| (r.to_string(), ps))
                .collect(),
            filters: vec![],
            sort: None,
            objects: serde_json::from_value(objects).unwrap(),
            selection: None,
            drill: vec![],
        };
        let page = self.page.clone();
        self.commit(vec![Change::AddVisual {
            page,
            visual,
            index: None,
        }]);
    }

    /// The visual's rows as named text cells.
    fn rows(&self, visual: &str) -> Vec<BTreeMap<String, String>> {
        let bytes = self
            .app
            .data(DataRequest::ReportVisual {
                visual: visual.into(),
                totals: false,
                filters: vec![],
                highlight: vec![],
                tag: None,
                search: None,
            })
            .unwrap_or_else(|e| panic!("{}", e.message));
        named(bytes)
    }

    fn query(&self, query: VisualQuery) -> Vec<BTreeMap<String, String>> {
        named(self.app.data(DataRequest::Visual { query }).unwrap())
    }
}

fn named(bytes: Vec<u8>) -> Vec<BTreeMap<String, String>> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let names: Vec<String> = batch
            .schema()
            .fields()
            .iter()
            .map(|f| f.name().clone())
            .collect();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).unwrap())
            .collect();
        for r in 0..batch.num_rows() {
            rows.push(
                names
                    .iter()
                    .zip(&fmts)
                    .map(|(n, f)| (n.clone(), f.value(r).to_string()))
                    .collect(),
            );
        }
    }
    rows
}

fn cents(s: &str) -> String {
    s.parse::<f64>()
        .map(|x| format!("{:.2}", x))
        .unwrap_or_else(|_| s.to_string())
}

#[test]
fn combo_line_values_belong_to_the_category_not_the_legend() {
    let mut s = sample();
    let (year, channel) = (s.col("Date", "Year"), s.col("Store", "Channel"));
    let (sales, units) = (s.measure("Total Sales"), s.measure("Units"));
    s.add(
        "combo",
        "lineClusteredColumnComboChart",
        vec![
            ("category", vec![year.clone()]),
            ("series", vec![channel]),
            ("values", vec![sales]),
            ("lineValues", vec![units.clone()]),
        ],
        serde_json::json!({}),
    );
    let rows = s.rows("combo");
    assert!(rows.len() > 4, "{rows:?}");
    // Every legend value of a year has the year's units.
    let by_year: BTreeMap<String, String> = s
        .query(VisualQuery {
            groups: vec![year],
            values: vec![units],
            ..Default::default()
        })
        .into_iter()
        .map(|r| (r["g0"].clone(), cents(&r["v0"])))
        .collect();
    for r in &rows {
        assert_eq!(
            cents(&r["lineValues:0"]),
            by_year[&r["category:0"]],
            "{r:?}"
        );
        assert!(r.contains_key("series:0") && r.contains_key("key:series:0"));
    }
}

#[test]
fn conditional_formatting_fields_are_hidden_values() {
    let mut s = sample();
    let (category, sales, units) = (
        s.col("Product", "Category"),
        s.measure("Total Sales"),
        s.measure("Units"),
    );
    let key = "m:sales:total|";
    s.add(
        "table",
        "table",
        vec![("columns", vec![category.clone(), sales])],
        serde_json::json!({ "conditionalFormatting": { key: { "icons": { "mode": "rules", "field": units, "rules": [] }, "background": { "mode": "gradient", "field": null } } } }),
    );
    let rows = s.rows("table");
    assert_eq!(rows.len(), 6);
    let by_cat: BTreeMap<String, String> = s
        .query(VisualQuery {
            groups: vec![category],
            values: vec![units],
            ..Default::default()
        })
        .into_iter()
        .map(|r| (r["g0"].clone(), cents(&r["v0"])))
        .collect();
    let hidden = format!("cf:{key}:icons");
    for r in &rows {
        assert_eq!(cents(&r[&hidden]), by_cat[&r["columns:0"]], "{r:?}");
        // A rule on the column's own values adds nothing.
        assert!(!r.contains_key(&format!("cf:{key}:background")));
    }
}

#[test]
fn top_n_filters_on_pages_and_the_report() {
    let mut s = sample();
    let (category, sales) = (s.col("Product", "Category"), s.measure("Total Sales"));
    s.add(
        "col",
        "columnChart",
        vec![
            ("category", vec![category.clone()]),
            ("values", vec![sales.clone()]),
        ],
        serde_json::json!({}),
    );
    let all: Vec<String> = s
        .rows("col")
        .iter()
        .map(|r| r["category:0"].clone())
        .collect();
    assert_eq!(all.len(), 6);
    let top = |count: u32, bottom: bool| ReportFilter {
        id: "f-top".into(),
        field: category.clone(),
        filter: FilterKind::TopN {
            count,
            bottom,
            by: sales.clone(),
        },
    };
    // Top 3 on the page: the three biggest, still ranked.
    let model = s.model();
    let mut settings = model.report.pages[0].settings.clone();
    settings.filters = vec![top(3, false)];
    let page = s.page.clone();
    s.commit(vec![Change::SetPage { page, settings }]);
    let got: Vec<String> = s
        .rows("col")
        .iter()
        .map(|r| r["category:0"].clone())
        .collect();
    assert_eq!(got, all[..3]);
    // Bottom 2 on the report, with the page's Top 3 off.
    let model = s.model();
    let mut settings = model.report.pages[0].settings.clone();
    settings.filters = vec![];
    let page = s.page.clone();
    s.commit(vec![
        Change::SetPage { page, settings },
        Change::SetReportFilters {
            filters: vec![top(2, true)],
        },
    ]);
    let got: Vec<String> = s
        .rows("col")
        .iter()
        .map(|r| r["category:0"].clone())
        .collect();
    assert_eq!(got, all[4..]);
}

#[test]
fn small_multiples_group_by_their_field() {
    let mut s = sample();
    let (category, region, sales) = (
        s.col("Product", "Category"),
        s.col("Store", "Region"),
        s.measure("Total Sales"),
    );
    s.add(
        "multi",
        "columnChart",
        vec![
            ("category", vec![category.clone()]),
            ("values", vec![sales.clone()]),
            ("smallMultiples", vec![region.clone()]),
        ],
        serde_json::json!({}),
    );
    let rows = s.rows("multi");
    let regions: std::collections::BTreeSet<&String> =
        rows.iter().map(|r| &r["smallMultiples:0"]).collect();
    assert!(regions.len() > 1, "{regions:?}");
    let expected = s
        .query(VisualQuery {
            groups: vec![category, region],
            values: vec![sales],
            ..Default::default()
        })
        .len();
    assert_eq!(rows.len(), expected);
    assert!(rows[0].contains_key("key:smallMultiples:0"));
}
