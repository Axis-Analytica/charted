//! Times SQL statements against a database file: `sqltime DB "SQL" ["SQL"…]`.
//! Each statement runs on one connection, so temporary tables persist between them.

use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: sqltime DB SQL...");
    let conn = duckdb::Connection::open(&path).expect("open database");
    for sql in args {
        let start = Instant::now();
        let mut stmt = conn.prepare(&sql).expect("prepare");
        let rows: usize = stmt
            .query_arrow([])
            .expect("query")
            .map(|b| b.num_rows())
            .sum();
        println!(
            "{:>8.1} ms  {rows:>9} rows  {sql}",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
