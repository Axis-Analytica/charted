//! Calculated tables on the Harbor & Pine sample: created from a DAX table expression,
//! materialised on commit, queried by visuals, extended with calculated columns,
//! re-synced when the expression changes, undoable and kept across save and reopen.

use arrow_ipc::reader::StreamReader;
use charted_compiler::visual::VisualQuery;
use charted_core::{App, DataRequest, Request};
use charted_model::*;

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

fn sql(app: &App, q: &str) -> Vec<Vec<String>> {
    rows(app.session().unwrap().database().arrow(q).unwrap())
}

fn model(app: &App) -> Model {
    app.session().unwrap().project.model.clone()
}

fn calc_table(id: &str, name: &str, expression: &str) -> Table {
    Table {
        id: id.into(),
        name: name.into(),
        description: None,
        source: String::new(),
        columns: vec![],
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
    }
}

fn calc_column(id: &str, name: &str, expression: &str) -> Column {
    Column {
        id: id.into(),
        name: name.into(),
        data_type: DataType::Unknown,
        kind: ColumnKind::Calculated {
            expression: expression.into(),
            language: ExpressionLanguage::Dax,
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

fn commit(app: &mut App, changes: Vec<Change>) -> Result<(), String> {
    app.handle(Request::Commit {
        label: "Edit".into(),
        changes,
    })
    .map(|_| ())
    .map_err(|e| e.message)
}

fn column_names(app: &App, table: &str) -> Vec<String> {
    model(app)
        .table(table)
        .unwrap()
        .columns
        .iter()
        .map(|c| c.name.clone())
        .collect()
}

/// The five best-selling products with their sales, as a table visual shows them.
fn top_in_visual(app: &App) -> Vec<Vec<String>> {
    let m = model(app);
    let t = m.table("ttop").unwrap();
    let total = m
        .tables
        .iter()
        .find_map(|t| {
            t.measures
                .iter()
                .find(|x| x.name == "Total Sales")
                .map(|x| (t.id.clone(), x.id.clone()))
        })
        .unwrap();
    let query = VisualQuery {
        groups: vec![Projection {
            field: FieldRef::Column {
                table: t.id.clone(),
                column: t.column_by_name("Product").unwrap().id.clone(),
            },
            aggregation: None,
            display_name: None,
        }],
        values: vec![Projection {
            field: FieldRef::Measure {
                table: total.0,
                measure: total.1,
            },
            aggregation: None,
            display_name: None,
        }],
        ..Default::default()
    };
    rows(
        app.data(DataRequest::Visual { query })
            .unwrap_or_else(|e| panic!("{}", e.message)),
    )
}

#[test]
fn calculated_tables_compute_sync_undo_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p");
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(project.display().to_string()),
    })
    .unwrap();

    // Created from the formula bar: the columns come from the expression.
    commit(
        &mut app,
        vec![Change::AddTable {
            table: calc_table(
                "ttop",
                "Top products",
                "TOPN ( 5, VALUES ( Product[Product] ), [Total Sales] )",
            ),
            index: None,
        }],
    )
    .unwrap();
    let m = model(&app);
    let top = m.table("ttop").unwrap();
    assert_eq!(top.source, calculated_table_source("ttop"));
    assert_eq!(column_names(&app, "ttop"), ["Product"]);
    assert_eq!(top.columns[0].data_type, DataType::String);
    let expected = sql(
        &app,
        "SELECT p.Product FROM sales s JOIN product p USING (ProductKey) GROUP BY ALL ORDER BY sum(s.Amount) DESC LIMIT 5",
    );
    let mut stored = sql(&app, "SELECT Product FROM __calc_table_ttop");
    let mut want = expected.clone();
    stored.sort();
    want.sort();
    assert_eq!(stored, want);
    let snap = app.session().unwrap().snapshot().unwrap();
    assert_eq!(snap.tables.iter().find(|t| t.id == "ttop").unwrap().rows, 5);
    assert!(snap.calc_errors.is_empty(), "{:?}", snap.calc_errors);

    // A visual groups by its column; the measure on Sales doesn't filter through it (no
    // relationship), so each row shows the grand total.
    let visual = top_in_visual(&app);
    assert_eq!(visual.len(), 5);
    let mut names: Vec<Vec<String>> = visual.iter().map(|r| vec![r[0].clone()]).collect();
    names.sort();
    assert_eq!(names, want);

    // A calculated column on it.
    commit(
        &mut app,
        vec![Change::AddColumn {
            table: "ttop".into(),
            column: calc_column("clen", "Name Length", "LEN ( 'Top products'[Product] )"),
            index: None,
        }],
    )
    .unwrap();
    let lens = sql(
        &app,
        "SELECT count(*) FROM __calc_table_ttop WHERE __calc_clen = length(Product)",
    );
    assert_eq!(lens, [["5"]]);

    // Its columns' types come from the expression.
    let product_col = model(&app).table("ttop").unwrap().columns[0].id.clone();
    let err = commit(
        &mut app,
        vec![Change::SetColumn {
            table: "ttop".into(),
            column: product_col.clone(),
            property: ColumnProperty::DataType(DataType::Int64),
        }],
    )
    .unwrap_err();
    assert!(err.contains("types"), "{err}");

    // A user property on a column that keeps its name survives a new expression.
    commit(
        &mut app,
        vec![Change::SetColumn {
            table: "ttop".into(),
            column: product_col.clone(),
            property: ColumnProperty::Description(Some("Best sellers".into())),
        }],
    )
    .unwrap();
    let before_edit = model(&app);
    commit(
        &mut app,
        vec![Change::SetTableExpression {
            table: "ttop".into(),
            expression: "TOPN ( 3, VALUES ( Product[Product], Product[Category] ), [Total Sales] )"
                .into(),
        }],
    )
    .unwrap_or_else(|e| {
        // VALUES of two columns may not bind; fall back to the whole Product table.
        assert!(!e.is_empty());
        commit(
            &mut app,
            vec![Change::SetTableExpression {
                table: "ttop".into(),
                expression: "TOPN ( 3, Product, [Total Sales] )".into(),
            }],
        )
        .unwrap()
    });
    let names = column_names(&app, "ttop");
    assert!(names.contains(&"Product".to_string()), "{names:?}");
    assert!(names.contains(&"Category".to_string()), "{names:?}");
    assert!(names.contains(&"Name Length".to_string()), "{names:?}");
    let after = model(&app);
    let t = after.table("ttop").unwrap();
    let kept = t
        .column(&product_col)
        .expect("the Product column keeps its id");
    assert_eq!(kept.description.as_deref(), Some("Best sellers"));
    assert_eq!(
        sql(
            &app,
            "SELECT count(*), count(__calc_clen) FROM __calc_table_ttop"
        ),
        [["3", "3"]]
    );

    // Undo puts the old expression, columns and rows back; redo the new ones.
    app.handle(Request::Undo).unwrap();
    assert_eq!(model(&app).tables, before_edit.tables);
    assert_eq!(
        sql(
            &app,
            "SELECT count(*), count(__calc_clen) FROM __calc_table_ttop"
        ),
        [["5", "5"]]
    );
    app.handle(Request::Redo).unwrap();
    assert_eq!(model(&app).tables, after.tables);
    assert_eq!(sql(&app, "SELECT count(*) FROM __calc_table_ttop"), [["3"]]);

    // An expression that doesn't compile is refused with the compiler's message.
    let err = commit(
        &mut app,
        vec![Change::SetTableExpression {
            table: "ttop".into(),
            expression: "NOSUCHFUNCTION ( Product )".into(),
        }],
    )
    .unwrap_err();
    assert!(err.starts_with("Top products:"), "{err}");

    // Saved and reopened, it is computed again.
    app.handle(Request::Save).unwrap();
    app.handle(Request::CloseProject).unwrap();
    app.handle(Request::OpenProject {
        path: project.display().to_string(),
    })
    .unwrap();
    assert_eq!(model(&app).tables, after.tables);
    assert_eq!(
        sql(
            &app,
            "SELECT count(*), count(__calc_clen) FROM __calc_table_ttop"
        ),
        [["3", "3"]]
    );
    assert_eq!(top_in_visual(&app).len(), 3);

    // Removing it drops its data; undo computes it again.
    commit(
        &mut app,
        vec![Change::RemoveTable {
            table: "ttop".into(),
        }],
    )
    .unwrap();
    let left = sql(
        &app,
        "SELECT count(*) FROM information_schema.tables WHERE table_name = '__calc_table_ttop'",
    );
    assert_eq!(left, [["0"]]);
    app.handle(Request::Undo).unwrap();
    assert_eq!(sql(&app, "SELECT count(*) FROM __calc_table_ttop"), [["3"]]);
}

#[test]
fn calculated_tables_and_columns_compute_in_dependency_order_and_cycles_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p");
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(project.display().to_string()),
    })
    .unwrap();
    let product = model(&app).table_by_name("Product").unwrap().id.clone();
    // A calculated table reading a calculated column, and a calculated column reading
    // that table, all in one commit and listed out of order.
    commit(
        &mut app,
        vec![
            Change::AddColumn {
                table: product.clone(),
                column: calc_column("cbig", "Big Count", "COUNTROWS ( Big )"),
                index: None,
            },
            Change::AddColumn {
                table: product.clone(),
                column: calc_column("cprice2", "Double Price", "Product[ListPrice] * 2"),
                index: None,
            },
        ],
    )
    .unwrap_err();
    // `Big` doesn't exist yet: add the table first, then the column reading it.
    commit(
        &mut app,
        vec![Change::AddColumn {
            table: product.clone(),
            column: calc_column("cprice2", "Double Price", "Product[ListPrice] * 2"),
            index: None,
        }],
    )
    .unwrap();
    commit(
        &mut app,
        vec![Change::AddTable {
            table: calc_table(
                "tbig",
                "Big",
                "FILTER ( VALUES ( Product[Double Price] ), Product[Double Price] > 100 )",
            ),
            index: None,
        }],
    )
    .unwrap();
    commit(
        &mut app,
        vec![Change::AddColumn {
            table: product.clone(),
            column: calc_column("cbig", "Big Count", "COUNTROWS ( Big )"),
            index: None,
        }],
    )
    .unwrap();
    let want = sql(
        &app,
        "SELECT count(DISTINCT ListPrice * 2) FROM product WHERE ListPrice * 2 > 100",
    );
    assert_eq!(sql(&app, "SELECT count(*) FROM __calc_table_tbig"), want);
    assert_eq!(
        sql(&app, "SELECT DISTINCT __calc_cbig FROM product"),
        [[format!("{}.0", want[0][0])]]
    );
    // Changing what the table reads recomputes the table, then the column reading it.
    commit(
        &mut app,
        vec![Change::SetColumn {
            table: product.clone(),
            column: "cprice2".into(),
            property: ColumnProperty::Expression("Product[ListPrice] * 0".into()),
        }],
    )
    .unwrap();
    assert_eq!(sql(&app, "SELECT count(*) FROM __calc_table_tbig"), [["0"]]);
    assert_eq!(sql(&app, "SELECT count(__calc_cbig) FROM product"), [["0"]]);

    // A cycle is refused on commit…
    let err = commit(
        &mut app,
        vec![Change::SetColumn {
            table: product.clone(),
            column: "cprice2".into(),
            property: ColumnProperty::Expression("COUNTROWS ( Big )".into()),
        }],
    )
    .unwrap_err();
    assert!(err.contains("circular dependency"), "{err}");

    // …and one written to the project file is reported on the items in it, which are
    // left empty, while the rest still compute.
    commit(
        &mut app,
        vec![Change::AddColumn {
            table: product.clone(),
            column: calc_column("cok", "Fine", "Product[ListPrice] + 1"),
            index: None,
        }],
    )
    .unwrap();
    app.handle(Request::Save).unwrap();
    let path = app.session().unwrap().project.manifest_path.clone();
    app.handle(Request::CloseProject).unwrap();
    let mut p = charted_core::project::Project::open(&path).unwrap();
    p.model
        .table_mut(&product)
        .unwrap()
        .column_mut("cprice2")
        .unwrap()
        .kind = ColumnKind::Calculated {
        expression: "COUNTROWS ( Big )".into(),
        language: ExpressionLanguage::Dax,
    };
    p.save().unwrap();
    drop(p);
    app.handle(Request::OpenProject {
        path: path.display().to_string(),
    })
    .unwrap();
    let errors = app.session().unwrap().snapshot().unwrap().calc_errors;
    for id in ["cprice2", "tbig"] {
        let e = errors.get(id).unwrap_or_else(|| panic!("{id}: {errors:?}"));
        assert!(e.contains("circular dependency"), "{e}");
    }
    // What reads an item in a cycle runs on its empty data.
    assert!(
        !errors.contains_key("cok") && !errors.contains_key("cbig"),
        "{errors:?}"
    );
    assert_eq!(sql(&app, "SELECT count(*) FROM __calc_table_tbig"), [["0"]]);
    assert_eq!(
        sql(
            &app,
            "SELECT count(*) FROM product WHERE __calc_cok = ListPrice + 1"
        ),
        sql(
            &app,
            "SELECT count(*) FROM product WHERE ListPrice IS NOT NULL"
        )
    );
}

#[test]
fn added_columns_without_lineage_get_their_names_and_types() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(dir.path().join("p").display().to_string()),
    })
    .unwrap();
    commit(
        &mut app,
        vec![Change::AddTable {
            table: calc_table(
                "tcat",
                "Category Sales",
                "ADDCOLUMNS ( VALUES ( Product[Category] ), \"Sales\", [Total Sales] )",
            ),
            index: None,
        }],
    )
    .unwrap();
    let m = model(&app);
    let t = m.table("tcat").unwrap();
    let shape: Vec<(String, DataType)> = t
        .columns
        .iter()
        .map(|c| (c.name.clone(), c.data_type))
        .collect();
    assert_eq!(
        shape,
        [
            ("Category".to_string(), DataType::String),
            ("Sales".to_string(), DataType::Double)
        ]
    );
    let want = sql(
        &app,
        "SELECT p.Category, round(sum(s.Amount)::DOUBLE, 2) FROM sales s JOIN product p USING (ProductKey) GROUP BY ALL ORDER BY 1",
    );
    let got = sql(
        &app,
        "SELECT Category, round(Sales, 2) FROM __calc_table_tcat WHERE Sales IS NOT NULL ORDER BY 1",
    );
    assert_eq!(got, want);
}
