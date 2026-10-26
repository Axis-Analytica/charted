use charted_model::*;
use proptest::prelude::*;

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

fn fixture() -> Model {
    let sales = Table {
        id: "t-sales".into(),
        name: "Sales".into(),
        description: None,
        source: "sales".into(),
        columns: vec![
            col("c-s-date", "OrderDateKey", DataType::Int64),
            col("c-s-prod", "ProductKey", DataType::Int64),
            col("c-s-amt", "Amount", DataType::Double),
        ],
        measures: vec![Measure {
            id: "m-total".into(),
            name: "Total Sales".into(),
            expression: "SUM(Sales[Amount])".into(),
            description: None,
            format_string: Some("#,0".into()),
            display_folder: None,
            hidden: false,
            language: ExpressionLanguage::Dax,
            extras: Default::default(),
        }],
        hidden: false,
        is_date_table: false,
        extras: Default::default(),
    };
    let product = Table {
        id: "t-prod".into(),
        name: "Product".into(),
        description: None,
        source: "product".into(),
        columns: vec![
            col("c-p-key", "ProductKey", DataType::Int64),
            col("c-p-cat", "Category", DataType::String),
        ],
        measures: vec![],
        hidden: false,
        is_date_table: false,
        extras: Default::default(),
    };
    Model {
        name: "Sales".into(),
        description: None,
        tables: vec![sales, product],
        relationships: vec![Relationship {
            id: "r-1".into(),
            from_table: "t-sales".into(),
            from_column: "c-s-prod".into(),
            to_table: "t-prod".into(),
            to_column: "c-p-key".into(),
            from_cardinality: Cardinality::Many,
            to_cardinality: Cardinality::One,
            cross_filter: CrossFilter::Single,
            active: true,
            extras: Default::default(),
        }],
        extras: Default::default(),
        report: Report {
            pages: vec![Page {
                id: "p-1".into(),
                settings: PageSettings::named("Overview"),
                visuals: vec![visual("v-1")],
            }],
            filters: vec![],
            diagram: Default::default(),
            theme: None,
            bookmarks: Vec::new(),
        },
    }
}

fn visual(id: &str) -> Visual {
    Visual {
        id: id.into(),
        visual_type: "card".into(),
        position: Position {
            x: 10.0,
            y: 10.0,
            width: 200.0,
            height: 120.0,
            z: 0,
        },
        roles: [(
            "values".to_string(),
            vec![Projection {
                field: FieldRef::Column {
                    table: "t-sales".into(),
                    column: "c-s-amt".into(),
                },
                aggregation: Some(Aggregation::Sum),
                display_name: None,
            }],
        )]
        .into(),
        filters: vec![],
        sort: None,
        objects: Default::default(),
        selection: None,
        drill: Vec::new(),
        hidden: false,
    }
}

fn arb_change() -> impl Strategy<Value = Change> {
    let table = prop_oneof![
        Just("t-sales".to_string()),
        Just("t-prod".to_string()),
        Just("t-missing".to_string())
    ];
    let column = prop_oneof![
        Just("c-s-date".to_string()),
        Just("c-s-prod".to_string()),
        Just("c-s-amt".to_string()),
        Just("c-p-key".to_string()),
        Just("c-p-cat".to_string()),
    ];
    let name = prop_oneof![
        Just("Sales".to_string()),
        Just("Facts".to_string()),
        Just(" bad".to_string()),
        "[A-Z][a-z]{1,8}"
    ];
    prop_oneof![
        (table.clone(), name.clone()).prop_map(|(table, name)| Change::RenameTable { table, name }),
        (table.clone(), column.clone(), name.clone()).prop_map(|(table, column, name)| {
            Change::RenameColumn {
                table,
                column,
                name,
            }
        }),
        (
            table.clone(),
            column.clone(),
            proptest::option::of("[#,0.%]{1,6}")
        )
            .prop_map(|(table, column, f)| Change::SetColumn {
                table,
                column,
                property: ColumnProperty::FormatString(f)
            }),
        (table.clone(), column.clone(), any::<bool>()).prop_map(|(table, column, h)| {
            Change::SetColumn {
                table,
                column,
                property: ColumnProperty::Hidden(h),
            }
        }),
        (
            table.clone(),
            column.clone(),
            proptest::option::of(column.clone())
        )
            .prop_map(|(table, column, s)| Change::SetColumn {
                table,
                column,
                property: ColumnProperty::SortBy(s)
            }),
        (table.clone(), column.clone())
            .prop_map(|(table, column)| Change::RemoveColumn { table, column }),
        table
            .clone()
            .prop_map(|table| Change::RemoveTable { table }),
        Just(Change::RemoveRelationship {
            relationship: "r-1".into()
        }),
        (table.clone(), name.clone()).prop_map(|(table, name)| Change::AddMeasure {
            table,
            measure: Measure {
                id: format!("m-{name}"),
                name,
                expression: "1".into(),
                description: None,
                format_string: None,
                display_folder: None,
                hidden: false,
                language: ExpressionLanguage::Dax,
                extras: Default::default(),
            },
            index: None,
        }),
        (table, any::<bool>()).prop_map(|(table, hidden)| Change::SetTableHidden { table, hidden }),
        report_change(),
    ]
}

fn report_change() -> impl Strategy<Value = Change> {
    let page = prop_oneof![
        Just("p-1".to_string()),
        Just("p-2".to_string()),
        Just("p-missing".to_string())
    ];
    let vis = prop_oneof![
        Just("v-1".to_string()),
        Just("v-2".to_string()),
        Just("v-missing".to_string())
    ];
    let name = prop_oneof![
        Just("Overview".to_string()),
        Just("Details".to_string()),
        Just("".to_string())
    ];
    prop_oneof![
        (page.clone(), name.clone()).prop_map(|(id, n)| Change::AddPage {
            page: Page {
                id,
                settings: PageSettings::named(n),
                visuals: vec![]
            },
            index: Some(0),
        }),
        page.clone().prop_map(|page| Change::RemovePage { page }),
        (page.clone(), name).prop_map(|(page, n)| Change::SetPage {
            page,
            settings: PageSettings::named(n)
        }),
        (page.clone(), 0usize..3).prop_map(|(page, index)| Change::MovePage { page, index }),
        (page.clone(), vis.clone()).prop_map(|(page, v)| Change::AddVisual {
            page,
            visual: visual(&v),
            index: None
        }),
        (page.clone(), vis.clone())
            .prop_map(|(page, visual)| Change::RemoveVisual { page, visual }),
        (page.clone(), vis, 0.0f64..500.0, any::<bool>(), -3i32..3).prop_map(
            |(page, v, x, hidden, z)| {
                let mut visual = visual(&v);
                visual.position.x = x;
                visual.position.z = z;
                visual.hidden = hidden;
                Change::UpdateVisual { page, visual }
            }
        ),
        (page, any::<bool>()).prop_map(|(page, hidden)| Change::SetPage {
            page,
            settings: PageSettings {
                hidden,
                ..PageSettings::named("Overview")
            }
        }),
        Just(Change::SetReportFilters { filters: vec![] }),
    ]
}

proptest! {
    /// Applying any change and then its inverse restores the exact model, and a failed
    /// change leaves the model untouched.
    #[test]
    fn inverse_restores_model(changes in proptest::collection::vec(arb_change(), 1..12)) {
        let mut model = fixture();
        for change in changes {
            let before = model.clone();
            match apply(&mut model, change) {
                Ok(inverse) => {
                    let after = model.clone();
                    let mut reverted = after.clone();
                    for c in inverse { apply(&mut reverted, c).unwrap(); }
                    prop_assert_eq!(&reverted, &before);
                }
                Err(_) => prop_assert_eq!(&model, &before),
            }
        }
    }

    /// Undoing everything returns to the start; redoing everything returns to the end.
    #[test]
    fn undo_redo_round_trip(changes in proptest::collection::vec(arb_change(), 1..12)) {
        let start = fixture();
        let mut model = start.clone();
        let mut history = History::new();
        for c in changes {
            let _ = history.commit(&mut model, "edit", vec![c]);
        }
        let end = model.clone();
        while history.undo(&mut model).unwrap().is_some() {}
        prop_assert_eq!(&model, &start);
        prop_assert!(!history.is_dirty());
        while history.redo(&mut model).unwrap().is_some() {}
        prop_assert_eq!(&model, &end);
    }
}

#[test]
fn removing_a_table_takes_its_relationships_and_undo_restores_them() {
    let mut model = fixture();
    let mut history = History::new();
    history
        .commit(
            &mut model,
            "Delete table",
            vec![Change::RemoveTable {
                table: "t-prod".into(),
            }],
        )
        .unwrap();
    assert!(model.relationships.is_empty());
    assert!(history.is_dirty());
    history.undo(&mut model).unwrap();
    assert_eq!(model, fixture());
}

#[test]
fn a_failing_transaction_rolls_back_completely() {
    let mut model = fixture();
    let mut history = History::new();
    let err = history.commit(
        &mut model,
        "Rename two things",
        vec![
            Change::RenameTable {
                table: "t-sales".into(),
                name: "Facts".into(),
            },
            Change::RenameTable {
                table: "t-prod".into(),
                name: "Facts".into(),
            },
        ],
    );
    assert!(matches!(err, Err(ChangeError::NameTaken { .. })));
    assert_eq!(model, fixture());
    assert!(history.undo_label().is_none());
}

#[test]
fn names_are_validated() {
    let mut model = fixture();
    for bad in ["", "  x", "x ", "a\u{7}b"] {
        let r = apply(
            &mut model,
            Change::RenameTable {
                table: "t-sales".into(),
                name: bad.into(),
            },
        );
        assert!(matches!(r, Err(ChangeError::InvalidName { .. })), "{bad:?}");
    }
    let r = apply(
        &mut model,
        Change::RenameColumn {
            table: "t-sales".into(),
            column: "c-s-amt".into(),
            name: "total sales".into(),
        },
    );
    assert!(
        matches!(r, Err(ChangeError::NameTaken { .. })),
        "columns and measures share a namespace"
    );
}
