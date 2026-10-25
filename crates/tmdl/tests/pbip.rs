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
