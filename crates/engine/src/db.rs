use duckdb::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use duckdb::InterruptHandle;

use crate::arrow::{ArrowError, query_arrow_ipc};
use crate::sql;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("{0}")]
    Duck(#[from] duckdb::Error),
    #[error(transparent)]
    Arrow(#[from] ArrowError),
    #[error("could not create {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Default)]
pub struct DbOptions {
    /// DuckDB memory limit, e.g. "4GB". Defaults to half of physical memory.
    pub memory_limit: Option<String>,
    /// Where DuckDB may spill when a query exceeds the memory limit.
    pub temp_directory: Option<PathBuf>,
    pub threads: Option<usize>,
}

/// A column as DuckDB describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnInfo {
    pub name: String,
    pub duckdb_type: String,
}

/// The project's analytical database. Cheap to clone; each clone shares the database.
#[derive(Clone)]
pub struct Database {
    conn: Arc<std::sync::Mutex<Connection>>,
}

impl Database {
    /// Opens (or creates) a database file, or an in-memory database for `None`.
    pub fn open(path: Option<&Path>, options: &DbOptions) -> Result<Self, DbError> {
        let conn = match path {
            Some(p) => {
                if let Some(dir) = p.parent() {
                    std::fs::create_dir_all(dir).map_err(|source| DbError::Io {
                        path: dir.to_path_buf(),
                        source,
                    })?;
                }
                Connection::open(p)?
            }
            None => Connection::open_in_memory()?,
        };
        let mut setup = String::from(
            "SET autoinstall_known_extensions = false; SET autoload_known_extensions = false;",
        );
        if let Some(m) = &options.memory_limit {
            setup.push_str(&format!("SET memory_limit = {};", sql::string(m)));
        }
        if let Some(t) = &options.temp_directory {
            std::fs::create_dir_all(t).map_err(|source| DbError::Io {
                path: t.clone(),
                source,
            })?;
            setup.push_str(&format!(
                "SET temp_directory = {};",
                sql::string(&t.display().to_string())
            ));
        }
        if let Some(n) = options.threads {
            setup.push_str(&format!("SET threads = {n};"));
        }
        conn.execute_batch(&setup)?;
        Ok(Self {
            conn: Arc::new(std::sync::Mutex::new(conn)),
        })
    }

    /// A fresh connection to the same database, for running queries in parallel.
    pub fn connection(&self) -> Result<Connection, DbError> {
        Ok(self.lock().try_clone()?)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn execute(&self, sql: &str) -> Result<(), DbError> {
        Ok(self.lock().execute_batch(sql)?)
    }

    pub fn arrow(&self, sql: &str) -> Result<Vec<u8>, DbError> {
        let conn = self.connection()?;
        Ok(query_arrow_ipc(&conn, sql)?)
    }

    /// Like `arrow`, on a connection whose interrupt handle is handed to `register`
    /// first, so the query can be cancelled from another thread.
    pub fn arrow_interruptible(
        &self,
        sql: &str,
        register: impl FnOnce(Arc<InterruptHandle>),
    ) -> Result<Vec<u8>, DbError> {
        let conn = self.connection()?;
        register(conn.interrupt_handle());
        Ok(query_arrow_ipc(&conn, sql)?)
    }

    /// Runs `sql` and returns each row as a JSON object (via DuckDB's `to_json`).
    pub fn json_rows(&self, sql: &str, limit: Option<u64>) -> Result<Vec<String>, DbError> {
        let conn = self.connection()?;
        let limit = limit.map(|n| format!(" LIMIT {n}")).unwrap_or_default();
        let mut stmt = conn.prepare(&format!(
            "SELECT to_json(q)::VARCHAR FROM ({sql}) AS q{limit}"
        ))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Runs `sql` and returns the result as CSV text with a header row.
    pub fn csv(&self, sql: &str) -> Result<String, DbError> {
        let dir = std::env::temp_dir().join(format!("charted-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|source| DbError::Io {
            path: dir.clone(),
            source,
        })?;
        let file = dir.join("out.csv");
        self.execute(&format!(
            "COPY ({sql}) TO {} (FORMAT csv, HEADER true)",
            sql::string(&file.display().to_string())
        ))?;
        let text = std::fs::read_to_string(&file).map_err(|source| DbError::Io {
            path: file.clone(),
            source,
        })?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok(text)
    }

    pub fn query_u64(&self, sql: &str) -> Result<u64, DbError> {
        Ok(self.lock().query_row(sql, [], |r| r.get::<_, i64>(0))? as u64)
    }

    pub fn table_exists(&self, table: &str) -> Result<bool, DbError> {
        let conn = self.lock();
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM information_schema.tables WHERE table_schema = 'main' AND table_name = ?",
            [table],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Columns of a table or of any SELECT, in order.
    pub fn describe(&self, relation_sql: &str) -> Result<Vec<ColumnInfo>, DbError> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("DESCRIBE {relation_sql}"))?;
        let rows = stmt.query_map([], |r| {
            Ok(ColumnInfo {
                name: r.get(0)?,
                duckdb_type: r.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Materialises `select_sql` as table `table`, replacing any previous contents.
    pub fn materialise(&self, table: &str, select_sql: &str) -> Result<u64, DbError> {
        let t = sql::ident(table);
        self.execute(&format!("CREATE OR REPLACE TABLE {t} AS {select_sql}"))?;
        self.query_u64(&format!("SELECT count(*) FROM {t}"))
    }

    pub fn interrupt_handle(&self) -> Arc<InterruptHandle> {
        self.lock().interrupt_handle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materialises_and_describes() {
        let db = Database::open(None, &DbOptions::default()).unwrap();
        let rows = db
            .materialise(
                "my table",
                "SELECT range AS id, 'x' AS label FROM range(10)",
            )
            .unwrap();
        assert_eq!(rows, 10);
        assert!(db.table_exists("my table").unwrap());
        let cols = db.describe(&sql::ident("my table")).unwrap();
        assert_eq!(
            cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            ["id", "label"]
        );
        assert_eq!(cols[0].duckdb_type, "BIGINT");
    }

    #[test]
    fn extensions_never_autoload() {
        let db = Database::open(None, &DbOptions::default()).unwrap();
        let conn = db.connection().unwrap();
        let v: String = conn
            .query_row(
                "SELECT current_setting('autoinstall_known_extensions')::VARCHAR",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, "false");
    }
}
