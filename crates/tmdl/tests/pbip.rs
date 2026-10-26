use charted_compiler::steps::{Query, Source, Step};
use charted_model::*;

fn col(id: &str, name: &str, ty: DataType) -> Column {
    Column {
        id: id.into(),
        name: name.into(),
        data_type: ty,
        kind: ColumnKind::Data {
            source_column: name.into(),
        },
        description: None,
        format_string: None,
        summarize_by: Summarize::Default,
        data_category: None,
        sort_by: None,
        display_folder: None,
        hidden: false,
        is_key: false,
        extras: Default::default(),
    }
}

#[test]
fn writes_a_power_bi_project() {
    let mut amount = col("c-amount", "Sales Amount", DataType::Decimal);
    amount.format_string = Some("\\$#,0.00".into());
    let mut month = col("c-month", "Month Name", DataType::String);
    month.sort_by = Some("c-mnum".into());
    let mut key = col("c-dk", "DateKey", DataType::Int64);
    key.is_key = true;
    let mut related = col("c-rel", "Month Of Sale", DataType::String);
    related.kind = ColumnKind::Calculated {
        expression: "RELATED ( 'Date'[Month Name] )".into(),
        language: ExpressionLanguage::Dax,
    };
    let model = Model {
        name: "Shop".into(),
        tables: vec![
            Table {
                id: "t-sales".into(),
                name: "Sales".into(),
                description: Some("One row per order line".into()),
                source: "sales".into(),
                columns: vec![col("c-sdk", "DateKey", DataType::Int64), amount, related],
                measures: vec![Measure {
                    id: "m-total".into(),
                    name: "Total Sales".into(),
                    expression: "SUM ( Sales[Sales Amount] )".into(),
                    language: ExpressionLanguage::Dax,
                    description: None,
                    format_string: Some("\\$#,0".into()),
                    display_folder: Some("Sales".into()),
                    hidden: false,
                    extras: Default::default(),
                }],
                hidden: false,
                is_date_table: false,
                extras: Default::default(),
            },
            Table {
                id: "t-date".into(),
                name: "Date".into(),
                description: None,
                source: "date".into(),
                columns: vec![key, month, col("c-mnum", "MonthNumber", DataType::Int64)],
                measures: vec![],
                hidden: false,
                is_date_table: true,
                extras: Default::default(),
            },
        ],
        relationships: vec![Relationship {
            id: "r-1".into(),
            from_table: "t-sales".into(),
            from_column: "c-sdk".into(),
            to_table: "t-date".into(),
            to_column: "c-dk".into(),
            from_cardinality: Cardinality::Many,
            to_cardinality: Cardinality::One,
            cross_filter: CrossFilter::Both,
            active: false,
            extras: Default::default(),
        }],
        ..Default::default()
    };
    let queries = vec![
        Query {
            id: "q1".into(),
            name: "Sales".into(),
            table: "sales".into(),
            source: Source::Parquet {
                path: "data/sales.parquet".into(),
            },
            steps: vec![Step::RenameColumns {
                renames: vec![("Amount".into(), "Sales Amount".into())],
            }],
            step_names: Default::default(),
            no_load: false,
        },
        Query {
            id: "q2".into(),
            name: "Date".into(),
            table: "date".into(),
            source: Source::Csv {
                path: "data/date.csv".into(),
                delimiter: None,
                header: true,
                encoding: None,
                decimal: None,
            },
            steps: vec![],
            step_names: Default::default(),
            no_load: false,
        },
    ];
    let files = charted_tmdl::pbip("Shop", &model, &queries, "/home/me/Shop");
    let all: String = files
        .iter()
        .map(|(p, c)| format!("=== {p}\n{c}\n"))
        .collect();
    insta::assert_snapshot!(all);
}

/// A model with one of every kind of report object, for the schema and round-trip tests.
fn reported() -> Model {
    let mut cat = col("c-cat", "Category", DataType::String);
    cat.sort_by = None;
    let mut model = Model {
        name: "Shop".into(),
        tables: vec![Table {
            id: "t-sales".into(),
            name: "Sales".into(),
            description: None,
            source: "sales".into(),
            columns: vec![
                cat,
                col("c-amt", "Amount", DataType::Double),
                col("c-day", "Day", DataType::Date),
                col("c-year", "Year", DataType::Int64),
            ],
            measures: vec![Measure {
                id: "m-total".into(),
                name: "Total".into(),
                expression: "SUM ( Sales[Amount] )".into(),
                language: ExpressionLanguage::Dax,
                description: None,
                format_string: None,
                display_folder: None,
                hidden: false,
                extras: Default::default(),
            }],
            hidden: false,
            is_date_table: false,
            extras: Default::default(),
        }],
        ..Default::default()
    };
    let c = |id: &str| Projection {
        field: FieldRef::Column {
            table: "t-sales".into(),
            column: id.into(),
        },
        aggregation: None,
        display_name: None,
    };
    let m = Projection {
        field: FieldRef::Measure {
            table: "t-sales".into(),
            measure: "m-total".into(),
        },
        aggregation: None,
        display_name: Some("Sales".into()),
    };
    let sum = Projection {
        aggregation: Some(Aggregation::Sum),
        ..c("c-amt")
    };
    let basic =
        |id: &str, field: Projection, values: Vec<Option<&str>>, exclude: bool| ReportFilter {
            id: id.into(),
            field,
            filter: FilterKind::Basic {
                values: values.into_iter().map(|v| v.map(str::to_string)).collect(),
                exclude,
            },
        };
    let types = [
        ("card", vec![("values", vec![m.clone()])]),
        (
            "columnChart",
            vec![
                ("category", vec![c("c-year"), c("c-cat")]),
                ("values", vec![m.clone(), sum.clone()]),
            ],
        ),
        (
            "barChart",
            vec![("category", vec![c("c-cat")]), ("values", vec![m.clone()])],
        ),
        (
            "stackedColumnChart",
            vec![
                ("category", vec![c("c-year")]),
                ("series", vec![c("c-cat")]),
                ("values", vec![m.clone()]),
            ],
        ),
        (
            "stackedBarChart",
            vec![
                ("category", vec![c("c-year")]),
                ("series", vec![c("c-cat")]),
                ("values", vec![m.clone()]),
            ],
        ),
        (
            "lineChart",
            vec![("category", vec![c("c-day")]), ("values", vec![m.clone()])],
        ),
        (
            "pieChart",
            vec![("legend", vec![c("c-cat")]), ("values", vec![m.clone()])],
        ),
        (
            "donutChart",
            vec![("legend", vec![c("c-cat")]), ("values", vec![m.clone()])],
        ),
        (
            "scatterChart",
            vec![
                ("details", vec![c("c-cat")]),
                ("x", vec![sum.clone()]),
                ("y", vec![m.clone()]),
            ],
        ),
        ("table", vec![("columns", vec![c("c-cat"), m.clone()])]),
        (
            "matrix",
            vec![
                ("rows", vec![c("c-cat")]),
                ("columns", vec![c("c-year")]),
                ("values", vec![m.clone()]),
            ],
        ),
        ("slicer", vec![("field", vec![c("c-cat")])]),
        (
            "hundredPercentStackedColumnChart",
            vec![
                ("category", vec![c("c-year")]),
                ("series", vec![c("c-cat")]),
                ("values", vec![m.clone()]),
            ],
        ),
        (
            "hundredPercentStackedBarChart",
            vec![
                ("category", vec![c("c-cat")]),
                ("values", vec![m.clone(), sum.clone()]),
            ],
        ),
        (
            "lineClusteredColumnComboChart",
            vec![
                ("category", vec![c("c-year")]),
                ("values", vec![sum.clone()]),
                ("lineValues", vec![m.clone()]),
            ],
        ),
        (
            "lineStackedColumnComboChart",
            vec![
                ("category", vec![c("c-year")]),
                ("series", vec![c("c-cat")]),
                ("values", vec![sum.clone()]),
                ("lineValues", vec![m.clone()]),
            ],
        ),
        (
            "waterfallChart",
            vec![
                ("category", vec![c("c-year")]),
                ("breakdown", vec![c("c-cat")]),
                ("values", vec![m.clone()]),
            ],
        ),
        (
            "funnel",
            vec![("category", vec![c("c-cat")]), ("values", vec![m.clone()])],
        ),
        (
            "treemap",
            vec![
                ("group", vec![c("c-cat")]),
                ("details", vec![c("c-year")]),
                ("values", vec![m.clone()]),
            ],
        ),
        (
            "gauge",
            vec![
                ("values", vec![m.clone()]),
                ("maxValue", vec![sum.clone()]),
                ("targetValue", vec![sum.clone()]),
            ],
        ),
        (
            "kpi",
            vec![
                ("indicator", vec![m.clone()]),
                ("trend", vec![c("c-year")]),
                ("goal", vec![sum.clone()]),
            ],
        ),
        (
            "multiRowCard",
            vec![("values", vec![m.clone(), sum.clone()])],
        ),
        (
            "columnChart",
            vec![
                ("category", vec![c("c-cat")]),
                ("values", vec![m.clone()]),
                ("smallMultiples", vec![c("c-year")]),
            ],
        ),
        ("textbox", vec![]),
        ("image", vec![]),
        ("shape", vec![]),
    ];
    let visuals: Vec<Visual> = types
        .iter()
        .enumerate()
        .map(|(i, (t, roles))| Visual {
            id: format!("v{i}"),
            visual_type: t.to_string(),
            position: Position {
                x: 10.0 * i as f64,
                y: 20.0,
                width: 300.0,
                height: 200.0,
                z: i as i32,
            },
            roles: roles
                .iter()
                .map(|(r, ps)| (r.to_string(), ps.clone()))
                .collect(),
            filters: if *t == "barChart" {
                vec![basic("f-v", c("c-cat"), vec![Some("Tents"), None], true)]
            } else {
                vec![]
            },
            sort: (*t == "barChart").then(|| VisualSort {
                role: "values".into(),
                index: 0,
                descending: true,
            }),
            objects: match *t {
                "textbox" => serde_json::from_value(
                    serde_json::json!({ "text": { "content": "Hello\nworld" } }),
                )
                .unwrap(),
                "card" => serde_json::from_value(
                    serde_json::json!({ "title": { "show": true, "text": "Sales, all time" } }),
                )
                .unwrap(),
                _ => Default::default(),
            },
            selection: (*t == "slicer").then(|| FilterKind::Basic {
                values: vec![Some("Tents".into())],
                exclude: false,
            }),
            drill: if *t == "columnChart" {
                vec![DrillStep::Into {
                    keys: vec![Some("2024".into())],
                }]
            } else {
                vec![]
            },
        })
        .collect();
    let mut overview = PageSettings::named("Overview");
    overview.filters = vec![
        basic("f-p", c("c-year"), vec![Some("2024"), Some("2025")], false),
        ReportFilter {
            id: "f-adv".into(),
            field: c("c-amt"),
            filter: FilterKind::Advanced {
                conditions: vec![Condition {
                    op: FilterOp::Greater,
                    value: Some("10".into()),
                    value2: None,
                }],
                or: false,
            },
        },
    ];
    let mut detail = PageSettings::named("Detail");
    detail.drillthrough = vec![c("c-cat")];
    detail.keep_all_filters = false;
    let mut tip = PageSettings::named("Tip");
    tip.tooltip = true;
    tip.tooltip_fields = vec![c("c-cat")];
    tip.width = 320;
    tip.height = 240;
    model.report = Report {
        pages: vec![
            Page {
                id: "p-overview".into(),
                settings: overview,
                visuals,
            },
            Page {
                id: "p-detail".into(),
                settings: detail,
                visuals: vec![],
            },
            Page {
                id: "p-tip".into(),
                settings: tip,
                visuals: vec![],
            },
        ],
        filters: vec![basic("f-r", c("c-day"), vec![Some("2024-03-01")], false)],
        diagram: Default::default(),
        theme: Some(ReportTheme {
            name: "Harbor".into(),
            json: serde_json::json!({ "name": "Harbor", "dataColors": ["#123456"] }),
        }),
        bookmarks: vec![Bookmark {
            id: "b-1".into(),
            name: "Only tents".into(),
            page: "p-overview".into(),
            report_filters: vec![],
            page_filters: vec![],
            visuals: Default::default(),
        }],
    };
    model
}

/// Every PBIR file validates against Microsoft's schema it names (vendored in
/// `schema/pbir`, references resolved there).
#[test]
fn the_report_matches_the_pbir_schemas() {
    use std::path::Path;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/pbir");
    const BASE: &str =
        "https://developer.microsoft.com/json-schemas/fabric/item/report/definition/";
    struct Local(std::path::PathBuf);
    impl jsonschema::Retrieve for Local {
        fn retrieve(
            &self,
            uri: &jsonschema::Uri<String>,
        ) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
            let rel = uri
                .as_str()
                .strip_prefix(BASE)
                .ok_or_else(|| format!("unexpected schema {uri}"))?;
            let rel = rel.replace("schema.embedded.json", "schema-embedded.json");
            Ok(serde_json::from_str(&load(&self.0.join(rel)))?)
        }
    }
    // The bookmark schema names a definition `DecomposedTree<…>`, which isn't a valid
    // URI fragment for the validator.
    fn load(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap().replace(
            "DecomposedTree<QueryExpressionContainer>",
            "DecomposedTree_QueryExpressionContainer",
        )
    }
    let files = charted_tmdl::pbip("Shop", &reported(), &[], "/data");
    let mut checked = 0;
    for (path, text) in files
        .iter()
        .filter(|(p, _)| p.starts_with("Shop.Report/definition/"))
    {
        let doc: serde_json::Value = serde_json::from_str(text).unwrap();
        let url = doc["$schema"]
            .as_str()
            .unwrap_or_else(|| panic!("{path} names no schema"));
        let rel = url.strip_prefix(BASE).unwrap();
        let schema: serde_json::Value = serde_json::from_str(&load(&root.join(rel))).unwrap();
        let v = jsonschema::options()
            .with_retriever(Local(root.clone()))
            .build(&schema)
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        let errors: Vec<String> = v
            .iter_errors(&doc)
            .map(|e| format!("{e} at {}", e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{path}:\n{}", errors.join("\n"));
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} files checked");
}

/// Exporting and importing again gives the same model and report.
#[test]
fn the_project_comes_back_from_its_export() {
    let model = reported();
    let queries = vec![Query {
        id: "q1".into(),
        name: "Sales".into(),
        table: "sales".into(),
        source: Source::Parquet {
            path: "data/sales.parquet".into(),
        },
        steps: vec![],
        step_names: Default::default(),
        no_load: false,
    }];
    let files = charted_tmdl::pbip("Shop", &model, &queries, "/data");
    let def: std::collections::BTreeMap<String, String> = files
        .iter()
        .filter_map(|(p, t)| {
            Some((
                p.strip_prefix("Shop.SemanticModel/definition/")?
                    .to_string(),
                t.clone(),
            ))
        })
        .collect();
    let mut back = charted_tmdl::read::from_tmdl(&def, "Shop");
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    assert_eq!(back.queries, queries);
    let report: std::collections::BTreeMap<String, String> = files
        .iter()
        .filter_map(|(p, t)| Some((p.strip_prefix("Shop.Report/")?.to_string(), t.clone())))
        .collect();
    let mut skipped = Vec::new();
    back.model.report = charted_tmdl::pbir::read_pbir(&back.model, &report, &mut skipped).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(
        serde_json::to_value(&back.model.tables).unwrap(),
        serde_json::to_value(&model.tables).unwrap()
    );
    assert_eq!(back.model.report, model.report);
    assert_eq!(back.model.name, model.name);
}

/// Power BI's own files (no charted annotations): the v1.2 visual types map to
/// charted's, with their roles.
#[test]
fn power_bi_visual_types_map_to_charted() {
    use serde_json::{Value, json};
    let model = reported();
    let field = |prop: &str| json!({ "Column": { "Expression": { "SourceRef": { "Entity": "Sales" } }, "Property": prop } });
    let measure = json!({ "Measure": { "Expression": { "SourceRef": { "Entity": "Sales" } }, "Property": "Total" } });
    let visual = |name: &str, ty: &str, state: Value| {
        (
            format!("definition/pages/p1/visuals/{name}/visual.json"),
            json!({ "name": name, "position": { "x": 0, "y": 0, "z": 0, "width": 300, "height": 200 }, "visual": { "visualType": ty, "query": { "queryState": state } } }).to_string(),
        )
    };
    let proj = |f: &Value| json!({ "projections": [{ "field": f, "queryRef": "x" }] });
    let mut files: std::collections::BTreeMap<String, String> = [
        ("definition/report.json".to_string(), "{}".to_string()),
        (
            "definition/pages/pages.json".to_string(),
            r#"{ "pageOrder": ["p1"] }"#.to_string(),
        ),
        (
            "definition/pages/p1/page.json".to_string(),
            r#"{ "name": "p1", "displayName": "One", "width": 1280, "height": 720 }"#.to_string(),
        ),
    ]
    .into_iter()
    .collect();
    files.extend([
        visual("a", "multiRowCard", json!({ "Values": proj(&measure) })),
        visual("b", "cardVisual", json!({ "Data": proj(&measure) })),
        visual(
            "c",
            "hundredPercentStackedColumnChart",
            json!({ "Category": proj(&field("Category")), "Y": proj(&measure) }),
        ),
        visual(
            "d",
            "lineClusteredColumnComboChart",
            json!({ "Category": proj(&field("Year")), "Y": proj(&measure), "Y2": proj(&measure) }),
        ),
        visual(
            "e",
            "gauge",
            json!({ "Y": proj(&measure), "TargetValue": proj(&measure) }),
        ),
        visual(
            "f",
            "kpi",
            json!({ "Indicator": proj(&measure), "TrendLine": proj(&field("Year")) }),
        ),
        visual(
            "g",
            "clusteredColumnChart",
            json!({ "Category": proj(&field("Category")), "Y": proj(&measure), "Rows": proj(&field("Year")) }),
        ),
        visual(
            "h",
            "waterfallChart",
            json!({ "Category": proj(&field("Year")), "Breakdown": proj(&field("Category")), "Y": proj(&measure) }),
        ),
        visual(
            "i",
            "treemap",
            json!({ "Group": proj(&field("Category")), "Values": proj(&measure) }),
        ),
        visual(
            "j",
            "funnel",
            json!({ "Category": proj(&field("Category")), "Y": proj(&measure) }),
        ),
    ]);
    let mut skipped = Vec::new();
    let report = charted_tmdl::pbir::read_pbir(&model, &files, &mut skipped).unwrap();
    assert!(skipped.is_empty(), "{skipped:?}");
    let mut got: Vec<(String, Vec<String>)> = report.pages[0]
        .visuals
        .iter()
        .map(|v| (v.visual_type.clone(), v.roles.keys().cloned().collect()))
        .collect();
    got.sort();
    let s = |t: &str, v: &[&str]| {
        (
            t.to_string(),
            v.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(
        got,
        vec![
            s("columnChart", &["category", "smallMultiples", "values"]),
            s("funnel", &["category", "values"]),
            s("gauge", &["targetValue", "values"]),
            s("hundredPercentStackedColumnChart", &["category", "values"]),
            s("kpi", &["indicator", "trend"]),
            s(
                "lineClusteredColumnComboChart",
                &["category", "lineValues", "values"]
            ),
            s("multiRowCard", &["values"]),
            s("multiRowCard", &["values"]),
            s("treemap", &["group", "values"]),
            s("waterfallChart", &["breakdown", "category", "values"]),
        ]
    );
}
