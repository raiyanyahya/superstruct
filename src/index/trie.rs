use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use std::collections::{HashMap, HashSet};

#[derive(Debug)]
struct TrieNode {
    children: HashMap<char, TrieNode>,
    ids: HashSet<u64>,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            ids: HashSet::new(),
        }
    }
}

#[derive(Debug)]
pub struct TrieIndex {
    attribute: String,
    root: TrieNode,
}

impl TrieIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            root: TrieNode::new(),
        }
    }

    fn walk_or_create(&mut self, value: &str) -> &mut TrieNode {
        let mut node = &mut self.root;
        for ch in value.chars() {
            node = node.children.entry(ch).or_insert_with(TrieNode::new);
        }
        node
    }

    fn walk_to(&self, value: &str) -> Option<&TrieNode> {
        let mut node = &self.root;
        for ch in value.chars() {
            node = node.children.get(&ch)?;
        }
        Some(node)
    }

    fn collect_subtree(node: &TrieNode) -> HashSet<u64> {
        let mut out = HashSet::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            out.extend(&current.ids);
            for child in current.children.values() {
                stack.push(child);
            }
        }
        out
    }
}

impl Index for TrieIndex {
    fn attribute(&self) -> &str {
        &self.attribute
    }

    fn supports_kind(&self, kind: PredicateKind) -> bool {
        matches!(kind, PredicateKind::Prefix | PredicateKind::Equals)
    }

    fn build_from_records(&mut self, records: &[Record]) {
        for rec in records {
            self.insert(rec);
        }
    }

    fn insert(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            if let Some(s) = value.as_str() {
                let terminal = self.walk_or_create(s);
                terminal.ids.insert(record.id);
            }
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some(value) = record.attrs.get(&self.attribute) {
            if let Some(s) = value.as_str() {
                let mut node = &mut self.root;
                for ch in s.chars() {
                    match node.children.get_mut(&ch) {
                        Some(n) => node = n,
                        None => return,
                    }
                }
                node.ids.remove(&record.id);
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> HashSet<u64> {
        let target = match predicate.value.as_str() {
            Some(s) => s,
            None => return HashSet::new(),
        };

        let node = match self.walk_to(target) {
            Some(n) => n,
            None => return HashSet::new(),
        };

        match predicate.kind {
            PredicateKind::Prefix => Self::collect_subtree(node),
            PredicateKind::Equals => node.ids.clone(),
            _ => HashSet::new(),
        }
    }

    fn memory_estimate_bytes(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        let mut stack = vec![&self.root];
        while let Some(node) = stack.pop() {
            total += node.children.capacity() * 64;
            total += node.ids.capacity() * std::mem::size_of::<u64>();
            for child in node.children.values() {
                stack.push(child);
            }
        }
        total
    }
}
