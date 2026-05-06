use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use std::collections::HashSet;

pub trait Index: Send + Sync {
    fn attribute(&self) -> &str;

    fn supports_kind(&self, kind: PredicateKind) -> bool;

    fn build_from_records(&mut self, records: &[Record]);

    fn insert(&mut self, record: &Record);

    fn remove(&mut self, record: &Record);

    fn execute(&self, predicate: &Predicate) -> HashSet<u64>;

    fn memory_estimate_bytes(&self) -> usize;

    fn can_answer(&self, predicate: &Predicate) -> bool {
        self.supports_kind(predicate.kind) && predicate.attribute == self.attribute()
    }
}
