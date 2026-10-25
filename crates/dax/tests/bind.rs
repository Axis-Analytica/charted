use charted_dax::{AggFunc, Bound, FilterArg, TableExpr, check};
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

fn model() -> Model {
    let measure = |id: &str, name: &str, e: &str| Measure {
        id: id.into(),
        name: name.into(),
        expression: e.into(),
        language: ExpressionLanguage::Dax,
        description: None,
        format_string: None,
        display_folder: None,
        hidden: false,
        extras: Default::default(),
    };
    Model {
        name: "m".into(),
        tables: vec![
            Table {
                id: "t-s".into(),
                name: "Sales Lines".into(),
                description: None,
                source: "sales".into(),
                columns: vec![
                    col("c-amt", "Amount", DataType::Decimal),
                    col("c-qty", "Qty", DataType::Int64),
                    col("c-ch", "Channel", DataType::String),
                ],
                measures: vec![measure("m-total", "Total", "SUM ( 'Sales Lines'[Amount] )")],
                hidden: false,
                is_date_table: false,
                extras: Default::default(),
            },
            Table {
                id: "t-p".into(),
                name: "Product".into(),
                description: None,
                source: "product".into(),
                columns: vec![col("c-cat", "Category", DataType::String)],
                measures: vec![],
                hidden: false,
                is_date_table: false,
                extras: Default::default(),
            },
        ],
        ..Default::default()
    }
}

fn ok(src: &str) -> Bound {
    let (b, d) = check(&model(), src);
    assert!(d.is_empty(), "{src}: {d:?}");
    b.unwrap()
}

fn err(src: &str) -> String {
    let (b, d) = check(&model(), src);
    assert!(b.is_none() && !d.is_empty(), "{src} should fail");
    d[0].message.clone()
}

#[test]
fn binds_aggregates_measures_and_scalars() {
    assert!(matches!(
        ok("SUM ( 'Sales Lines'[Amount] )"),
        Bound::Aggregate { func: AggFunc::Sum, source: TableExpr::Table(ref table), .. } if table == "t-s"
    ));
    // Names are case-insensitive; Table[Measure] is a measure reference.
    assert!(matches!(ok("[total]"), Bound::Measure { ref measure, .. } if measure == "m-total"));
    assert!(matches!(ok("'sales lines'[Total]"), Bound::Measure { .. }));
    ok("DIVIDE ( [Total] - SUM ( 'Sales Lines'[Qty] ), [Total], 0 )");
    ok("IF ( [Total] > 100, \"big\", BLANK () )");
    ok("SUMX ( 'Sales Lines', 'Sales Lines'[Qty] * [Amount] )");
    ok("COUNTROWS ( 'Sales Lines' ) + DISTINCTCOUNT ( Product[Category] )");
    ok("ROUND ( ABS ( [Total] ), 2 )");
    ok("COALESCE ( [Total], 0, 1 )");
    // Context transition: a measure inside an iterator.
    ok("SUMX ( VALUES ( Product[Category] ), [Total] )");
    ok("VAR t = [Total] RETURN IF ( t > 0, t, BLANK () )");
    ok("SWITCH ( TRUE (), [Total] > 10, \"big\", \"small\" )");
    ok("SELECTEDVALUE ( Product[Category], \"All\" )");
    ok("COUNTROWS ( FILTER ( 'Sales Lines', 'Sales Lines'[Qty] > 2 ) )");
}

#[test]
fn calculate_filter_arguments() {
    let Bound::Calculate { filters, .. } = ok(
        "CALCULATE ( [Total], Product[Category] = \"Packs\", ALL ( 'Sales Lines' ), KEEPFILTERS ( 'Sales Lines'[Qty] > 1 ) )",
    ) else {
        panic!("not a CALCULATE")
    };
    assert!(
        matches!(&filters[0], FilterArg::Table(TableExpr::Filter { source, .. }) if matches!(**source, TableExpr::All { ref columns, .. } if columns.len() == 1))
    );
    assert!(matches!(&filters[1], FilterArg::RemoveFilters { columns, .. } if columns.is_empty()));
    assert!(matches!(&filters[2], FilterArg::Keep(_)));
    assert!(
        err("CALCULATE ( [Total], 'Sales Lines'[Qty] > Product[Category] )")
            .contains("columns of one table")
    );
}

#[test]
fn explains_what_is_wrong() {
    assert!(err("SUM ( Sales[Amount] )").contains("no table named 'Sales'"));
    assert!(err("SUM ( 'Sales Lines'[Amt] )").contains("no column named [Amt]"));
    assert!(err("[Nope]").contains("no measure named [Nope]"));
    assert!(err("'Sales Lines'[Amount]").contains("can't be determined"));
    assert!(err("SUM ( 'Sales Lines'[Channel] )").contains("isn't numeric"));
    assert!(err("SUMX ( 'Sales Lines', Product[Category] )").contains("RELATED"));
    assert!(err("RELATED ( Product[Category] )").contains("needs a row context"));
    assert!(
        err("TOTALYTD ( [Total], Product[Category] )").contains("isn't supported by charted yet")
    );
    assert!(err("FOO ( 1 )").contains("isn't a DAX function"));
    assert!(err("DIVIDE ( 1 )").contains("takes 2 to 3 argument(s), not 1"));
    assert!(err("COUNTROWS ( [Total] )").contains("this needs a table"));
    assert!(err("SUM ( 'Sales Lines'[Amount] ").contains("expected \")\""));
}
