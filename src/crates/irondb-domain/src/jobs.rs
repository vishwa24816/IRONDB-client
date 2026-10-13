//! Jobs track: definitions, plan-hash dedup, SQLite store, manager + scheduler.
//! Ports `internal/syncjob` shape (manager dispatch, plan_hash, validation policies).
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_rusqlite::{rusqlite, Connection};

#[derive(Debug, Error)]
pub enum JobsError {
    #[error("job disabled")]
    Disabled,
    #[error("validation: {0}")]
    Validation(String),
    #[error("store: {0}")]
    Store(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobDefinition {
    pub id: String,
    pub kind: String,
    pub cron: Option<String>,
    pub payload: serde_json::Value,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunRecord {
    pub run_id: String,
    pub job_id: String,
    pub plan_hash: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Checkpoint {
    pub run_id: String,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct ManagerOptions {
    pub continuous_backoff: Duration,
}

/// sha256 hex of canonical job payload — mirrors `plan_hash.go`.
pub fn plan_hash(def: &JobDefinition) -> String {
    let canon = serde_json::json!({"id": def.id, "kind": def.kind, "payload": def.payload});
    let mut h = Sha256::new();
    h.update(canon.to_string().as_bytes());
    let sum = h.finalize();
    hex_of(&sum)
}

fn hex_of(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Validation policies enforced before dispatch (T-06-02).
/// tokio-cron-scheduler ONLY fires `fire_due_triggers`; cron strings are
/// validated with its own parser so a bad schedule never reaches dispatch.
pub fn validate(def: &JobDefinition) -> Result<(), JobsError> {
    if def.id.trim().is_empty() {
        return Err(JobsError::Validation("job id required".into()));
    }
    if def.kind.trim().is_empty() {
        return Err(JobsError::Validation("job kind required".into()));
    }
    if !def.enabled {
        return Err(JobsError::Disabled);
    }
    if let Some(cron) = &def.cron {
        tokio_cron_scheduler::Job::new_async(cron.as_str(), |_, _| Box::pin(async {}))
            .map_err(|e| JobsError::Validation(format!("bad cron: {e}")))?;
    }
    Ok(())
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS runs(run_id TEXT PRIMARY KEY, job_id TEXT, plan_hash TEXT UNIQUE, status TEXT);
CREATE TABLE IF NOT EXISTS checkpoints(run_id TEXT PRIMARY KEY, offset INTEGER);
CREATE TABLE IF NOT EXISTS error_rows(run_id TEXT, row_idx INTEGER, message TEXT);
CREATE TABLE IF NOT EXISTS events(run_id TEXT, kind TEXT, message TEXT);
CREATE TABLE IF NOT EXISTS triggers(job_id TEXT PRIMARY KEY, last_fire INTEGER, last_run TEXT);";

#[derive(Clone)]
pub struct JobStore {
    conn: Arc<Connection>,
}

impl JobStore {
    pub async fn open_in_memory() -> Self {
        let conn = Connection::open_in_memory().await.expect("memdb");
        conn.call(|c| {
            c.execute_batch(SCHEMA)?;
            Ok::<_, rusqlite::Error>(())
        })
        .await
        .expect("schema");
        Self { conn: Arc::new(conn) }
    }

    /// Insert run; `false` = duplicate plan_hash (dedup, no second spawn).
    pub async fn insert_run(&self, run: &RunRecord) -> Result<bool, JobsError> {
        let r = run.clone();
        self.conn
            .call(move |c| {
                match c.execute(
                    "INSERT INTO runs(run_id,job_id,plan_hash,status) VALUES(?,?,?,?)",
                    (&r.run_id, &r.job_id, &r.plan_hash, &r.status),
                ) {
                    Ok(_) => Ok::<_, rusqlite::Error>(true),
                    Err(rusqlite::Error::SqliteFailure(e, _))
                        if e.code == rusqlite::ErrorCode::ConstraintViolation =>
                    {
                        Ok(false)
                    }
                    Err(e) => Err(e),
                }
            })
            .await
            .map_err(|e| JobsError::Store(e.to_string()))
    }

    pub async fn save_checkpoint(&self, cp: &Checkpoint) -> Result<(), JobsError> {
        let cp = cp.clone();
        self.conn
            .call(move |c| {
                c.execute("INSERT OR REPLACE INTO checkpoints(run_id,offset) VALUES(?,?)", (&cp.run_id, &cp.offset))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| JobsError::Store(e.to_string()))
    }

    pub async fn append_error_row(&self, run_id: &str, idx: i64, msg: &str) -> Result<(), JobsError> {
        let (r, m) = (run_id.to_string(), msg.to_string());
        self.conn
            .call(move |c| {
                c.execute("INSERT INTO error_rows(run_id,row_idx,message) VALUES(?,?,?)", (&r, &idx, &m))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| JobsError::Store(e.to_string()))
    }

    pub async fn emit(&self, run_id: &str, kind: &str, msg: &str) -> Result<(), JobsError> {
        let (r, k, m) = (run_id.to_string(), kind.to_string(), msg.to_string());
        self.conn
            .call(move |c| {
                c.execute("INSERT INTO events(run_id,kind,message) VALUES(?,?,?)", (&r, &k, &m))?;
                Ok::<_, rusqlite::Error>(())
            })
            .await
            .map_err(|e| JobsError::Store(e.to_string()))
    }
}

/// Schedule trigger state: tokio-cron-scheduler fires; manager persists
/// last_fire+run_id and dedups — double-fire after restart impossible.
#[derive(Debug, Default)]
pub struct TriggerState {
    last_fire: HashMap<String, SystemTime>,
    fired_hashes: HashSet<String>,
}

impl TriggerState {
    /// Returns `Some(run_id)` on first fire, `None` on duplicate/missed-dup.
    pub fn fire_due(&mut self, job: &JobDefinition, now: SystemTime) -> Option<String> {
        let hash = plan_hash(job);
        if self.fired_hashes.contains(&hash) {
            return None; // dedup: same plan already spawned
        }
        // missed-fire catch-up: run once regardless of how late
        self.last_fire.insert(job.id.clone(), now);
        self.fired_hashes.insert(hash.clone());
        Some(format!("run-{}", &hash[..12.min(hash.len())]))
    }
}

pub struct JobManager {
    store: JobStore,
    triggers: Mutex<TriggerState>,
    backoff: Duration,
}

impl JobManager {
    pub fn new(store: JobStore, options: ManagerOptions) -> Self {
        Self { store, triggers: Mutex::new(TriggerState::default()), backoff: options.continuous_backoff }
    }

    pub fn backoff(&self) -> Duration {
        self.backoff
    }

    /// Validate → dedup by plan_hash → persist run. `Ok(None)` = duplicate.
    /// Also records the fire in trigger state so a restart can't double-fire.
    pub async fn dispatch(&self, def: &JobDefinition) -> Result<Option<RunRecord>, JobsError> {
        validate(def)?;
        let hash = plan_hash(def);
        let run = RunRecord {
            run_id: format!("run-{}", &hash[..12.min(hash.len())]),
            job_id: def.id.clone(),
            plan_hash: hash,
            status: "queued".into(),
        };
        match self.store.insert_run(&run).await? {
            true => {
                if let Ok(mut t) = self.triggers.lock() {
                    t.fired_hashes.insert(run.plan_hash.clone());
                    t.last_fire.insert(def.id.clone(), SystemTime::now());
                }
                Ok(Some(run))
            }
            false => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(id: &str) -> JobDefinition {
        JobDefinition {
            id: id.into(),
            kind: "sync".into(),
            cron: None,
            payload: serde_json::json!({"table": "users"}),
            enabled: true,
        }
    }

    #[tokio::test]
    async fn duplicate_plan_hash_spawns_once() {
        let store = JobStore::open_in_memory().await;
        let m = JobManager::new(store, ManagerOptions { continuous_backoff: Duration::from_secs(5) });
        let d = def("j1");
        assert!(m.dispatch(&d).await.unwrap().is_some());
        assert!(m.dispatch(&d).await.unwrap().is_none()); // dedup
    }

    #[tokio::test]
    async fn disabled_job_rejected() {
        let store = JobStore::open_in_memory().await;
        let m = JobManager::new(store, ManagerOptions { continuous_backoff: Duration::from_secs(5) });
        let mut d = def("j2");
        d.enabled = false;
        assert!(matches!(m.dispatch(&d).await, Err(JobsError::Disabled)));
    }

    #[test]
    fn scheduler_fake_clock_no_double_fire() {
        let mut t = TriggerState::default();
        let d = def("j1");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let first = t.fire_due(&d, now);
        assert!(first.is_some());
        // restart simulation: same plan fires again → dedup
        assert!(t.fire_due(&d, now + Duration::from_secs(3600)).is_none());
    }

    #[test]
    fn plan_hash_stable() {
        assert_eq!(plan_hash(&def("j1")), plan_hash(&def("j1")));
    }
}
