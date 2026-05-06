use crate::graph::GraphStore;
use crate::planner::Planner;
use crate::primary::{PrimaryStore, Record};
use crate::query::{Predicate, PredicateKind, Node, And, Or, Not, TopK, Query};
use crate::sketch::{BloomSketch, CountMinSketch};
use crate::value::{Attrs, Value};
use crate::workload::WorkloadTracker;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::RwLock;

pub const DEFAULT_MEMORY_BUDGET_BYTES: usize = 64 * 1024 * 1024;

struct Inner {
    primary: PrimaryStore,
    workload: WorkloadTracker,
    planner: Planner,
    blooms: HashMap<String, BloomSketch>,
    counts: HashMap<String, CountMinSketch>,
    graph: GraphStore,
}

pub struct Superstruct {
    inner: RwLock<Inner>,
}

impl Superstruct {
    pub fn new(memory_budget_bytes: Option<usize>, _thread_safe: bool) -> Self {
        let budget = memory_budget_bytes.unwrap_or(DEFAULT_MEMORY_BUDGET_BYTES);
        Self {
            inner: RwLock::new(Inner {
                primary: PrimaryStore::new(),
                workload: WorkloadTracker::new(),
                planner: Planner::new(budget),
                blooms: HashMap::new(),
                counts: HashMap::new(),
                graph: GraphStore::new(),
            }),
        }
    }

    pub fn insert(&self, attrs: Attrs) -> u64 {
        let mut inner = self.inner.write().unwrap();
        let record = inner.primary.insert(attrs);
        inner.planner.on_insert(&record);
        for (attr, value) in &record.attrs {
            inner.blooms.entry(attr.clone()).or_default().add(value);
            inner.counts.entry(attr.clone()).or_default().add(value);
        }
        record.id
    }

    pub fn delete(&self, record_id: u64) -> bool {
        let mut inner = self.inner.write().unwrap();
        match inner.primary.delete(record_id) {
            None => false,
            Some(record) => {
                inner.planner.on_delete(&record);
                inner.graph.remove_node(record_id);
                true
            }
        }
    }

    pub fn get(&self, record_id: u64) -> Option<Attrs> {
        let inner = self.inner.read().unwrap();
        inner.primary.get(record_id).map(|r| r.attrs.clone())
    }

    pub fn find(&self) -> QueryBuilder<'_> {
        QueryBuilder::new(self)
    }

    pub fn execute(&self, query: Query) -> Vec<Attrs> {
        let mut inner = self.inner.write().unwrap();
        let Inner { primary, workload, planner, .. } = &mut *inner;
        planner.execute(&query, primary, workload)
    }

    pub fn maybe_contains(&self, attribute: &str, value: &Value) -> bool {
        let inner = self.inner.read().unwrap();
        inner.blooms.get(attribute).map_or(false, |b| b.maybe_contains(value))
    }

    pub fn estimate_count(&self, attribute: &str, value: &Value) -> u64 {
        let inner = self.inner.read().unwrap();
        inner.counts.get(attribute).map_or(0, |cm| cm.estimate(value))
    }

    pub fn add_edge(&self, a: u64, b: u64, label: Option<String>, directed: bool) {
        let mut inner = self.inner.write().unwrap();
        inner.graph.add_edge(a, b, label, directed);
    }

    pub fn remove_edge(&self, a: u64, b: u64, label: Option<String>, directed: bool) {
        let mut inner = self.inner.write().unwrap();
        inner.graph.remove_edge(a, b, label, directed);
    }

    pub fn neighbors(&self, record_id: u64, label: Option<String>) -> HashSet<u64> {
        let inner = self.inner.read().unwrap();
        inner.graph.neighbors(record_id, &label)
    }

    pub fn bfs(&self, start: u64, max_depth: Option<usize>, label: Option<String>) -> HashMap<u64, usize> {
        let inner = self.inner.read().unwrap();
        inner.graph.bfs(start, max_depth, &label)
    }

    pub fn shortest_path(&self, source: u64, target: u64, label: Option<String>) -> Option<Vec<u64>> {
        let inner = self.inner.read().unwrap();
        inner.graph.shortest_path(source, target, &label)
    }

    pub fn set_memory_budget(&self, bytes_limit: usize) {
        let mut inner = self.inner.write().unwrap();
        let Inner { planner, workload, .. } = &mut *inner;
        planner.set_memory_budget(bytes_limit, workload);
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.primary.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn index_inventory(&self) -> Vec<(String, String, usize)> {
        let inner = self.inner.read().unwrap();
        inner.planner.index_inventory()
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let inner = self.inner.read().unwrap();
        let records: Vec<SerializableRecord> = inner
            .primary
            .iter()
            .map(|r| SerializableRecord { id: r.id, attrs: r.attrs.clone() })
            .collect();
        let edges: Vec<SerializableEdge> = inner
            .graph
            .edges()
            .into_iter()
            .map(|(from, to, label)| SerializableEdge { from, to, label })
            .collect();
        let payload = PersistencePayload {
            version: 1,
            next_id: inner.primary.next_id(),
            records,
            edges,
        };
        let json = serde_json::to_string_pretty(&payload)?;
        fs::write(path, json)
    }

    pub fn load(path: &str, memory_budget_bytes: Option<usize>, thread_safe: bool) -> std::io::Result<Self> {
        let json = fs::read_to_string(path)?;
        let payload: PersistencePayload = serde_json::from_str(&json)?;
        let budget = memory_budget_bytes.unwrap_or(DEFAULT_MEMORY_BUDGET_BYTES);

        let ss = Self::new(Some(budget), thread_safe);
        let mut inner = ss.inner.write().unwrap();
        inner.primary.set_next_id(payload.next_id);

        for entry in &payload.records {
            let record = Record { id: entry.id, attrs: entry.attrs.clone() };
            inner.primary.insert_at(entry.id, entry.attrs.clone());
            inner.planner.on_insert(&record);
            for (attr, value) in &record.attrs {
                inner.blooms.entry(attr.clone()).or_default().add(value);
                inner.counts.entry(attr.clone()).or_default().add(value);
            }
        }

        inner.primary.set_next_id(payload.next_id);

        for edge in &payload.edges {
            inner.graph.add_edge(edge.from, edge.to, edge.label.clone(), true);
        }

        inner.planner = Planner::new(budget);

        drop(inner);
        Ok(ss)
    }
}

#[derive(Serialize, Deserialize)]
struct PersistencePayload {
    version: u32,
    next_id: u64,
    records: Vec<SerializableRecord>,
    edges: Vec<SerializableEdge>,
}

#[derive(Serialize, Deserialize)]
struct SerializableRecord {
    id: u64,
    attrs: Attrs,
}

#[derive(Serialize, Deserialize)]
struct SerializableEdge {
    from: u64,
    to: u64,
    label: Option<String>,
}

pub struct QueryBuilder<'a> {
    owner: &'a Superstruct,
    and_children: Vec<Node>,
    top_k: Option<TopK>,
}

impl<'a> QueryBuilder<'a> {
    pub fn new(owner: &'a Superstruct) -> Self {
        Self { owner, and_children: Vec::new(), top_k: None }
    }

    pub fn equals(mut self, attribute: &str, value: impl Into<Value>) -> Self {
        self.and_children.push(Node::Predicate(Predicate::new(
            PredicateKind::Equals, attribute.to_string(), value.into(),
        )));
        self
    }

    pub fn range(mut self, attribute: &str, low: impl Into<Value>, high: impl Into<Value>) -> Self {
        self.and_children.push(Node::Predicate(Predicate::new(
            PredicateKind::Range,
            attribute.to_string(),
            Value::List(vec![low.into(), high.into()]),
        )));
        self
    }

    pub fn prefix(mut self, attribute: &str, prefix: &str) -> Self {
        self.and_children.push(Node::Predicate(Predicate::new(
            PredicateKind::Prefix, attribute.to_string(), Value::from(prefix),
        )));
        self
    }

    pub fn contains(mut self, attribute: &str, word: &str) -> Self {
        self.and_children.push(Node::Predicate(Predicate::new(
            PredicateKind::Contains, attribute.to_string(), Value::from(word),
        )));
        self
    }

    pub fn fuzzy(mut self, attribute: &str, value: &str, threshold: f64) -> Self {
        self.and_children.push(Node::Predicate(
            Predicate::new(PredicateKind::Fuzzy, attribute.to_string(), Value::from(value))
                .with_threshold(threshold),
        ));
        self
    }

    pub fn any_of(mut self, nodes: Vec<Node>) -> Self {
        self.and_children.push(Node::Or(Or { children: nodes }));
        self
    }

    pub fn exclude(mut self, node: Node) -> Self {
        self.and_children.push(Node::Not(Not { child: Some(Box::new(node)) }));
        self
    }

    pub fn top_k(mut self, attribute: &str, k: usize, descending: bool) -> Self {
        self.top_k = Some(TopK { attribute: attribute.to_string(), k, descending });
        self
    }

    pub fn to_node(&self) -> Option<Node> {
        match self.and_children.len() {
            0 => None,
            1 => Some(self.and_children[0].clone()),
            _ => Some(Node::And(And { children: self.and_children.clone() })),
        }
    }

    pub fn execute(&self) -> Vec<Attrs> {
        let query = Query { r#where: self.to_node(), top_k: self.top_k.clone() };
        self.owner.execute(query)
    }
}
