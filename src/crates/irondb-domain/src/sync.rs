//! Sync migration kernels per family — port of `internal/sync`.
//! Drivers execute work units; triggers flow through [`crate::jobs`].
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SyncFamily {
    Relational,
    Document,
    KeyValue,
    Wide,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub family: SyncFamily,
    pub source: String,
    pub target: String,
    pub tables: Vec<String>,
    pub batch_size: usize,
}

impl SyncPlan {
    pub fn batch_count(&self, table_rows: usize) -> usize {
        if self.batch_size == 0 {
            return 0;
        }
        table_rows.div_ceil(self.batch_size)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncProgress {
    pub tables_done: usize,
    pub rows_moved: u64,
}

/// Pure kernel: advance progress by one batch. Drivers do the I/O.
pub fn apply_batch(plan: &SyncPlan, progress: &mut SyncProgress, rows: u64) {
    progress.rows_moved += rows;
    let _ = plan;
}

pub fn mark_table_done(progress: &mut SyncProgress) {
    progress.tables_done += 1;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_count_ceil() {
        let p = SyncPlan {
            family: SyncFamily::Relational,
            source: "a".into(),
            target: "b".into(),
            tables: vec!["t".into()],
            batch_size: 100,
        };
        assert_eq!(p.batch_count(250), 3);
    }

    #[test]
    fn progress_kernel() {
        let p = SyncPlan {
            family: SyncFamily::Document,
            source: "a".into(),
            target: "b".into(),
            tables: vec![],
            batch_size: 10,
        };
        let mut prog = SyncProgress::default();
        apply_batch(&p, &mut prog, 10);
        mark_table_done(&mut prog);
        assert_eq!((prog.rows_moved, prog.tables_done), (10, 1));
    }
}
