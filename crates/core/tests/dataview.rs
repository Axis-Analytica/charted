//! Data view queries against the Harbor & Pine sample: paging, sort, filters, distinct
//! values, statistics, find, and the data checks on column changes.

use arrow_ipc::reader::StreamReader;
use charted_core::dataview::*;
use charted_core::{App, DataRequest, Request};
use charted_model::{Change, ColumnProperty, DataType, Model};
use serde_json::Value;
use std::sync::{Mutex, MutexGuard, OnceLock};

struct Fixture {
    app: App,
    _dir: tempfile::TempDir,
}

/// One sample shared by every test (generating it takes a couple of seconds).
fn fixture() -> MutexGuard<'static, Fixture> {
    static F: OnceLock<Mutex<Fixture>> = OnceLock::new();
    F.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state");
        let mut app = App::new(state);
        let project = dir.path().join("sample");
        app.handle(Request::OpenSample {
            dir: Some(project.display().to_string()),
        })
        .unwrap();
        Mutex::new(Fixture { app, _dir: dir })
    })
    .lock()
    .unwrap_or_else(|p| p.into_inner())
}

fn model(f: &Fixture) -> Model {
    f.app.session().unwrap().project.model.clone()
}

/// (table id, column id) by names.
fn ids(f: &Fixture, table: &str, column: &str) -> (String, String) {
    let m = model(f);
    let t = m.table_by_name(table).unwrap();
    (t.id.clone(), t.column_by_name(column).unwrap().id.clone())
}

fn spec(table: &str) -> ViewSpec {
    ViewSpec {
        table: table.into(),
        sort: vec![],
        filters: vec![],
    }
}

/// One column of an Arrow page, as JSON-ish values via Arrow's display.
fn column_values(bytes: Vec<u8>, column: &str) -> Vec<String> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let col = batch.column_by_name(column).expect("column in page");
        let fmt = arrow_cast::display::ArrayFormatter::try_new(col, &Default::default()).unwrap();
        for i in 0..batch.num_rows() {
            out.push(fmt.value(i).to_string());
        }
    }
    out
}

fn json(f: &Fixture, r: DataRequest) -> Value {
    serde_json::from_slice(&f.app.data(r).unwrap()).unwrap()
}

fn rows(f: &Fixture, view: &ViewSpec, offset: u64, limit: u32, column: &str) -> Vec<String> {
    column_values(
        f.app
            .data(DataRequest::Rows {
                view: view.clone(),
                columns: None,
                offset,
                limit,
            })
            .unwrap(),
        column,
    )
}

#[test]
fn pages_follow_the_source_order_without_sort() {
    let f = fixture();
    let (sales, line) = ids(&f, "Sales", "OrderNumber");
    let v = spec(&sales);
    let all = rows(&f, &v, 0, 400, &line);
    assert_eq!(all.len(), 400);
    assert_eq!(rows(&f, &v, 200, 200, &line), all[200..]);
    let total = json(
        &f,
        DataRequest::Count {
            view: v.clone(),
            column: None,
        },
    )["rows"]
        .as_u64()
        .unwrap();
    // The last page is short; past the end is empty.
    assert_eq!(rows(&f, &v, total - 5, 200, &line).len(), 5);
    assert!(rows(&f, &v, total + 10, 200, &line).is_empty());
}

#[test]
fn multi_column_sort_pages_are_consistent() {
    let f = fixture();
    let (sales, price) = ids(&f, "Sales", "UnitPrice");
    let (_, qty) = ids(&f, "Sales", "Quantity");
    let v = ViewSpec {
        sort: vec![
            SortKey {
                column: qty.clone(),
                descending: true,
            },
            SortKey {
                column: price.clone(),
                descending: false,
            },
        ],
        ..spec(&sales)
    };
    let q: Vec<i64> = rows(&f, &v, 0, 1000, &qty)
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert!(q.windows(2).all(|w| w[0] >= w[1]), "quantity descending");
    let p: Vec<f64> = rows(&f, &v, 0, 1000, &price)
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    for i in 1..q.len() {
        if q[i] == q[i - 1] {
            assert!(p[i] >= p[i - 1], "price ascending within quantity");
        }
    }
    assert_eq!(
        rows(&f, &v, 500, 100, &price),
        rows(&f, &v, 0, 1000, &price)[500..600]
    );
}

#[test]
fn sort_by_column_orders_month_names_by_number() {
    let f = fixture();
    let (date, month) = ids(&f, "Date", "MonthName");
    let v = ViewSpec {
        sort: vec![SortKey {
            column: month.clone(),
            descending: false,
        }],
        ..spec(&date)
    };
    let names = rows(&f, &v, 0, 20, &month);
    assert!(names.iter().all(|n| n == "January"), "{names:?}");
}

#[test]
fn value_and_condition_filters() {
    let f = fixture();
    let (sales, channel) = ids(&f, "Sales", "Channel");
    let (_, qty) = ids(&f, "Sales", "Quantity");
    let count = |v: ViewSpec| {
        json(
            &f,
            DataRequest::Count {
                view: v,
                column: None,
            },
        )["rows"]
            .as_u64()
            .unwrap()
    };
    let db = f.app.session().unwrap().database().clone();
    let online = db
        .query_u64("SELECT count(*) FROM sales WHERE Channel = 'Online'")
        .unwrap();
    let only = |values: Vec<Option<String>>, exclude| ViewSpec {
        filters: vec![ColumnFilter {
            column: channel.clone(),
            filter: Filter::Values { values, exclude },
        }],
        ..spec(&sales)
    };
    assert_eq!(count(only(vec![Some("Online".into())], false)), online);
    let all = count(spec(&sales));
    assert_eq!(count(only(vec![Some("Online".into())], true)), all - online);
    assert_eq!(count(only(vec![], false)), 0);

    let big = db
        .query_u64("SELECT count(*) FROM sales WHERE Quantity >= 3 AND Channel = 'Online'")
        .unwrap();
    let mut v = only(vec![Some("Online".into())], false);
    v.filters.push(ColumnFilter {
        column: qty.clone(),
        filter: Filter::Condition {
            op: FilterOp::GreaterOrEqual,
            value: Some("3".into()),
            value2: None,
        },
    });
    assert_eq!(count(v.clone()), big);
    // Filtered pages contain only matching rows.
    assert!(rows(&f, &v, 0, 500, &channel).iter().all(|c| c == "Online"));

    let contains = ViewSpec {
        filters: vec![ColumnFilter {
            column: channel.clone(),
            filter: Filter::Condition {
                op: FilterOp::Contains,
                value: Some("ONLI".into()),
                value2: None,
            },
        }],
        ..spec(&sales)
    };
    assert_eq!(count(contains), online);

    let bad = ViewSpec {
        filters: vec![ColumnFilter {
            column: qty,
            filter: Filter::Condition {
                op: FilterOp::Less,
                value: Some("lots".into()),
                value2: None,
            },
        }],
        ..spec(&sales)
    };
    let err = f
        .app
        .data(DataRequest::Count {
            view: bad,
            column: None,
        })
        .unwrap_err();
    assert!(
        err.message.contains("not a valid whole number"),
        "{}",
        err.message
    );
}

#[test]
fn distinct_values_ignore_their_own_filter() {
    let f = fixture();
    let (sales, channel) = ids(&f, "Sales", "Channel");
    let v = ViewSpec {
        filters: vec![ColumnFilter {
            column: channel.clone(),
            filter: Filter::Values {
                values: vec![Some("Online".into())],
                exclude: false,
            },
        }],
        ..spec(&sales)
    };
    let d = json(
        &f,
        DataRequest::Distinct {
            view: v.clone(),
            column: channel.clone(),
            search: None,
            limit: 1000,
        },
    );
    let keys: Vec<&str> = d["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["key"].as_str().unwrap())
        .collect();
    assert!(keys.len() > 1 && keys.contains(&"Online"), "{keys:?}");
    assert_eq!(d["total"].as_u64().unwrap() as usize, keys.len());
    let mut sorted = keys.clone();
    sorted.sort_by_key(|k| k.to_lowercase());
    assert_eq!(keys, sorted);

    let searched = json(
        &f,
        DataRequest::Distinct {
            view: v,
            column: channel,
            search: Some("onl".into()),
            limit: 1000,
        },
    );
    assert_eq!(searched["values"].as_array().unwrap().len(), 1);
}

#[test]
fn statistics_by_type() {
    let f = fixture();
    let (sales, price) = ids(&f, "Sales", "UnitPrice");
    let s = json(
        &f,
        DataRequest::Stats {
            view: spec(&sales),
            column: price,
        },
    );
    assert!(s["count"].as_u64().unwrap() > 1000);
    let (min, max, mean) = (
        s["min"].as_f64().unwrap(),
        s["max"].as_f64().unwrap(),
        s["mean"].as_f64().unwrap(),
    );
    assert!(min <= mean && mean <= max);
    assert!(s["p25"].as_f64().unwrap() <= s["p75"].as_f64().unwrap());
    let bins = s["histogram"].as_array().unwrap();
    assert_eq!(bins.len(), 20);
    let in_bins: u64 = bins.iter().map(|b| b["count"].as_u64().unwrap()).sum();
    assert_eq!(in_bins, s["count"].as_u64().unwrap());

    let (date, d) = ids(&f, "Date", "Date");
    let s = json(
        &f,
        DataRequest::Stats {
            view: spec(&date),
            column: d,
        },
    );
    assert!(s["spanDays"].as_i64().unwrap() > 365);
    assert!(s["min"].as_str().unwrap().starts_with("20"));

    let (_, month) = ids(&f, "Date", "MonthName");
    let s = json(
        &f,
        DataRequest::Stats {
            view: spec(&date),
            column: month,
        },
    );
    assert_eq!(s["distinct"].as_u64(), Some(12));
    assert_eq!(s["top"].as_array().unwrap().len(), 10);

    let (_, year) = ids(&f, "Date", "Year");
    let s = json(
        &f,
        DataRequest::Stats {
            view: spec(&date),
            column: year,
        },
    );
    // A short whole-number range gets one bin per value.
    assert_eq!(
        s["histogram"].as_array().unwrap().len() as u64,
        s["distinct"].as_u64().unwrap()
    );
}

#[test]
fn find_walks_matches_in_view_order() {
    let f = fixture();
    let (store, city) = ids(&f, "Store", "City");
    let v = ViewSpec {
        sort: vec![SortKey {
            column: city.clone(),
            descending: true,
        }],
        ..spec(&store)
    };
    let cities = rows(&f, &v, 0, 1000, &city);
    let target = cities[cities.len() / 2].clone();
    let needle = target.to_uppercase();
    let find = |after| {
        json(
            &f,
            DataRequest::Find {
                view: v.clone(),
                text: needle.clone(),
                columns: Some(vec![city.clone()]),
                after,
                backwards: false,
            },
        )
    };
    let first = find(None);
    let row = first["row"].as_u64().unwrap();
    assert!(cities[row as usize].to_uppercase().contains(&needle));
    assert_eq!(first["column"].as_str(), Some(city.as_str()));
    let expected = cities
        .iter()
        .filter(|c| c.to_uppercase().contains(&needle))
        .count() as u64;
    assert_eq!(first["matches"].as_u64(), Some(expected));
    if expected == 1 {
        assert!(find(Some(row)).is_null());
    } else {
        assert!(find(Some(row))["row"].as_u64().unwrap() > row);
    }
}

#[test]
fn type_changes_and_sort_by_are_checked_against_the_data() {
    let mut f = fixture();
    let (sales, channel) = ids(&f, "Sales", "Channel");
    let (date, month) = ids(&f, "Date", "MonthName");
    let (_, weekday) = ids(&f, "Date", "Weekday");
    let (_, number) = ids(&f, "Date", "MonthNumber");
    let (_, year) = ids(&f, "Date", "Year");
    let set = |table: &str, column: &str, property| Request::Commit {
        label: "Change column".into(),
        changes: vec![Change::SetColumn {
            table: table.into(),
            column: column.into(),
            property,
        }],
    };

    let err = f
        .app
        .handle(set(
            &sales,
            &channel,
            ColumnProperty::DataType(DataType::Int64),
        ))
        .unwrap_err();
    assert!(err.message.contains("don't convert"), "{}", err.message);

    // Whole numbers become text; the grid then shows text values.
    f.app
        .handle(set(
            &date,
            &year,
            ColumnProperty::DataType(DataType::String),
        ))
        .unwrap();
    let v = ViewSpec {
        sort: vec![SortKey {
            column: year.clone(),
            descending: false,
        }],
        ..spec(&date)
    };
    assert!(rows(&f, &v, 0, 1, &year)[0].starts_with("20"));
    f.app.handle(Request::Undo).unwrap();

    let err = f
        .app
        .handle(set(
            &date,
            &weekday,
            ColumnProperty::SortBy(Some(number.clone())),
        ))
        .unwrap_err();
    assert!(
        err.message.contains("can't be sorted by"),
        "{}",
        err.message
    );
    f.app
        .handle(set(&date, &month, ColumnProperty::SortBy(Some(number))))
        .unwrap();
    f.app.handle(Request::Undo).unwrap();
}
