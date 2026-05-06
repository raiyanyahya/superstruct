use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use crate::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
pub struct HashIndex {
    attribute: String,
    buckets: HashMap<Value, HashSet<u64>>,
}

impl HashIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            buckets: HashMap::new(),
        }
    }
}

impl Index for HashIndex {
    fn attribute(&self) -> &str {
        &self.attribute
    }

    fn supports_kind(&self, kind: PredicateKind) -> bool {
        kind == PredicateKind::Equals
    }

    fn build_from_records(&mut self, records: &[Record]) {
        for rec in records {
            self.insert(rec);
        }
    }

    fn insert(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            self.buckets.entry(value.clone()).or_default().insert(record.id);
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            if let Some(bucket) = self.buckets.get_mut(value) {
                bucket.remove(&record.id);
                if bucket.is_empty() {
                    self.buckets.remove(value);
                }
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> HashSet<u64> {
        self.buckets
            .get(&predicate.value)
            .cloned()
            .unwrap_or_default()
    }

    fn memory_estimate_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        for (_, bucket) in self.buckets.iter() {
            total += bucket.capacity() * std::mem::size_of::<u64>();
        }
        total += self.buckets.capacity() * 64;
        total
    }
}
