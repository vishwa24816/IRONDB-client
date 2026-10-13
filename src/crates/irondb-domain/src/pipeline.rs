//! QueryService::execute — fixed stage order:
//! param -> guard/audit -> execute -> timeout/cancel -> row-budget -> diff(noop).
//!
//! SQLite handle decision (OQ1): user DBs use a per-connection
//! tokio-rusqlite handle, never the shared app-state store handle,
//! so a slow/cancelled query cannot lock app state.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::audit::AuditLog;
use crate::guard::{check_guard, ConnectionPolicy, Engine, GuardError, Operation};
use crate::param::{bind, Dialect, ParamError};
use crate::row_budget::{apply_budget, estimate_bytes, RowBudget};
use crate::timeout::{with_deadline, CancelRegistry, TimeoutConfig, TimeoutError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryInput {
    pub sql: String,
    pub args: Vec<Value>,
    pub dialect: String,
    pub engine: String,
    pub operation: String,
    pub max_rows: usize,
    pub timeout_ms: u64,
    pub query_id: String,
    pub principal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryOutput {
    pub rows: Vec<Value>,
    pub truncated: bool,
    pub sql: String,
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("param: {0}")]
    Param(#[from] ParamError),
    #[error("guard: {0}")]
    Guard(#[from] GuardError),
    #[error("timeout: {0}")]
    Timeout(#[from] TimeoutError),
    #[error("driver: {0}")]
    Driver(String),
    #[error("cancelled")]
    Cancelled,
}

fn parse_dialect(s: &str) -> Dialect {
    match s.to_lowercase().as_str() {
        "postgres" | "pg" => Dialect::Postgres,
        "mssql" | "sqlserver" => Dialect::Mssql,
        "sqlite" => Dialect::Sqlite,
        "mongo" | "mongodb" => Dialect::Mongo,
        "redis" => Dialect::Redis,
        "es" | "elasticsearch" => Dialect::Elasticsearch,
        _ => Dialect::Mysql,
    }
}

fn parse_engine(s: &str) -> Engine {
    match s.to_lowercase().as_str() {
        "postgres" | "pg" => Engine::Postgres,
        "sqlserver" | "mssql" => Engine::Sqlserver,
        "sqlite" => Engine::Sqlite,
        "mongodb" | "mongo" => Engine::Mongodb,
        "elasticsearch" | "es" => Engine::Elasticsearch,
        "redis" => Engine::Redis,
        _ => Engine::Mysql,
    }
}

fn parse_op(s: &str) -> Operation {
    match s.to_lowercase().as_str() {
        "write" => Operation::Write,
        "structure" => Operation::Structure,
        "script" => Operation::Script,
        "import" => Operation::Import,
        _ => Operation::Read,
    }
}

/// Driver boundary. Stages call this exactly once, after guard/audit.
#[async_trait::async_trait]
pub trait PipelineDriver: Send + Sync {
    async fn execute(&self, sql: &str, args: &[Value]) -> Result<Vec<Value>, String>;
    fn spy(&self) -> Vec<String> { vec![] }
}

pub struct QueryService<D> {
    pub driver: D,
    pub policy: ConnectionPolicy,
    pub cancels: CancelRegistry,
    pub audit: std::sync::Mutex<AuditLog>,
}

impl<D: PipelineDriver> QueryService<D> {
    pub fn new(driver: D, policy: ConnectionPolicy) -> Self {
        Self { driver, policy, cancels: CancelRegistry::default(), audit: std::sync::Mutex::new(AuditLog::default()) }
    }

    pub fn cancel(&self, id: &str) {
        self.cancels.cancel(id);
    }

    /// Fixed order: param -> guard/audit -> execute -> timeout/cancel -> row-budget.
    pub async fn execute(&self, input: QueryInput) -> Result<QueryOutput, PipelineError> {
        // 1. PARAM
        let dialect = parse_dialect(&input.dialect);
        let bound_sql = bind(&input.sql, &input.args, dialect)?;
        // 2. GUARD
        let engine = parse_engine(&input.engine);
        let op = parse_op(&input.operation);
        let guard_result = check_guard(engine, &self.policy, op, None);
        // 2b. AUDIT (pre-execute, records allowed + blocked)
        self.audit.lock().unwrap().append(&input.principal, &input.engine, &bound_sql, guard_result.is_ok(), "execute");
        guard_result?;
        // 3+4. EXECUTE under deadline + cancel registry
        self.cancels.register(&input.query_id);
        let cfg = TimeoutConfig { timeout_ms: input.timeout_ms };
        let args = input.args.clone();
        let sql = bound_sql.clone();
        let driver = &self.driver;
        let cancels = self.cancels.clone();
        let qid = input.query_id.clone();
        let exec = async move {
            if cancels.is_cancelled(&qid) {
                return Err(TimeoutError::Cancelled);
            }
            let rows = driver.execute(&sql, &args).await.map_err(|e| TimeoutError::Driver(e))?;
            if cancels.is_cancelled(&qid) {
                return Err(TimeoutError::Cancelled);
            }
            Ok::<_, TimeoutError>(rows)
        };
        let rows = with_deadline(cfg, exec).await;
        self.cancels.complete(&input.query_id);
        let rows = rows.map_err(|e| match e {
            TimeoutError::Cancelled => PipelineError::Cancelled,
            TimeoutError::Driver(msg) => PipelineError::Driver(msg),
            other => PipelineError::Timeout(other),
        })?;
        // 5. ROW-BUDGET
        let budget = RowBudget { max_rows: input.max_rows, max_bytes: 8 * 1024 * 1024 };
        let outcome = apply_budget(rows, budget, estimate_bytes);
        Ok(QueryOutput { rows: outcome.rows, truncated: outcome.truncated, sql: bound_sql })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    struct FakeDriver {
        calls: Arc<Mutex<Vec<String>>>,
        rows: Vec<Value>,
        delay_ms: u64,
    }

    #[async_trait::async_trait]
    impl PipelineDriver for FakeDriver {
        async fn execute(&self, sql: &str, _args: &[Value]) -> Result<Vec<Value>, String> {
            self.calls.lock().unwrap().push(format!("execute:{sql}"));
            if self.delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(self.delay_ms)).await;
            }
            Ok(self.rows.clone())
        }
        fn spy(&self) -> Vec<String> { self.calls.lock().unwrap().clone() }
    }

    fn input(sql: &str, qid: &str) -> QueryInput {
        QueryInput {
            sql: sql.into(), args: vec![json!(1)], dialect: "postgres".into(),
            engine: "postgres".into(), operation: "read".into(),
            max_rows: 100, timeout_ms: 1000, query_id: qid.into(), principal: "u".into(),
        }
    }

    #[tokio::test]
    async fn stage_order_and_rebind() {
        let calls = Arc::new(Mutex::new(vec![]));
        let svc = QueryService::new(FakeDriver { calls: calls.clone(), rows: vec![json!({"a":1})], delay_ms: 0 }, ConnectionPolicy::default());
        let out = svc.execute(input("select ?","q1")).await.unwrap();
        assert_eq!(out.sql, "select $1"); // param ran before execute
        assert_eq!(*calls.lock().unwrap(), vec!["execute:select $1".to_string()]);
        assert_eq!(svc.audit.lock().unwrap().len(), 1); // audit pre-execute
    }

    #[tokio::test]
    async fn guard_blocks_before_driver() {
        let calls = Arc::new(Mutex::new(vec![]));
        let policy = ConnectionPolicy { readonly: true, ..Default::default() };
        let svc = QueryService::new(FakeDriver { calls: calls.clone(), rows: vec![], delay_ms: 0 }, policy);
        let mut i = input("select ?","q2");
        i.operation = "write".into();
        let r = svc.execute(i).await;
        assert!(matches!(r, Err(PipelineError::Guard(_))));
        assert!(calls.lock().unwrap().is_empty()); // driver never called
    }

    #[tokio::test]
    async fn timeout_fires() {
        let svc = QueryService::new(FakeDriver { calls: Arc::new(Mutex::new(vec![])), rows: vec![], delay_ms: 200 }, ConnectionPolicy::default());
        let mut i = input("select ?","q3");
        i.timeout_ms = 20;
        let r = svc.execute(i).await;
        assert!(matches!(r, Err(PipelineError::Timeout(_))));
    }

    #[tokio::test]
    async fn cancel_makes_connection_reusable() {
        let svc = QueryService::new(FakeDriver { calls: Arc::new(Mutex::new(vec![])), rows: vec![json!(1)], delay_ms: 100 }, ConnectionPolicy::default());
        svc.cancels.register("slow");
        svc.cancels.cancel("slow");
        // registry reusable after complete
        svc.cancels.complete("slow");
        assert_eq!(svc.cancels.len(), 0);
        let out = svc.execute(input("select ?","slow")).await.unwrap();
        assert!(!out.truncated);
    }

    #[tokio::test]
    async fn over_budget_truncated() {
        let rows: Vec<Value> = (0..10).map(|n| json!({"n": n})).collect();
        let svc = QueryService::new(FakeDriver { calls: Arc::new(Mutex::new(vec![])), rows, delay_ms: 0 }, ConnectionPolicy::default());
        let mut i = input("select ?","q4");
        i.max_rows = 3;
        let out = svc.execute(i).await.unwrap();
        assert!(out.truncated);
        assert_eq!(out.rows.len(), 3);
    }
}
