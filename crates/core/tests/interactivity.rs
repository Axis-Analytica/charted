//! Report interactivity on the Harbor & Pine sample: slicers and the page's
//! interactions, highlights, the matrix's subtotals, relative dates and cancelling a
//! superseded query.

use arrow_ipc::reader::StreamReader;
use charted_compiler::visual::{QueryFilter, VisualQuery};
use charted_core::api::{DataJob, Running};
use charted_core::{App, DataRequest, Request};
use charted_model::*;
use std::collections::HashMap;

type Row = HashMap<String, String>;

fn rows(bytes: Vec<u8>) -> Vec<Row> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let schema = batch.schema();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).unwrap())
            .collect();
        for r in 0..batch.num_rows() {
            out.push(
                schema
                    .fields()
                    .iter()
                    .zip(&fmts)
                    .map(|(f, x)| (f.name().clone(), x.value(r).to_string()))
                    .collect(),
            );
        }
    }
    out
}

fn num(r: &Row, k: &str) -> f64 {
    r.get(k)
        .unwrap_or_else(|| panic!("no column {k} in {r:?}"))
        .parse()
        .unwrap_or(f64::NAN)
}

struct Sample {
    app: App,
    page: Id,
    /// The roles the report is viewed as.
    viewing: Vec<Id>,
    _dir: tempfile::TempDir,
}

impl Sample {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"));
        app.handle(Request::OpenSample {
            dir: Some(dir.path().join("p").display().to_string()),
        })
        .unwrap();
        let page = app.session().unwrap().project.model.report.pages[0]
            .id
            .clone();
        Self {
            app,
            page,
            viewing: Vec::new(),
            _dir: dir,
        }
    }

    fn model(&self) -> &Model {
        &self.app.session().unwrap().project.model
    }

    fn col(&self, table: &str, column: &str) -> Projection {
        let t = self.model().table_by_name(table).unwrap();
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
        let (t, m) = self
            .model()
            .tables
            .iter()
            .find_map(|t| t.measures.iter().find(|x| x.name == name).map(|x| (t, x)))
            .unwrap();
        Projection {
            field: FieldRef::Measure {
                table: t.id.clone(),
                measure: m.id.clone(),
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
            .unwrap_or_else(|e| panic!("{}", e.message));
    }

    fn add(&mut self, id: &str, kind: &str, roles: Vec<(&str, Vec<Projection>)>) {
        let visual = Visual {
            id: id.into(),
            visual_type: kind.into(),
            position: Position {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 200.0,
                z: 0,
            },
            roles: roles.into_iter().map(|(r, p)| (r.to_string(), p)).collect(),
            filters: vec![],
            sort: None,
            objects: Default::default(),
            selection: None,
            drill: Vec::new(),
            hidden: false,
        };
        let page = self.page.clone();
        self.commit(vec![Change::AddVisual {
            page,
            visual,
            index: None,
        }]);
    }

    fn update(&mut self, id: &str, f: impl FnOnce(&mut Visual)) {
        let mut v = self.model().report.visual(id).unwrap().1.clone();
        f(&mut v);
        let page = self.page.clone();
        self.commit(vec![Change::UpdateVisual { page, visual: v }]);
    }

    fn data(&self, id: &str, filters: Vec<QueryFilter>, highlight: Vec<QueryFilter>) -> Vec<Row> {
        self.search(id, filters, highlight, None)
    }

    fn search(
        &self,
        id: &str,
        filters: Vec<QueryFilter>,
        highlight: Vec<QueryFilter>,
        search: Option<&str>,
    ) -> Vec<Row> {
        rows(
            self.app
                .data(DataRequest::ReportVisual {
                    visual: id.into(),
                    totals: false,
                    filters,
                    highlight,
                    tag: None,
                    search: search.map(String::from),
                    roles: self.viewing.clone(),
                })
                .unwrap_or_else(|e| panic!("{}", e.message)),
        )
    }

    fn total(&self, filters: Vec<QueryFilter>) -> f64 {
        let r = rows(
            self.app
                .data(DataRequest::Visual {
                    query: VisualQuery {
                        values: vec![self.measure("Total Sales")],
                        filters,
                        ..Default::default()
                    },
                })
                .unwrap(),
        );
        num(&r[0], "v0")
    }
}

fn equals(field: Projection, value: &str) -> QueryFilter {
    QueryFilter {
        field,
        filter: FilterKind::Basic {
            values: vec![Some(value.into())],
            exclude: false,
        },
    }
}

#[test]
fn slicers_filter_the_page_unless_the_interaction_is_off() {
    let mut s = Sample::new();
    let year = s.col("Date", "Year");
    let category = s.col("Product", "Category");
    let sales = s.measure("Total Sales");
    s.add(
        "chart",
        "columnChart",
        vec![
            ("category", vec![category.clone()]),
            ("values", vec![sales.clone()]),
        ],
    );
    s.add("slicer", "slicer", vec![("field", vec![year.clone()])]);

    let slicer = s.data("slicer", vec![], vec![]);
    let years: Vec<&str> = slicer.iter().map(|r| r["key:field:0"].as_str()).collect();
    assert_eq!(years, ["2022", "2023", "2024", "2025"]);

    let all: f64 = s
        .data("chart", vec![], vec![])
        .iter()
        .map(|r| num(r, "values:0"))
        .sum();
    s.update("slicer", |v| {
        v.selection = Some(FilterKind::Basic {
            values: vec![Some("2024".into())],
            exclude: false,
        })
    });
    let chart = s.data("chart", vec![], vec![]);
    let sliced: f64 = chart.iter().map(|r| num(r, "values:0")).sum();
    let expected = s.total(vec![equals(year.clone(), "2024")]);
    assert!((sliced - expected).abs() < 0.01, "{sliced} vs {expected}");
    assert!(sliced < all);
    // Keys are the group values as filters compare them.
    assert!(chart.iter().all(|r| r["key:category:0"] == r["category:0"]));
    // The slicer still lists every year (it doesn't filter itself).
    assert_eq!(s.data("slicer", vec![], vec![]).len(), 4);

    let page = s.page.clone();
    let mut settings = s.model().report.page(&page).unwrap().settings.clone();
    settings.interactions.push(Interaction {
        source: "slicer".into(),
        target: "chart".into(),
        kind: InteractionKind::None,
    });
    s.commit(vec![Change::SetPage {
        page: page.clone(),
        settings,
    }]);
    let unsliced: f64 = s
        .data("chart", vec![], vec![])
        .iter()
        .map(|r| num(r, "values:0"))
        .sum();
    assert!((unsliced - all).abs() < 0.01);
    let p = s.model().report.page(&page).unwrap();
    assert_eq!(p.interaction("slicer", "chart"), InteractionKind::None);
    assert_eq!(p.interaction("chart", "slicer"), InteractionKind::Filter);

    // A range slicer reports its field's smallest and largest values, under the
    // other slicers.
    s.add(
        "range",
        "slicer",
        vec![("field", vec![s.col("Date", "Date")])],
    );
    s.update("range", |v| {
        v.objects
            .insert("slicer".into(), serde_json::json!({ "style": "between" }));
    });
    let r = s.data("range", vec![], vec![]);
    assert_eq!(r.len(), 1);
    assert!(r[0]["min"].starts_with("2024-01-01"), "{r:?}");
    assert!(r[0]["max"].starts_with("2024-12-31"), "{r:?}");
}

#[test]
fn a_slicer_search_finds_values_past_the_ones_listed() {
    let mut s = Sample::new();
    s.add(
        "slicer",
        "slicer",
        vec![("field", vec![s.col("Customer", "CustomerKey")])],
    );
    // 20,000 customers; the slicer lists 5,000 of them.
    let listed = s.data("slicer", vec![], vec![]);
    assert_eq!(listed.len(), 5000);
    let keys: Vec<&str> = listed.iter().map(|r| r["key:field:0"].as_str()).collect();
    // The search runs before the limit, so it finds one that isn't listed.
    let missing = (1..=20_000)
        .map(|k| k.to_string())
        .find(|k| !keys.contains(&k.as_str()))
        .unwrap();
    let found = s.search("slicer", vec![], vec![], Some(&format!(" {missing} ")));
    assert!(
        found.iter().any(|r| r["key:field:0"] == missing),
        "{missing} not in {found:?}"
    );
    assert!(found.iter().all(|r| r["key:field:0"].contains(&missing)));
    // Quotes are text, not SQL.
    assert!(
        s.search("slicer", vec![], vec![], Some("1' OR '1'='1"))
            .is_empty()
    );
}

#[test]
fn highlights_are_computed_alongside_the_full_values() {
    let mut s = Sample::new();
    let category = s.col("Product", "Category");
    let region = s.col("Store", "Region");
    let sales = s.measure("Total Sales");
    s.add(
        "chart",
        "columnChart",
        vec![
            ("category", vec![category.clone()]),
            ("values", vec![sales.clone()]),
        ],
    );

    // Highlighting by a region: every category keeps its full value and gains a part.
    let r = s.data(
        "chart",
        vec![],
        vec![equals(region.clone(), "North America")],
    );
    assert_eq!(r.len(), 6);
    let full: Vec<f64> = r.iter().map(|x| num(x, "values:0")).collect();
    assert!(
        full.windows(2).all(|w| w[0] >= w[1]),
        "order kept: {full:?}"
    );
    for x in &r {
        let part = num(x, "hl:values:0");
        assert!(part > 0.0 && part < num(x, "values:0"), "{x:?}");
    }
    let parts: f64 = r.iter().map(|x| num(x, "hl:values:0")).sum();
    let west = s.total(vec![equals(region.clone(), "North America")]);
    assert!((parts - west).abs() < 0.01);

    // Highlighting one of its own categories leaves the others blank.
    let first = r[0]["key:category:0"].clone();
    let r = s.data("chart", vec![], vec![equals(category.clone(), &first)]);
    for x in &r {
        if x["key:category:0"] == first {
            assert_eq!(x["hl:values:0"], x["values:0"]);
        } else {
            assert_eq!(x["hl:values:0"], "");
        }
    }
    // A filter instead keeps only the matching rows.
    let r = s.data("chart", vec![equals(category, &first)], vec![]);
    assert_eq!(r.len(), 1);
}

#[test]
fn matrix_returns_every_level_with_recomputed_subtotals() {
    let mut s = Sample::new();
    let category = s.col("Product", "Category");
    let subcategory = s.col("Product", "Subcategory");
    let year = s.col("Date", "Year");
    let sales = s.measure("Total Sales");
    s.add(
        "m",
        "matrix",
        vec![
            ("rows", vec![category.clone(), subcategory.clone()]),
            ("columns", vec![year.clone()]),
            ("values", vec![sales.clone()]),
        ],
    );
    let r = s.data("m", vec![], vec![]);
    let level = |a: &str, b: &str| -> Vec<&Row> {
        r.iter()
            .filter(|x| x["level:rows"] == a && x["level:columns"] == b)
            .collect()
    };
    let grand = level("0", "0");
    assert_eq!(grand.len(), 1);
    let total = s.total(vec![]);
    assert!((num(grand[0], "values:0") - total).abs() < 0.01);
    assert_eq!(level("0", "1").len(), 4, "a total per year");
    assert_eq!(level("1", "0").len(), 6, "a subtotal per category");
    let sum: f64 = level("1", "0").iter().map(|x| num(x, "values:0")).sum();
    assert!((sum - total).abs() < 0.01);
    let leaves = level("2", "1");
    assert!(leaves.len() > 20);
    assert!(
        leaves
            .iter()
            .all(|x| !x["rows:1"].is_empty() && !x["columns:0"].is_empty())
    );
    // Each level is in order: categories ascend.
    let cats: Vec<&str> = level("1", "0")
        .iter()
        .map(|x| x["rows:0"].as_str())
        .collect();
    let mut sorted = cats.clone();
    sorted.sort();
    assert_eq!(cats, sorted);
    let order: Vec<f64> = level("1", "0").iter().map(|x| num(x, "order")).collect();
    assert_eq!(order, (1..=6).map(f64::from).collect::<Vec<_>>());
}

#[test]
fn relative_dates_are_relative_to_today() {
    let s = Sample::new();
    let date = s.col("Date", "Date");
    let rel = |period, count, unit| QueryFilter {
        field: date.clone(),
        filter: FilterKind::RelativeDate {
            period,
            count,
            unit,
        },
    };
    let all = s.total(vec![]);
    // The sample covers 2022–2025.
    let decade = s.total(vec![rel(RelativePeriod::Last, 10, DateUnit::Years)]);
    assert!((decade - all).abs() < 0.01);
    let next = s.total(vec![rel(RelativePeriod::Next, 3, DateUnit::Months)]);
    assert!(next.is_nan(), "nothing in the future: {next}");

    // The ranges themselves, checked in DuckDB.
    let db = s.app.session().unwrap().database();
    let check = |period, count, unit, expect: &str| {
        let (lo, hi) = charted_compiler::visual::relative_range(period, count, unit);
        let got: Vec<String> = db
            .json_rows(
                &format!("SELECT ({hi}) - ({lo}) AS days, ({expect}) AS ok"),
                None,
            )
            .unwrap();
        assert!(
            got[0].contains("\"ok\":true"),
            "{period:?} {count} {unit:?}: {got:?}"
        );
    };
    check(
        RelativePeriod::Last,
        7,
        DateUnit::Days,
        &format!(
            "({}) - ({}) = 7",
            "CAST(current_date + 1 AS DATE)", "CAST(current_date - 6 AS DATE)"
        ),
    );
    check(
        RelativePeriod::This,
        1,
        DateUnit::Months,
        "day(CAST(date_trunc('month', current_date) AS DATE)) = 1",
    );
    check(RelativePeriod::Last, 1, DateUnit::Years, "TRUE");
}

#[test]
fn a_newer_tagged_query_cancels_the_running_one() {
    let s = Sample::new();
    let db = s.app.session().unwrap().database().clone();
    let running = Running::default();
    let slow = "SELECT count(*) FROM range(20000000000) AS a(x) WHERE x % 7 = 3";
    let first = {
        let (db, running) = (db.clone(), running.clone());
        std::thread::spawn(move || DataJob::Tagged(db, slow.into(), running, "v1".into()).run())
    };
    std::thread::sleep(std::time::Duration::from_millis(400));
    let second = DataJob::Tagged(db, "SELECT 42 AS x".into(), running, "v1".into()).run();
    assert!(second.is_ok());
    let first = first.join().unwrap();
    assert_eq!(first.err().map(|e| e.code), Some("cancelled".into()));
}

#[test]
fn themes_import_and_visual_data_exports_as_csv() {
    let mut s = Sample::new();
    let dir = tempfile::tempdir().unwrap();
    let theme = dir.path().join("tidal.json");
    std::fs::write(
        &theme,
        r##"{"name":"Tidal","dataColors":["#094782","#0B72D7"],"background":"#FFFFFF","foreground":"#252423"}"##,
    )
    .unwrap();
    s.app
        .handle(Request::ImportTheme {
            path: theme.display().to_string(),
        })
        .unwrap();
    assert_eq!(s.model().report.theme.as_ref().unwrap().name, "Tidal");
    s.app.handle(Request::Undo).unwrap();
    assert!(s.model().report.theme.is_none());
    std::fs::write(&theme, "{}").unwrap();
    assert!(
        s.app
            .handle(Request::ImportTheme {
                path: theme.display().to_string()
            })
            .is_err()
    );

    let category = s.col("Product", "Category");
    let sales = s.measure("Total Sales");
    s.add(
        "chart",
        "columnChart",
        vec![
            ("category", vec![category.clone()]),
            ("values", vec![sales]),
        ],
    );
    let csv = dir.path().join("out.csv");
    s.app
        .handle(Request::ExportVisualCsv {
            visual: "chart".into(),
            path: csv.display().to_string(),
            filters: vec![equals(category, "Camping")],
            roles: vec![],
        })
        .unwrap();
    let text = std::fs::read_to_string(&csv).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Category,Total Sales");
    assert_eq!(lines.len(), 2);
    assert!(lines[1].starts_with("Camping,"));
}

#[test]
fn drilling_a_hierarchy_shows_one_level_filtered_to_its_members() {
    let mut s = Sample::new();
    let (year, quarter, month) = (
        s.col("Date", "Year"),
        s.col("Date", "Quarter"),
        s.col("Date", "MonthName"),
    );
    s.add(
        "drill",
        "columnChart",
        vec![
            (
                "category",
                vec![year.clone(), quarter.clone(), month.clone()],
            ),
            ("values", vec![s.measure("Total Sales")]),
        ],
    );
    let cats = |rows: &[Row], k: &str| rows.iter().map(|r| r[k].clone()).collect::<Vec<_>>();
    // The top level only.
    let top = s.data("drill", vec![], vec![]);
    assert_eq!(cats(&top, "category:0"), ["2022", "2023", "2024", "2025"]);
    assert!(!top[0].contains_key("category:1"));
    // Into 2024: its quarters, adding up to the year.
    s.update("drill", |v| {
        v.drill = vec![DrillStep::Into {
            keys: vec![Some("2024".into())],
        }]
    });
    let q = s.data("drill", vec![], vec![]);
    let mut quarters = cats(&q, "category:1");
    quarters.sort();
    assert_eq!(quarters, ["Q1", "Q2", "Q3", "Q4"]);
    let sum: f64 = q.iter().map(|r| num(r, "values:0")).sum();
    let y2024 = top.iter().find(|r| r["category:0"] == "2024").unwrap();
    assert!((sum - num(y2024, "values:0")).abs() < 0.01);
    // Next level for everything: quarters of every year (summed across years).
    s.update("drill", |v| v.drill = vec![DrillStep::Next]);
    assert_eq!(s.data("drill", vec![], vec![]).len(), 4);
    // Expand all down: year and quarter together.
    s.update("drill", |v| v.drill = vec![DrillStep::Expand]);
    let e = s.data("drill", vec![], vec![]);
    assert_eq!(e.len(), 16);
    assert!(e[0].contains_key("category:0") && e[0].contains_key("key:category:1"));
    // Then into "2025 Q3": its months.
    s.update("drill", |v| {
        v.drill.push(DrillStep::Into {
            keys: vec![Some("2025".into()), Some("Q3".into())],
        })
    });
    assert_eq!(
        cats(&s.data("drill", vec![], vec![]), "category:2"),
        ["July", "August", "September"]
    );
}

fn role(id: &str, filters: Vec<(&Id, &str)>) -> Role {
    Role {
        id: id.into(),
        name: id.into(),
        description: None,
        filters: filters
            .into_iter()
            .map(|(t, e)| RoleFilter {
                table: t.clone(),
                expression: e.into(),
            })
            .collect(),
        members: vec![],
        extras: Default::default(),
    }
}

/// View as: the roles' filters restrict every query a report visual makes (cards,
/// slicers, totals, filter cards, exports), and several roles combine as a union.
#[test]
fn viewing_as_roles_restricts_every_report_query() {
    let mut s = Sample::new();
    let region = s.col("Store", "Region");
    let category = s.col("Product", "Category");
    let sales = s.measure("Total Sales");
    let store = s.model().table_by_name("Store").unwrap().id.clone();
    let product = s.model().table_by_name("Product").unwrap().id.clone();
    s.commit(vec![
        Change::AddRole {
            role: role("north", vec![(&store, "Store[Region] = \"North America\"")]),
            index: None,
        },
        Change::AddRole {
            role: role("europe", vec![(&store, "[Region] = \"Europe\"")]),
            index: None,
        },
        Change::AddRole {
            role: role(
                "camping",
                vec![(&product, "Product[Category] = \"Camping\"")],
            ),
            index: None,
        },
    ]);
    s.add("card", "card", vec![("values", vec![sales.clone()])]);
    s.add("slicer", "slicer", vec![("field", vec![region.clone()])]);
    s.add(
        "table",
        "table",
        vec![("values", vec![category.clone(), sales.clone()])],
    );
    let card = |s: &Sample| num(&s.data("card", vec![], vec![])[0], "values:0");
    let everything = card(&s);
    let na = s.total(vec![equals(region.clone(), "North America")]);
    let eu = s.total(vec![equals(region.clone(), "Europe")]);
    assert!(na > 0.0 && na < everything);

    s.viewing = vec!["north".into()];
    assert!((card(&s) - na).abs() < 0.01);
    let listed: Vec<String> = s
        .data("slicer", vec![], vec![])
        .iter()
        .map(|r| r["key:field:0"].clone())
        .collect();
    assert_eq!(listed, vec!["North America".to_string()]);
    let rows: f64 = s
        .data("table", vec![], vec![])
        .iter()
        .map(|r| num(r, "values:1"))
        .sum();
    assert!((rows - na).abs() < 0.01);

    // The union of two roles on the same table.
    s.viewing = vec!["north".into(), "europe".into()];
    assert!((card(&s) - (na + eu)).abs() < 0.01);
    // A role that leaves Store unrestricted makes the union unrestricted there.
    s.viewing = vec!["north".into(), "camping".into()];
    assert!((card(&s) - everything).abs() < 0.01);
    s.viewing = vec![];
    assert!((card(&s) - everything).abs() < 0.01);

    // Filter cards list what the role sees.
    let values: serde_json::Value = serde_json::from_slice(
        &s.app
            .data(DataRequest::FieldValues {
                field: region.clone(),
                filters: vec![],
                search: None,
                limit: 100,
                roles: vec!["europe".into()],
            })
            .unwrap(),
    )
    .unwrap();
    assert_eq!(values["total"], 1, "{values}");

    // Exports too.
    let csv = s._dir.path().join("roles.csv");
    s.app
        .handle(Request::ExportVisualCsv {
            visual: "table".into(),
            path: csv.display().to_string(),
            filters: vec![],
            roles: vec!["camping".into()],
        })
        .unwrap();
    let text = std::fs::read_to_string(&csv).unwrap();
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.contains("Camping"));

    // A filter must be a row condition.
    let err = s
        .app
        .handle(Request::Commit {
            label: "test".into(),
            changes: vec![Change::AddRole {
                role: role("bad", vec![(&store, "SUM ( Sales[Quantity] ) > 0")]),
                index: None,
            }],
        })
        .unwrap_err();
    assert!(
        err.message.contains("condition on each row"),
        "{}",
        err.message
    );
}

/// Field parameters: a visual using the parameter's column shows the fields picked in
/// it by a slicer (all of them, in order, when nothing is picked), and the snapshot
/// carries the expanded roles the UI renders.
#[test]
fn field_parameters_show_the_fields_picked_in_their_slicer() {
    let mut s = Sample::new();
    let category = s.col("Product", "Category");
    s.commit(vec![Change::AddTable {
        table: Table {
            id: "metric".into(),
            name: "Metric".into(),
            description: None,
            source: String::new(),
            columns: vec![],
            measures: vec![],
            hierarchies: vec![],
            hidden: false,
            is_date_table: false,
            calculated: Some(CalculatedTable {
                expression: "{ ( \"Cost\", NAMEOF ( Sales[Total Cost] ), 1 ), ( \"Units\", NAMEOF ( [Units] ), 0 ) }".into(),
                language: ExpressionLanguage::Dax,
            }),
            extras: Default::default(),
        },
        index: None,
    }]);
    let label = s.col("Metric", "Value1");
    s.add(
        "chart",
        "columnChart",
        vec![
            ("category", vec![category.clone()]),
            ("values", vec![label.clone()]),
        ],
    );
    s.add("pick", "slicer", vec![("field", vec![label.clone()])]);

    // Nothing picked: both measures, in their order (Units first).
    let names = |s: &Sample| -> Vec<String> {
        let (page, v) = s.model().report.visual("chart").unwrap();
        charted_core::fieldparams::expand(s.model(), page, v).unwrap()["values"]
            .iter()
            .map(|p| charted_core::visualq::projection_name(s.model(), p))
            .collect()
    };
    assert_eq!(names(&s), ["Units", "Total Cost"]);
    let rows = s.data("chart", vec![], vec![]);
    assert!(rows.len() > 1);
    assert!(
        rows.iter()
            .all(|r| r.contains_key("values:0") && r.contains_key("values:1"))
    );

    // The slicer lists the labels; picking Cost leaves one value, the cost.
    let listed: Vec<String> = s
        .data("pick", vec![], vec![])
        .iter()
        .map(|r| r["key:field:0"].clone())
        .collect();
    assert_eq!(listed.len(), 2, "{listed:?}");
    s.update("pick", |v| {
        v.selection = Some(FilterKind::Basic {
            values: vec![Some("Cost".into())],
            exclude: false,
        })
    });
    assert_eq!(names(&s), ["Total Cost"]);
    let rows = s.data("chart", vec![], vec![]);
    assert!(rows.iter().all(|r| !r.contains_key("values:1")));
    let cost: f64 = rows.iter().map(|r| num(r, "values:0")).sum();
    assert!(cost > 0.0);

    // The snapshot has the expanded roles, for the chart but not the slicer.
    let snapshot = s.app.session().unwrap().snapshot().unwrap();
    assert_eq!(
        snapshot.expanded_roles.keys().collect::<Vec<_>>(),
        ["chart"]
    );
}
