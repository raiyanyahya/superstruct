use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use crate::Value;
use roaring::RoaringTreemap;

#[derive(Debug)]
pub struct SortedIndex {
    attribute: String,
    values: Vec<Value>,
    ids: Vec<u64>,
}

impl SortedIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            values: Vec::new(),
            ids: Vec::new(),
        }
    }
}

impl Index for SortedIndex {
    fn attribute(&self) -> &str {
        &self.attribute
    }

    fn supports_kind(&self, kind: PredicateKind) -> bool {
        matches!(kind, PredicateKind::Range | PredicateKind::Equals)
    }

    fn build_from_records(&mut self, records: &[Record]) {
        let mut pairs: Vec<(&Value, u64)> = Vec::new();
        for rec in records {
            if let Some(v) = rec.attrs.get(&self.attribute) {
                pairs.push((v, rec.id));
            }
        }
        pairs.sort_by(|a, b| a.0.cmp(b.0));
        self.values = pairs.iter().map(|(v, _)| (*v).clone()).collect();
        self.ids = pairs.iter().map(|(_, id)| *id).collect();
    }

    fn insert(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            let pos = self.values.partition_point(|v| v < value);
            self.values.insert(pos, value.clone());
            self.ids.insert(pos, record.id);
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            let lo = self.values.partition_point(|v| v < value);
            let hi = self.values.partition_point(|v| v <= value);
            for i in lo..hi {
                if self.ids[i] == record.id {
                    self.values.remove(i);
                    self.ids.remove(i);
                    return;
                }
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> RoaringTreemap {
        match predicate.kind {
            PredicateKind::Range => {
                let values = match &predicate.value {
                    Value::List(v) if v.len() == 2 => v,
                    _ => return RoaringTreemap::new(),
                };
                let lo = self.values.partition_point(|v| v < &values[0]);
                let hi = self.values.partition_point(|v| v <= &values[1]);
                if lo > hi {
                    return RoaringTreemap::new();
                }
                self.ids[lo..hi].iter().copied().collect()
            }
            PredicateKind::Equals => {
                // Both partition_point calls use monotone predicates so the
                // search is well defined per the std contract.
                let lo = self.values.partition_point(|v| v < &predicate.value);
                if lo >= self.values.len() || self.values[lo] != predicate.value {
                    return RoaringTreemap::new();
                }
                let hi = self.values.partition_point(|v| v <= &predicate.value);
                self.ids[lo..hi].iter().copied().collect()
            }
            _ => RoaringTreemap::new(),
        }
    }

    fn memory_estimate_bytes(&self) -> usize {
        self.values.capacity() * std::mem::size_of::<Value>()
            + self.ids.capacity() * std::mem::size_of::<u64>()
    }
}
