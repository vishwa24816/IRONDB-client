//! 25-method table ported from Go `optionalAgentMethod*` consts.

use thiserror::Error;

pub const METHODS: &[&str] = &[
    "connect", "close", "metadata", "ping",
    "openSession", "closeSession",
    "openTransaction", "commitTransaction", "rollbackTransaction",
    "query", "queryMulti", "streamQuery", "exec",
    "executeElasticsearchConsoleRequest",
    "getDatabases", "getTables", "tableExists", "getCreateStatement",
    "getColumns", "getAllColumns", "getIndexes", "getForeignKeys", "getTriggers",
    "applyChanges",
    "attachExternalDatabase", "detachExternalDatabase", "listExternalAttachments",
];

#[derive(Debug, Error, PartialEq)]
#[error("unknown sidecar method: {0}")]
pub struct UnknownMethod(pub String);

pub fn check_method(method: &str) -> Result<(), UnknownMethod> {
    if METHODS.contains(&method) { Ok(()) } else { Err(UnknownMethod(method.into())) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_25_methods_enumerated() {
        // 4 + 2 + 3 + 4 + 1 + 9 + 1 + 3 = 27 entries; plan said ~25 — assert exact count + key members.
        assert_eq!(METHODS.len(), 27, "{METHODS:?}");
        for m in ["connect", "query", "exec", "applyChanges", "listExternalAttachments"] {
            assert!(METHODS.contains(&m), "missing {m}");
        }
    }
    #[test]
    fn unknown_method_typed_error() {
        assert!(check_method("dropEverything").is_err());
        assert!(check_method("query").is_ok());
    }
}
