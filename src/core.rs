use crate::graph::GraphStore;
use crate::planner::Planner;
use crate::primary::PrimaryStore;
use crate::query::{Predicate, PredicateKind, Node, And, Or, Not, TopK, Query};
use crate::sketch::{BloomSketch, CountMinSketch};
use crate::value::{Attrs, Value};
use crate::workload::WorkloadTracker;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::RwLock;

pub const DEFAULT_MEMORY_BUDGET_BYTES: usize = 64 * 1024 * 1024;

// Each piece of state lives behind its own lock so concurrent reads of the
// primary store and existing indexes do not serialize on a single mutex.
// WorkloadTracker has its own internal synchronization (atomics + RwLock on
// the inner map) so it does not need an outer lock here. Acquisition order
// for the locks below, to avoid deadlock: primary, planner, blooms, counts,
// graph. All call sites in this file follow that order.
pub struct Superstruct {
    primary: RwLock<PrimaryStore>,
    planner: RwLock<Planner>,
    workload: WorkloadTracker,
    blooms: RwLock<HashMap<String, BloomSketch>>,
    counts: RwLock<HashMap<String, CountMinSketch>>,
    graph: RwLock<GraphStore>,
}

impl Superstruct {
    pub fn new(memory_budget_bytes: Option<usize>) -> Self {
        let budget = memory_budget_bytes.unwrap_or(DEFAULT_MEMORY_BUDGET_BYTES);
        Self {
            primary: RwLock::new(PrimaryStore::new()),
            planner: RwLock::new(Planner::new(budget)),
            workload: WorkloadTracker::new(),
            blooms: RwLock::new(HashMap::new()),
            counts: RwLock::new(HashMap::new()),
            graph: RwLock::new(GraphStore::new()),
        }
    }

    pub fn insert(&self, attrs: Attrs) -> u64 {
        let record = self.primary.write().unwrap().insert(attrs);
        // Per-index propagation only needs a read lock on the planner: the
        // index map is not changing, only each per-index RwLock is taken
        // briefly inside on_insert. Two parallel inserts of records that
        // touch disjoint attributes can therefore run with full overlap on
        // their per-index writes.
        self.planner.read().unwrap().on_insert(&record);
        {
            let mut blooms = self.blooms.write().unwrap();
            let mut counts = self.counts.write().unwrap();
            for (attr, value) in &record.attrs {
                blooms.entry(attr.clone()).or_default().add(value);
                counts.entry(attr.clone()).or_default().add(value);
            }
        }
        record.id
    }

    pub fn delete(&self, record_id: u64) -> bool {
        let removed = self.primary.write().unwrap().delete(record_id);
        match removed {
            None => false,
            Some(record) => {
                self.planner.read().unwrap().on_delete(&record);
                self.graph.write().unwrap().remove_node(record_id);
                true
            }
        }
    }

    pub fn get(&self, record_id: u64) -> Option<Attrs> {
        self.primary
            .read()
            .unwrap()
            .get(record_id)
            .map(|r| r.attrs.clone())
    }

    pub fn find(&self) -> QueryBuilder<'_> {
        QueryBuilder::new(self)
    }

    pub fn execute(&self, query: Query) -> Vec<Attrs> {
        // Fast path. Take read locks on everything mutating state could need
        // and try the query straight against the existing index set. If every
        // predicate is already covered, we never touch a write lock and many
        // threads can run this path at once. Workload is internally
        // synchronized via atomics, so no lock to take there.
        {
            let planner = self.planner.read().unwrap();
            if planner.covers_query(&query) {
                let primary = self.primary.read().unwrap();
                return planner.execute(&query, &primary, &self.workload);
            }
        }

        // Slow path. At least one predicate index has to be built. Take the
        // planner write lock, build whatever the query needs, then drop the
        // write lock and run the query under read locks. Other threads only
        // serialize on the build window.
        {
            let primary = self.primary.read().unwrap();
            let mut planner = self.planner.write().unwrap();
            planner.prepare_for_query(&query, &primary, &self.workload);
        }

        let planner = self.planner.read().unwrap();
        let primary = self.primary.read().unwrap();
        planner.execute(&query, &primary, &self.workload)
    }

    pub fn maybe_contains(&self, attribute: &str, value: &Value) -> bool {
        self.blooms
            .read()
            .unwrap()
            .get(attribute)
            .is_some_and(|b| b.maybe_contains(value))
    }

    pub fn estimate_count(&self, attribute: &str, value: &Value) -> u64 {
        self.counts
            .read()
            .unwrap()
            .get(attribute)
            .map_or(0, |cm| cm.estimate(value))
    }

    pub fn add_edge(&self, a: u64, b: u64, label: Option<String>, directed: bool) {
        self.graph.write().unwrap().add_edge(a, b, label, directed);
    }

    pub fn add_weighted_edge(
        &self,
        a: u64,
        b: u64,
        weight: f64,
        label: Option<String>,
        directed: bool,
    ) {
        self.graph
            .write()
            .unwrap()
            .add_weighted_edge(a, b, weight, label, directed);
    }

    pub fn remove_edge(&self, a: u64, b: u64, label: Option<String>, directed: bool) {
        self.graph.write().unwrap().remove_edge(a, b, label, directed);
    }

    pub fn neighbors(&self, record_id: u64, label: Option<String>) -> HashSet<u64> {
        self.graph.read().unwrap().neighbors(record_id, &label)
    }

    pub fn bfs(&self, start: u64, max_depth: Option<usize>, label: Option<String>) -> HashMap<u64, usize> {
        self.graph.read().unwrap().bfs(start, max_depth, &label)
    }

    pub fn shortest_path(&self, source: u64, target: u64, label: Option<String>) -> Option<Vec<u64>> {
        self.graph.read().unwrap().shortest_path(source, target, &label)
    }

    // Single-source shortest weighted distance to every reachable node.
    pub fn dijkstra(&self, source: u64, label: Option<String>) -> HashMap<u64, f64> {
        self.graph.read().unwrap().dijkstra(source, &label)
    }

    // Shortest weighted path between two nodes. Returns the node sequence and
    // the total distance, or None if target is unreachable.
    pub fn shortest_path_weighted(
        &self,
        source: u64,
        target: u64,
        label: Option<String>,
    ) -> Option<(Vec<u64>, f64)> {
        self.graph
            .read()
            .unwrap()
            .shortest_path_weighted(source, target, &label)
    }

    // PageRank over all nodes that appear in the graph. damping is typically
    // 0.85 and iterations 20 to 50 for the power method to converge for
    // most graph shapes.
    pub fn pagerank(&self, damping: f64, iterations: usize) -> HashMap<u64, f64> {
        self.graph.read().unwrap().pagerank(damping, iterations)
    }

    pub fn set_memory_budget(&self, bytes_limit: usize) {
        let mut planner = self.planner.write().unwrap();
        planner.set_memory_budget(bytes_limit, &self.workload);
    }

    pub fn len(&self) -> usize {
        self.primary.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn index_inventory(&self) -> Vec<(String, String, usize)> {
        self.planner.read().unwrap().index_inventory()
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let primary = self.primary.read().unwrap();
        let graph = self.graph.read().unwrap();
        let records: Vec<SerializableRecord> = primary
            .iter()
            .map(|r| SerializableRecord { id: r.id, attrs: r.attrs.clone() })
            .collect();
        let edges: Vec<SerializableEdge> = graph
            .weighted_edges()
            .into_iter()
            .map(|(from, to, label, weight)| SerializableEdge {
                from,
                to,
                label,
                weight,
            })
            .collect();
        let payload = PersistencePayload {
            version: 1,
            next_id: primary.next_id(),
            records,
            edges,
        };
        let json = serde_json::to_string_pretty(&payload)?;
        fs::write(path, json)
    }

    pub fn load(path: &str, memory_budget_bytes: Option<usize>) -> std::io::Result<Self> {
        let json = fs::read_to_string(path)?;
        let payload: PersistencePayload = serde_json::from_str(&json)?;

        let ss = Self::new(memory_budget_bytes);
        {
            let mut primary = ss.primary.write().unwrap();
            let mut blooms = ss.blooms.write().unwrap();
            let mut counts = ss.counts.write().unwrap();
            for entry in &payload.records {
                primary.insert_at(entry.id, entry.attrs.clone());
                for (attr, value) in &entry.attrs {
                    blooms.entry(attr.clone()).or_default().add(value);
                    counts.entry(attr.clone()).or_default().add(value);
                }
            }
            primary.set_next_id(payload.next_id);
        }

        {
            let mut graph = ss.graph.write().unwrap();
            for edge in &payload.edges {
                graph.add_weighted_edge(
                    edge.from,
                    edge.to,
                    edge.weight,
                    edge.label.clone(),
                    true,
                );
            }
        }

        // Indexes intentionally not rebuilt here. They materialize lazily on
        // the first query that needs them, same contract as the live struct.
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
    // Default to 1.0 so snapshots written by older code (without a weight
    // field) deserialize as unweighted edges.
    #[serde(default = "default_edge_weight")]
    weight: f64,
}

fn default_edge_weight() -> f64 {
    1.0
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

    pub fn substring(mut self, attribute: &str, value: &str) -> Self {
        self.and_children.push(Node::Predicate(Predicate::new(
            PredicateKind::Substring,
            attribute.to_string(),
            Value::from(value),
        )));
        self
    }

    pub fn within_box(
        mut self,
        attribute: &str,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
    ) -> Self {
        self.and_children.push(Node::Predicate(Predicate::new(
            PredicateKind::Within,
            attribute.to_string(),
            Value::List(vec![
                Value::Float(min_x),
                Value::Float(min_y),
                Value::Float(max_x),
                Value::Float(max_y),
            ]),
        )));
        self
    }

    pub fn near(mut self, attribute: &str, x: f64, y: f64, radius: f64) -> Self {
        self.and_children.push(Node::Predicate(
            Predicate::new(
                PredicateKind::Near,
                attribute.to_string(),
                Value::List(vec![Value::Float(x), Value::Float(y)]),
            )
            .with_threshold(radius),
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
