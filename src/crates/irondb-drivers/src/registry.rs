use std::collections::HashMap;
use crate::{CapabilityFlags, DatabaseDriver, DriverError};

pub type DriverFactory = fn() -> Box<dyn DatabaseDriver>;

struct Entry {
    factory: DriverFactory,
    caps: CapabilityFlags,
}

#[derive(Default)]
pub struct DriverRegistry {
    entries: HashMap<&'static str, Entry>,
}

impl DriverRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, engine_id: &'static str, caps: CapabilityFlags, factory: DriverFactory) {
        self.entries.insert(engine_id, Entry { factory, caps });
    }

    pub fn resolve(&self, engine_id: &str) -> Result<Box<dyn DatabaseDriver>, DriverError> {
        self.entries
            .get(engine_id)
            .map(|e| (e.factory)())
            .ok_or_else(|| DriverError::UnknownEngine(engine_id.to_string()))
    }

    pub fn capabilities(&self, engine_id: &str) -> Result<CapabilityFlags, DriverError> {
        self.entries
            .get(engine_id)
            .map(|e| e.caps)
            .ok_or_else(|| DriverError::UnknownEngine(engine_id.to_string()))
    }

    pub fn engines(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }
}
