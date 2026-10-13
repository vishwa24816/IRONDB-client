//! Positional param binding per dialect. Never string-interpolate args.
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Mysql,
    Postgres,
    Mssql,
    Sqlite,
    Mongo,
    Redis,
    Elasticsearch,
}

#[derive(Debug, Error)]
pub enum ParamError {
    #[error("arg count mismatch: placeholders={expected} args={actual}")]
    CountMismatch { expected: usize, actual: usize },
}

pub fn count_placeholders(sql: &str) -> usize {
    sql.chars().filter(|&c| c == '?').count()
}

/// Rebind `?` placeholders to the target dialect.
/// MySQL/SQLite: `?` unchanged. Postgres: `$n`. MSSQL: `@pN`.
/// Document/CLI dialects (Mongo/Redis/ES) pass through unchanged.
pub fn rebind(sql: &str, dialect: Dialect) -> String {
    match dialect {
        Dialect::Mysql | Dialect::Sqlite | Dialect::Mongo | Dialect::Redis | Dialect::Elasticsearch => {
            sql.to_string()
        }
        Dialect::Postgres | Dialect::Mssql => {
            let mut out = String::with_capacity(sql.len() + 8);
            let mut n = 0usize;
            for ch in sql.chars() {
                if ch == '?' {
                    n += 1;
                    if dialect == Dialect::Postgres {
                        out.push_str(&format!("${n}"));
                    } else {
                        out.push_str(&format!("@p{n}"));
                    }
                } else {
                    out.push(ch);
                }
            }
            out
        }
    }
}

/// Validate arg count matches placeholder count, return rebound SQL.
pub fn bind(sql: &str, args: &[Value], dialect: Dialect) -> Result<String, ParamError> {
    let expected = count_placeholders(sql);
    if expected != args.len() {
        return Err(ParamError::CountMismatch { expected, actual: args.len() });
    }
    Ok(rebind(sql, dialect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn mysql_passthrough() {
        assert_eq!(rebind("select * from t where a=? and b=?", Dialect::Mysql), "select * from t where a=? and b=?");
    }

    #[test]
    fn sqlite_passthrough() {
        assert_eq!(rebind("select ?", Dialect::Sqlite), "select ?");
    }

    #[test]
    fn postgres_numbered() {
        assert_eq!(rebind("select * from t where a=? and b=?", Dialect::Postgres), "select * from t where a=$1 and b=$2");
    }

    #[test]
    fn mssql_named() {
        assert_eq!(rebind("select * from t where a=?", Dialect::Mssql), "select * from t where a=@p1");
    }

    #[test]
    fn doc_dialects_passthrough() {
        for d in [Dialect::Mongo, Dialect::Redis, Dialect::Elasticsearch] {
            assert_eq!(rebind("find(?)", d), "find(?)");
        }
    }

    #[test]
    fn count_mismatch_rejected() {
        let err = bind("select ? ?", &[json!(1)], Dialect::Mysql).unwrap_err();
        assert!(matches!(err, ParamError::CountMismatch { expected: 2, actual: 1 }));
    }

    #[test]
    fn all_seven_dialects_bind() {
        for d in [Dialect::Mysql, Dialect::Postgres, Dialect::Mssql, Dialect::Sqlite, Dialect::Mongo, Dialect::Redis, Dialect::Elasticsearch] {
            bind("select * from t where a=?", &[json!(1)], d).unwrap();
        }
    }
}
