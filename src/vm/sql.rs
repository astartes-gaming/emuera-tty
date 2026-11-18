//! Emuera.NET's SQL_* functions (its Runtime/SQL/SQL.cs): an SQLite
//! database per name in the save folder's `temp_db/`, read through
//! readers kept by number. eraKATANA keeps its town-map routes there.
//!
//! The databases travel with the saves as Emuera.NET has them: saving slot
//! N copies `temp_db/*` into a folder `saveNN/` beside `saveNN.sav`;
//! loading it empties `temp_db/` and copies that folder back; a run starts
//! with `temp_db/` emptied. Nothing happens for games without `temp_db/`.
//!
//! A database name naming another folder (`../x`, `a/b`, an absolute
//! path) is refused: Emuera.NET would write wherever it points.
//!
//! SQLite is compiled in with the `sqlite` cargo feature (on by default);
//! without it every SQL_* call is an error saying so.

use super::value::{err, VmResult};
use std::path::{Path, PathBuf};

/// One value of a result row.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(feature = "sqlite"), allow(dead_code))]
pub enum Cell {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
}

impl Cell {
    /// As Microsoft.Data.Sqlite's GetInt64 (sqlite3_column_int64): text
    /// read as a number, NULL refused.
    fn as_int(&self) -> VmResult<i64> {
        match self {
            Cell::Int(n) => Ok(*n),
            Cell::Real(r) => Ok(*r as i64),
            Cell::Text(t) => Ok(leading_int(t)),
            Cell::Null => err("SQL: the value is NULL"),
        }
    }

    /// As GetString (sqlite3_column_text): numbers written out.
    fn as_text(&self) -> VmResult<String> {
        match self {
            Cell::Int(n) => Ok(n.to_string()),
            Cell::Real(r) => Ok(r.to_string()),
            Cell::Text(t) => Ok(t.clone()),
            Cell::Null => err("SQL: the value is NULL"),
        }
    }
}

/// SQLite's text-to-integer reading: the leading decimal digits (with a
/// sign), 0 if there are none.
fn leading_int(t: &str) -> i64 {
    let t = t.trim_start();
    let end = t.char_indices().take_while(|&(i, c)| c.is_ascii_digit() || (i == 0 && (c == '-' || c == '+'))).last().map_or(0, |(i, c)| i + c.len_utf8());
    t[..end].parse().unwrap_or(0)
}

/// A reader: the rows of its query, and the one `SQL_READER_READ` is on.
struct Reader {
    rows: Vec<Vec<Cell>>,
    /// index of the current row; None before the first READ
    at: Option<usize>,
}

#[derive(Default)]
pub struct Sql {
    #[cfg(feature = "sqlite")]
    conn: Option<rusqlite::Connection>,
    readers: std::collections::HashMap<i64, Reader>,
}

/// Is `name` usable as a database file name inside `temp_db/`?
fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', ':', '\0']) && name != "." && name != ".."
}

#[cfg(not(feature = "sqlite"))]
fn no_sqlite<T>() -> VmResult<T> {
    err("SQL_* functions need SQLite, which this build of emuera-tty leaves out (the `sqlite` cargo feature)")
}

impl Sql {
    /// SQL_CONNECTION_OPEN: open (creating) `temp_db/<name>.db`.
    pub fn open(&mut self, temp_db: &Path, name: &str) -> VmResult<()> {
        if !safe_name(name) {
            return err(format!("SQL_CONNECTION_OPEN: \"{}\" isn't a database name (no folders)", name));
        }
        #[cfg(feature = "sqlite")]
        {
            std::fs::create_dir_all(temp_db).map_err(|e| super::Ctrl::Error(format!("SQL: {}: {}", temp_db.display(), e), false))?;
            self.conn = None;
            let c = rusqlite::Connection::open(temp_db.join(format!("{}.db", name))).map_err(sql_err)?;
            // as Emuera.NET: no journal, no syncing (it's a scratch copy)
            c.execute_batch("PRAGMA journal_mode = OFF;PRAGMA synchronous  = OFF;").map_err(sql_err)?;
            self.conn = Some(c);
            Ok(())
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = temp_db;
            no_sqlite()
        }
    }

    #[cfg(feature = "sqlite")]
    fn conn(&self) -> VmResult<&rusqlite::Connection> {
        self.conn.as_ref().ok_or_else(|| super::Ctrl::Error("SQLコネクションが開いていません".into(), false))
    }

    /// The rows `sql` gives (its first statement).
    #[allow(unused_variables)]
    fn query(&self, sql: &str) -> VmResult<Vec<Vec<Cell>>> {
        #[cfg(feature = "sqlite")]
        {
            use rusqlite::types::ValueRef;
            let mut st = self.conn()?.prepare(sql).map_err(sql_err)?;
            let n = st.column_count();
            let mut rows = st.query([]).map_err(sql_err)?;
            let mut out = Vec::new();
            while let Some(r) = rows.next().map_err(sql_err)? {
                let mut row = Vec::with_capacity(n);
                for i in 0..n {
                    row.push(match r.get_ref(i).map_err(sql_err)? {
                        ValueRef::Null => Cell::Null,
                        ValueRef::Integer(v) => Cell::Int(v),
                        ValueRef::Real(v) => Cell::Real(v),
                        ValueRef::Text(t) | ValueRef::Blob(t) => Cell::Text(String::from_utf8_lossy(t).into_owned()),
                    });
                }
                out.push(row);
            }
            Ok(out)
        }
        #[cfg(not(feature = "sqlite"))]
        no_sqlite()
    }

    /// SQL_EXECUTE_READER: run `sql`, its rows kept as reader `id`.
    pub fn execute_reader(&mut self, id: i64, sql: &str) -> VmResult<()> {
        let rows = self.query(sql)?;
        self.readers.insert(id, Reader { rows, at: None });
        Ok(())
    }

    /// SQL_EXECUTE_SCALER_LONG / _STRING: the first column of the first row
    /// (0 / "" when there is none, or it is NULL).
    pub fn scalar(&self, sql: &str) -> VmResult<Cell> {
        Ok(self.query(sql)?.into_iter().next().and_then(|r| r.into_iter().next()).unwrap_or(Cell::Null))
    }

    /// SQL_EXECUTE_NONQUERY: run `sql` (several statements allowed).
    #[allow(unused_variables)]
    pub fn execute(&self, sql: &str) -> VmResult<()> {
        #[cfg(feature = "sqlite")]
        {
            self.conn()?.execute_batch(sql).map_err(sql_err)
        }
        #[cfg(not(feature = "sqlite"))]
        no_sqlite()
    }

    fn reader(&mut self, id: i64) -> VmResult<&mut Reader> {
        self.readers.get_mut(&id).ok_or_else(|| super::Ctrl::Error(format!("IDが {} のREADERは存在しません", id), false))
    }

    /// SQL_READER_READ: move to the next row; false past the last.
    pub fn read(&mut self, id: i64) -> VmResult<bool> {
        let r = self.reader(id)?;
        let next = r.at.map_or(0, |i| i + 1);
        r.at = Some(next.min(r.rows.len()));
        Ok(next < r.rows.len())
    }

    /// Column `index` of the reader's current row.
    fn cell(&mut self, id: i64, index: i64) -> VmResult<Cell> {
        let r = self.reader(id)?;
        let row = r.at.and_then(|i| r.rows.get(i)).ok_or_else(|| super::Ctrl::Error("SQL: the reader isn't on a row (SQL_READER_READ first)".into(), false))?;
        usize::try_from(index).ok().and_then(|i| row.get(i)).cloned().ok_or_else(|| super::Ctrl::Error(format!("SQL: no column {}", index), false))
    }

    pub fn get_long(&mut self, id: i64, index: i64) -> VmResult<i64> {
        self.cell(id, index)?.as_int()
    }

    pub fn get_string(&mut self, id: i64, index: i64) -> VmResult<String> {
        self.cell(id, index)?.as_text()
    }

    /// SQL_READER_IS_NULL: a column past the row's last counts as NULL.
    pub fn is_null(&mut self, id: i64, index: i64) -> VmResult<bool> {
        let r = self.reader(id)?;
        let Some(row) = r.at.and_then(|i| r.rows.get(i)) else { return err("SQL: the reader isn't on a row (SQL_READER_READ first)") };
        Ok(usize::try_from(index).ok().and_then(|i| row.get(i)).is_none_or(|c| *c == Cell::Null))
    }

    /// Close the database (before its file is replaced or removed).
    fn close(&mut self) {
        #[cfg(feature = "sqlite")]
        {
            self.conn = None;
        }
        self.readers.clear();
    }
}

#[cfg(feature = "sqlite")]
fn sql_err(e: rusqlite::Error) -> super::Ctrl {
    super::Ctrl::Error(format!("SQL: {}", e), false)
}

/// The folder holding the running game's databases.
pub fn temp_db(save_dir: &Path) -> PathBuf {
    save_dir.join("temp_db")
}

/// The folder a save slot's databases are kept in: `saveNN.sav` → `saveNN/`.
fn slot_dir(save_file: &Path) -> Option<PathBuf> {
    Some(save_file.parent()?.join(save_file.file_stem()?))
}

/// Copy the plain files of `from` into `to` (created).
fn copy_files(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        if e.file_type().is_ok_and(|t| t.is_file()) {
            std::fs::copy(e.path(), to.join(e.file_name()))?;
        }
    }
    Ok(())
}

/// Empty `temp_db/` if it exists (Emuera.NET's SetUpTempDB): at start and
/// before a load. Only that folder's own files are removed.
pub fn reset(sql: &mut Sql, save_dir: &Path) {
    let dir = temp_db(save_dir);
    if !dir.is_dir() {
        return;
    }
    sql.close();
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_file()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Saving `save_file`: the running databases into its slot folder.
pub fn on_save(save_dir: &Path, save_file: &Path) -> Result<(), String> {
    let dir = temp_db(save_dir);
    let Some(dest) = slot_dir(save_file).filter(|_| dir.is_dir()) else { return Ok(()) };
    copy_files(&dir, &dest).map_err(|e| format!("{}: {}", dest.display(), e))
}

/// Loading `save_file`: its slot folder's databases become the running ones.
pub fn on_load(sql: &mut Sql, save_dir: &Path, save_file: &Path) -> Result<(), String> {
    reset(sql, save_dir);
    let Some(src) = slot_dir(save_file).filter(|d| d.is_dir()) else { return Ok(()) };
    copy_files(&src, &temp_db(save_dir)).map_err(|e| format!("{}: {}", src.display(), e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_numbers() {
        assert!(safe_name("eraKATANA_123") && !safe_name("../x") && !safe_name("a/b") && !safe_name("C:x") && !safe_name(""));
        assert_eq!((leading_int(" 42abc"), leading_int("-7"), leading_int("x")), (42, -7, 0));
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn readers_and_save_slots() {
        let d = std::env::temp_dir().join(format!("emuera-tty-sql-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut s = Sql::default();
        assert!(s.execute("select 1").is_err(), "no connection yet");
        s.open(&temp_db(&d), "game").unwrap();
        s.execute("create table t(a integer, b text); insert into t values (1, 'x'), (2, null);").unwrap();
        s.execute_reader(5, "select a, b from t order by a").unwrap();
        assert!(s.get_long(5, 0).is_err(), "before the first READ");
        assert!(s.read(5).unwrap());
        assert_eq!((s.get_long(5, 0).unwrap(), s.get_string(5, 1).unwrap()), (1, "x".to_string()));
        assert!(s.read(5).unwrap());
        assert!(s.is_null(5, 1).unwrap() && s.is_null(5, 9).unwrap() && !s.is_null(5, 0).unwrap());
        assert!(!s.read(5).unwrap());
        assert_eq!(s.scalar("select count(*) from t").unwrap(), Cell::Int(2));
        assert!(s.read(6).is_err());
        // a save takes the database along; a load brings it back
        let slot = d.join("save03.sav");
        on_save(&d, &slot).unwrap();
        assert!(d.join("save03/game.db").is_file());
        s.execute("delete from t").unwrap();
        on_load(&mut s, &d, &slot).unwrap();
        s.open(&temp_db(&d), "game").unwrap();
        assert_eq!(s.scalar("select count(*) from t").unwrap(), Cell::Int(2));
        let _ = std::fs::remove_dir_all(&d);
    }
}
