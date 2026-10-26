//! The query editor's core: every step type against a small CSV, then cleaning the
//! sample's messy store list, reading an Excel sheet, merging into Store and applying
//! the changes to the model.

use arrow_ipc::reader::StreamReader;
use charted_compiler::steps::*;
use charted_core::queryedit::{Analysis, QueryInfo, QueryProfile};
use charted_core::{App, DataRequest, Request};
use charted_model::{Condition, DataType, FilterOp};
use std::collections::BTreeMap;

fn rows(bytes: Vec<u8>) -> Vec<Vec<String>> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).unwrap())
            .collect();
        for r in 0..batch.num_rows() {
            out.push(fmts.iter().map(|f| f.value(r).to_string()).collect());
        }
    }
    out
}

struct Fx {
    app: App,
    dir: tempfile::TempDir,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"));
        app.handle(Request::OpenSample {
            dir: Some(dir.path().join("p").display().to_string()),
        })
        .unwrap();
        Self { app, dir }
    }

    fn data_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("p/data")
    }

    fn preview(&self, q: &Query, all: &[Query]) -> Vec<Vec<String>> {
        rows(
            self.app
                .data(DataRequest::QueryPreview {
                    query: q.clone(),
                    queries: all.to_vec(),
                    upto: q.steps.len(),
                    limit: 1000,
                })
                .unwrap_or_else(|e| panic!("{}: {}", q.name, e.message)),
        )
    }

    fn info(&self, q: &Query, all: &[Query]) -> QueryInfo {
        serde_json::from_slice(
            &self
                .app
                .data(DataRequest::QueryInfo {
                    query: q.clone(),
                    queries: all.to_vec(),
                    upto: q.steps.len(),
                })
                .unwrap(),
        )
        .unwrap()
    }

    fn analyze(&self, q: &Query, upto: usize, analysis: Analysis) -> serde_json::Value {
        serde_json::from_slice(
            &self
                .app
                .data(DataRequest::QueryAnalyze {
                    query: q.clone(),
                    queries: vec![],
                    upto,
                    analysis,
                })
                .unwrap(),
        )
        .unwrap()
    }
}

fn query(id: &str, name: &str, source: Source, steps: Vec<Step>) -> Query {
    Query {
        id: id.into(),
        name: name.into(),
        table: id.into(),
        source,
        steps,
        step_names: BTreeMap::new(),
        no_load: false,
    }
}

fn csv(path: &str, header: bool) -> Source {
    Source::Csv {
        path: path.into(),
        delimiter: None,
        header,
        encoding: None,
        decimal: None,
    }
}

fn cond(op: FilterOp, v: &str) -> Condition {
    Condition {
        op,
        value: Some(v.into()),
        value2: None,
    }
}

#[test]
fn every_step_runs_and_has_m() {
    let fx = Fx::new();
    std::fs::write(
        fx.data_dir().join("small.csv"),
        "name,city,qty,price,month\nAlpha,\"Oslo, Norway\",3,2.5,Jan\nbeta,\"Lima, Peru\",,4,\ngamma,\"Oslo, Norway\",5,1,Mar\nAlpha,\"Oslo, Norway\",3,2.5,Jan\n",
    )
    .unwrap();
    std::fs::write(fx.data_dir().join("more.csv"), "name,qty\ndelta,9\n").unwrap();
    let more = query("more", "More", csv("data/more.csv", true), vec![]);
    let cases: Vec<(Step, usize, &str)> = vec![
        (Step::RemoveTopRows { count: 1 }, 3, "beta"),
        (Step::KeepTopRows { count: 2 }, 2, "Alpha"),
        (
            Step::ChangeTypes {
                columns: vec![("qty".into(), DataType::Double)],
            },
            4,
            "Alpha",
        ),
        (
            Step::SelectColumns {
                columns: vec!["name".into()],
            },
            4,
            "Alpha",
        ),
        (
            Step::RemoveColumns {
                columns: vec!["city".into()],
            },
            4,
            "Alpha",
        ),
        (
            Step::RenameColumns {
                renames: vec![("name".into(), "Name".into())],
            },
            4,
            "Alpha",
        ),
        (
            Step::FilterRows {
                column: "city".into(),
                conditions: vec![cond(FilterOp::Contains, "Oslo")],
                or: false,
            },
            3,
            "Alpha",
        ),
        (Step::RemoveDuplicates { columns: vec![] }, 3, "Alpha"),
        (Step::RemoveBlankRows, 4, "Alpha"),
        (
            Step::ReplaceValues {
                columns: vec!["name".into()],
                find: "Alpha".into(),
                replace: "A".into(),
                whole: true,
            },
            4,
            "A",
        ),
        (
            Step::SplitColumn {
                column: "city".into(),
                delimiter: ", ".into(),
                mode: SplitMode::Left,
                names: vec!["City".into(), "Country".into()],
            },
            4,
            "Alpha",
        ),
        (
            Step::MergeColumns {
                columns: vec!["name".into(), "month".into()],
                separator: "-".into(),
                name: "Key".into(),
            },
            4,
            "Oslo, Norway",
        ),
        (
            Step::CustomColumn {
                name: "Total".into(),
                formula: "[qty] * [price]".into(),
            },
            4,
            "Alpha",
        ),
        (
            Step::ConditionalColumn {
                name: "Size".into(),
                rules: vec![Rule {
                    column: "qty".into(),
                    op: FilterOp::GreaterOrEqual,
                    value: Some("5".into()),
                    output: "Big".into(),
                }],
                otherwise: Some("Small".into()),
            },
            4,
            "Alpha",
        ),
        (
            Step::GroupBy {
                keys: vec!["city".into()],
                aggregates: vec![
                    Aggregate {
                        name: "Qty".into(),
                        op: AggOp::Sum,
                        column: Some("qty".into()),
                    },
                    Aggregate {
                        name: "Rows".into(),
                        op: AggOp::CountRows,
                        column: None,
                    },
                ],
            },
            2,
            "Lima, Peru",
        ),
        (
            Step::Unpivot {
                columns: vec!["qty".into(), "price".into()],
                attribute: "Attribute".into(),
                value: "Value".into(),
            },
            7,
            "Alpha",
        ),
        (
            Step::Pivot {
                column: "month".into(),
                values_column: "qty".into(),
                values: vec!["Jan".into(), "Mar".into()],
                aggregate: AggOp::Sum,
            },
            3,
            "Alpha",
        ),
        (
            Step::FillDown {
                columns: vec!["month".into()],
            },
            4,
            "Alpha",
        ),
        (
            Step::AddIndex {
                name: "Index".into(),
                start: 1,
                step: 1,
            },
            4,
            "Alpha",
        ),
        (
            Step::ExtractText {
                column: "city".into(),
                extract: Extract::BeforeDelimiter {
                    delimiter: ",".into(),
                },
                new_column: Some("Town".into()),
            },
            4,
            "Alpha",
        ),
        (
            Step::FormatText {
                columns: vec!["name".into()],
                format: TextOp::Upper,
            },
            4,
            "ALPHA",
        ),
        (
            Step::SortRows {
                columns: vec![("name".into(), true)],
            },
            4,
            "gamma",
        ),
        (
            Step::AppendQueries {
                queries: vec!["more".into()],
            },
            5,
            "Alpha",
        ),
    ];
    for (step, expect_rows, first) in cases {
        let name = step.default_name();
        let q = query("small", "Small", csv("data/small.csv", true), vec![step]);
        let all = vec![q.clone(), more.clone()];
        let r = fx.preview(&q, &all);
        assert_eq!(r.len(), expect_rows, "{name}: {r:?}");
        assert_eq!(r[0][0], first, "{name}: {r:?}");
        let m = to_m(&q, &all);
        assert!(m.contains(&format!("#\"{name}\" = ")), "{name}: {m}");
    }
    // A few results in detail.
    let split = fx.preview(
        &query(
            "small",
            "Small",
            csv("data/small.csv", true),
            vec![Step::SplitColumn {
                column: "city".into(),
                delimiter: ", ".into(),
                mode: SplitMode::Left,
                names: vec!["City".into(), "Country".into()],
            }],
        ),
        &[],
    );
    assert_eq!(split[1][4..], ["Lima".to_string(), "Peru".to_string()]);
    let filled = fx.preview(
        &query(
            "small",
            "Small",
            csv("data/small.csv", true),
            vec![Step::FillDown {
                columns: vec!["month".into()],
            }],
        ),
        &[],
    );
    assert_eq!(filled[1][4], "Jan", "filled from the row above: {filled:?}");
    let custom = fx.preview(
        &query(
            "small",
            "Small",
            csv("data/small.csv", true),
            vec![Step::CustomColumn {
                name: "Total".into(),
                formula: "[qty] * [price]".into(),
            }],
        ),
        &[],
    );
    assert_eq!(custom[0][5], "7.5");
    assert_eq!(custom[1][5], "", "blank times a number is blank");
}

#[test]
fn clean_the_store_list_merge_it_into_store_and_apply() {
    let mut fx = Fx::new();
    let state = fx.app.handle(Request::State).unwrap();
    let mut queries: Vec<Query> = state
        .project
        .unwrap()
        .queries
        .into_iter()
        .map(|q| q.definition)
        .collect();

    // Clean the messy list, step by step, resolving names as the editor does.
    let mut raw = query(
        "raw",
        "Store details",
        csv("data/stores_raw.csv", false),
        vec![Step::RemoveTopRows { count: 2 }],
    );
    let names: Vec<String> =
        serde_json::from_value(fx.analyze(&raw, 1, Analysis::FirstRow)).unwrap();
    assert_eq!(
        names,
        [
            "store code",
            "store name",
            "location",
            "manager",
            "format",
            "square feet"
        ]
    );
    raw.steps.push(Step::PromoteHeaders { names });
    raw.steps.push(Step::RemoveBlankRows);
    raw.steps.push(Step::RemoveDuplicates { columns: vec![] });
    raw.steps.push(Step::FormatText {
        columns: vec!["store name".into(), "format".into()],
        format: TextOp::Trim,
    });
    raw.steps.push(Step::FormatText {
        columns: vec!["store name".into(), "format".into()],
        format: TextOp::Capitalize,
    });
    let parts = fx.analyze(
        &raw,
        raw.steps.len(),
        Analysis::SplitParts {
            column: "location".into(),
            delimiter: ", ".into(),
        },
    );
    assert_eq!(parts, serde_json::json!(2));
    raw.steps.push(Step::SplitColumn {
        column: "location".into(),
        delimiter: ", ".into(),
        mode: SplitMode::Left,
        names: vec!["city".into(), "country".into()],
    });
    raw.steps.push(Step::ReplaceValues {
        columns: vec!["manager".into()],
        find: String::new(),
        replace: "(vacant)".into(),
        whole: true,
    });
    raw.steps.push(Step::ChangeTypes {
        columns: vec![("square feet".into(), DataType::Int64)],
    });
    raw.steps.push(Step::ConditionalColumn {
        name: "size band".into(),
        rules: vec![
            Rule {
                column: "square feet".into(),
                op: FilterOp::GreaterOrEqual,
                value: Some("24000".into()),
                output: "Large".into(),
            },
            Rule {
                column: "square feet".into(),
                op: FilterOp::GreaterOrEqual,
                value: Some("12000".into()),
                output: "Medium".into(),
            },
        ],
        otherwise: Some("Small".into()),
    });
    raw.no_load = true;
    let r = fx.preview(&raw, &[]);
    assert_eq!(r.len(), 40, "one row per physical store");
    let info = fx.info(&raw, &[]);
    let col = |n: &str| info.columns.iter().position(|c| c.name == n).unwrap();
    assert_eq!(info.columns[col("square feet")].data_type, DataType::Int64);
    assert_eq!(r[1][col("store name")], "Auckland Pine Ridge");
    assert_eq!(r[0][col("city")], "Lyon");
    assert!(r.iter().any(|x| x[col("manager")] == "(vacant)"));
    assert!(
        r.iter()
            .all(|x| ["Flagship", "Standard", "Outlet"].contains(&x[col("format")].as_str())),
        "{r:?}"
    );
    assert!(
        info.m.contains("Table.ReplaceValue(") && info.m.contains("Replacer.ReplaceValue"),
        "{}",
        info.m
    );
    assert!(info.step_names.contains(&"Formatted Text 2".to_string()));

    // The type change step's profile counts what didn't convert (none here).
    let profile: QueryProfile = serde_json::from_slice(
        &fx.app
            .data(DataRequest::QueryProfile {
                query: raw.clone(),
                queries: vec![],
                upto: 9,
                full: false,
            })
            .unwrap(),
    )
    .unwrap();
    let sq = profile
        .columns
        .iter()
        .find(|c| c.name == "square feet")
        .unwrap();
    assert_eq!((sq.errors, sq.count), (0, 40));
    let manager = profile
        .columns
        .iter()
        .find(|c| c.name == "manager")
        .unwrap();
    assert!(manager.distinct > 5 && !manager.top.is_empty());

    // Merge into Store, rename a region, and read targets from the workbook.
    let store = queries.iter_mut().find(|q| q.name == "Store").unwrap();
    store.steps.push(Step::MergeQueries {
        query: "raw".into(),
        left: vec!["StoreCode".into()],
        right: vec!["store code".into()],
        kind: JoinKind::LeftOuter,
        columns: vec!["manager".into(), "size band".into()],
        prefix: None,
    });
    store.steps.push(Step::RenameColumns {
        renames: vec![
            ("manager".into(), "Manager".into()),
            ("size band".into(), "Size Band".into()),
        ],
    });
    store.steps.push(Step::ReplaceValues {
        columns: vec!["Region".into()],
        find: "North America".into(),
        replace: "Americas".into(),
        whole: true,
    });
    let sheets: Vec<String> = serde_json::from_slice(
        &fx.app
            .data(DataRequest::ExcelSheets {
                path: "data/targets.xlsx".into(),
            })
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sheets, ["Targets 2024", "Targets 2025", "Notes"]);
    let excel = query(
        "xl",
        "Targets 2025",
        Source::Excel {
            path: "data/targets.xlsx".into(),
            sheet: "Targets 2025".into(),
            header: true,
        },
        vec![],
    );
    let xr = fx.preview(&excel, &[]);
    assert!(
        xr.len() > 300,
        "{} rows: {:?}",
        xr.len(),
        &xr[..xr.len().min(3)]
    );
    assert_eq!(
        xr[0][1], "2025-01-01",
        "dates come through as dates: {:?}",
        xr[0]
    );
    queries.push(raw);
    queries.push(excel);

    let before_undo = fx
        .app
        .handle(Request::State)
        .unwrap()
        .project
        .unwrap()
        .undo_label;
    assert!(before_undo.is_none());
    fx.app
        .handle(Request::ApplyQueries {
            queries: queries.clone(),
        })
        .unwrap();
    let p = fx.app.handle(Request::State).unwrap().project.unwrap();
    let st = p.model.table_by_name("Store").unwrap();
    assert!(st.column_by_name("Manager").is_some() && st.column_by_name("Size Band").is_some());
    let t25 = p.model.table_by_name("Targets 2025").unwrap();
    assert_eq!(
        t25.column_by_name("Month").unwrap().data_type,
        DataType::Date
    );
    assert!(
        p.model.table_by_name("Store details").is_none(),
        "a query that doesn't load makes no table"
    );
    assert!(
        p.undo_label.is_none() && p.dirty,
        "applying can't be undone, and needs saving"
    );
    let regions = fx
        .app
        .session()
        .unwrap()
        .database()
        .json_rows("SELECT DISTINCT Region FROM store ORDER BY 1", None)
        .unwrap();
    assert!(
        regions.iter().any(|r| r.contains("Americas"))
            && !regions.iter().any(|r| r.contains("North America"))
    );

    // Saved and reopened, the queries come back; changing a source shows it changed.
    fx.app.handle(Request::Save).unwrap();
    let path = p.path.clone();
    fx.app.handle(Request::CloseProject).unwrap();
    fx.app.handle(Request::OpenProject { path }).unwrap();
    let back: Vec<Query> = fx
        .app
        .handle(Request::State)
        .unwrap()
        .project
        .unwrap()
        .queries
        .into_iter()
        .map(|q| q.definition)
        .collect();
    assert_eq!(back.len(), queries.len());
    let status = |fx: &Fx| -> serde_json::Value {
        serde_json::from_slice(&fx.app.data(DataRequest::SourceStatus).unwrap()).unwrap()
    };
    assert!(
        status(&fx)
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["state"] == "current"),
        "{}",
        status(&fx)
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let raw_path = fx.data_dir().join("stores_raw.csv");
    let text = std::fs::read_to_string(&raw_path).unwrap();
    // Written again with the same bytes (as a copy does): still current.
    std::fs::write(&raw_path, &text).unwrap();
    assert!(
        status(&fx)
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["state"] == "current"),
        "{}",
        status(&fx)
    );
    // A same-size edit is only noticed once the modification time moves on (some
    // filesystems keep whole seconds).
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&raw_path, text.replace("Ben Okafor", "Bea Okafor")).unwrap();
    let s = status(&fx);
    let changed: Vec<&str> = s
        .as_array()
        .unwrap()
        .iter()
        .filter(|x| x["state"] == "changed")
        .map(|x| x["name"].as_str().unwrap())
        .collect();
    assert_eq!(changed, ["Store details"]);
    fx.app.handle(Request::Refresh).unwrap();
    assert!(
        status(&fx)
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["state"] == "current")
    );
    let managers = fx
        .app
        .session()
        .unwrap()
        .database()
        .json_rows(
            "SELECT count(*) AS n FROM store WHERE Manager = 'Bea Okafor'",
            None,
        )
        .unwrap();
    assert!(!managers[0].contains("\"n\":0"), "{managers:?}");

    // A merge with an unknown query, or a loop, is refused before anything changes.
    let mut bad = back.clone();
    let first = bad[0].id.clone();
    bad.iter_mut()
        .find(|q| q.id == "raw")
        .unwrap()
        .steps
        .push(Step::AppendQueries {
            queries: vec![first.clone()],
        });
    bad.iter_mut()
        .find(|q| q.id == first)
        .unwrap()
        .steps
        .push(Step::AppendQueries {
            queries: vec!["raw".into()],
        });
    assert!(
        fx.app
            .handle(Request::ApplyQueries { queries: bad })
            .is_err()
    );
}

#[test]
fn database_sources_ask_before_downloading() {
    let mut fx = Fx::new();
    let q = query(
        "lite",
        "Lite",
        Source::Sqlite {
            path: "data/shop.db".into(),
            table: "orders".into(),
        },
        vec![],
    );
    let e = fx
        .app
        .data(DataRequest::QueryPreview {
            query: q.clone(),
            queries: vec![],
            upto: 0,
            limit: 10,
        })
        .unwrap_err();
    // Either already installed on this machine (then the missing file is the problem)
    // or charted asks first.
    if e.code == "consent" {
        assert_eq!(e.detail.as_deref(), Some("sqlite"));
    }
    let consent = charted_core::sources::Consent::read(&fx.dir.path().join("state"));
    assert!(
        consent.extensions.is_empty(),
        "nothing is downloaded without asking"
    );
    let _ = &mut fx;
}

#[test]
fn the_date_table_wizard_makes_a_related_calendar() {
    let mut fx = Fx::new();
    let model = fx.app.session().unwrap().project.model.clone();
    let store = model.table_by_name("Store").unwrap();
    let open = store.column_by_name("OpenDate").unwrap();
    fx.app
        .handle(Request::CreateDateTable {
            name: "Calendar".into(),
            start: "2000-01-01".into(),
            end: "2025-12-31".into(),
            fiscal_start: 7,
            relate: vec![(store.id.clone(), open.id.clone())],
        })
        .unwrap();
    let s = fx.app.session().unwrap();
    let m = &s.project.model;
    let cal = m.table_by_name("Calendar").unwrap();
    assert!(cal.is_date_table);
    let names: Vec<&str> = cal.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "Date",
            "Year",
            "Quarter",
            "MonthNumber",
            "MonthName",
            "YearMonth",
            "Weekday",
            "WeekdayNumber",
            "FiscalYear"
        ]
    );
    assert_eq!(
        cal.column_by_name("Date").unwrap().data_type,
        DataType::Date
    );
    assert!(cal.column_by_name("Date").unwrap().is_key);
    let month = cal.column_by_name("MonthNumber").unwrap().id.clone();
    assert_eq!(
        cal.column_by_name("MonthName").unwrap().sort_by.as_ref(),
        Some(&month)
    );
    let rel = m
        .relationships
        .iter()
        .find(|r| r.to_table == cal.id)
        .unwrap();
    assert_eq!(
        (rel.from_column.as_str(), rel.active),
        (open.id.as_str(), true)
    );
    let got = rows(
        s.database()
            .arrow(&format!(
                "SELECT count(*), min(\"Date\"), max(FiscalYear), any_value(FiscalYear) FILTER (WHERE \"Date\" = DATE '2024-07-01') FROM {}",
                cal.source
            ))
            .unwrap(),
    );
    assert_eq!(got, [["9497", "2000-01-01", "FY2026", "FY2025"]]);
    let q = s
        .project
        .queries
        .iter()
        .find(|q| q.name == "Calendar")
        .unwrap();
    let m_text = charted_tmdl::m_query(q, Some(cal), &s.project.queries);
    assert!(m_text.contains("List.Dates(#date(2000, 1, 1)"), "{m_text}");
    // A name that's taken, and a column that isn't a date.
    let key = store.column_by_name("StoreKey").unwrap();
    let mut err = |name: &str, relate| {
        fx.app
            .handle(Request::CreateDateTable {
                name: name.into(),
                start: "2024-01-01".into(),
                end: "2024-12-31".into(),
                fiscal_start: 1,
                relate,
            })
            .unwrap_err()
            .message
    };
    assert!(err("Calendar", vec![]).contains("already a table"));
    assert!(err("Dates", vec![(store.id.clone(), key.id.clone())]).contains("isn't a date column"));
}

#[test]
fn a_new_project_starts_empty_and_takes_data() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    let parent = dir.path().join("docs");
    app.handle(Request::NewProject {
        name: "Budget 2026".into(),
        dir: Some(parent.display().to_string()),
    })
    .unwrap();
    let p = &app.session().unwrap().project;
    assert_eq!(p.dir, parent.join("Budget 2026"));
    assert!(p.manifest_path.exists());
    assert!(p.model.tables.is_empty());
    assert_eq!(p.model.report.pages.len(), 1);
    // The same name again, and a bad one, are refused.
    let err = |app: &mut App, name: &str| {
        app.handle(Request::NewProject {
            name: name.into(),
            dir: Some(parent.display().to_string()),
        })
        .unwrap_err()
        .message
    };
    assert!(err(&mut app, "Budget 2026").contains("already a folder"));
    assert!(err(&mut app, "a/b").contains("can't be a folder name"));
    assert!(err(&mut app, "  ").contains("needs a name"));
    // Get data: a CSV through the query editor's Apply.
    let file = dir.path().join("sales.csv");
    std::fs::write(&file, "Region,Amount\nNorth,10\nSouth,32\n").unwrap();
    app.handle(Request::OpenProject {
        path: parent.join("Budget 2026").display().to_string(),
    })
    .unwrap();
    let q = query("q1", "Sales", csv(file.to_str().unwrap(), true), vec![]);
    app.handle(Request::ApplyQueries { queries: vec![q] })
        .unwrap();
    let m = &app.session().unwrap().project.model;
    assert_eq!(m.tables.len(), 1);
    assert_eq!(m.tables[0].columns.len(), 2);
}

#[test]
fn references_and_entered_tables_load_and_follow_their_sources() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::NewProject {
        name: "Refs".into(),
        dir: Some(dir.path().display().to_string()),
    })
    .unwrap();
    app.handle(Request::OpenProject {
        path: dir.path().join("Refs").display().to_string(),
    })
    .unwrap();
    let file = dir.path().join("sales.csv");
    std::fs::write(&file, "Region,Amount\nNorth,10\nSouth,32\nNorth,5\n").unwrap();
    let mut sales = query("sales", "Sales", csv(file.to_str().unwrap(), true), vec![]);
    let big = query(
        "big",
        "Big sales",
        Source::Reference {
            query: "sales".into(),
        },
        vec![Step::FilterRows {
            column: "Amount".into(),
            conditions: vec![cond(FilterOp::Greater, "6")],
            or: false,
        }],
    );
    let targets = query(
        "targets",
        "Targets",
        Source::Table {
            columns: vec![
                ("Region".into(), DataType::String),
                ("Target".into(), DataType::Int64),
                ("Share".into(), DataType::Double),
            ],
            rows: vec![
                vec![Some("North".into()), Some("12".into()), Some("0.5".into())],
                vec![Some("South".into()), Some("2.5".into()), None],
            ],
        },
        vec![],
    );
    app.handle(Request::ApplyQueries {
        queries: vec![sales.clone(), big.clone(), targets.clone()],
    })
    .unwrap();
    let get = |app: &App, sql: &str| rows(app.session().unwrap().database().arrow(sql).unwrap());
    assert_eq!(get(&app, "SELECT count(*) FROM big"), [["2"]]);
    assert_eq!(
        get(&app, "SELECT * FROM targets"),
        [["North", "12", "0.5"], ["South", "", ""]]
    );
    let m = &app.session().unwrap().project.model;
    let t = m.table_by_name("Targets").unwrap();
    assert_eq!(
        t.columns.iter().map(|c| c.data_type).collect::<Vec<_>>(),
        [DataType::String, DataType::Int64, DataType::Double]
    );
    // Changing Sales reloads the query that references it.
    sales.steps.push(Step::FilterRows {
        column: "Region".into(),
        conditions: vec![cond(FilterOp::Equals, "North")],
        or: false,
    });
    app.handle(Request::ApplyQueries {
        queries: vec![sales, big, targets],
    })
    .unwrap();
    assert_eq!(get(&app, "SELECT count(*) FROM big"), [["1"]]);
}

/// A calendar written in M (as in public Power BI projects), imported and loaded: its
/// columns hold what Power Query gives.
#[test]
fn an_imported_m_calendar_loads_as_power_query_makes_it() {
    let m = r#"let
    StartDate = #date(2024, 1, 1),
    EndDate = #date(2024, 12, 31),
    DateList = List.Dates(StartDate, Duration.Days(EndDate - StartDate) + 1, #duration(1, 0, 0, 0)),
    Calendar = Table.FromList(DateList, Splitter.SplitByNothing(), {"Date"}),
    AddYear = Table.AddColumn(Calendar, "Year", each Date.Year([Date])),
    AddMonthName = Table.AddColumn(AddYear, "Month Name", each Date.ToText([Date], "MMM")),
    AddYearMonth = Table.AddColumn(AddMonthName, "Year-Month", each Text.From(Date.Year([Date])) & " " & Date.ToText([Date], "MMM")),
    AddQuarter = Table.AddColumn(AddYearMonth, "Quarter", each "Q" & Text.From(Date.QuarterOfYear([Date]))),
    AddWeek = Table.AddColumn(AddQuarter, "Week Number", each Date.WeekOfYear([Date])),
    AddDayOfWeek = Table.AddColumn(AddWeek, "Day of Week", each Date.DayOfWeek([Date]) + 1),
    AddIsoDay = Table.AddColumn(AddDayOfWeek, "ISO Day", each Date.DayOfWeek([Date], Day.Monday) + 1),
    AddDayName = Table.AddColumn(AddIsoDay, "Day Name", each Date.ToText([Date], "dddd")),
    AddIsWeekend = Table.AddColumn(AddDayName, "Is Weekend", each if Date.DayOfWeek([Date]) >= 5 then true else false),
    AddFiscalYear = Table.AddColumn(AddIsWeekend, "Fiscal Year", each if Date.Month([Date]) >= 7 then Date.Year([Date]) + 1 else Date.Year([Date])),
    AddFiscalQuarter = Table.AddColumn(AddFiscalYear, "Fiscal Quarter", each "FQ" & Text.From(Number.IntegerDivide((Date.Month([Date]) + 5), 3))),
    #"Changed Type" = Table.TransformColumnTypes(AddFiscalQuarter, {{"Date", type date}, {"Year", Int64.Type}, {"Is Weekend", type logical}})
in
    #"Changed Type""#;
    let (source, steps) = charted_tmdl::mquery::to_query(m, &BTreeMap::new()).unwrap();
    let fx = Fx::new();
    let q = query("cal", "Calendar", source, steps);
    let r = fx.preview(&q, &[]);
    assert_eq!(r.len(), 366);
    // Monday 1 January, Sunday 7 January (a new week), Saturday 6 July.
    assert_eq!(
        r[0],
        [
            "2024-01-01",
            "2024",
            "Jan",
            "2024 Jan",
            "Q1",
            "1",
            "2",
            "1",
            "Monday",
            "false",
            "2024",
            "FQ2"
        ]
    );
    assert_eq!(r[6][4..7], ["Q1", "2", "1"]);
    assert_eq!(
        r[187],
        [
            "2024-07-06",
            "2024",
            "Jul",
            "2024 Jul",
            "Q3",
            "27",
            "7",
            "6",
            "Saturday",
            "true",
            "2025",
            "FQ4"
        ]
    );
}
