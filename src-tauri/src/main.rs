// Prevents an extra console window on Windows release builds; harmless elsewhere.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use duckdb::Connection;
use tauri::ipc::Response;

/// Spike command: generate `rows` synthetic sales rows in DuckDB and return them as Arrow IPC.
#[tauri::command]
async fn spike_arrow(rows: u64) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        let sql = format!(
            "SELECT range AS id, DATE '2022-01-01' + CAST(range / 700 AS INTEGER) AS order_date, \
             (range % 40)::INTEGER AS store, 'Product ' || (range % 800) AS product, \
             (1 + range % 5)::INTEGER AS quantity, (10 + (range * 7919) % 300)::DOUBLE AS amount \
             FROM range({rows})"
        );
        charted_engine::query_arrow_ipc(&conn, &sql).map(Response::new).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![spike_arrow])
        .run(tauri::generate_context!())
        .expect("error while running charted");
}
