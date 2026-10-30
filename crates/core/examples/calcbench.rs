//! Times calculated columns that read other rows of their table (EARLIER ranks, counts
//! of rows alike, context transition): `calcbench PROJECT [RUNS]`. Run on the 10M
//! sample, whose Customer table has 20,000 rows.

use charted_compiler::visual;
use charted_core::Session;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: calcbench PROJECT [RUNS]");
    let runs: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let s = Session::open(std::path::Path::new(&path)).expect("open");
    let m = &s.project.model;
    let cases: &[(&str, &str, &str)] = &[
        (
            "rank (EARLIER <)",
            "Customer",
            "COUNTROWS ( FILTER ( Customer, Customer[CustomerKey] < EARLIER ( Customer[CustomerKey] ) ) ) + 1",
        ),
        (
            "same city (EARLIER =)",
            "Customer",
            "COUNTROWS ( FILTER ( Customer, Customer[City] = EARLIER ( Customer[City] ) ) )",
        ),
        (
            "older in city (= and <)",
            "Customer",
            "COUNTROWS ( FILTER ( Customer, Customer[City] = EARLIER ( Customer[City] ) && Customer[BirthYear] < EARLIER ( Customer[BirthYear] ) ) )",
        ),
        (
            "country sum (SUMX =)",
            "Customer",
            "SUMX ( FILTER ( Customer, Customer[Country] = EARLIER ( Customer[Country] ) ), Customer[BirthYear] )",
        ),
        (
            "segment count (inner only)",
            "Customer",
            "COUNTROWS ( FILTER ( Customer, Customer[Segment] = \"Consumer\" ) )",
        ),
        (
            "running days (<=)",
            "Date",
            "SUMX ( FILTER ( 'Date', 'Date'[Date] <= EARLIER ( 'Date'[Date] ) ), 1 )",
        ),
        (
            "customer sales (CALCULATE)",
            "Customer",
            "CALCULATE ( SUM ( Sales[Amount] ) )",
        ),
    ];
    for (name, table, expr) in cases {
        let t = m.table_by_name(table).expect("table");
        let started = Instant::now();
        let compiled = visual::calculated_column(m, s.schema(), &t.id, expr);
        let compile = started.elapsed();
        let sql = match compiled {
            Ok((sql, _)) => sql,
            Err(e) => {
                println!("{name:32} compile error: {e}");
                continue;
            }
        };
        let mut times = Vec::new();
        let mut check = 0;
        for _ in 0..runs {
            let started = Instant::now();
            check = s
                .database()
                .query_u64(&format!(
                    "SELECT CAST(sum(hash(rid, v)) % 1000000007 AS UBIGINT) FROM ({sql})"
                ))
                .expect("run");
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{name:32} p50 {:8.1} ms  max {:8.1} ms  (compile {:.1} ms, check {check})",
            times[times.len() / 2],
            times[times.len() - 1],
            compile.as_secs_f64() * 1000.0
        );
    }
}
