use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use roaring::RoaringTreemap;
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

// Trigram inverted index. Stores the lowercased value once per record so the
// per-candidate Jaccard pass can recompute trigrams cheaply at query time.
// Postings are roaring bitmaps so the union over target trigrams to gather
// candidates is a constant-factor cheap roaring OR rather than HashSet copies.
#[derive(Debug)]
pub struct NgramIndex {
    attribute: String,
    postings: HashMap<String, RoaringTreemap>,
    record_value: HashMap<u64, String>,
}

impl NgramIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            postings: HashMap::new(),
            record_value: HashMap::new(),
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
                let lowered = s.to_lowercase();
                for g in trigrams(&lowered) {
                    self.postings.entry(g).or_default().insert(record.id);
                }
                self.record_value.insert(record.id, lowered);
            }
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some(lowered) = self.record_value.remove(&record.id) {
            for g in trigrams(&lowered) {
                let empty = if let Some(posting) = self.postings.get_mut(&g) {
                    posting.remove(record.id);
                    posting.is_empty()
                } else {
                    false
                };
                if empty {
                    self.postings.remove(&g);
                }
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> RoaringTreemap {
        let target = match predicate.value.as_str() {
            Some(s) => s,
            None => return RoaringTreemap::new(),
        };
        let target_grams = trigrams(target);
        if target_grams.is_empty() {
            return RoaringTreemap::new();
        }

        let mut candidates = RoaringTreemap::new();
        for g in &target_grams {
            if let Some(ids) = self.postings.get(g) {
                candidates |= ids;
            }
        }

        let threshold = predicate.threshold;
        let target_size = target_grams.len();
        let mut out = RoaringTreemap::new();
        for rid in candidates.iter() {
            let lowered = match self.record_value.get(&rid) {
                Some(s) => s,
                None => continue,
            };
            let rec_grams = trigrams(lowered);
            if rec_grams.is_empty() {
                continue;
            }
            let overlap = target_grams.intersection(&rec_grams).count();
            let union_size = target_size + rec_grams.len() - overlap;
            if union_size > 0 {
                let similarity = overlap as f64 / union_size as f64;
                if similarity >= threshold {
                    out.insert(rid);
                }
            }
        }
        out
    }

    fn memory_estimate_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        for (key, v) in self.postings.iter() {
            total += key.len();
            total += v.serialized_size();
        }
        total += self.postings.capacity() * 64;
        for (_, s) in self.record_value.iter() {
            total += s.len();
        }
        total += self.record_value.capacity() * (std::mem::size_of::<u64>() + 24);
        total
    }
}
