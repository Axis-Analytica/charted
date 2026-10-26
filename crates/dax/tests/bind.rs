use charted_dax::{
    AggFunc, Anchor, Bound, DateOp, FilterArg, FormatSpec, Period, ScalarFunc, TableExpr, check,
};
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
            Table {
                id: "t-d".into(),
                name: "Date".into(),
                description: None,
                source: "date".into(),
                columns: vec![
                    col("c-date", "Date", DataType::Date),
                    col("c-ship", "Ship Date", DataType::Date),
                ],
                measures: vec![],
                hidden: false,
                is_date_table: true,
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

fn func(src: &str) -> ScalarFunc {
    match ok(src) {
        Bound::Call { func, .. } => func,
        other => panic!("{src}: not a call: {other:?}"),
    }
}

#[test]
fn binds_text_math_and_date_functions() {
    for src in [
        "UPPER ( SELECTEDVALUE ( Product[Category] ) ) & LOWER ( \"A\" )",
        "LEN ( TRIM ( \" a  b \" ) ) + INT ( [Total] / 3 )",
        "LEFT ( \"abc\" ) & RIGHT ( \"abc\", 2 ) & MID ( \"abcdef\", 2, 3 )",
        "SUBSTITUTE ( \"a-b-c\", \"-\", \"+\" ) & SUBSTITUTE ( \"a-b-c\", \"-\", \"+\", 2 )",
        "EOMONTH ( MAX ( 'Date'[Date] ), -1 )",
        "EDATE ( DATE ( 2024, 13, 1 ), 2 )",
    ] {
        ok(src);
    }
    assert!(matches!(ok("PI ()"), Bound::Number(x) if x == std::f64::consts::PI));
    assert!(matches!(
        ok("CONCATENATE ( \"a\", [Total] )"),
        Bound::Binary {
            op: charted_dax::BinaryOp::Concat,
            ..
        }
    ));
    // MIN and MAX of a column aggregate; of two values they compare.
    assert!(matches!(
        ok("MIN ( 'Sales Lines'[Qty] )"),
        Bound::Aggregate {
            func: AggFunc::Min,
            ..
        }
    ));
    assert_eq!(func("MIN ( [Total], 0 )"), ScalarFunc::Min);
    assert_eq!(func("MAX ( [Total], BLANK () )"), ScalarFunc::Max);
    assert_eq!(
        func("DATEDIFF ( MIN ( 'Date'[Date] ), MAX ( 'Date'[Date] ), quarter )"),
        ScalarFunc::DateDiff(Period::Quarter)
    );
    assert!(matches!(
        func("FORMAT ( [Total], \"$#,0.0%\" )"),
        ScalarFunc::Format(FormatSpec::Number {
            decimals: 1,
            thousands: true,
            percent: true,
            prefix: "$"
        })
    ));
    assert!(matches!(
        func("FORMAT ( MAX ( 'Date'[Date] ), \"mmm yyyy\" )"),
        ScalarFunc::Format(FormatSpec::Date(_))
    ));
    assert!(err("FORMAT ( [Total], \"0.##\" )").contains("\"0.##\" isn't supported yet"));
    assert!(err("FORMAT ( [Total], \"0\", \"en-US\" )").contains("takes 2 argument(s)"));
    assert!(err("VAR f = \"0\" RETURN FORMAT ( [Total], f )").contains("written out as text"));
    assert!(err("DATEDIFF ( 1, 2, WEEK )").contains("DAY, MONTH, QUARTER or YEAR"));
    assert!(err("MID ( \"abc\", 2 )").contains("takes 3 argument(s)"));
}

/// The first time-intelligence filter in an expression.
fn dates_op(src: &str) -> DateOp {
    let mut found = None;
    charted_dax::bind::walk(&ok(src), &mut |b| {
        if let Bound::Calculate { filters, .. } = b
            && let Some(FilterArg::Table(TableExpr::Dates { op, .. })) = filters.first()
            && found.is_none()
        {
            found = Some(op.clone());
        }
    });
    found.unwrap_or_else(|| panic!("{src}: no time-intelligence filter"))
}

#[test]
fn binds_rolling_periods_and_date_ranges() {
    assert_eq!(
        dates_op(
            "CALCULATE ( [Total], DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -12, MONTH ) )"
        ),
        DateOp::InPeriod(Anchor::Last, -12, Period::Month)
    );
    // A variable defined just outside CALCULATE stands for its definition.
    assert_eq!(
        dates_op(
            "VAR d = FIRSTDATE ( 'Date'[Date] ) RETURN CALCULATE ( [Total], DATESINPERIOD ( 'Date'[Date], d, 2, QUARTER ) )"
        ),
        DateOp::InPeriod(Anchor::First, 2, Period::Quarter)
    );
    assert_eq!(
        dates_op(
            "CALCULATE ( [Total], DATESBETWEEN ( 'Date'[Date], DATE ( 2024, 1, 1 ), BLANK () ) )"
        ),
        DateOp::Between(Anchor::Date(2024, 1, 1), Anchor::Open)
    );
    // A variable's definition sees only the variables defined before it.
    assert_eq!(
        dates_op(
            "VAR d = MAX ( 'Date'[Date] ) VAR d = d RETURN CALCULATE ( [Total], DATESBETWEEN ( 'Date'[Date], d, d ) )"
        ),
        DateOp::Between(Anchor::Last, Anchor::Last)
    );
    // Other dates are expressions evaluated in the outer context.
    let expr = |op: DateOp| match op {
        DateOp::InPeriod(Anchor::Expr(e), ..) | DateOp::Between(Anchor::Expr(e), _) => *e,
        other => panic!("not an expression anchor: {other:?}"),
    };
    assert!(matches!(
        expr(dates_op(
            "CALCULATE ( [Total], DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Ship Date] ), -1, YEAR ) )"
        )),
        Bound::Aggregate { .. }
    ));
    assert!(matches!(
        expr(dates_op(
            "CALCULATE ( [Total], DATESINPERIOD ( 'Date'[Date], EOMONTH ( MAX ( 'Date'[Date] ), -1 ), -3, MONTH ) )"
        )),
        Bound::Call {
            func: ScalarFunc::EoMonth,
            ..
        }
    ));
    assert!(matches!(
        expr(dates_op(
            "CALCULATE ( [Total], DATESBETWEEN ( 'Date'[Date], TODAY (), BLANK () ) )"
        )),
        Bound::Call {
            func: ScalarFunc::Today,
            ..
        }
    ));
    // Defined outside an iterator, the variable keeps its outer value.
    assert!(matches!(
        expr(dates_op(
            "VAR d = MAX ( 'Date'[Date] ) RETURN SUMX ( VALUES ( Product[Category] ), CALCULATE ( [Total], DATESINPERIOD ( 'Date'[Date], d, -1, YEAR ) ) )"
        )),
        Bound::Var(_)
    ));
}

#[test]
fn removefilters_without_arguments_clears_everything() {
    let Bound::Calculate { filters, .. } =
        ok("CALCULATE ( [Total], REMOVEFILTERS (), Product[Category] = \"A\" )")
    else {
        panic!("not a CALCULATE")
    };
    assert_eq!(filters[0], FilterArg::RemoveAll);
    assert!(matches!(
        ok("CALCULATE ( [Total], ALL () )"),
        Bound::Calculate { filters, .. } if filters == vec![FilterArg::RemoveAll]
    ));
    assert!(err("COUNTROWS ( ALL () )").contains("only works as a CALCULATE filter"));
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
    assert!(err("RANKX ( Product, [Total] )").contains("isn't supported by charted yet"));
    assert!(err("TOTALYTD ( [Total], Product[Category] )").contains("isn't a date column"));
    assert!(err("FOO ( 1 )").contains("isn't a DAX function"));
    assert!(err("DIVIDE ( 1 )").contains("takes 2 to 3 argument(s), not 1"));
    assert!(err("COUNTROWS ( [Total] )").contains("this needs a table"));
    assert!(err("SUM ( 'Sales Lines'[Amount] ").contains("expected \")\""));
}
