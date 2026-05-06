use crate::index::base::Index;
use crate::index::{HashIndex, SortedIndex, TrieIndex, InvertedIndex, NgramIndex};
use crate::primary::{PrimaryStore, Record};
use crate::query::{Predicate, PredicateKind, Node, And, Or, Not, Query};
use crate::value::Attrs;
use crate::workload::WorkloadTracker;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

type IndexKey = (String, String);

fn build_index_for(kind: PredicateKind, attribute: &str) -> Box<dyn Index> {
    match kind {
        PredicateKind::Equals => Box::new(HashIndex::new(attribute.to_string())),
        PredicateKind::Range => Box::new(SortedIndex::new(attribute.to_string())),
        PredicateKind::Prefix => Box::new(TrieIndex::new(attribute.to_string())),
        PredicateKind::Contains => Box::new(InvertedIndex::new(attribute.to_string())),
        PredicateKind::Fuzzy => Box::new(NgramIndex::new(attribute.to_string())),
    }
}

fn index_type_name(kind: PredicateKind) -> &'static str {
    match kind {
        PredicateKind::Equals => "HashIndex",
        PredicateKind::Range => "SortedIndex",
        PredicateKind::Prefix => "TrieIndex",
        PredicateKind::Contains => "InvertedIndex",
        PredicateKind::Fuzzy => "NgramIndex",
    }
}

pub struct Planner {
    indexes: HashMap<IndexKey, Box<dyn Index>>,
    memory_budget_bytes: usize,
}

impl Planner {
    pub fn new(memory_budget_bytes: usize) -> Self {
        Self {
            indexes: HashMap::new(),
            memory_budget_bytes,
        }
    }

    pub fn set_memory_budget(&mut self, bytes_limit: usize, workload: &mut WorkloadTracker) {
        self.memory_budget_bytes = bytes_limit;
        self.enforce_memory_budget(workload);
    }

    fn enforce_memory_budget(&mut self, workload: &mut WorkloadTracker) {
        let mut total: usize = self.indexes.values().map(|i| i.memory_estimate_bytes()).sum();
        if total <= self.memory_budget_bytes {
            return;
        }

        let mut keys: Vec<IndexKey> = self.indexes.keys().cloned().collect();
        keys.sort_by(|a, b| {
            workload
                .score(a)
                .partial_cmp(&workload.score(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for key in &keys {
            if total <= self.memory_budget_bytes {
                break;
            }
            if let Some(idx) = self.indexes.remove(key) {
                total = total.saturating_sub(idx.memory_estimate_bytes());
                workload.forget(key);
            }
        }
    }

    fn ensure_index_for(
        &mut self,
        predicate: &Predicate,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> IndexKey {
        for idx in self.indexes.values() {
            if idx.can_answer(predicate) {
                let kind_name = index_type_name(predicate.kind);
                return (kind_name.to_string(), predicate.attribute.clone());
            }
        }

        let attr = predicate.attribute.clone();
        let kind_name = index_type_name(predicate.kind);
        let mut idx = build_index_for(predicate.kind, &attr);

        let records: Vec<Record> = primary.iter().cloned().collect();
        let start = Instant::now();
        idx.build_from_records(&records);
        let elapsed = start.elapsed().as_secs_f64();

        let key: IndexKey = (kind_name.to_string(), attr);
        workload.record_build(key.clone(), elapsed);
        self.indexes.insert(key.clone(), idx);
        self.enforce_memory_budget(workload);
        key
    }

    fn get_index(&self, key: &IndexKey) -> Option<&dyn Index> {
        self.indexes.get(key).map(|b| b.as_ref())
    }

    pub fn execute(
        &mut self,
        query: &Query,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> Vec<Attrs> {
        match (&query.r#where, &query.top_k) {
            (None, None) => primary.iter().map(|r| r.attrs.clone()).collect(),
            (None, Some(tk)) => {
                let ids: HashSet<u64> = primary.ids().collect();
                self.apply_top_k(ids, tk, primary)
            }
            (Some(where_clause), _) => {
                let ids = self.evaluate_node(where_clause, primary, workload);
                if let Some(tk) = &query.top_k {
                    self.apply_top_k(ids, tk, primary)
                } else {
                    let mut sorted: Vec<u64> = ids.into_iter().collect();
                    sorted.sort_unstable();
                    sorted
                        .iter()
                        .filter_map(|id| primary.get(*id))
                        .map(|r| r.attrs.clone())
                        .collect()
                }
            }
        }
    }

    fn apply_top_k(
        &self,
        ids: HashSet<u64>,
        top_k: &crate::query::TopK,
        primary: &PrimaryStore,
    ) -> Vec<Attrs> {
        let attr = &top_k.attribute;
        let mut records: Vec<(&Attrs, &crate::Value)> = ids
            .iter()
            .filter_map(|id| primary.get(*id))
            .filter_map(|r| r.attrs.get(attr).map(|v| (&r.attrs, v)))
            .collect();

        records.sort_by(|(_, a), (_, b)| {
            let cmp = a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal);
            if top_k.descending {
                cmp.reverse()
            } else {
                cmp
            }
        });

        records
            .into_iter()
            .take(top_k.k)
            .map(|(attrs, _)| attrs.clone())
            .collect()
    }

    fn evaluate_node(
        &mut self,
        node: &Node,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> HashSet<u64> {
        match node {
            Node::Predicate(p) => self.evaluate_predicate(p, primary, workload),
            Node::And(a) => self.evaluate_and(a, primary, workload),
            Node::Or(o) => self.evaluate_or(o, primary, workload),
            Node::Not(n) => self.evaluate_not(n, primary, workload),
        }
    }

    fn evaluate_predicate(
        &mut self,
        pred: &Predicate,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> HashSet<u64> {
        let key = self.ensure_index_for(pred, primary, workload);
        match self.get_index(&key) {
            Some(idx) => {
                workload.record_hit(key);
                idx.execute(pred)
            }
            None => self.scan(pred, primary),
        }
    }

    fn evaluate_and(
        &mut self,
        node: &And,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> HashSet<u64> {
        if node.children.is_empty() {
            return primary.ids().collect();
        }
        let mut children = node.children.iter();
        let mut result = self.evaluate_node(children.next().unwrap(), primary, workload);
        for child in children {
            let child_ids = self.evaluate_node(child, primary, workload);
            result = result.intersection(&child_ids).copied().collect();
            if result.is_empty() {
                break;
            }
        }
        result
    }

    fn evaluate_or(
        &mut self,
        node: &Or,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> HashSet<u64> {
        let mut out = HashSet::new();
        for child in &node.children {
            out.extend(self.evaluate_node(child, primary, workload));
        }
        out
    }

    fn evaluate_not(
        &mut self,
        node: &Not,
        primary: &PrimaryStore,
        workload: &mut WorkloadTracker,
    ) -> HashSet<u64> {
        let universe: HashSet<u64> = primary.ids().collect();
        match &node.child {
            None => universe,
            Some(child) => {
                let excluded = self.evaluate_node(child, primary, workload);
                universe.difference(&excluded).copied().collect()
            }
        }
    }

    fn scan(&self, predicate: &Predicate, primary: &PrimaryStore) -> HashSet<u64> {
        let mut out = HashSet::new();
        for record in primary.iter() {
            if let Some(v) = record.attrs.get(&predicate.attribute) {
                if predicate.kind == PredicateKind::Equals && v == &predicate.value {
                    out.insert(record.id);
                }
            }
        }
        out
    }

    pub fn on_insert(&mut self, record: &Record) {
        for idx in self.indexes.values_mut() {
            idx.insert(record);
        }
    }

    pub fn on_delete(&mut self, record: &Record) {
        for idx in self.indexes.values_mut() {
            idx.remove(record);
        }
    }

    pub fn index_inventory(&self) -> Vec<(String, String, usize)> {
        self.indexes
            .iter()
            .map(|(k, idx)| {
                (k.0.clone(), idx.attribute().to_string(), idx.memory_estimate_bytes())
            })
            .collect()
    }
}
