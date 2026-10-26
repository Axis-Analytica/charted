//! Reading sources for queries: Excel sheets (read with calamine into a cached CSV),
//! DuckDB extensions for databases (installed only after the user agrees, as they are
//! downloaded), PostgreSQL connections (password from the desktop keyring), and noticing
//! when a source file changes or disappears.

use calamine::{Data, Reader, open_workbook_auto};
use charted_compiler::steps::{self, Ctx, Query, Source};
use charted_engine::{Database, sql};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use ts_rs::TS;

use crate::session::CoreError;

/// The sheets of an Excel workbook, in order.
pub fn excel_sheets(path: &Path) -> Result<Vec<String>, CoreError> {
    let wb = open_workbook_auto(path).map_err(|e| {
        CoreError::Invalid(format!(
            "couldn't open {} as a workbook: {e}",
            path.display()
        ))
    })?;
    Ok(wb.sheet_names().to_vec())
}

fn csv_cell(d: &Data) -> String {
    let text = match d {
        Data::Empty => return String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => excel_date(dt.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        // An error (#DIV/0!) is a blank cell, so the column keeps its type.
        Data::Error(_) => String::new(),
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

/// An Excel serial date (days since 1899-12-30) as ISO text: a date, or a date and
/// time when it has a time of day.
fn excel_date(serial: f64) -> String {
    let days = serial.floor() as i64;
    // Days from 1970-01-01, then the civil date (Howard Hinnant's algorithm).
    let z = days - 25569 + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let date = format!("{y:04}-{m:02}-{d:02}");
    let secs = ((serial - serial.floor()) * 86400.0).round() as i64;
    if secs == 0 {
        date
    } else {
        format!(
            "{date} {:02}:{:02}:{:02}",
            secs / 3600,
            secs / 60 % 60,
            secs % 60
        )
    }
}

/// Writes a sheet as CSV (dates as ISO text) for DuckDB to read.
fn sheet_to_csv(path: &Path, sheet: &str, out: &Path) -> Result<(), CoreError> {
    let mut wb = open_workbook_auto(path).map_err(|e| {
        CoreError::Invalid(format!(
            "couldn't open {} as a workbook: {e}",
            path.display()
        ))
    })?;
    let range = wb
        .worksheet_range(sheet)
        .map_err(|e| CoreError::Invalid(format!("couldn't read the sheet \"{sheet}\": {e}")))?;
    let mut text = String::new();
    for row in range.rows() {
        text.push_str(&row.iter().map(csv_cell).collect::<Vec<_>>().join(","));
        text.push('\n');
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CoreError::Invalid(e.to_string()))?;
    }
    crate::atomic::write(out, text.as_bytes()).map_err(|e| CoreError::Invalid(e.to_string()))
}

/// A file's modification time and size, or a folder's newest matching file and count:
/// what changes when the data does.
pub fn stamp(project_dir: &str, source: &Source) -> Option<(u64, u64)> {
    let path = PathBuf::from(steps::resolve_path(project_dir, &source.path()));
    let meta = |p: &Path| {
        std::fs::metadata(p).ok().map(|m| {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            (t, m.len())
        })
    };
    match source {
        Source::Postgres { .. } | Source::Calendar { .. } => None,
        Source::Folder { pattern, .. } => {
            let ext = pattern.trim_start_matches('*').to_lowercase();
            let entries = std::fs::read_dir(&path).ok()?;
            let mut newest = 0;
            let mut count = 0;
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.ends_with(&ext)
                    && let Some((t, _)) = meta(&e.path())
                {
                    newest = newest.max(t);
                    count += 1;
                }
            }
            Some((newest, count))
        }
        _ => meta(&path),
    }
}

/// The content of a source: a file's bytes, or a folder's matching file names and
/// sizes. Copying a project changes modification times, not content.
pub fn content_hash(project_dir: &str, source: &Source) -> Option<u64> {
    use std::io::Read;
    let path = PathBuf::from(steps::resolve_path(project_dir, &source.path()));
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
    };
    match source {
        Source::Postgres { .. } | Source::Calendar { .. } => return None,
        Source::Folder { pattern, .. } => {
            let ext = pattern.trim_start_matches('*').to_lowercase();
            let mut files: Vec<(String, u64)> = std::fs::read_dir(&path)
                .ok()?
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    let len = e.metadata().ok()?.len();
                    name.to_lowercase().ends_with(&ext).then_some((name, len))
                })
                .collect();
            files.sort();
            for (n, l) in files {
                feed(n.as_bytes());
                feed(&l.to_le_bytes());
            }
        }
        _ => {
            let mut f = std::fs::File::open(&path).ok()?;
            let mut buf = vec![0; 1 << 16];
            loop {
                let n = f.read(&mut buf).ok()?;
                if n == 0 {
                    break;
                }
                feed(&buf[..n]);
            }
        }
    }
    Some(h)
}

/// A source as loaded: its modification time and size, and its content's hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub mtime: u64,
    pub size: u64,
    #[serde(default)]
    pub hash: Option<u64>,
}

/// Stamps a source now (reads it whole to hash it).
pub fn full_stamp(project_dir: &str, source: &Source) -> Option<Stamp> {
    let (mtime, size) = stamp(project_dir, source)?;
    Some(Stamp {
        mtime,
        size,
        hash: content_hash(project_dir, source),
    })
}

/// Reads the start of a CSV file for what DuckDB doesn't detect: UTF-16 (by its byte
/// order mark) and Windows' Western European text (bytes that aren't UTF-8), and
/// European numbers ("1.234,5") in a file separated by semicolons.
pub fn detect_csv(file: &Path) -> steps::CsvDetected {
    use std::io::Read;
    let mut head = Vec::new();
    let Ok(f) = std::fs::File::open(file) else {
        return Default::default();
    };
    let size = f.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
    let _ = f.take(256 << 10).read_to_end(&mut head);
    let (encoding, text) = if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        let le = head[0] == 0xFF;
        let units: Vec<u16> = head[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if le {
                    u16::from_le_bytes(c)
                } else {
                    u16::from_be_bytes(c)
                }
            })
            .collect();
        (Some("utf-16"), String::from_utf16_lossy(&units))
    } else {
        match std::str::from_utf8(&head) {
            Ok(t) => (None, t.to_string()),
            // Cut through a character at the end of what was read: still UTF-8.
            Err(e) if e.error_len().is_none() => (
                None,
                String::from_utf8_lossy(&head[..e.valid_up_to()]).into_owned(),
            ),
            Err(_) => (Some("latin-1"), head.iter().map(|&b| b as char).collect()),
        }
    };
    steps::CsvDetected {
        encoding: encoding.map(String::from),
        decimal: european_numbers(&text).then(|| ",".to_string()),
        whole: false,
        ragged: ragged(&text, size as usize > head.len()),
    }
}

/// Whether rows below the header have a different number of values than it (title
/// lines above the header, which DuckDB skips, don't count). Quoted values may hold
/// delimiters and line breaks.
fn ragged(text: &str, cut: bool) -> bool {
    let delim = [',', ';', '\t', '|']
        .into_iter()
        .max_by_key(|&d| {
            text.lines()
                .take(50)
                .map(|l| l.matches(d).count())
                .sum::<usize>()
        })
        .unwrap_or(',');
    let mut counts = Vec::new();
    let (mut fields, mut quoted, mut any) = (1, false, false);
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c == delim && !quoted => fields += 1,
            '\n' if !quoted => {
                if any {
                    counts.push(fields);
                }
                (fields, any) = (1, false);
                continue;
            }
            '\r' => continue,
            _ => {}
        }
        any = true;
    }
    // The last line may be cut off where reading stopped.
    if any && !cut {
        counts.push(fields);
    }
    let Some(&width) = counts
        .iter()
        .max_by_key(|&&n| counts.iter().filter(|&&m| m == n).count())
    else {
        return false;
    };
    let body = counts.iter().position(|&n| n == width).unwrap_or(0);
    counts[body..].iter().any(|&n| n != width)
}

/// Whether a file separated by semicolons writes its numbers "1.234,5": some values
/// have a decimal comma and none a decimal point.
fn european_numbers(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(200)
        .collect();
    let count = |c: char| lines.iter().map(|l| l.matches(c).count()).sum::<usize>();
    if count(';') <= count(',') || count(';') < lines.len() {
        return false;
    }
    let (mut comma, mut point) = (0, 0);
    for v in lines.iter().flat_map(|l| l.split(';')) {
        let v = v.trim().trim_matches('"').trim_start_matches('-');
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        if let Some((int, frac)) = v.rsplit_once(',') {
            if digits(frac) && int.split('.').all(digits) {
                comma += 1;
            }
        } else if let Some((int, frac)) = v.split_once('.') {
            // "1.234" could be a thousand; "1.5" is a decimal point.
            if digits(int) && digits(frac) && frac.len() != 3 {
                point += 1;
            }
        }
    }
    comma > 0 && point == 0
}

/// After a load failed on a value that doesn't fit its column's type (found far into
/// a CSV file, past what DuckDB reads to work out types): the same settings with the
/// whole file read for types, or None when that can't help.
pub fn whole_file_types(
    sheets: &BTreeMap<String, String>,
    query: &Query,
    error: &str,
) -> Option<BTreeMap<String, String>> {
    let Source::Csv { path, .. } = &query.source else {
        return None;
    };
    if !error.contains("Conversion Error") && !error.contains("Could not convert") {
        return None;
    }
    let key = steps::csv_key(path);
    let mut found = steps::CsvDetected::decode(sheets.get(&key)?);
    if found.whole {
        return None;
    }
    found.whole = true;
    let mut out = sheets.clone();
    out.insert(key, found.encode());
    Some(out)
}

/// Caches every Excel sheet the queries read (again when the workbook changed) and
/// returns where each is, for `Ctx::sheets`, with what reading each CSV file's start
/// found (see `detect_csv`).
pub fn prepare_sheets(
    project_dir: &str,
    cache: &Path,
    queries: &[Query],
) -> Result<BTreeMap<String, String>, CoreError> {
    let mut out = BTreeMap::new();
    for q in queries {
        if let Source::Csv {
            path,
            encoding,
            decimal,
            ..
        } = &q.source
        {
            let file = PathBuf::from(steps::resolve_path(project_dir, path));
            let mut found = detect_csv(&file);
            if encoding.is_some() {
                found.encoding = None;
            }
            if decimal.is_some() {
                found.decimal = None;
            }
            out.insert(steps::csv_key(path), found.encode());
            continue;
        }
        let Source::Excel { path, sheet, .. } = &q.source else {
            continue;
        };
        let key = steps::sheet_key(path, sheet);
        if out.contains_key(&key) {
            continue;
        }
        let file = PathBuf::from(steps::resolve_path(project_dir, path));
        let (mtime, size) = stamp(project_dir, &q.source)
            .ok_or_else(|| CoreError::Invalid(format!("the workbook {path} doesn't exist")))?;
        let mut h: u64 = 0xcbf29ce484222325;
        for b in format!("{key}\u{1}{mtime}\u{1}{size}").bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }
        let csv = cache.join("sheets").join(format!("{h:016x}.csv"));
        if !csv.exists() {
            sheet_to_csv(&file, sheet, &csv)?;
        }
        out.insert(key, csv.display().to_string());
    }
    Ok(out)
}

/// The DuckDB extensions the user has agreed to download, kept in charted's state
/// folder (one choice per user, not per project).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Consent {
    #[serde(default)]
    pub extensions: Vec<String>,
}

impl Consent {
    fn file(state_dir: &Path) -> PathBuf {
        state_dir.join("extensions.json")
    }

    pub fn read(state_dir: &Path) -> Self {
        std::fs::read_to_string(Self::file(state_dir))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn allow(state_dir: &Path, name: &str) -> Result<Self, CoreError> {
        let mut c = Self::read(state_dir);
        if !c.extensions.iter().any(|e| e == name) {
            c.extensions.push(name.to_string());
        }
        std::fs::create_dir_all(state_dir).map_err(|e| CoreError::Invalid(e.to_string()))?;
        crate::atomic::write(
            &Self::file(state_dir),
            serde_json::to_string_pretty(&c)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|e| CoreError::Invalid(e.to_string()))?;
        Ok(c)
    }
}

/// What a source extension is, for the consent prompt.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ExtensionNeeded {
    pub name: String,
    pub installed: bool,
}

fn installed(db: &Database, name: &str) -> bool {
    db.query_u64(&format!(
        "SELECT count(*) FROM duckdb_extensions() WHERE extension_name = {} AND installed",
        sql::string(name)
    ))
    .is_ok_and(|n| n > 0)
}

/// Loads the extensions the queries need. One not yet downloaded is installed only when
/// the user has agreed; otherwise this fails with code `consent` naming it.
pub fn load_extensions(
    db: &Database,
    queries: &[Query],
    consent: &Consent,
) -> Result<(), CoreError> {
    let mut names: Vec<&str> = queries
        .iter()
        .filter_map(|q| q.source.extension())
        .collect();
    names.sort();
    names.dedup();
    for name in names {
        if !installed(db, name) {
            if !consent.extensions.iter().any(|e| e == name) {
                return Err(CoreError::Consent(name.to_string()));
            }
            db.execute(&format!("INSTALL {name}")).map_err(|e| {
                CoreError::Invalid(format!("couldn't download DuckDB's {name} extension: {e}"))
            })?;
        }
        db.execute(&format!("LOAD {name}")).map_err(|e| {
            CoreError::Invalid(format!("couldn't load DuckDB's {name} extension: {e}"))
        })?;
    }
    Ok(())
}

const KEYRING_SERVICE: &str = "charted";

fn keyring_user(host: &str, port: u16, database: &str, user: &str) -> String {
    format!("postgresql://{user}@{host}:{port}/{database}")
}

/// Stores a PostgreSQL password in the desktop keyring.
pub fn set_password(
    host: &str,
    port: u16,
    database: &str,
    user: &str,
    password: &str,
) -> Result<(), CoreError> {
    keyring::Entry::new(KEYRING_SERVICE, &keyring_user(host, port, database, user))
        .and_then(|e| e.set_password(password))
        .map_err(|e| CoreError::Invalid(format!("couldn't save the password in the keyring: {e}")))
}

/// Attaches every PostgreSQL database the queries read (read only), with the password
/// from the keyring. A missing password fails with code `password`.
pub fn attach_postgres(db: &Database, queries: &[Query]) -> Result<(), CoreError> {
    for q in queries {
        let Source::Postgres {
            host,
            port,
            database,
            user,
            ..
        } = &q.source
        else {
            continue;
        };
        let alias = steps::postgres_alias(host, *port, database, user);
        let attached = db
            .query_u64(&format!(
                "SELECT count(*) FROM duckdb_databases() WHERE database_name = {}",
                sql::string(&alias)
            ))
            .unwrap_or(0)
            > 0;
        if attached {
            continue;
        }
        let password =
            keyring::Entry::new(KEYRING_SERVICE, &keyring_user(host, *port, database, user))
                .and_then(|e| e.get_password())
                .map_err(|_| CoreError::Password(format!("{user}@{host}:{port}/{database}")))?;
        let secret = format!("{alias}_secret");
        db.execute(&format!(
            "CREATE OR REPLACE SECRET {} (TYPE postgres, HOST {}, PORT {port}, DATABASE {}, USER {}, PASSWORD {}); ATTACH '' AS {} (TYPE postgres, SECRET {}, READ_ONLY)",
            sql::ident(&secret),
            sql::string(host),
            sql::string(database),
            sql::string(user),
            sql::string(&password),
            sql::ident(&alias),
            sql::ident(&secret)
        ))
        .map_err(|e| CoreError::Invalid(format!("couldn't connect to {host}:{port}/{database}: {e}")))?;
    }
    Ok(())
}

/// Gets everything ready to run the queries and returns the compile context's sheets.
pub fn prepare(
    db: &Database,
    project_dir: &str,
    cache: &Path,
    queries: &[Query],
    consent: &Consent,
) -> Result<BTreeMap<String, String>, CoreError> {
    load_extensions(db, queries, consent)?;
    attach_postgres(db, queries)?;
    prepare_sheets(project_dir, cache, queries)
}

/// The SQL of a query (up to `upto` steps), ready to run.
pub fn query_sql(
    project_dir: &str,
    queries: &[Query],
    sheets: &BTreeMap<String, String>,
    query: &Query,
    upto: Option<usize>,
) -> Result<String, CoreError> {
    let ctx = Ctx {
        project_dir,
        queries,
        sheets,
    };
    match upto {
        Some(n) => steps::to_sql_upto(query, n, &ctx),
        None => steps::to_sql(query, &ctx),
    }
    .map_err(|e| CoreError::Invalid(e.to_string()))
}

/// A source's state compared with when it was last loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SourceState {
    Current,
    Changed,
    Missing,
}

#[cfg(test)]
mod tests {
    #[test]
    fn excel_dates() {
        assert_eq!(super::excel_date(45658.0), "2025-01-01");
        assert_eq!(super::excel_date(45658.75), "2025-01-01 18:00:00");
        assert_eq!(super::excel_date(1.0), "1899-12-31");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceStatus {
    pub query: String,
    pub name: String,
    pub path: String,
    pub state: SourceState,
}

/// Stamps of the sources as last loaded, by query id (kept in the project cache).
pub type Stamps = BTreeMap<String, Stamp>;

pub fn read_stamps(cache: &Path) -> Stamps {
    std::fs::read_to_string(cache.join("sources.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn write_stamps(cache: &Path, stamps: &Stamps) {
    let _ = crate::atomic::write(
        &cache.join("sources.json"),
        serde_json::to_string(stamps).unwrap_or_default().as_bytes(),
    );
}

type HashCache = std::collections::HashMap<(String, u64), Option<u64>>;

pub fn status(project_dir: &str, queries: &[Query], stamps: &Stamps) -> Vec<SourceStatus> {
    // Hashes of sources whose time changed, by path and time, so a poll hashes a file
    // once rather than every few seconds.
    static HASHES: std::sync::LazyLock<std::sync::Mutex<HashCache>> =
        std::sync::LazyLock::new(Default::default);
    queries
        .iter()
        .filter(|q| q.source.is_file())
        .map(|q| {
            let now = stamp(project_dir, &q.source);
            let state = match (now, stamps.get(&q.id)) {
                (None, _) => SourceState::Missing,
                (Some((mtime, size)), Some(was)) if (mtime, size) != (was.mtime, was.size) => {
                    // Only the time changed (a copy, a save of the same bytes): look inside.
                    let same = size == was.size
                        && was.hash.is_some()
                        && *HASHES
                            .lock()
                            .unwrap_or_else(|p| p.into_inner())
                            .entry((q.source.path(), mtime))
                            .or_insert_with(|| content_hash(project_dir, &q.source))
                            == was.hash;
                    if same {
                        SourceState::Current
                    } else {
                        SourceState::Changed
                    }
                }
                _ => SourceState::Current,
            };
            SourceStatus {
                query: q.id.clone(),
                name: q.name.clone(),
                path: q.source.path(),
                state,
            }
        })
        .collect()
}
