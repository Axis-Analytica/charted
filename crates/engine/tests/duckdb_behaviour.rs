//! DuckDB behaviour the DAX compiler relies on (the M11.0 spike, kept so an
//! upgrade that changes any of it fails here first).
use std::sync::Arc;

use duckdb::arrow::array::{Array, StringArray};
use duckdb::arrow::datatypes::DataType;
use duckdb::arrow::record_batch::RecordBatch;
use duckdb::vscalar::{ArrowFunctionSignature, VArrowScalar};
use duckdb::Connection;

fn val(conn: &Connection, sql: &str) -> Result<Option<String>, String> {
    conn.query_row(&format!("SELECT CAST(({sql}) AS VARCHAR)"), [], |r| r.get(0))
        .map_err(|e| e.to_string())
}

fn conn() -> Connection {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("SET autoinstall_known_extensions = false; SET autoload_known_extensions = false;")
        .unwrap();
    c
}

#[test]
fn try_turns_failed_conversions_into_null_but_refuses_error() {
    let c = conn();
    assert_eq!(val(&c, "TRY(CAST('a' AS INT))"), Ok(None));
    assert_eq!(val(&c, "TRY(CAST('2024-13-01' AS DATE))"), Ok(None));
    // error() is volatile, so IFERROR(ERROR(..)) can't lean on TRY.
    assert!(val(&c, "TRY(error('x'))").unwrap_err().contains("volatile"));
}

#[test]
fn float_division_by_zero_follows_ieee() {
    let c = conn();
    assert_eq!(val(&c, "current_setting('ieee_floating_point_ops')"), Ok(Some("true".into())));
    assert_eq!(val(&c, "1/0"), Ok(Some("inf".into())));
    assert_eq!(val(&c, "-1.0/0"), Ok(Some("-inf".into())));
    assert_eq!(val(&c, "0.0/0"), Ok(Some("nan".into())));
    assert_eq!(val(&c, "ln(0)"), Ok(Some("-inf".into())));
    assert_eq!(val(&c, "sqrt(-1)"), Ok(Some("nan".into())));
    // Integer division still errors.
    assert!(val(&c, "1 // 0").is_err());
}

#[test]
fn clocks_work_without_extensions() {
    let c = conn();
    for f in ["now()", "current_timestamp", "current_date", "localtimestamp"] {
        assert!(matches!(val(&c, f), Ok(Some(_))), "{f}");
    }
}

#[test]
fn quantile_over_a_list() {
    let c = conn();
    assert_eq!(val(&c, "list_aggregate([1,2,3,4], 'quantile_cont', 0.25)"), Ok(Some("1.75".into())));
}

struct Shout;
impl VArrowScalar for Shout {
    type State = ();
    fn invoke(_: &(), input: RecordBatch) -> Result<Arc<dyn Array>, Box<dyn std::error::Error>> {
        let a = input.column(0).as_any().downcast_ref::<StringArray>().unwrap();
        let out: StringArray = a.iter().map(|s| s.map(|s| s.to_uppercase())).collect();
        Ok(Arc::new(out))
    }
    fn signatures() -> Vec<ArrowFunctionSignature> {
        vec![ArrowFunctionSignature::exact(vec![DataType::Utf8], DataType::Utf8)]
    }
}

#[test]
fn a_udf_registered_once_is_seen_by_cloned_connections_only() {
    let c = conn();
    c.register_scalar_function::<Shout>("shout").unwrap();
    assert_eq!(val(&c.try_clone().unwrap(), "shout('hi')"), Ok(Some("HI".into())));
    assert!(val(&conn(), "shout('hi')").is_err());
}
