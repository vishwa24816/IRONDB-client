//! Unbypassable guards: readonly + production-confirm flags.
//! All 7 engines gated. Redis uses command-level allowlist (no `redis`
//! key in the original readonly map — writes gated per command).
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    Mysql,
    Postgres,
    Sqlserver,
    Sqlite,
    Mongodb,
    Elasticsearch,
    Redis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Read,
    Write,
    Structure,
    Script,
    Import,
}

#[derive(Debug, Clone, Default)]
pub struct ConnectionPolicy {
    pub readonly: bool,
    pub restrict_data_edit: bool,
    pub restrict_structure_edit: bool,
    pub restrict_script_execution: bool,
    pub restrict_data_import: bool,
    /// Production connection requires explicit confirm flag per op.
    pub production: bool,
    pub prod_confirmed: bool,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GuardError {
    #[error("readonly connection blocks write operation")]
    Readonly,
    #[error("data edit restricted")]
    DataEditRestricted,
    #[error("structure edit restricted")]
    StructureEditRestricted,
    #[error("script execution restricted")]
    ScriptRestricted,
    #[error("data import restricted")]
    ImportRestricted,
    #[error("production operation requires confirmation")]
    ProdConfirmRequired,
    #[error("redis command not allowlisted for readonly: {0}")]
    RedisCommandBlocked(String),
}

/// Readonly Redis commands (observation-only). Everything else = write.
pub fn redis_is_readonly_command(cmd: &str) -> bool {
    matches!(
        cmd.to_ascii_uppercase().as_str(),
        "GET" | "MGET" | "HGET" | "HGETALL" | "HMGET" | "LRANGE" | "SMEMBERS"
            | "ZRANGE" | "SCAN" | "HSCAN" | "SSCAN" | "ZSCAN" | "KEYS" | "TYPE"
            | "TTL" | "EXISTS" | "INFO" | "PING"
    )
}

pub fn check_guard(engine: Engine, policy: &ConnectionPolicy, op: Operation, redis_cmd: Option<&str>) -> Result<(), GuardError> {
    let _ = engine; // all 7 engines share the same gate
    // Redis command-level gate first.
    if let Some(cmd) = redis_cmd {
        if policy.readonly && !redis_is_readonly_command(cmd) {
            return Err(GuardError::RedisCommandBlocked(cmd.to_string()));
        }
    }
    match op {
        Operation::Read => Ok(()),
        Operation::Write => {
            if policy.readonly { return Err(GuardError::Readonly); }
            if policy.restrict_data_edit { return Err(GuardError::DataEditRestricted); }
            if policy.production && !policy.prod_confirmed { return Err(GuardError::ProdConfirmRequired); }
            Ok(())
        }
        Operation::Structure => {
            if policy.readonly { return Err(GuardError::Readonly); }
            if policy.restrict_structure_edit { return Err(GuardError::StructureEditRestricted); }
            if policy.production && !policy.prod_confirmed { return Err(GuardError::ProdConfirmRequired); }
            Ok(())
        }
        Operation::Script => {
            if policy.readonly { return Err(GuardError::Readonly); }
            if policy.restrict_script_execution { return Err(GuardError::ScriptRestricted); }
            if policy.production && !policy.prod_confirmed { return Err(GuardError::ProdConfirmRequired); }
            Ok(())
        }
        Operation::Import => {
            if policy.readonly { return Err(GuardError::Readonly); }
            if policy.restrict_data_import { return Err(GuardError::ImportRestricted); }
            if policy.production && !policy.prod_confirmed { return Err(GuardError::ProdConfirmRequired); }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ro() -> ConnectionPolicy { ConnectionPolicy { readonly: true, ..Default::default() } }

    #[test]
    fn readonly_blocks_writes_all_engines() {
        for e in [Engine::Mysql, Engine::Postgres, Engine::Sqlserver, Engine::Sqlite, Engine::Mongodb, Engine::Elasticsearch, Engine::Redis] {
            assert_eq!(check_guard(e, &ro(), Operation::Write, None), Err(GuardError::Readonly), "{e:?}");
            assert!(check_guard(e, &ro(), Operation::Read, None).is_ok(), "{e:?}");
        }
    }

    #[test]
    fn prod_confirm_required() {
        let p = ConnectionPolicy { production: true, ..Default::default() };
        assert_eq!(check_guard(Engine::Mysql, &p, Operation::Write, None), Err(GuardError::ProdConfirmRequired));
        let ok = ConnectionPolicy { production: true, prod_confirmed: true, ..Default::default() };
        assert!(check_guard(Engine::Mysql, &ok, Operation::Write, None).is_ok());
    }

    #[test]
    fn redis_command_allowlist() {
        assert!(check_guard(Engine::Redis, &ro(), Operation::Read, Some("GET")).is_ok());
        assert_eq!(
            check_guard(Engine::Redis, &ro(), Operation::Read, Some("SET")),
            Err(GuardError::RedisCommandBlocked("SET".into()))
        );
    }

    #[test]
    fn restriction_flags() {
        let p = ConnectionPolicy { restrict_data_edit: true, ..Default::default() };
        assert_eq!(check_guard(Engine::Postgres, &p, Operation::Write, None), Err(GuardError::DataEditRestricted));
        let p = ConnectionPolicy { restrict_structure_edit: true, ..Default::default() };
        assert_eq!(check_guard(Engine::Postgres, &p, Operation::Structure, None), Err(GuardError::StructureEditRestricted));
    }
}
