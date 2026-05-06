use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use std::collections::{HashMap, HashSet};

fn trigrams(value: &str) -> HashSet<String> {
    let padded = format!("  {}  ", value.to_lowercase());
    let chars: Vec<char> = padded.chars().collect();
    if chars.len() < 3 {
        return HashSet::new();
    }
    (0..chars.len() - 2)
        .map(|i| chars[i..i + 3].iter().collect())
        .collect()
}

#[derive(Debug)]
pub struct NgramIndex {
    attribute: String,
    postings: HashMap<String, HashSet<u64>>,
    record_trigrams: HashMap<u64, HashSet<String>>,
}

impl NgramIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            postings: HashMap::new(),
            record_trigrams: HashMap::new(),
        }
    }
}

impl Index for NgramIndex {
    fn attribute(&self) -> &str {
        &self.attribute
    }

    fn supports_kind(&self, kind: PredicateKind) -> bool {
        kind == PredicateKind::Fuzzy
    }

    fn build_from_records(&mut self, records: &[Record]) {
        for rec in records {
            self.insert(rec);
        }
    }

    fn insert(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            if let Some(s) = value.as_str() {
                let grams = trigrams(s);
                for g in &grams {
                    self.postings.entry(g.clone()).or_default().insert(record.id);
                }
                self.record_trigrams.insert(record.id, grams);
            }
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some(grams) = self.record_trigrams.remove(&record.id) {
            for g in &grams {
                let empty = if let Some(posting) = self.postings.get_mut(g) {
                    posting.remove(&record.id);
                    posting.is_empty()
                } else {
                    false
                };
                if empty {
                    self.postings.remove(g);
                }
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> HashSet<u64> {
        let target = match predicate.value.as_str() {
            Some(s) => s,
            None => return HashSet::new(),
        };
        let target_grams = trigrams(target);

        let mut candidates: HashSet<u64> = HashSet::new();
        for g in &target_grams {
            if let Some(ids) = self.postings.get(g) {
                candidates.extend(ids);
            }
        }

        let threshold = predicate.threshold;
        let mut out = HashSet::new();
        for rid in &candidates {
            if let Some(rec_grams) = self.record_trigrams.get(rid) {
                let overlap = target_grams.intersection(rec_grams).count();
                let union_size = target_grams.union(rec_grams).count();
                if union_size > 0 {
                    let similarity = overlap as f64 / union_size as f64;
                    if similarity >= threshold {
                        out.insert(*rid);
                    }
                }
            }
        }
        out
    }

    fn memory_estimate_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        for (_, v) in self.postings.iter() {
            total += v.capacity() * std::mem::size_of::<u64>();
        }
        for (_, v) in self.record_trigrams.iter() {
            total += v.capacity() * 64;
        }
        total
    }
}
