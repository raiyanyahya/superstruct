use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

// Edge weights are stored alongside (target, label). Backward compatibility:
// the old add_edge / remove_edge entry points keep working with weight 1.0,
// which preserves the BFS and unweighted shortest-path semantics. New
// weighted entry points (add_weighted_edge, dijkstra, etc.) work with
// arbitrary nonnegative weights.
#[derive(Debug)]
pub struct GraphStore {
    adj: HashMap<u64, HashMap<(u64, Option<String>), f64>>,
}

// Newtype wrapper so f64 distances can be ordered inside BinaryHeap. NaN is
// pushed to the bottom which is the safe choice given Dijkstra never sees
// NaN under valid input (we reject negative weights at insert time).
#[derive(Copy, Clone, PartialEq)]
struct OrderedF64(f64);

impl Eq for OrderedF64 {}

impl PartialOrd for OrderedF64 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedF64 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.partial_cmp(&other.0).unwrap_or(Ordering::Equal)
    }
}

impl GraphStore {
    pub fn new() -> Self {
        Self {
            adj: HashMap::new(),
        }
    }

    // Adds an unweighted edge. Equivalent to add_weighted_edge with weight 1.
    pub fn add_edge(&mut self, a: u64, b: u64, label: Option<String>, directed: bool) {
        self.add_weighted_edge(a, b, 1.0, label, directed);
    }

    pub fn add_weighted_edge(
        &mut self,
        a: u64,
        b: u64,
        weight: f64,
        label: Option<String>,
        directed: bool,
    ) {
        self.adj
            .entry(a)
            .or_default()
            .insert((b, label.clone()), weight);
        if !directed {
            self.adj
                .entry(b)
                .or_default()
                .insert((a, label), weight);
        }
    }

    pub fn remove_edge(
        &mut self,
        a: u64,
        b: u64,
        label: Option<String>,
        directed: bool,
    ) {
        if let Some(neighbors) = self.adj.get_mut(&a) {
            neighbors.remove(&(b, label.clone()));
        }
        if !directed {
            if let Some(neighbors) = self.adj.get_mut(&b) {
                neighbors.remove(&(a, label));
            }
        }
    }

    pub fn remove_node(&mut self, node_id: u64) {
        let outgoing = self.adj.remove(&node_id).unwrap_or_default();
        for ((neighbor, _), _) in &outgoing {
            if let Some(neighbors) = self.adj.get_mut(neighbor) {
                neighbors.retain(|(n, _), _| *n != node_id);
            }
        }
        self.adj.retain(|_, v| !v.is_empty());
    }

    pub fn neighbors(&self, node_id: u64, label: &Option<String>) -> HashSet<u64> {
        match self.adj.get(&node_id) {
            None => HashSet::new(),
            Some(neighbors) => match label {
                None => neighbors.keys().map(|(n, _)| *n).collect(),
                Some(l) => neighbors
                    .keys()
                    .filter(|(_, edge_label)| edge_label.as_ref() == Some(l))
                    .map(|(n, _)| *n)
                    .collect(),
            },
        }
    }

    fn weighted_neighbors(
        &self,
        node_id: u64,
        label: &Option<String>,
    ) -> Vec<(u64, f64)> {
        match self.adj.get(&node_id) {
            None => Vec::new(),
            Some(neighbors) => neighbors
                .iter()
                .filter(|((_, edge_label), _)| match label {
                    None => true,
                    Some(l) => edge_label.as_ref() == Some(l),
                })
                .map(|((n, _), w)| (*n, *w))
                .collect(),
        }
    }

    pub fn bfs(
        &self,
        start: u64,
        max_depth: Option<usize>,
        label: &Option<String>,
    ) -> HashMap<u64, usize> {
        let mut depths: HashMap<u64, usize> = HashMap::new();
        depths.insert(start, 0);
        let mut frontier = VecDeque::new();
        frontier.push_back(start);

        while let Some(node) = frontier.pop_front() {
            let current_depth = depths[&node];
            if let Some(max) = max_depth {
                if current_depth >= max {
                    continue;
                }
            }
            for neighbor in self.neighbors(node, label) {
                if depths.contains_key(&neighbor) {
                    continue;
                }
                depths.insert(neighbor, current_depth + 1);
                frontier.push_back(neighbor);
            }
        }
        depths
    }

    pub fn shortest_path(
        &self,
        source: u64,
        target: u64,
        label: &Option<String>,
    ) -> Option<Vec<u64>> {
        if source == target {
            return Some(vec![source]);
        }

        let mut predecessor: HashMap<u64, u64> = HashMap::new();
        predecessor.insert(source, source);
        let mut frontier = VecDeque::new();
        frontier.push_back(source);

        while let Some(node) = frontier.pop_front() {
            for neighbor in self.neighbors(node, label) {
                if predecessor.contains_key(&neighbor) {
                    continue;
                }
                predecessor.insert(neighbor, node);
                if neighbor == target {
                    let mut path = vec![target];
                    while path[path.len() - 1] != source {
                        let prev = predecessor[path.last().unwrap()];
                        path.push(prev);
                    }
                    path.reverse();
                    return Some(path);
                }
                frontier.push_back(neighbor);
            }
        }
        None
    }

    // Dijkstra over the directed graph from a single source. Returns the
    // minimum-cost distance to every reachable node. Edges with negative
    // weight are skipped at the insert layer so this loop never relaxes a
    // negative edge.
    pub fn dijkstra(
        &self,
        source: u64,
        label: &Option<String>,
    ) -> HashMap<u64, f64> {
        let mut dist: HashMap<u64, f64> = HashMap::new();
        let mut heap: BinaryHeap<(std::cmp::Reverse<OrderedF64>, u64)> = BinaryHeap::new();
        dist.insert(source, 0.0);
        heap.push((std::cmp::Reverse(OrderedF64(0.0)), source));

        while let Some((std::cmp::Reverse(OrderedF64(d)), node)) = heap.pop() {
            // Skip stale heap entries that have been superseded.
            if d > *dist.get(&node).unwrap_or(&f64::INFINITY) {
                continue;
            }
            for (neighbor, w) in self.weighted_neighbors(node, label) {
                if w < 0.0 {
                    continue;
                }
                let new_dist = d + w;
                let entry = dist.entry(neighbor).or_insert(f64::INFINITY);
                if new_dist < *entry {
                    *entry = new_dist;
                    heap.push((std::cmp::Reverse(OrderedF64(new_dist)), neighbor));
                }
            }
        }
        dist
    }

    // Dijkstra-based shortest weighted path. Returns the path from source to
    // target plus the total cost. None if target is unreachable.
    pub fn shortest_path_weighted(
        &self,
        source: u64,
        target: u64,
        label: &Option<String>,
    ) -> Option<(Vec<u64>, f64)> {
        if source == target {
            return Some((vec![source], 0.0));
        }
        let mut dist: HashMap<u64, f64> = HashMap::new();
        let mut prev: HashMap<u64, u64> = HashMap::new();
        let mut heap: BinaryHeap<(std::cmp::Reverse<OrderedF64>, u64)> = BinaryHeap::new();
        dist.insert(source, 0.0);
        heap.push((std::cmp::Reverse(OrderedF64(0.0)), source));

        while let Some((std::cmp::Reverse(OrderedF64(d)), node)) = heap.pop() {
            if node == target {
                break;
            }
            if d > *dist.get(&node).unwrap_or(&f64::INFINITY) {
                continue;
            }
            for (neighbor, w) in self.weighted_neighbors(node, label) {
                if w < 0.0 {
                    continue;
                }
                let new_dist = d + w;
                let entry = dist.entry(neighbor).or_insert(f64::INFINITY);
                if new_dist < *entry {
                    *entry = new_dist;
                    prev.insert(neighbor, node);
                    heap.push((std::cmp::Reverse(OrderedF64(new_dist)), neighbor));
                }
            }
        }

        let total = *dist.get(&target)?;
        let mut path = vec![target];
        let mut cursor = target;
        while cursor != source {
            cursor = *prev.get(&cursor)?;
            path.push(cursor);
        }
        path.reverse();
        Some((path, total))
    }

    // Iterative power-method PageRank. Uses out-edge weights as relative
    // transition probabilities. Dangling nodes (no outgoing edges) distribute
    // their mass uniformly over all nodes, which matches the original
    // formulation.
    pub fn pagerank(&self, damping: f64, iterations: usize) -> HashMap<u64, f64> {
        let mut nodes: HashSet<u64> = HashSet::new();
        for (&from, neighbors) in &self.adj {
            nodes.insert(from);
            for ((to, _), _) in neighbors {
                nodes.insert(*to);
            }
        }
        let n = nodes.len();
        if n == 0 {
            return HashMap::new();
        }
        let n_f = n as f64;

        let mut rank: HashMap<u64, f64> = nodes.iter().map(|&id| (id, 1.0 / n_f)).collect();

        // Cache per-node total outgoing weight so we avoid recomputing it on
        // every iteration.
        let out_weight: HashMap<u64, f64> = self
            .adj
            .iter()
            .map(|(&node, neighbors)| {
                let total: f64 = neighbors.values().sum();
                (node, total)
            })
            .collect();

        let teleport = (1.0 - damping) / n_f;
        for _ in 0..iterations {
            // Sum of rank held by dangling nodes. Distributed uniformly.
            let dangling_mass: f64 = nodes
                .iter()
                .filter(|id| out_weight.get(id).copied().unwrap_or(0.0) == 0.0)
                .map(|id| rank.get(id).copied().unwrap_or(0.0))
                .sum();
            let dangling_share = damping * dangling_mass / n_f;

            let mut new_rank: HashMap<u64, f64> =
                nodes.iter().map(|&id| (id, teleport + dangling_share)).collect();

            for (&from, neighbors) in &self.adj {
                let total = *out_weight.get(&from).unwrap_or(&0.0);
                if total <= 0.0 {
                    continue;
                }
                let from_rank = *rank.get(&from).unwrap_or(&0.0);
                for ((to, _), w) in neighbors {
                    let contrib = damping * from_rank * (w / total);
                    *new_rank.entry(*to).or_insert(0.0) += contrib;
                }
            }
            rank = new_rank;
        }
        rank
    }

    pub fn edges(&self) -> Vec<(u64, u64, Option<String>)> {
        let mut out = Vec::new();
        for (&node, neighbors) in &self.adj {
            for ((neighbor, label), _) in neighbors {
                out.push((node, *neighbor, label.clone()));
            }
        }
        out
    }

    pub fn weighted_edges(&self) -> Vec<(u64, u64, Option<String>, f64)> {
        let mut out = Vec::new();
        for (&node, neighbors) in &self.adj {
            for ((neighbor, label), weight) in neighbors {
                out.push((node, *neighbor, label.clone(), *weight));
            }
        }
        out
    }
}

impl Default for GraphStore {
    fn default() -> Self {
        Self::new()
    }
}
