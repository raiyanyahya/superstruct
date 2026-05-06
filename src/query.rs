use crate::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredicateKind {
    Equals,
    Range,
    Prefix,
    Contains,
    Fuzzy,
}

#[derive(Debug, Clone)]
pub struct Predicate {
    pub kind: PredicateKind,
    pub attribute: String,
    pub value: Value,
    pub threshold: f64,
}

impl Predicate {
    pub fn new(kind: PredicateKind, attribute: String, value: Value) -> Self {
        Self { kind, attribute, value, threshold: 0.5 }
    }

    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = threshold;
        self
    }
}

#[derive(Debug, Clone)]
pub struct And {
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Or {
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct Not {
    pub child: Option<Box<Node>>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Predicate(Predicate),
    And(And),
    Or(Or),
    Not(Not),
}

#[derive(Debug, Clone)]
pub struct TopK {
    pub attribute: String,
    pub k: usize,
    pub descending: bool,
}

#[derive(Debug, Clone)]
pub struct Query {
    pub r#where: Option<Node>,
    pub top_k: Option<TopK>,
}
