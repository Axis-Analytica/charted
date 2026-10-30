use charted_model::*;
use charted_ossie::*;

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

fn measure(id: &str, name: &str, expr: &str) -> Measure {
    Measure {
        id: id.into(),
        name: name.into(),
        expression: expr.into(),
        language: ExpressionLanguage::Dax,
        description: None,
        format_string: None,
        display_folder: None,
        hidden: false,
        extras: Default::default(),
    }
}

fn table(id: &str, name: &str, columns: Vec<Column>) -> Table {
    Table {
        id: id.into(),
        name: name.into(),
        description: None,
        source: name.to_lowercase(),
        columns,
        measures: vec![],
        hidden: false,
        hierarchies: Vec::new(),
        is_date_table: false,
        calculated: None,
        extras: Default::default(),
    }
}

fn rel(id: &str, ft: &str, fc: &str, tt: &str, tc: &str) -> Relationship {
    Relationship {
        id: id.into(),
        from_table: ft.into(),
        from_column: fc.into(),
        to_table: tt.into(),
        to_column: tc.into(),
        from_cardinality: Cardinality::Many,
        to_cardinality: Cardinality::One,
        cross_filter: CrossFilter::Single,
        active: true,
        extras: Default::default(),
    }
}

fn sample_model() -> Model {
    let mut sales = table(
        "11111111-0000-0000-0000-000000000001",
        "Sales",
        vec![
            col("c-order-date", "OrderDateKey", DataType::Int64),
            col("c-ship-date", "ShipDateKey", DataType::Int64),
            col("c-prod", "ProductKey", DataType::Int64),
            Column {
                format_string: Some("\\$#,0.00;(\\$#,0.00)".into()),
                summarize_by: Summarize::Sum,
                ..col("c-amt", "Amount", DataType::Decimal)
            },
            Column {
                kind: ColumnKind::Data {
                    source_column: "unit cost".into(),
                },
                hidden: true,
                ..col("c-cost", "Unit Cost", DataType::Double)
            },
            Column {
                kind: ColumnKind::Calculated {
                    expression: "Sales[Amount] - Sales[Unit Cost]".into(),
                    language: ExpressionLanguage::Dax,
                },
                // Group dialog settings (charted-only) live in the CHARTED blob.
                extras: [(
                    "charted".to_string(),
                    serde_json::json!({ "group": { "kind": "bin", "column": "c-amt", "unit": "number", "size": 10 } }),
                )]
                .into(),
                ..col("c-margin", "Line Margin", DataType::Decimal)
            },
        ],
    );
    sales.measures = vec![
        Measure {
            format_string: Some("#,0".into()),
            display_folder: Some("Revenue".into()),
            ..measure("m-total", "Total Sales", "SUM ( Sales[Amount] )")
        },
        Measure {
            description: Some("Share of the selected category".into()),
            format_string: Some("0.0%".into()),
            ..measure(
                "m-share",
                "Category Share",
                "DIVIDE ( [Total Sales], CALCULATE ( [Total Sales], ALL ( Product[Category] ) ) )",
            )
        },
    ];
    let product = table(
        "p-1",
        "Product",
        vec![
            Column {
                is_key: true,
                ..col("c-p-key", "ProductKey", DataType::Int64)
            },
            Column {
                data_category: Some("Uncategorized".into()),
                ..col("c-p-cat", "Category", DataType::String)
            },
        ],
    );
    let mut date = table(
        "d-1",
        "Date",
        vec![
            Column {
                is_key: true,
                ..col("c-d-key", "DateKey", DataType::Int64)
            },
            Column {
                format_string: Some("yyyy-mm-dd".into()),
                ..col("c-d-date", "Date", DataType::Date)
            },
            col("c-d-msort", "MonthNumber", DataType::Int64),
            Column {
                sort_by: Some("c-d-msort".into()),
                ..col("c-d-month", "Month Name", DataType::String)
            },
        ],
    );
    date.is_date_table = true;
    date.hierarchies = vec![Hierarchy {
        id: "h-cal".into(),
        name: "Calendar".into(),
        description: Some("Months by number".into()),
        levels: vec![
            Level {
                name: "Month".into(),
                column: "c-d-month".into(),
            },
            Level {
                name: "Day".into(),
                column: "c-d-date".into(),
            },
        ],
        hidden: true,
        display_folder: Some("Dates\\Calendars".into()),
        extras: Default::default(),
    }];
    let mut ship = rel(
        "r-ship",
        "11111111-0000-0000-0000-000000000001",
        "c-ship-date",
        "d-1",
        "c-d-key",
    );
    ship.active = false;
    let mut prod = rel(
        "r-prod",
        "11111111-0000-0000-0000-000000000001",
        "c-prod",
        "p-1",
        "c-p-key",
    );
    prod.cross_filter = CrossFilter::Both;
    // A calculated table, with a calculated column of its own.
    let mut top = table(
        "t-top",
        "Top products",
        vec![
            Column {
                description: Some("Best sellers".into()),
                ..col("c-t-name", "Product", DataType::String)
            },
            Column {
                kind: ColumnKind::Calculated {
                    expression: "LEN ( 'Top products'[Product] )".into(),
                    language: ExpressionLanguage::Dax,
                },
                ..col("c-t-len", "Name Length", DataType::Double)
            },
        ],
    );
    top.source = calculated_table_source("t-top");
    top.calculated = Some(CalculatedTable {
        expression: "TOPN ( 5, VALUES ( Product[Product] ), [Total Sales] )".into(),
        language: ExpressionLanguage::Dax,
    });
    Model {
        name: "Harbor & Pine".into(),
        description: Some("Retail sales sample".into()),
        tables: vec![sales, product, date, top],
        relationships: vec![
            rel(
                "r-order",
                "11111111-0000-0000-0000-000000000001",
                "c-order-date",
                "d-1",
                "c-d-key",
            ),
            ship,
            prod,
        ],
        extras: Default::default(),
        report: Default::default(),
    }
}

fn schema_errors(yaml: &str) -> Vec<String> {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schema/ossie-schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let instance: serde_json::Value = serde_yaml_ng::from_str(yaml).unwrap();
    validator
        .iter_errors(&instance)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect()
}

#[test]
fn model_round_trips_losslessly_and_validates() {
    let model = sample_model();
    let mut generated = GeneratedDialects::new();
    generated.insert(
        "m-total".into(),
        vec![(OSSIE_SQL.into(), "SUM(Sales.Amount)".into())],
    );
    let yaml = write_string(&model, &generated);
    let errors = schema_errors(&yaml);
    assert!(errors.is_empty(), "schema violations: {errors:#?}\n{yaml}");
    let (back, warnings) = read_str(&yaml).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(back, model);
    // Writing again is byte-stable.
    assert_eq!(write_string(&back, &generated), yaml);
}

#[test]
fn written_yaml_snapshot() {
    let mut generated = GeneratedDialects::new();
    generated.insert(
        "m-total".into(),
        vec![(OSSIE_SQL.into(), "SUM(Sales.Amount)".into())],
    );
    insta::assert_snapshot!(write_string(&sample_model(), &generated));
}

#[test]
fn foreign_content_survives_a_round_trip() {
    let yaml = r#"
version: 0.2.0.dev0
name: shop
ai_context: "Online shop orders"
datasets:
  - name: orders
    source: sales.public.orders
    primary_key: [id]
    fields:
      - name: id
        expression: { dialects: [ { dialect: ANSI_SQL, expression: id } ] }
        datatype: Integer
      - name: amount
        expression:
          dialects:
            - { dialect: ANSI_SQL, expression: amount }
            - { dialect: SNOWFLAKE, expression: AMOUNT }
        datatype: Decimal
        ai_context: { synonyms: [revenue] }
        custom_extensions:
          - { vendor_name: SIGMA, data: '{"color": "blue"}' }
          - { vendor_name: POWER_BI, data: '{"_v": 2, "formatString": "0.00", "variations": [{"name": "x"}]}' }
metrics:
  - name: total_revenue
    expression: { dialects: [ { dialect: ANSI_SQL, expression: SUM(orders.amount) } ] }
    custom_extensions:
      - { vendor_name: POWER_BI, data: '{"_v": 2, "table": "orders"}' }
"#;
    let (model, warnings) = read_str(yaml).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let orders = &model.tables[0];
    assert!(orders.columns[0].is_key);
    assert_eq!(orders.measures[0].language, ExpressionLanguage::Sql);
    assert_eq!(orders.columns[1].format_string.as_deref(), Some("0.00"));

    let written = write_string(&model, &GeneratedDialects::new());
    assert!(schema_errors(&written).is_empty());
    for needle in [
        "SNOWFLAKE",
        "AMOUNT",
        "SIGMA",
        "synonyms",
        "variations",
        "Online shop orders",
    ] {
        assert!(written.contains(needle), "lost {needle}:\n{written}");
    }
    let (again, _) = read_str(&written).unwrap();
    assert_eq!(again, model);
}

#[test]
fn reports_what_it_cannot_represent() {
    let yaml = r#"
version: 0.2.0.dev0
name: m
datasets:
  - name: a
    source: a
    fields:
      - { name: k, expression: { dialects: [ { dialect: ANSI_SQL, expression: k } ] } }
      - { name: t, expression: { dialects: [ { dialect: TABLEAU, expression: "[k] * 2" } ] } }
    metrics:
      - { name: n, expression: { dialects: [ { dialect: ANSI_SQL, expression: "COUNT(*)" } ] } }
relationships:
  - { name: bad, from: a, to: missing, from_columns: [k], to_columns: [k] }
"#;
    let (model, warnings) = read_str(yaml).unwrap();
    assert_eq!(
        model.tables[0].measures.len(),
        1,
        "dataset-scoped metrics are read"
    );
    let text: Vec<String> = warnings
        .iter()
        .map(|w| format!("{}: {}", w.scope, w.message))
        .collect();
    assert!(text.iter().any(|w| w.contains("TABLEAU")), "{text:?}");
    assert!(
        text.iter().any(|w| w.contains("relationship bad")),
        "{text:?}"
    );
}

#[test]
fn rejects_other_spec_versions() {
    let err = read_str("version: 0.1.0\nname: x\ndatasets: []\n").unwrap_err();
    assert!(err.to_string().contains("0.1.0"));
}
