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

/// A calculated table with a calculated column of its own.
fn top_table() -> Table {
    Table {
        id: "t-top".into(),
        name: "Top Days".into(),
        description: None,
        source: calculated_table_source("t-top"),
        columns: vec![
            Column {
                description: Some("Best days".into()),
                ..col("c-top-day", "Day", DataType::Date)
            },
            Column {
                kind: ColumnKind::Calculated {
                    expression: "YEAR ( 'Top Days'[Day] )".into(),
                    language: ExpressionLanguage::Dax,
                },
                ..col("c-top-year", "Top Year", DataType::Double)
            },
        ],
        measures: vec![],
        hierarchies: vec![],
        hidden: false,
        is_date_table: false,
        calculation_group: None,
        calculated: Some(CalculatedTable {
            expression: "TOPN (\n\t3,\n\tVALUES ( Sales[Day] ),\n\t[Total]\n)".into(),
            language: ExpressionLanguage::Dax,
        }),
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
                hierarchies: Vec::new(),
                hidden: false,
                is_date_table: false,
                calculation_group: None,
                calculated: None,
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
                hierarchies: vec![Hierarchy {
                    id: "h-cal".into(),
                    name: "Calendar".into(),
                    description: Some("Months by number".into()),
                    levels: vec![
                        Level {
                            name: "Month Number".into(),
                            column: "c-mnum".into(),
                        },
                        Level {
                            name: "Month".into(),
                            column: "c-month".into(),
                        },
                    ],
                    hidden: false,
                    display_folder: Some("Dates".into()),
                    extras: Default::default(),
                }],
                is_date_table: true,
                calculation_group: None,
                calculated: None,
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
        roles: vec![Role {
            id: "role-1".into(),
            name: "Last month's sales".into(),
            description: Some("Sales staff".into()),
            filters: vec![
                RoleFilter {
                    table: "t-date".into(),
                    expression: "'Date'[MonthNumber] = 12".into(),
                },
                RoleFilter {
                    table: "t-sales".into(),
                    expression: "VAR x = 10\nRETURN Sales[Sales Amount] > x".into(),
                },
            ],
            members: vec!["sales@shop.example".into()],
            extras: Default::default(),
        }],
        ..Default::default()
    };
    let mut model = model;
    model.tables.push(top_table());
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
                Column {
                    kind: ColumnKind::Calculated {
                        expression: "IF ( ISBLANK ( 'Sales'[Amount] ), BLANK (), INT ( 'Sales'[Amount] / 10 ) * 10 )".into(),
                        language: ExpressionLanguage::Dax,
                    },
                    extras: [(
                        "charted".to_string(),
                        serde_json::json!({ "group": { "kind": "bin", "column": "c-amt", "unit": "number", "size": 10, "by": "size", "count": 10 } }),
                    )]
                    .into(),
                    ..col("c-bins", "Amount (bins)", DataType::Double)
                },
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
            hierarchies: vec![Hierarchy {
                id: "h-1".into(),
                name: "Calendar".into(),
                description: None,
                levels: vec![
                    Level {
                        name: "Year".into(),
                        column: "c-year".into(),
                    },
                    Level {
                        name: "Day of sale".into(),
                        column: "c-day".into(),
                    },
                ],
                hidden: true,
                display_folder: Some("Time\\Calendars".into()),
                extras: Default::default(),
            }],
            hidden: false,
            is_date_table: false,
            calculation_group: None,
            calculated: None,
            extras: Default::default(),
        }],
        ..Default::default()
    };
    model.tables.push(top_table());
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
            hidden: false,
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
    model.roles = vec![
        Role {
            id: "role-a".into(),
            name: "Bikes".into(),
            description: None,
            filters: vec![RoleFilter {
                table: "t-sales".into(),
                expression: "Sales[Category] = \"Bikes\"".into(),
            }],
            members: vec![],
            extras: Default::default(),
        },
        Role {
            id: "role-b".into(),
            name: "Admins".into(),
            description: Some("Everything".into()),
            filters: vec![],
            members: vec!["admins".into(), "it@shop.example".into()],
            extras: [(
                "powerbi".to_string(),
                serde_json::json!({ "modelPermission": "administrator" }),
            )]
            .into(),
        },
    ];
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
    // Roles keep their ids through an annotation (Power BI roles have no lineage tag).
    assert_eq!(back.model.roles, model.roles);
}

/// A theme edited in charted (text classes, visual styles, more colours) comes back
/// from its export, also when Power BI has dropped charted's annotation and only the
/// custom theme file is left.
#[test]
fn edited_themes_come_back_from_the_report() {
    let mut model = reported();
    let theme = ReportTheme {
        name: "Harbor (edited)".into(),
        json: serde_json::json!({
            "name": "Harbor (edited)",
            "dataColors": ["#123456", "#abcdef", "#0f0f0f"],
            "background": "#FFFFFF",
            "foreground": "#252423",
            "tableAccent": "#123456",
            "textClasses": { "title": { "fontSize": 16, "fontFace": "Georgia", "color": "#AA0000" } },
            "visualStyles": { "*": { "*": { "title": [{ "show": true, "fontSize": 20 }], "legend": [{ "position": "Bottom" }], "labels": [{ "show": true }] } } }
        }),
    };
    model.report.theme = Some(theme.clone());
    let files = charted_tmdl::pbir::report_files(&model);
    let mut report: std::collections::BTreeMap<String, String> = files.into_iter().collect();
    let mut skipped = Vec::new();
    let back = charted_tmdl::pbir::read_pbir(&model, &report, &mut skipped).unwrap();
    assert_eq!(back.theme.as_ref(), Some(&theme));
    // Without charted's annotation, the theme comes from its resource file.
    let json: &mut String = report.get_mut("definition/report.json").unwrap();
    let mut v: serde_json::Value = serde_json::from_str(json).unwrap();
    v.as_object_mut().unwrap().remove("annotations");
    *json = v.to_string();
    let back = charted_tmdl::pbir::read_pbir(&model, &report, &mut skipped).unwrap();
    assert_eq!(back.theme, Some(theme));
}

/// Roles as Power BI Desktop writes them, in TMDL and in model.bim.
#[test]
fn power_bi_roles_are_imported() {
    let mut def = std::collections::BTreeMap::new();
    def.insert(
        "model.tmdl".to_string(),
        "model Model\n\tculture: en-US\n\nref table Store\nref role 'North only'\n".to_string(),
    );
    def.insert(
        "tables/Store.tmdl".to_string(),
        "table Store\n\tlineageTag: t-store\n\n\tcolumn Region\n\t\tdataType: string\n\t\tlineageTag: c-region\n\t\tsourceColumn: Region\n".to_string(),
    );
    def.insert(
        "roles/North only.tmdl".to_string(),
        "/// Northern managers\nrole 'North only'\n\tmodelPermission: read\n\n\tmember 'north@contoso.com'\n\t\tmemberType: user\n\n\ttablePermission Store = [Region] = \"North\"\n\n\ttablePermission Missing = TRUE()\n\n\tannotation PBI_Id = 123\n".to_string(),
    );
    let back = charted_tmdl::read::from_tmdl(&def, "Shop");
    let r = &back.model.roles[0];
    assert_eq!(r.name, "North only");
    assert_eq!(r.description.as_deref(), Some("Northern managers"));
    assert_eq!(r.members, vec!["north@contoso.com".to_string()]);
    assert_eq!(
        r.filters,
        vec![RoleFilter {
            table: "t-store".into(),
            expression: "[Region] = \"North\"".into()
        }]
    );
    assert!(
        back.skipped.iter().any(|s| s.kind == "role filter"),
        "{:?}",
        back.skipped
    );

    let bim = r#"{"name": "M", "model": {"tables": [{"name": "Store", "lineageTag": "t-store", "columns": [{"name": "Region", "dataType": "string", "sourceColumn": "Region"}]}],
      "roles": [{"name": "North", "modelPermission": "read", "members": [{"memberName": "a@b.example"}],
                 "tablePermissions": [{"name": "Store", "filterExpression": ["[Region]", " = \"North\""]}]},
                {"name": "Admins", "modelPermission": "administrator"}]}}"#;
    let back = charted_tmdl::read::from_bim(bim, "Shop").unwrap();
    assert_eq!(back.model.roles.len(), 2);
    assert_eq!(back.model.roles[0].filters[0].table, "t-store");
    assert_eq!(
        back.model.roles[0].filters[0].expression,
        "[Region]\n = \"North\""
    );
    assert_eq!(
        back.model.roles[1].extras["powerbi"]["modelPermission"],
        "administrator"
    );
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
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

/// References, entered tables, calendars and decimal-comma CSV files come back from an
/// export: through charted's annotation, and through their M alone (as after Power BI
/// rewrites the files).
#[test]
fn power_query_sources_come_back_from_their_m() {
    let table = |id: &str, name: &str, cols: Vec<Column>| Table {
        id: id.into(),
        name: name.into(),
        description: None,
        source: name.to_lowercase(),
        columns: cols,
        measures: vec![],
        hidden: false,
        is_date_table: false,
        calculation_group: None,
        calculated: None,
        hierarchies: Vec::new(),
        extras: Default::default(),
    };
    let q = |id: &str, name: &str, source: Source, steps: Vec<Step>| Query {
        id: id.into(),
        name: name.into(),
        table: name.to_lowercase(),
        source,
        steps,
        step_names: Default::default(),
        no_load: false,
    };
    let model = Model {
        name: "Shop".into(),
        tables: vec![
            table(
                "t1",
                "Sales",
                vec![
                    col("c1", "Region", DataType::String),
                    col("c2", "Amount", DataType::Double),
                ],
            ),
            table(
                "t2",
                "Big",
                vec![
                    col("c3", "Region", DataType::String),
                    col("c4", "Amount", DataType::Double),
                ],
            ),
            table(
                "t3",
                "Targets",
                vec![
                    col("c5", "Region", DataType::String),
                    col("c6", "Target", DataType::Int64),
                ],
            ),
            table("t4", "Calendar", vec![col("c7", "Date", DataType::Date)]),
        ],
        ..Default::default()
    };
    let queries = vec![
        q(
            "q1",
            "Sales",
            Source::Csv {
                path: "data/sales.csv".into(),
                delimiter: Some(";".into()),
                header: true,
                encoding: None,
                decimal: Some(",".into()),
            },
            vec![],
        ),
        q(
            "q2",
            "Big",
            Source::Reference { query: "q1".into() },
            vec![Step::RemoveTopRows { count: 1 }],
        ),
        q(
            "q3",
            "Targets",
            Source::Table {
                columns: vec![
                    ("Region".into(), DataType::String),
                    ("Target".into(), DataType::Int64),
                ],
                rows: vec![
                    vec![Some("North".into()), Some("10".into())],
                    vec![Some("Sø \"x\"".into()), None],
                ],
            },
            vec![],
        ),
        q(
            "q4",
            "Calendar",
            Source::Calendar {
                start: "2024-01-01".into(),
                end: "2025-06-30".into(),
                fiscal_start: 4,
            },
            vec![],
        ),
    ];
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
    back.queries.sort_by(|a, b| a.id.cmp(&b.id));
    assert_eq!(back.queries, queries);
    // Without charted's annotations, from the M alone.
    let bare: std::collections::BTreeMap<String, String> = def
        .iter()
        .map(|(p, t)| {
            let lines: Vec<&str> = t
                .lines()
                .filter(|l| !l.contains("annotation chartedQuery"))
                .collect();
            (p.clone(), lines.join("\n"))
        })
        .collect();
    let back = charted_tmdl::read::from_tmdl(&bare, "Shop");
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    let name_of = |qs: &[Query], id: &str| qs.iter().find(|q| q.id == id).map(|q| q.name.clone());
    for want in &queries {
        let got = back.queries.iter().find(|q| q.name == want.name).unwrap();
        match (&got.source, &want.source) {
            (Source::Reference { query: a }, Source::Reference { query: b }) => {
                assert_eq!(name_of(&back.queries, a), name_of(&queries, b))
            }
            // Read through the SourceFolder parameter; the culture keeps the comma.
            (Source::Csv { path, decimal, .. }, Source::Csv { .. }) => {
                assert_eq!(path, "/data/data/sales.csv");
                assert_eq!(decimal.as_deref(), Some(","));
            }
            (a, b) => assert_eq!(a, b, "{}", want.name),
        }
        // The CSV's type change (from the table's columns) comes back as a step.
        if want.name == "Sales" {
            assert!(
                matches!(got.steps.as_slice(), [Step::ChangeTypes { .. }]),
                "{:?}",
                got.steps
            );
        } else {
            assert_eq!(got.steps, want.steps, "{}", want.name);
        }
    }
}

/// A model.bim with a calculated table, a what-if parameter and a field parameter: all
/// three come in as calculated tables.
#[test]
fn model_bim_calculated_tables_and_parameters_come_in() {
    let bim = serde_json::json!({
        "name": "Shop",
        "model": {
            "tables": [
                {
                    "name": "Top Products",
                    "lineageTag": "t-top",
                    "columns": [
                        {"name": "Product", "dataType": "string", "type": "calculatedTableColumn",
                         "isNameInferred": true, "sourceColumn": "Product[Product]", "lineageTag": "c-p"},
                        {"name": "Sales", "dataType": "double", "type": "calculatedTableColumn",
                         "isNameInferred": true, "sourceColumn": "[Sales]", "lineageTag": "c-s"}
                    ],
                    "partitions": [{"name": "Top Products", "mode": "import", "source": {
                        "type": "calculated",
                        "expression": ["ADDCOLUMNS (", "    TOPN ( 5, VALUES ( Product[Product] ), [Total] ),", "    \"Sales\", [Total]", ")"]
                    }}]
                },
                {
                    "name": "Discount",
                    "columns": [
                        {"name": "Discount", "dataType": "int64", "sourceColumn": "[Value]",
                         "extendedProperties": [{"type": "json", "name": "ParameterMetadata", "value": {"version": 0}}]}
                    ],
                    "measures": [{"name": "Discount Value", "expression": "SELECTEDVALUE('Discount'[Discount], 10)"}],
                    "partitions": [{"name": "Discount", "mode": "import", "source": {
                        "type": "calculated",
                        "expression": "GENERATESERIES(0, 50, 5)"
                    }}]
                },
                {
                    "name": "Metric",
                    "columns": [
                        {"name": "Metric", "dataType": "string", "sourceColumn": "[Value1]",
                         "extendedProperties": [{"type": "json", "name": "ParameterMetadata", "value": {"version": 3, "kind": 2}}]}
                    ],
                    "partitions": [{"name": "Metric", "mode": "import", "source": {
                        "type": "calculated",
                        "expression": "{ (\"Total\", NAMEOF ( [Total] ), 0) }"
                    }}]
                }
            ]
        }
    });
    let im = charted_tmdl::read::from_bim(&bim.to_string(), "Shop").unwrap();
    let skipped: Vec<String> = im
        .skipped
        .iter()
        .map(|s| format!("{} {}", s.kind, s.name))
        .collect();
    assert!(skipped.is_empty(), "{skipped:?}");
    let top = im.model.table("t-top").unwrap();
    assert_eq!(top.source, calculated_table_source("t-top"));
    let calc = top.calculated.as_ref().unwrap();
    assert!(
        calc.expression.starts_with("ADDCOLUMNS (\n    TOPN"),
        "{}",
        calc.expression
    );
    let sources: Vec<String> = top
        .columns
        .iter()
        .map(|c| match &c.kind {
            ColumnKind::Data { source_column } => source_column.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(sources, ["Product", "Sales"]);
    let discount = im.model.table_by_name("Discount").unwrap();
    assert_eq!(
        discount.calculated.as_ref().unwrap().expression,
        "GENERATESERIES(0, 50, 5)"
    );
    assert!(matches!(
        &discount.columns[0].kind,
        ColumnKind::Data { source_column } if source_column == "Value"
    ));
    assert_eq!(discount.measures[0].name, "Discount Value");
    let metric = im.model.table_by_name("Metric").unwrap();
    assert_eq!(
        metric.calculated.as_ref().unwrap().expression,
        "{ (\"Total\", NAMEOF ( [Total] ), 0) }"
    );
    assert!(matches!(
        &metric.columns[0].kind,
        ColumnKind::Data { source_column } if source_column == "Value1"
    ));
}

/// Field and what-if parameters are written as Power BI writes them (`ParameterMetadata`
/// on the label or value column, names not inferred) and read back as they were.
#[test]
fn parameters_round_trip_with_their_metadata() {
    let data = |id: &str, name: &str, source: &str, ty: DataType| Column {
        kind: ColumnKind::Data {
            source_column: source.into(),
        },
        ..col(id, name, ty)
    };
    let calc = |id: &str, name: &str, expression: &str, columns: Vec<Column>| Table {
        id: id.into(),
        name: name.into(),
        description: None,
        source: calculated_table_source(id),
        columns,
        measures: vec![],
        hierarchies: vec![],
        hidden: false,
        is_date_table: false,
        calculation_group: None,
        calculated: Some(CalculatedTable {
            expression: expression.into(),
            language: ExpressionLanguage::Dax,
        }),
        extras: Default::default(),
    };
    let mut label = data("c-label", "Metric", "Value1", DataType::String);
    label.sort_by = Some("c-order".into());
    let model = Model {
        name: "Shop".into(),
        tables: vec![
            calc(
                "t-metric",
                "Metric",
                "{\n    ( \"Sales\", NAMEOF ( 'Sales'[Total] ), 0 )\n}",
                vec![
                    label,
                    data("c-fields", "Metric Fields", "Value2", DataType::String),
                    data("c-order", "Metric Order", "Value3", DataType::Int64),
                ],
            ),
            calc(
                "t-discount",
                "Discount",
                "GENERATESERIES ( 0, 50, 5 )",
                vec![data("c-discount", "Discount", "Value", DataType::Int64)],
            ),
        ],
        ..Default::default()
    };
    let files = charted_tmdl::pbip("Shop", &model, &[], "/data");
    let text = |name: &str| {
        files
            .iter()
            .find(|(p, _)| p.ends_with(&format!("tables/{name}.tmdl")))
            .map(|(_, t)| t.clone())
            .unwrap()
    };
    let metric = text("Metric");
    assert!(
        metric.contains("\"version\": 3,\n\t\t\t\t  \"kind\": 2"),
        "{metric}"
    );
    assert_eq!(
        metric.matches("extendedProperty ParameterMetadata").count(),
        1
    );
    assert!(!metric.contains("isNameInferred"), "{metric}");
    assert!(text("Discount").contains("\"version\": 0"));
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
    let back = charted_tmdl::read::from_tmdl(&def, "Shop");
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    // Indentation inside expressions comes back as tabs; the DAX is the same.
    let words = |t: &Table| {
        t.calculated.as_ref().map(|c| {
            c.expression
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
    };
    for t in &model.tables {
        let b = back.model.table(&t.id).unwrap();
        assert_eq!(words(b), words(t));
        let cols = |t: &Table| {
            t.columns
                .iter()
                .map(|c| (c.name.clone(), c.kind.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(cols(b), cols(t));
    }
}

#[test]
fn calculation_groups_come_in_and_round_trip() {
    // As Power BI Desktop writes one.
    let tmdl = "table 'Time Intelligence'\n\
        \tlineageTag: t-ti\n\
        \n\
        \tcalculationGroup\n\
        \t\tprecedence: 5\n\
        \n\
        \t\tcalculationItem YTD =\n\
        \t\t\t\tCALCULATE (\n\
        \t\t\t\t    SELECTEDMEASURE (),\n\
        \t\t\t\t    DATESYTD ( 'Date'[Date] )\n\
        \t\t\t\t)\n\
        \t\t\tordinal: 1\n\
        \n\
        \t\t/// The measure as it is\n\
        \t\tcalculationItem Current = SELECTEDMEASURE()\n\
        \t\t\tordinal: 0\n\
        \t\t\tformatStringDefinition = \"0.0\"\n\
        \n\
        \tcolumn 'Time Calculation'\n\
        \t\tdataType: string\n\
        \t\tlineageTag: c-name\n\
        \t\tsummarizeBy: none\n\
        \t\tsourceColumn: Name\n\
        \t\tsortByColumn: Ordinal\n\
        \n\
        \tcolumn Ordinal\n\
        \t\tdataType: int64\n\
        \t\tisHidden\n\
        \t\tlineageTag: c-ord\n\
        \t\tsourceColumn: Ordinal\n\
        \n\
        \tpartition 'Time Intelligence' = calculationGroup\n\
        \t\tmode: import\n";
    let def: std::collections::BTreeMap<String, String> = [(
        "tables/Time Intelligence.tmdl".to_string(),
        tmdl.to_string(),
    )]
    .into();
    let im = charted_tmdl::read::from_tmdl(&def, "Shop");
    assert!(im.skipped.is_empty(), "{:?}", im.skipped);
    let t = im.model.table("t-ti").unwrap();
    let g = t.calculation_group.as_ref().unwrap();
    assert_eq!(g.precedence, 5);
    let names: Vec<&str> = g.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["Current", "YTD"]);
    assert_eq!(g.items[0].format_string.as_deref(), Some("\"0.0\""));
    assert_eq!(
        g.items[0].description.as_deref(),
        Some("The measure as it is")
    );
    assert!(
        g.items[1].expression.contains("DATESYTD"),
        "{}",
        g.items[1].expression
    );
    assert_eq!(
        t.calculated.as_ref().unwrap().expression,
        "{ ( \"Current\", 0 ), ( \"YTD\", 1 ) }"
    );
    assert_eq!(t.source, calculated_table_source("t-ti"));
    let sources: Vec<(String, ColumnKind)> = t
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.kind.clone()))
        .collect();
    assert_eq!(
        sources,
        [
            (
                "Time Calculation".into(),
                ColumnKind::Data {
                    source_column: "Value1".into()
                }
            ),
            (
                "Ordinal".into(),
                ColumnKind::Data {
                    source_column: "Value2".into()
                }
            ),
        ]
    );
    assert_eq!(t.columns[0].sort_by.as_deref(), Some("c-ord"));

    // Written back as Power BI has it, and read again the same.
    let files = charted_tmdl::pbip("Shop", &im.model, &[], "/data");
    let text = |suffix: &str| {
        files
            .iter()
            .find(|(p, _)| p.ends_with(suffix))
            .map(|(_, t)| t.clone())
            .unwrap()
    };
    let out = text("tables/Time Intelligence.tmdl");
    for want in [
        "\tcalculationGroup\n\t\tprecedence: 5\n",
        "\t\tcalculationItem Current = SELECTEDMEASURE()\n\t\t\tordinal: 0\n\t\t\tformatStringDefinition = \"0.0\"\n",
        "\t\tsourceColumn: Name\n",
        "\t\tsourceColumn: Ordinal\n",
        "\tpartition 'Time Intelligence' = calculationGroup\n",
    ] {
        assert!(out.contains(want), "{want:?} in\n{out}");
    }
    assert!(!out.contains("ParameterMetadata"), "{out}");
    assert!(text("definition/model.tmdl").contains("\tdiscourageImplicitMeasures\n"));
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
    let back = charted_tmdl::read::from_tmdl(&def, "Shop");
    assert!(back.skipped.is_empty(), "{:?}", back.skipped);
    let b = back.model.table("t-ti").unwrap();
    let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let bg = b.calculation_group.as_ref().unwrap();
    assert_eq!(bg.precedence, g.precedence);
    for (x, y) in bg.items.iter().zip(&g.items) {
        assert_eq!(
            (&x.name, words(&x.expression)),
            (&y.name, words(&y.expression))
        );
        assert_eq!(x.format_string, y.format_string);
        assert_eq!(x.description, y.description);
    }
    assert_eq!(b.columns, t.columns);

    // model.bim has the same in TMSL.
    let bim = serde_json::json!({
        "name": "Shop",
        "model": {
            "tables": [{
                "name": "Scenario",
                "lineageTag": "t-s",
                "calculationGroup": {
                    "precedence": 2,
                    "calculationItems": [
                        {"name": "Double", "ordinal": 1, "expression": ["SELECTEDMEASURE ()", "    * 2"]},
                        {"name": "Base", "ordinal": 0, "expression": "SELECTEDMEASURE ()",
                         "formatStringDefinition": {"expression": "\"#,0\""}}
                    ]
                },
                "columns": [
                    {"name": "Scenario", "dataType": "string", "sourceColumn": "Name", "sortByColumn": "Ordinal"},
                    {"name": "Ordinal", "dataType": "int64", "sourceColumn": "Ordinal", "isHidden": true}
                ],
                "partitions": [{"name": "Scenario", "mode": "import", "source": {"type": "calculationGroup"}}]
            }]
        }
    });
    let im = charted_tmdl::read::from_bim(&bim.to_string(), "Shop").unwrap();
    assert!(im.skipped.is_empty(), "{:?}", im.skipped);
    let g = im
        .model
        .table("t-s")
        .unwrap()
        .calculation_group
        .clone()
        .unwrap();
    assert_eq!(g.precedence, 2);
    assert_eq!(
        g.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        ["Base", "Double"]
    );
    assert_eq!(g.items[1].expression, "SELECTEDMEASURE ()\n    * 2");
    assert_eq!(g.items[0].format_string.as_deref(), Some("\"#,0\""));
}
