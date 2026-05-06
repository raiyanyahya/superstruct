use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug)]
pub struct GraphStore {
    adj: HashMap<u64, HashSet<(u64, Option<String>)>>,
}

impl GraphStore {
    pub fn new() -> Self {
        Self {
            adj: HashMap::new(),
        }
    }

    pub fn add_edge(
        &mut self,
        a: u64,
        b: u64,
        label: Option<String>,
        directed: bool,
    ) {
        self.adj.entry(a).or_default().insert((b, label.clone()));
        if !directed {
            self.adj.entry(b).or_default().insert((a, label));
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
        for (neighbor, _) in &outgoing {
            if let Some(neighbors) = self.adj.get_mut(neighbor) {
                neighbors.retain(|(n, _)| *n != node_id);
            }
        }
        self.adj.retain(|_, v| !v.is_empty());
    }

    pub fn neighbors(&self, node_id: u64, label: &Option<String>) -> HashSet<u64> {
        match self.adj.get(&node_id) {
            None => HashSet::new(),
            Some(neighbors) => match label {
                None => neighbors.iter().map(|(n, _)| *n).collect(),
                Some(l) => neighbors
                    .iter()
                    .filter(|(_, edge_label)| edge_label.as_ref() == Some(l))
                    .map(|(n, _)| *n)
                    .collect(),
            },
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

    pub fn edges(&self) -> Vec<(u64, u64, Option<String>)> {
        let mut out = Vec::new();
        for (&node, neighbors) in &self.adj {
            for &(neighbor, ref label) in neighbors {
                out.push((node, neighbor, label.clone()));
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
