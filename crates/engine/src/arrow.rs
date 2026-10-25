use arrow_ipc::writer::StreamWriter;
use duckdb::Connection;

#[derive(Debug, thiserror::Error)]
pub enum ArrowError {
    #[error("query failed: {0}")]
    Query(#[from] duckdb::Error),
    #[error("could not encode Arrow IPC: {0}")]
    Encode(String),
}

/// Runs `sql` and encodes every result batch as one Arrow IPC stream.
pub fn query_arrow_ipc(conn: &Connection, sql: &str) -> Result<Vec<u8>, ArrowError> {
    let mut stmt = conn.prepare(sql)?;
    let batches = stmt.query_arrow([])?;
    let schema = batches.get_schema();
    let mut out = Vec::new();
    {
        let mut writer = StreamWriter::try_new(&mut out, &schema)
            .map_err(|e| ArrowError::Encode(e.to_string()))?;
        for batch in batches {
            writer
                .write(&batch)
                .map_err(|e| ArrowError::Encode(e.to_string()))?;
        }
        writer
            .finish()
            .map_err(|e| ArrowError::Encode(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_ipc::reader::StreamReader;

    #[test]
    fn round_trips_rows_through_ipc() {
        let conn = Connection::open_in_memory().unwrap();
        let bytes = query_arrow_ipc(
            &conn,
            "SELECT range AS id, range * 1.5 AS amount FROM range(5000)",
        )
        .unwrap();
        let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
        let rows: usize = reader.map(|b| b.unwrap().num_rows()).sum();
        assert_eq!(rows, 5000);
    }
}
