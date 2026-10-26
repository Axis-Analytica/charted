//! The quick measures' DAX (as `ui/src/report/quickMeasures.ts` writes it) on the
//! Harbor & Pine sample, checked against the same numbers computed another way.

use arrow_ipc::reader::StreamReader;
use charted_compiler::visual::VisualQuery;
use charted_core::{App, DataRequest, Request};
use charted_model::*;
use std::collections::HashMap;

type Row = HashMap<String, String>;

fn rows(bytes: Vec<u8>) -> Vec<Row> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let schema = batch.schema();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).unwrap())
            .collect();
        for r in 0..batch.num_rows() {
            out.push(
                schema
                    .fields()
                    .iter()
                    .zip(&fmts)
                    .map(|(f, x)| (f.name().clone(), x.value(r).to_string()))
                    .collect(),
            );
        }
    }
    out
}

fn num(r: &Row, k: &str) -> f64 {
    r[k].parse().unwrap_or(f64::NAN)
}

struct Sample {
    app: App,
    _dir: tempfile::TempDir,
}

impl Sample {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"));
        app.handle(Request::OpenSample {
            dir: Some(dir.path().join("p").display().to_string()),
        })
        .unwrap();
        Self { app, _dir: dir }
    }

    fn model(&self) -> &Model {
        &self.app.session().unwrap().project.model
    }

    fn col(&self, table: &str, column: &str) -> Projection {
        let t = self.model().table_by_name(table).unwrap();
        Projection {
            field: FieldRef::Column {
                table: t.id.clone(),
                column: t.column_by_name(column).unwrap().id.clone(),
            },
            aggregation: None,
            display_name: None,
        }
    }

    /// Adds a measure to Sales and returns it as a projection.
    fn measure(&mut self, name: &str, expression: &str) -> Projection {
        let table = self.model().table_by_name("Sales").unwrap().id.clone();
        let id = format!("m-{}", name.replace(' ', "-"));
        let measure: Measure = serde_json::from_value(serde_json::json!({
            "id": id, "name": name, "expression": expression,
        }))
        .unwrap();
        self.app
            .handle(Request::Commit {
                label: "test".into(),
                changes: vec![Change::AddMeasure {
                    table: table.clone(),
                    measure,
                    index: None,
                }],
            })
            .unwrap_or_else(|e| panic!("{name}: {}", e.message));
        Projection {
            field: FieldRef::Measure { table, measure: id },
            aggregation: None,
            display_name: None,
        }
    }

    fn by(&self, groups: Vec<Projection>, values: Vec<Projection>) -> Vec<Row> {
        rows(
            self.app
                .data(DataRequest::Visual {
                    query: VisualQuery {
                        groups,
                        values,
                        ..Default::default()
                    },
                })
                .unwrap_or_else(|e| panic!("{}", e.message)),
        )
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn quick_measures_compute_what_they_say() {
    let mut s = Sample::new();
    let year = s.col("Date", "Year");
    let category = s.col("Product", "Category");
    let total = s.measure("Total", "SUM ( Sales[Amount] )");

    // Year over year.
    let yoy = s.measure(
        "YoY",
        "[Total] - CALCULATE ( [Total], SAMEPERIODLASTYEAR ( 'Date'[Date] ) )",
    );
    let yoy_pct = s.measure(
        "YoY %",
        "VAR prior = CALCULATE ( [Total], SAMEPERIODLASTYEAR ( 'Date'[Date] ) ) RETURN DIVIDE ( [Total] - prior, prior )",
    );
    let ytd = s.measure("YTD", "TOTALYTD ( [Total], 'Date'[Date] )");
    let r = s.by(vec![year.clone()], vec![total.clone(), yoy, yoy_pct, ytd]);
    let mut prev: Option<f64> = None;
    for row in &r {
        let t = num(row, "v0");
        match prev {
            Some(p) => {
                assert!(close(num(row, "v1"), t - p), "yoy {row:?}");
                assert!(close(num(row, "v2"), (t - p) / p), "yoy% {row:?}");
            }
            // The first year has no prior year: the change is the total, the
            // percentage blank.
            None => assert!(row["v2"].is_empty() || row["v2"] == "null", "{row:?}"),
        }
        // A whole year's YTD is the year's total.
        assert!(close(num(row, "v3"), t), "ytd {row:?}");
        prev = Some(t);
    }

    // A running total over dates, by year: each year adds its total.
    let running = s.measure(
        "Running",
        "CALCULATE ( [Total], FILTER ( ALLSELECTED ( 'Date'[Date] ), 'Date'[Date] <= MAX ( 'Date'[Date] ) ) )",
    );
    let r = s.by(vec![year.clone()], vec![total.clone(), running]);
    let mut acc = 0.0;
    for row in &r {
        acc += num(row, "v0");
        assert!(close(num(row, "v1"), acc), "running total {row:?}");
    }

    // Shares of the grand total add up to one.
    let share = s.measure(
        "Share",
        "DIVIDE ( [Total], CALCULATE ( [Total], ALLSELECTED ( Product[Category] ) ) )",
    );
    let r = s.by(vec![category.clone()], vec![share]);
    let sum: f64 = r.iter().map(|x| num(x, "v0")).sum();
    assert!(close(sum, 1.0), "{r:?}");

    // Per-category average, largest and smallest, with no grouping.
    let avg = s.measure(
        "Avg per category",
        "AVERAGEX ( VALUES ( Product[Category] ), [Total] )",
    );
    let max = s.measure(
        "Max per category",
        "MAXX ( VALUES ( Product[Category] ), [Total] )",
    );
    let min = s.measure(
        "Min per category",
        "MINX ( VALUES ( Product[Category] ), [Total] )",
    );
    let per = s.by(vec![category.clone()], vec![total.clone()]);
    let vals: Vec<f64> = per.iter().map(|x| num(x, "v0")).collect();
    let r = &s.by(vec![], vec![avg, max, min])[0];
    assert!(close(
        num(r, "v0"),
        vals.iter().sum::<f64>() / vals.len() as f64
    ));
    assert!(close(
        num(r, "v1"),
        vals.iter().cloned().fold(f64::MIN, f64::max)
    ));
    assert!(close(
        num(r, "v2"),
        vals.iter().cloned().fold(f64::MAX, f64::min)
    ));

    // Weighted average: price weighted by quantity.
    let weighted = s.measure(
        "Weighted price",
        "DIVIDE ( SUMX ( Sales, Sales[UnitPrice] * Sales[Quantity] ), SUM ( Sales[Quantity] ) )",
    );
    let revenue = s.measure(
        "Revenue",
        "SUMX ( Sales, Sales[UnitPrice] * Sales[Quantity] )",
    );
    let qty = s.measure("Qty", "SUM ( Sales[Quantity] )");
    let r = &s.by(vec![], vec![weighted, revenue, qty])[0];
    assert!(close(num(r, "v0"), num(r, "v1") / num(r, "v2")));

    // A filtered value.
    let first = per[0]["g0"].clone();
    let filtered = s.measure(
        "Filtered",
        &format!("CALCULATE ( [Total], Product[Category] = \"{first}\" )"),
    );
    let r = &s.by(vec![], vec![filtered])[0];
    assert!(close(num(r, "v0"), vals[0]), "{r:?} vs {first}");
}
