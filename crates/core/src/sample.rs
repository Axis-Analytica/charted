//! Creates the "Harbor & Pine Outfitters" sample project.

use charted_compiler::steps::{Query, Source};
use charted_engine::{Database, sql};
use charted_model::*;
use charted_samplegen::Scale;
use std::path::Path;

use crate::ids::stable_id;
use crate::project::{Project, new_project};
use crate::session::{CoreError, load_query};
use crate::types::from_duckdb;

pub const SAMPLE_NAME: &str = "Harbor & Pine";

/// Generates data files and a ready-to-explore project in `dir`, which must be empty or
/// not exist yet. Returns the saved project.
pub fn create(dir: &Path, scale: Scale) -> Result<Project, CoreError> {
    if dir.exists()
        && std::fs::read_dir(dir)
            .map(|mut d| d.next().is_some())
            .unwrap_or(false)
    {
        return Err(CoreError::Invalid(format!(
            "{} is not empty; choose a new folder for the sample",
            dir.display()
        )));
    }
    let data = dir.join("data");
    let files =
        charted_samplegen::generate(&data, scale).map_err(|e| CoreError::Invalid(e.to_string()))?;

    let queries: Vec<Query> = files
        .iter()
        .map(|f| {
            let file = format!("data/{}", f.file.file_name().unwrap().to_string_lossy());
            let source = if file.ends_with(".parquet") {
                Source::Parquet { path: file }
            } else {
                Source::Csv {
                    path: file,
                    delimiter: None,
                    header: true,
                    encoding: None,
                }
            };
            Query {
                id: stable_id(&format!("query/{}", f.name)),
                name: f.name.to_string(),
                table: f.name.to_lowercase(),
                source,
                steps: vec![],
            }
        })
        .collect();

    let mut project = new_project(
        dir,
        SAMPLE_NAME,
        Model {
            name: SAMPLE_NAME.into(),
            // A new report starts with one empty page, as in Power BI.
            report: charted_model::Report {
                pages: vec![charted_model::Page {
                    id: stable_id("page/Page 1"),
                    settings: charted_model::PageSettings::named("Page 1"),
                    visuals: vec![],
                }],
                filters: vec![],
            },
            ..Default::default()
        },
        queries,
    );
    let db = Database::open(Some(&project.database_path()), &Default::default())?;
    for (q, f) in project.queries.iter().zip(&files) {
        load_query(&db, q, &project.dir_str())?;
        let cols = db.describe(&sql::ident(&q.table))?;
        project.model.tables.push(Table {
            id: stable_id(&format!("table/{}", f.name)),
            name: f.name.to_string(),
            description: None,
            source: q.table.clone(),
            columns: cols
                .iter()
                .map(|c| {
                    let data_type = from_duckdb(&c.duckdb_type);
                    let is_key_like = c.name.ends_with("Key");
                    Column {
                        id: stable_id(&format!("column/{}/{}", f.name, c.name)),
                        name: c.name.clone(),
                        data_type,
                        kind: ColumnKind::Data {
                            source_column: c.name.clone(),
                        },
                        description: None,
                        format_string: None,
                        summarize_by: if is_key_like
                            || c.name.ends_with("Number")
                            || c.name == "BirthYear"
                            || c.name == "Year"
                        {
                            Summarize::None
                        } else {
                            Summarize::Default
                        },
                        data_category: None,
                        sort_by: None,
                        display_folder: None,
                        hidden: false,
                        is_key: false,
                        extras: Default::default(),
                    }
                })
                .collect(),
            measures: vec![],
            hidden: false,
            is_date_table: false,
            extras: Default::default(),
        });
    }
    decorate(&mut project.model);
    project.save()?;
    Ok(project)
}

fn col_id(model: &Model, table: &str, column: &str) -> (Id, Id) {
    let t = model
        .table_by_name(table)
        .unwrap_or_else(|| panic!("sample table {table}"));
    let c = t
        .column_by_name(column)
        .unwrap_or_else(|| panic!("sample column {table}.{column}"));
    (t.id.clone(), c.id.clone())
}

fn set(model: &mut Model, table: &str, column: &str, f: impl FnOnce(&mut Column)) {
    let (t, c) = col_id(model, table, column);
    f(model.table_mut(&t).unwrap().column_mut(&c).unwrap());
}

fn measure(name: &str, expression: &str, format: &str, folder: Option<&str>) -> Measure {
    Measure {
        id: stable_id(&format!("measure/{name}")),
        name: name.into(),
        expression: expression.into(),
        language: ExpressionLanguage::Dax,
        description: None,
        format_string: Some(format.into()),
        display_folder: folder.map(str::to_string),
        hidden: false,
        extras: Default::default(),
    }
}

/// Keys, formats, sort orders, relationships and starter measures.
fn decorate(m: &mut Model) {
    for (t, c) in [
        ("Date", "DateKey"),
        ("Product", "ProductKey"),
        ("Customer", "CustomerKey"),
        ("Store", "StoreKey"),
    ] {
        set(m, t, c, |c| c.is_key = true);
    }
    let money = "\\$#,0.00;(\\$#,0.00)";
    for c in ["UnitPrice", "UnitCost", "Amount", "Cost"] {
        set(m, "Sales", c, |col| col.format_string = Some(money.into()));
    }
    set(m, "Sales", "Discount", |c| {
        c.format_string = Some("0%".into());
        c.summarize_by = Summarize::Average;
    });
    for c in ["ListPrice", "UnitCost"] {
        set(m, "Product", c, |col| {
            col.format_string = Some(money.into())
        });
    }
    set(m, "Targets", "TargetAmount", |c| {
        c.format_string = Some("\\$#,0".into())
    });
    set(m, "Date", "Date", |c| {
        c.format_string = Some("yyyy-mm-dd".into())
    });
    let (_, month_number) = col_id(m, "Date", "MonthNumber");
    set(m, "Date", "MonthName", |c| c.sort_by = Some(month_number));
    let (_, weekday_number) = col_id(m, "Date", "WeekdayNumber");
    set(m, "Date", "Weekday", |c| c.sort_by = Some(weekday_number));
    for (t, c, cat) in [
        ("Customer", "City", "City"),
        ("Customer", "Country", "Country"),
        ("Store", "City", "City"),
        ("Store", "Country", "Country"),
    ] {
        set(m, t, c, |col| col.data_category = Some(cat.into()));
    }
    let date_id = m.table_by_name("Date").unwrap().id.clone();
    m.table_mut(&date_id).unwrap().is_date_table = true;

    let rel = |from: (&str, &str), to: (&str, &str), active: bool| {
        let (ft, fc) = col_id(m, from.0, from.1);
        let (tt, tc) = col_id(m, to.0, to.1);
        Relationship {
            id: stable_id(&format!(
                "relationship/{}/{}/{}/{}",
                from.0, from.1, to.0, to.1
            )),
            from_table: ft,
            from_column: fc,
            to_table: tt,
            to_column: tc,
            from_cardinality: Cardinality::Many,
            to_cardinality: Cardinality::One,
            cross_filter: CrossFilter::Single,
            active,
            extras: Default::default(),
        }
    };
    let rels = vec![
        rel(("Sales", "OrderDateKey"), ("Date", "DateKey"), true),
        rel(("Sales", "ShipDateKey"), ("Date", "DateKey"), false),
        rel(("Sales", "ProductKey"), ("Product", "ProductKey"), true),
        rel(("Sales", "CustomerKey"), ("Customer", "CustomerKey"), true),
        rel(("Sales", "StoreKey"), ("Store", "StoreKey"), true),
        rel(("Targets", "DateKey"), ("Date", "DateKey"), true),
    ];
    m.relationships = rels;

    let sales_id = m.table_by_name("Sales").unwrap().id.clone();
    m.table_mut(&sales_id).unwrap().measures = vec![
        measure(
            "Total Sales",
            "SUM ( Sales[Amount] )",
            "\\$#,0",
            Some("Sales"),
        ),
        measure("Total Cost", "SUM ( Sales[Cost] )", "\\$#,0", Some("Sales")),
        measure("Units", "SUM ( Sales[Quantity] )", "#,0", Some("Sales")),
        measure(
            "Orders",
            "DISTINCTCOUNT ( Sales[OrderNumber] )",
            "#,0",
            Some("Sales"),
        ),
    ];
    let targets_id = m.table_by_name("Targets").unwrap().id.clone();
    m.table_mut(&targets_id).unwrap().measures = vec![measure(
        "Target",
        "SUM ( Targets[TargetAmount] )",
        "\\$#,0",
        None,
    )];
    m.description = Some(
        "Harbor & Pine Outfitters, a fictional outdoor retailer. Synthetic sample data (CC0)."
            .into(),
    );
}
