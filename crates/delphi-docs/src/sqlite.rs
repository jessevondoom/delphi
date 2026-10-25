//! SQLite databases, read-only (`specs/014` §4, §6): the schema and row counts by default, and `query` for a
//! `SELECT`. The file is opened from memory, never written to disk, in defensive mode with an untrusted schema
//! (so a hostile view or trigger cannot call risky functions), only read-only statements are accepted, and a
//! query is interrupted after two seconds and shows at most 200 rows.

use crate::archive::human;
use crate::sniff::Format;
use crate::{DocError, Extract, Limits, Request, Unit};
use rusqlite::config::DbConfig;
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const MAX_ROWS: usize = 200;
const MAX_TABLES: usize = 100;
const TIME_LIMIT: Duration = Duration::from_secs(2);
const CELL: usize = 200;

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn open(bytes: &[u8]) -> Result<Connection, DocError> {
    let bad = |e: rusqlite::Error| DocError(format!("not a readable SQLite database ({})", e));
    let mut conn = Connection::open_in_memory().map_err(bad)?;
    conn.deserialize_read_exact("main", Cursor::new(bytes), bytes.len(), true).map_err(bad)?;
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true);
    let _ = conn.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false);
    conn.execute_batch("PRAGMA query_only = ON;").map_err(bad)?;
    // the first read is where a damaged header shows up
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(())).map_err(bad)?;
    Ok(conn)
}

/// Interrupt the connection if work outlasts the limit; stops when dropped.
struct Watchdog(Arc<AtomicBool>);

impl Watchdog {
    fn start(conn: &Connection) -> Watchdog {
        let done = Arc::new(AtomicBool::new(false));
        let (flag, handle) = (done.clone(), conn.get_interrupt_handle());
        std::thread::spawn(move || {
            let t = Instant::now();
            while t.elapsed() < TIME_LIMIT {
                if flag.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            if !flag.load(Ordering::Relaxed) {
                handle.interrupt();
            }
        });
        Watchdog(done)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

fn clip(s: &str) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().replace('|', "\\|");
    if flat.chars().count() > CELL { format!("{}…", flat.chars().take(CELL).collect::<String>()) } else { flat }
}

fn cell(v: ValueRef) -> String {
    match v {
        ValueRef::Null => "NULL".into(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(r) => r.to_string(),
        ValueRef::Text(t) => clip(&String::from_utf8_lossy(t)),
        ValueRef::Blob(b) => format!("<blob {} bytes>", b.len()),
    }
}

/// The text of the statement without leading blanks and SQL comments.
fn first_word(sql: &str) -> String {
    let mut s = sql.trim_start();
    loop {
        if let Some(r) = s.strip_prefix("--") {
            s = r.split_once('\n').map(|(_, rest)| rest).unwrap_or("").trim_start();
        } else if let Some(r) = s.strip_prefix("/*") {
            s = r.split_once("*/").map(|(_, rest)| rest).unwrap_or("").trim_start();
        } else {
            break;
        }
    }
    s.chars().take_while(|c| c.is_ascii_alphabetic()).collect::<String>().to_ascii_lowercase()
}

fn run_query(conn: &Connection, sql: &str) -> Result<(String, usize, bool), DocError> {
    if !matches!(first_word(sql).as_str(), "select" | "with" | "values") {
        return Err(DocError("only SELECT queries are accepted (WITH and VALUES too); nothing can be changed".into()));
    }
    let mut stmt = conn.prepare(sql.trim().trim_end_matches(';')).map_err(|e| DocError(format!("query error: {}", e)))?;
    if !stmt.readonly() {
        return Err(DocError("only read-only statements are accepted".into()));
    }
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let mut out = format!("| {} |\n|{}\n", names.iter().map(|n| clip(n)).collect::<Vec<_>>().join(" | "), "---|".repeat(names.len().max(1)));
    let _watch = Watchdog::start(conn);
    let mut rows = stmt.query([]).map_err(|e| DocError(format!("query error: {}", e)))?;
    let (mut n, mut more) = (0usize, false);
    loop {
        match rows.next() {
            Ok(Some(r)) => {
                if n == MAX_ROWS {
                    more = true;
                    break;
                }
                n += 1;
                let cells: Vec<String> = (0..names.len()).map(|i| r.get_ref(i).map(cell).unwrap_or_else(|_| "?".into())).collect();
                out.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            Ok(None) => break,
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::OperationInterrupted => {
                return Err(DocError(format!("the query was stopped after {} seconds; narrow it with WHERE or LIMIT", TIME_LIMIT.as_secs())));
            }
            Err(e) => return Err(DocError(format!("query error: {}", e))),
        }
    }
    Ok((out, n, more))
}

pub fn extract(bytes: &[u8], req: &Request, limits: &Limits) -> Result<Extract, DocError> {
    let conn = open(bytes)?;
    if let Some(q) = &req.query {
        let (table, n, more) = run_query(&conn, q)?;
        let mut text = format!("SQLite query · {} row{}{}\n{}", n, if n == 1 { "" } else { "s" }, if more { format!(" shown (first {}; add LIMIT/OFFSET for more)", MAX_ROWS) } else { String::new() }, table);
        if text.len() > limits.max_output {
            let mut c = limits.max_output;
            while !text.is_char_boundary(c) {
                c -= 1;
            }
            text.truncate(c);
        }
        let loss = if more { vec![format!("more than {} rows matched; only the first {} are shown", MAX_ROWS, MAX_ROWS)] } else { vec![] };
        return Ok(Extract { format: Format::Sqlite, text, unit: Unit::Line, total_units: Some(n), truncated: more, loss, supported: true });
    }
    // the schema view
    let _watch = Watchdog::start(&conn);
    let mut objects: Vec<(String, String)> = vec![];
    {
        let mut st = conn.prepare("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name").map_err(|e| DocError(e.to_string()))?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|e| DocError(e.to_string()))?;
        objects.extend(rows.flatten());
    }
    let count = |kind: &str| objects.iter().filter(|(k, _)| k == kind).count();
    let mut text = format!("SQLite database · {} · {} table(s), {} view(s), {} index(es), {} trigger(s)\n", human(bytes.len() as u64), count("table"), count("view"), count("index"), count("trigger"));
    let mut loss = vec![];
    let mut first_table = None;
    for (kind, name) in objects.iter().filter(|(k, _)| k == "table" || k == "view").take(MAX_TABLES) {
        let cols: Vec<String> = conn
            .prepare(&format!("PRAGMA table_info({})", quote_ident(name)))
            .and_then(|mut s| {
                let v: Vec<String> = s.query_map([], |r| Ok(format!("{} {}{}", r.get::<_, String>(1)?, r.get::<_, String>(2)?, if r.get::<_, i64>(5)? > 0 { " PRIMARY KEY" } else { "" })))?.flatten().collect();
                Ok(v)
            })
            .unwrap_or_default();
        let rows = if kind == "table" { conn.query_row(&format!("SELECT count(*) FROM {}", quote_ident(name)), [], |r| r.get::<_, i64>(0)).map(|n| format!(" · {} row{}", n, if n == 1 { "" } else { "s" })).unwrap_or_else(|_| " · rows not counted".into()) } else { String::new() };
        text.push_str(&format!("{} {}{}\n  {}\n", kind, name, rows, if cols.is_empty() { "(columns unreadable)".to_string() } else { cols.join(", ") }));
        first_table.get_or_insert(name.clone());
    }
    let listed = objects.iter().filter(|(k, _)| k == "table" || k == "view").count();
    if listed > MAX_TABLES {
        loss.push(format!("{} more table(s)/view(s) not listed", listed - MAX_TABLES));
    }
    if count("trigger") > 0 {
        loss.push(format!("{} trigger(s) exist and are never run", count("trigger")));
    }
    if let Some(t) = first_table {
        text.push_str(&format!("\nPass `query` to read rows, for example: SELECT * FROM {} LIMIT 20\n", quote_ident(&t)));
    }
    Ok(Extract { format: Format::Sqlite, text, unit: Unit::Line, total_units: Some(count("table")), truncated: false, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Vec<u8> {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, bio TEXT, avatar BLOB);
             CREATE TABLE \"odd \"\"name\" (x);
             CREATE VIEW names AS SELECT name FROM users;
             CREATE INDEX users_name ON users(name);
             CREATE TRIGGER t AFTER INSERT ON users BEGIN SELECT 1; END;
             INSERT INTO users VALUES (1, 'Ada', 'x|y', x'0102');
             INSERT INTO users VALUES (2, 'Lin', NULL, NULL);",
        )
        .unwrap();
        c.serialize("main").unwrap().to_vec()
    }

    fn q(sql: &str) -> Result<Extract, DocError> {
        extract(&db(), &Request { query: Some(sql.into()), ..Request::default() }, &Limits::default())
    }

    #[test]
    fn the_default_view_is_the_schema_with_row_counts() {
        let e = extract(&db(), &Request::default(), &Limits::default()).unwrap();
        assert!(e.text.contains("2 table(s), 1 view(s), 1 index(es), 1 trigger(s)"), "{}", e.text);
        assert!(e.text.contains("table users · 2 rows\n  id INTEGER PRIMARY KEY, name TEXT, bio TEXT, avatar BLOB"), "{}", e.text);
        assert!(e.text.contains("table odd \"name · 0 rows") && e.text.contains("view names\n") && e.text.contains("SELECT * FROM \"odd \"\"name\" LIMIT 20"), "{}", e.text);
        assert!(e.loss.iter().any(|l| l.contains("1 trigger(s)")), "{:?}", e.loss);
    }

    #[test]
    fn select_returns_rows_with_blobs_nulls_and_escaped_cells() {
        let e = q("SELECT id, name, bio, avatar FROM users ORDER BY id;").unwrap();
        assert!(e.text.starts_with("SQLite query · 2 rows\n| id | name | bio | avatar |\n|---|---|---|---|\n| 1 | Ada | x\\|y | <blob 2 bytes> |\n| 2 | Lin | NULL | NULL |"), "{}", e.text);
        assert!(q("-- a comment\n WITH t(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM t WHERE n<3) SELECT n FROM t").unwrap().text.contains("| 3 |"));
        assert!(q("VALUES (1),(2)").unwrap().text.contains("2 rows"));
    }

    #[test]
    fn nothing_that_writes_or_runs_is_accepted() {
        for sql in ["INSERT INTO users VALUES (3,'x','y',NULL)", "DELETE FROM users", "DROP TABLE users", "UPDATE users SET name='z'", "PRAGMA writable_schema=ON", "ATTACH DATABASE '/tmp/x' AS x", "CREATE TABLE t(a)", "VACUUM", "SELECT 1; DELETE FROM users", "/* sneaky */ DELETE FROM users"] {
            let r = q(sql);
            assert!(r.is_err(), "{} was accepted: {:?}", sql, r.map(|e| e.text));
        }
        // the data is untouched after all of that
        assert!(q("SELECT count(*) AS n FROM users").unwrap().text.contains("| 2 |"));
        assert!(q("SELECT load_extension('x')").is_err());
    }

    #[test]
    fn row_and_time_limits_hold() {
        let e = q("WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM t WHERE n<100000) SELECT n FROM t").unwrap();
        assert!(e.truncated && e.text.contains("first 200") && e.loss[0].contains("more than 200 rows"), "{}", &e.text[..120]);
        let started = Instant::now();
        let r = q("WITH RECURSIVE t(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM t) SELECT count(*) FROM t");
        assert!(r.unwrap_err().0.contains("stopped after 2 seconds"));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn damaged_and_hostile_files_are_errors_not_crashes() {
        assert!(extract(b"SQLite format 3\0not really", &Request::default(), &Limits::default()).unwrap_err().0.contains("not a readable SQLite database"));
        assert!(extract(&[], &Request::default(), &Limits::default()).is_err());
        let good = db();
        let mut x: u64 = 0x5eed_1234_abcd_ef01;
        for _ in 0..200 {
            let mut f = good.clone();
            for _ in 0..6 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x as usize) % f.len();
                f[at] = (x >> 40) as u8;
            }
            let _ = extract(&f, &Request::default(), &Limits::default());
            let _ = extract(&f, &Request { query: Some("SELECT * FROM users".into()), ..Request::default() }, &Limits::default());
        }
        for cut in (0..good.len()).step_by(997) {
            let _ = extract(&good[..cut], &Request::default(), &Limits::default());
        }
    }
}
