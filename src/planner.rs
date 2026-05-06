use crate::index::base::Index;
use crate::index::{HashIndex, SortedIndex, TrieIndex, InvertedIndex, NgramIndex};
use crate::primary::{PrimaryStore, Record};
use crate::query::{Predicate, PredicateKind, Node, And, Or, Not, Query};
use crate::value::Attrs;
use crate::workload::WorkloadTracker;
use roaring::RoaringTreemap;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

type IndexKey = (String, String);
// Each live index is wrapped in its own RwLock so writers serialize per index
// while readers of other indexes proceed unblocked. The Arc lets us clone the
// handle out of the planner's index map and drop the planner-level lock
// before doing any actual index work.
type IndexCell = Arc<RwLock<Box<dyn Index>>>;

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

// Lock-free check that a key would point at an index capable of answering the
// given predicate. Lets the planner search its index table without taking any
// per-index read locks, since the type encoded in the key fully determines
// which predicate kinds the index supports.
fn key_can_answer(key: &IndexKey, predicate: &Predicate) -> bool {
    if key.1 != predicate.attribute {
        return false;
    }
    matches!(
        (key.0.as_str(), predicate.kind),
        ("HashIndex", PredicateKind::Equals)
            | ("SortedIndex", PredicateKind::Range | PredicateKind::Equals)
            | ("TrieIndex", PredicateKind::Prefix | PredicateKind::Equals)
            | ("InvertedIndex", PredicateKind::Contains)
            | ("NgramIndex", PredicateKind::Fuzzy)
    )
}

pub struct Planner {
    indexes: HashMap<IndexKey, IndexCell>,
    memory_budget_bytes: usize,
}

impl Planner {
    pub fn new(memory_budget_bytes: usize) -> Self {
        Self {
            indexes: HashMap::new(),
            memory_budget_bytes,
        }
    }

    pub fn set_memory_budget(&mut self, bytes_limit: usize, workload: &WorkloadTracker) {
        self.memory_budget_bytes = bytes_limit;
        self.enforce_memory_budget(workload);
    }

    fn total_memory(&self) -> usize {
        self.indexes
            .values()
            .map(|cell| cell.read().unwrap().memory_estimate_bytes())
            .sum()
    }

    fn enforce_memory_budget(&mut self, workload: &WorkloadTracker) {
        let mut total = self.total_memory();
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
            if let Some(cell) = self.indexes.remove(key) {
                let bytes = cell.read().unwrap().memory_estimate_bytes();
                total = total.saturating_sub(bytes);
                workload.forget(key);
            }
        }
    }

    // True if there is already an index that can answer this predicate.
    pub fn has_index_for(&self, predicate: &Predicate) -> bool {
        self.indexes.keys().any(|k| key_can_answer(k, predicate))
    }

    fn lookup_cell(&self, predicate: &Predicate) -> Option<(IndexKey, IndexCell)> {
        for (key, cell) in self.indexes.iter() {
            if key_can_answer(key, predicate) {
                return Some((key.clone(), cell.clone()));
            }
        }
        None
    }

    fn ensure_index_for(
        &mut self,
        predicate: &Predicate,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> IndexKey {
        for key in self.indexes.keys() {
            if key_can_answer(key, predicate) {
                return key.clone();
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
        self.indexes
            .insert(key.clone(), Arc::new(RwLock::new(idx)));
        self.enforce_memory_budget(workload);
        key
    }

    // Walk the query and build any missing index for each predicate. Caller
    // holds the planner write lock around this. Once it returns, every
    // predicate the query references has an index that can answer it (modulo
    // immediate eviction from a tight budget).
    pub fn prepare_for_query(
        &mut self,
        query: &Query,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) {
        if let Some(node) = &query.r#where {
            self.prepare_node(node, primary, workload);
        }
    }

    fn prepare_node(
        &mut self,
        node: &Node,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) {
        match node {
            Node::Predicate(p) => {
                self.ensure_index_for(p, primary, workload);
            }
            Node::And(a) => {
                for child in &a.children {
                    self.prepare_node(child, primary, workload);
                }
            }
            Node::Or(o) => {
                for child in &o.children {
                    self.prepare_node(child, primary, workload);
                }
            }
            Node::Not(n) => {
                if let Some(child) = &n.child {
                    self.prepare_node(child, primary, workload);
                }
            }
        }
    }

    // True if every predicate in the query has a matching live index.
    pub fn covers_query(&self, query: &Query) -> bool {
        match &query.r#where {
            None => true,
            Some(node) => self.covers_node(node),
        }
    }

    fn covers_node(&self, node: &Node) -> bool {
        match node {
            Node::Predicate(p) => self.has_index_for(p),
            Node::And(a) => a.children.iter().all(|c| self.covers_node(c)),
            Node::Or(o) => o.children.iter().all(|c| self.covers_node(c)),
            Node::Not(n) => match &n.child {
                None => true,
                Some(child) => self.covers_node(child),
            },
        }
    }

    // Read-only execute. Each predicate clones an Arc out of the index map
    // and takes a read lock on the per-index RwLock just for the duration of
    // the lookup. Writers updating a different index can run concurrently.
    pub fn execute(
        &self,
        query: &Query,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> Vec<Attrs> {
        match (&query.r#where, &query.top_k) {
            (None, None) => primary.iter().map(|r| r.attrs.clone()).collect(),
            (None, Some(tk)) => {
                let ids: RoaringTreemap = primary.ids().collect();
                self.apply_top_k(ids, tk, primary)
            }
            (Some(where_clause), _) => {
                let ids = self.evaluate_node(where_clause, primary, workload);
                if let Some(tk) = &query.top_k {
                    self.apply_top_k(ids, tk, primary)
                } else {
                    ids.iter()
                        .filter_map(|id| primary.get(id))
                        .map(|r| r.attrs.clone())
                        .collect()
                }
            }
        }
    }

    fn apply_top_k(
        &self,
        ids: RoaringTreemap,
        top_k: &crate::query::TopK,
        primary: &PrimaryStore,
    ) -> Vec<Attrs> {
        let attr = &top_k.attribute;
        let mut records: Vec<(&Attrs, &crate::Value)> = ids
            .iter()
            .filter_map(|id| primary.get(id))
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
        &self,
        node: &Node,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> RoaringTreemap {
        match node {
            Node::Predicate(p) => self.evaluate_predicate(p, primary, workload),
            Node::And(a) => self.evaluate_and(a, primary, workload),
            Node::Or(o) => self.evaluate_or(o, primary, workload),
            Node::Not(n) => self.evaluate_not(n, primary, workload),
        }
    }

    fn evaluate_predicate(
        &self,
        pred: &Predicate,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> RoaringTreemap {
        if let Some((key, cell)) = self.lookup_cell(pred) {
            workload.record_hit(key);
            let guard = cell.read().unwrap();
            return guard.execute(pred);
        }
        self.scan(pred, primary)
    }

    fn evaluate_and(
        &self,
        node: &And,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> RoaringTreemap {
        if node.children.is_empty() {
            return primary.ids().collect();
        }
        let mut children = node.children.iter();
        let mut result = self.evaluate_node(children.next().unwrap(), primary, workload);
        for child in children {
            let child_ids = self.evaluate_node(child, primary, workload);
            result &= child_ids;
            if result.is_empty() {
                break;
            }
        }
        result
    }

    fn evaluate_or(
        &self,
        node: &Or,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> RoaringTreemap {
        let mut out = RoaringTreemap::new();
        for child in &node.children {
            out |= self.evaluate_node(child, primary, workload);
        }
        out
    }

    fn evaluate_not(
        &self,
        node: &Not,
        primary: &PrimaryStore,
        workload: &WorkloadTracker,
    ) -> RoaringTreemap {
        let universe: RoaringTreemap = primary.ids().collect();
        match &node.child {
            None => universe,
            Some(child) => {
                let excluded = self.evaluate_node(child, primary, workload);
                universe - excluded
            }
        }
    }

    fn scan(&self, predicate: &Predicate, primary: &PrimaryStore) -> RoaringTreemap {
        let mut out = RoaringTreemap::new();
        for record in primary.iter() {
            if let Some(v) = record.attrs.get(&predicate.attribute) {
                if predicate.kind == PredicateKind::Equals && v == &predicate.value {
                    out.insert(record.id);
                }
            }
        }
        out
    }

    // Propagates an insert to every live index. Each per-index write lock is
    // held only for the duration of one index update so writers touching
    // different indexes do not block each other.
    pub fn on_insert(&self, record: &Record) {
        for cell in self.indexes.values() {
            cell.write().unwrap().insert(record);
        }
    }

    pub fn on_delete(&self, record: &Record) {
        for cell in self.indexes.values() {
            cell.write().unwrap().remove(record);
        }
    }

    pub fn index_inventory(&self) -> Vec<(String, String, usize)> {
        self.indexes
            .iter()
            .map(|(k, cell)| {
                let guard = cell.read().unwrap();
                (k.0.clone(), guard.attribute().to_string(), guard.memory_estimate_bytes())
            })
            .collect()
    }
}
