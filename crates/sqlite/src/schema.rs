//! Trusted expected schema is rebuilt from compiled migration, never a DB manifest.
use crate::storage::{MIGRATION, MIGRATION_RECORD};
use rusqlite::Connection;
use yixu_domain::{Error, Result};

// Tokenize rather than strip whitespace: quoted literals and token boundaries matter.
// Only whitespace/comments and ASCII unquoted case are normalized. Broader semantic
// equivalence is deliberately not guessed; unknown forms require offline review.
fn tokens(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            continue;
        }
        let start = i;
        i += 1;
        if ['\'', '"', '`', '['].contains(&c) {
            let end = if c == '[' { ']' } else { c };
            while i < chars.len() {
                if chars[i] == end {
                    i += 1;
                    if end != ']' && chars.get(i) == Some(&end) {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            out.push(chars[start..i].iter().collect());
        } else if c.is_ascii_alphanumeric() || c == '_' {
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(
                chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase(),
            );
        } else {
            out.push(c.to_string());
        }
    }
    // SQLite omits a trailing semicolon on stored CREATE statements.
    while out.last().is_some_and(|s| s == ";") {
        out.pop();
    }
    out
}
#[derive(Debug, PartialEq, Eq)]
struct Object {
    kind: String,
    name: String,
    table: String,
    sql: Option<Vec<String>>,
}
fn objects(c: &Connection) -> Result<Vec<Object>> {
    let mut q = c
        .prepare("SELECT type,name,tbl_name,sql FROM main.sqlite_schema ORDER BY type,name")
        .map_err(|_| Error::IncompatibleSchema)?;
    q.query_map([], |r| {
        let sql: Option<String> = r.get(3)?;
        Ok(Object {
            kind: r.get(0)?,
            name: r.get(1)?,
            table: r.get(2)?,
            sql: sql.as_deref().map(tokens),
        })
    })
    .map_err(|_| Error::IncompatibleSchema)?
    .collect::<std::result::Result<_, _>>()
    .map_err(|_| Error::IncompatibleSchema)
}
// Engine-interpreted constraint metadata supplements CREATE text, including implicit
// sqlite_autoindex_* unique/PK indexes. Root pages, physical row order and statistics
// contents are not identities. No internal object is blindly ignored by name prefix.
fn interpreted(c: &Connection, objects: &[Object]) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for object in objects {
        let queries: &[&str] = match object.kind.as_str() {
            "table" => &[
                "SELECT * FROM pragma_table_xinfo(?1)",
                "SELECT * FROM pragma_foreign_key_list(?1)",
                "SELECT * FROM pragma_index_list(?1)",
            ],
            "index" => &["SELECT * FROM pragma_index_xinfo(?1)"],
            _ => &[],
        };
        for query in queries {
            let mut stmt = c.prepare(query).map_err(|_| Error::IncompatibleSchema)?;
            let columns = stmt.column_count();
            let rows = stmt
                .query_map([&object.name], |r| {
                    let mut values = Vec::new();
                    for i in 0..columns {
                        values.push(format!("{:?}", r.get_ref(i)?));
                    }
                    Ok(values.join("|"))
                })
                .map_err(|_| Error::IncompatibleSchema)?;
            let mut rows = rows
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| Error::IncompatibleSchema)?;
            rows.sort();
            result.push(format!("{}:{query}:{rows:?}", object.name));
        }
    }
    Ok(result)
}
pub(crate) fn validate(actual: &Connection) -> Result<()> {
    validate_definitions(actual, MIGRATION, "")
}
// Private trusted-code inputs only; never accept request- or database-supplied DDL.
pub(crate) fn validate_definitions(
    actual: &Connection,
    foundation: &str,
    extension: &str,
) -> Result<()> {
    let expected = Connection::open_in_memory().map_err(|_| Error::Database)?;
    expected
        .execute_batch(MIGRATION_RECORD)
        .map_err(|_| Error::Database)?;
    expected
        .execute_batch(foundation)
        .map_err(|_| Error::Database)?;
    expected
        .execute_batch(extension)
        .map_err(|_| Error::Database)?;
    let got = objects(actual)?;
    // ANALYZE may create engine-owned statistics tables. Accept only definitions
    // regenerated by this same trusted SQLite build, not arbitrary sqlite_* names.
    if got
        .iter()
        .any(|o| o.name == "sqlite_stat1" || o.name == "sqlite_stat4")
    {
        expected
            .execute_batch("ANALYZE")
            .map_err(|_| Error::Database)?;
    }
    let want = objects(&expected)?;
    if got != want || interpreted(actual, &got)? != interpreted(&expected, &want)? {
        return Err(Error::IncompatibleSchema);
    }
    Ok(())
}
