use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-zA-Z0-9]+").unwrap());

fn tokenize(s: &str) -> Vec<String> {
    TOKEN_RE
        .find_iter(s)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

#[derive(Debug)]
pub struct InvertedIndex {
    attribute: String,
    postings: HashMap<String, HashSet<u64>>,
}

impl InvertedIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            postings: HashMap::new(),
        }
    }
}

impl Index for InvertedIndex {
    fn attribute(&self) -> &str {
        &self.attribute
    }

    fn supports_kind(&self, kind: PredicateKind) -> bool {
        kind == PredicateKind::Contains
    }

    fn build_from_records(&mut self, records: &[Record]) {
        for rec in records {
            self.insert(rec);
        }
    }

    fn insert(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            if let Some(s) = value.as_str() {
                for token in tokenize(s) {
                    self.postings.entry(token).or_default().insert(record.id);
                }
            }
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            if let Some(s) = value.as_str() {
                for token in tokenize(s) {
                    let empty = if let Some(posting) = self.postings.get_mut(&token) {
                        posting.remove(&record.id);
                        posting.is_empty()
                    } else {
                        false
                    };
                    if empty {
                        self.postings.remove(&token);
                    }
                }
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> HashSet<u64> {
        match predicate.value.as_str() {
            Some(word) => self
                .postings
                .get(&word.to_lowercase())
                .cloned()
                .unwrap_or_default(),
            None => HashSet::new(),
        }
    }

    fn memory_estimate_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        for (_, v) in self.postings.iter() {
            total += v.capacity() * std::mem::size_of::<u64>();
        }
        total += self.postings.capacity() * 64;
        total
    }
}
