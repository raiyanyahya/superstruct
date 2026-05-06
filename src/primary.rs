use crate::Value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub id: u64,
    pub attrs: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
pub struct PrimaryStore {
    records: HashMap<u64, Record>,
    next_id: u64,
}

impl PrimaryStore {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            next_id: 0,
        }
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn set_next_id(&mut self, id: u64) {
        self.next_id = id;
    }

    pub fn insert(&mut self, attrs: HashMap<String, Value>) -> Record {
        let id = self.next_id;
        self.next_id += 1;
        let record = Record { id, attrs };
        self.records.insert(id, record.clone());
        record
    }

    pub fn insert_at(&mut self, id: u64, attrs: HashMap<String, Value>) -> Record {
        let record = Record { id, attrs };
        self.records.insert(id, record.clone());
        record
    }

    pub fn get(&self, id: u64) -> Option<&Record> {
        self.records.get(&id)
    }

    pub fn delete(&mut self, id: u64) -> Option<Record> {
        self.records.remove(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.records.values()
    }

    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.records.keys().copied()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

impl Default for PrimaryStore {
    fn default() -> Self {
        Self::new()
    }
}
