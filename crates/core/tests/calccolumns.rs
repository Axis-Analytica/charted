//! Calculated columns that aggregate (v1.5) on the Harbor & Pine sample: computed on
//! commit, in dependency order, and the same as the numbers computed another way.

use arrow_ipc::reader::StreamReader;
use charted_core::{App, Request};
use charted_model::*;
use std::time::Instant;

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

fn column(id: &str, name: &str, expression: &str) -> Column {
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

fn sql(app: &App, q: &str) -> Vec<Vec<String>> {
    rows(app.session().unwrap().database().arrow(q).unwrap())
}

fn add(app: &mut App, table: &str, cols: Vec<Column>) {
    let t = app
        .session()
        .unwrap()
        .project
        .model
        .table_by_name(table)
        .unwrap()
        .id
        .clone();
    app.handle(Request::Commit {
        label: "New columns".into(),
        changes: cols
            .into_iter()
            .map(|c| Change::AddColumn {
                table: t.clone(),
                column: c,
                index: None,
            })
            .collect(),
    })
    .unwrap();
}

#[test]
fn aggregating_columns_compute_on_commit() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path().join("state"));
    app.handle(Request::OpenSample {
        dir: Some(dir.path().join("p").display().to_string()),
    })
    .unwrap();
    // The rank reads the sales column, so it's computed after it.
    add(
        &mut app,
        "Product",
        vec![
            column("csales", "Product Sales", "CALCULATE ( [Total Sales] )"),
            column(
                "cunits",
                "Product Units",
                "SUMX ( RELATEDTABLE ( Sales ), Sales[Quantity] )",
            ),
            column("call", "All Units", "SUM ( Sales[Quantity] )"),
        ],
    );
    add(
        &mut app,
        "Product",
        vec![column(
            "crank",
            "Sales Rank",
            "RANKX ( ALL ( Product ), Product[Product Sales] )",
        )],
    );
    let m = app.session().unwrap().project.model.clone();
    let p = m.table_by_name("Product").unwrap();
    for id in ["crank", "csales", "cunits", "call"] {
        assert_ne!(
            p.column(id).unwrap().data_type,
            DataType::Unknown,
            "{id} is typed"
        );
    }
    // Per product, the same as SQL computes it.
    let got = sql(
        &app,
        "SELECT ProductKey, round(__calc_csales, 2), __calc_cunits, __calc_call, __calc_crank FROM product ORDER BY ProductKey",
    );
    let expected = sql(
        &app,
        "WITH s AS (SELECT ProductKey, sum(Quantity) AS q FROM sales GROUP BY 1) \
         SELECT p.ProductKey, s.q, (SELECT sum(Quantity) FROM sales) FROM product p LEFT JOIN s USING (ProductKey) ORDER BY 1",
    );
    assert_eq!(got.len(), expected.len());
    for (g, e) in got.iter().zip(&expected) {
        assert_eq!(g[0], e[0]);
        let num = |s: &str| s.parse::<f64>().unwrap_or(f64::NAN);
        assert_eq!(num(&g[2]), num(&e[1]), "units of product {}", g[0]);
        assert_eq!(num(&g[3]), num(&e[2]), "all units");
    }
    // The best seller ranks 1.
    let best = sql(
        &app,
        "SELECT __calc_crank FROM product ORDER BY __calc_csales DESC NULLS LAST LIMIT 1",
    );
    assert_eq!(best[0][0].parse::<f64>().unwrap(), 1.0);

    // A rank over 20,000 customers: time it (window rewrites make this fast).
    let t0 = Instant::now();
    add(
        &mut app,
        "Customer",
        vec![column(
            "ccustrank",
            "Spend Rank",
            "RANKX ( ALL ( Customer ), CALCULATE ( [Total Sales] ) )",
        )],
    );
    eprintln!("customer rank column: {:?}", t0.elapsed());
    let top = sql(&app, "SELECT min(__calc_ccustrank), count(*) FROM customer");
    assert_eq!(top[0][0].parse::<f64>().unwrap(), 1.0);

    // The same with EARLIER over a column of the spend.
    add(
        &mut app,
        "Customer",
        vec![column("cspend", "Spend", "CALCULATE ( [Total Sales] )")],
    );
    let t0 = Instant::now();
    add(
        &mut app,
        "Customer",
        vec![column(
            "cearlier",
            "Spend Rank 2",
            "COUNTROWS ( FILTER ( Customer, Customer[Spend] > EARLIER ( Customer[Spend] ) ) ) + 1",
        )],
    );
    eprintln!("customer EARLIER column: {:?}", t0.elapsed());
    let same = sql(
        &app,
        "SELECT count(*) FROM customer WHERE __calc_cearlier IS DISTINCT FROM __calc_ccustrank",
    );
    assert_eq!(same[0][0], "0", "EARLIER and RANKX ranks agree");
}
