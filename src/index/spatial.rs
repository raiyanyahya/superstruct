use crate::index::base::Index;
use crate::primary::Record;
use crate::query::{Predicate, PredicateKind};
use crate::value::Value;
use roaring::RoaringTreemap;
use std::collections::HashMap;

// Reads a 2D point out of a Value. The expected shape is a List of two
// numerics, e.g. Value::List(vec![Value::Float(x), Value::Float(y)]).
// Integers are accepted as a convenience and coerced to f64.
fn parse_point(v: &Value) -> Option<(f64, f64)> {
    match v {
        Value::List(items) if items.len() == 2 => {
            let x = items[0].as_f64()?;
            let y = items[1].as_f64()?;
            Some((x, y))
        }
        _ => None,
    }
}

// Spatial index for 2D points. Stores all points in three parallel vectors
// kept sorted by x. Bounding-box and radius queries first do binary search
// on x to bracket the candidates, then linear scan within the band to check
// the y axis or the squared distance. Building from scratch is O(n log n).
// Inserting one record after build is O(n) because of the Vec::insert shift,
// which mirrors how SortedIndex behaves and matches the project's design.
//
// Memory cost is roughly 24 bytes per record for the parallel vectors plus
// the by_id lookup. At one million records that is around 50 MB, which is
// inside the default 64 MiB budget.
#[derive(Debug)]
pub struct SpatialIndex {
    attribute: String,
    xs: Vec<f64>,
    ys: Vec<f64>,
    ids: Vec<u64>,
    // Reverse lookup so remove() can find a record's coordinates without
    // walking every entry.
    by_id: HashMap<u64, (f64, f64)>,
}

impl SpatialIndex {
    pub fn new(attribute: String) -> Self {
        Self {
            attribute,
            xs: Vec::new(),
            ys: Vec::new(),
            ids: Vec::new(),
            by_id: HashMap::new(),
        }
    }

    fn x_range(&self, min_x: f64, max_x: f64) -> (usize, usize) {
        let lo = self.xs.partition_point(|&v| v < min_x);
        let hi = self.xs.partition_point(|&v| v <= max_x);
        (lo, hi)
    }
}

impl Index for SpatialIndex {
    fn attribute(&self) -> &str {
        &self.attribute
    }

    fn supports_kind(&self, kind: PredicateKind) -> bool {
        matches!(kind, PredicateKind::Within | PredicateKind::Near)
    }

    fn build_from_records(&mut self, records: &[Record]) {
        let mut buffer: Vec<(f64, f64, u64)> = Vec::new();
        for rec in records {
            if let Some(v) = rec.attrs.get(&self.attribute) {
                if let Some((x, y)) = parse_point(v) {
                    buffer.push((x, y, rec.id));
                }
            }
        }
        // partial_cmp is enough because we already filtered out non-numeric
        // values via parse_point. NaN should not appear in indexed points.
        buffer.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        self.xs = buffer.iter().map(|t| t.0).collect();
        self.ys = buffer.iter().map(|t| t.1).collect();
        self.ids = buffer.iter().map(|t| t.2).collect();
        self.by_id = buffer.iter().map(|t| (t.2, (t.0, t.1))).collect();
    }

    fn insert(&mut self, record: &Record) {
        if let Some(v) = record.attrs.get(&self.attribute) {
            if let Some((x, y)) = parse_point(v) {
                let pos = self.xs.partition_point(|&xv| xv < x);
                self.xs.insert(pos, x);
                self.ys.insert(pos, y);
                self.ids.insert(pos, record.id);
                self.by_id.insert(record.id, (x, y));
            }
        }
    }

    fn remove(&mut self, record: &Record) {
        if let Some((x, _y)) = self.by_id.remove(&record.id) {
            // Several records can share the same x coordinate. Walk the equal-x
            // band linearly to find the one whose id matches.
            let lo = self.xs.partition_point(|&v| v < x);
            let hi = self.xs.partition_point(|&v| v <= x);
            for i in lo..hi {
                if self.ids[i] == record.id {
                    self.xs.remove(i);
                    self.ys.remove(i);
                    self.ids.remove(i);
                    return;
                }
            }
        }
    }

    fn execute(&self, predicate: &Predicate) -> RoaringTreemap {
        match predicate.kind {
            PredicateKind::Within => {
                let coords = match &predicate.value {
                    Value::List(items) if items.len() == 4 => items,
                    _ => return RoaringTreemap::new(),
                };
                let min_x = match coords[0].as_f64() { Some(v) => v, None => return RoaringTreemap::new() };
                let min_y = match coords[1].as_f64() { Some(v) => v, None => return RoaringTreemap::new() };
                let max_x = match coords[2].as_f64() { Some(v) => v, None => return RoaringTreemap::new() };
                let max_y = match coords[3].as_f64() { Some(v) => v, None => return RoaringTreemap::new() };
                let (lo, hi) = self.x_range(min_x, max_x);
                let mut out = RoaringTreemap::new();
                for i in lo..hi {
                    if self.ys[i] >= min_y && self.ys[i] <= max_y {
                        out.insert(self.ids[i]);
                    }
                }
                out
            }
            PredicateKind::Near => {
                let coords = match &predicate.value {
                    Value::List(items) if items.len() == 2 => items,
                    _ => return RoaringTreemap::new(),
                };
                let cx = match coords[0].as_f64() { Some(v) => v, None => return RoaringTreemap::new() };
                let cy = match coords[1].as_f64() { Some(v) => v, None => return RoaringTreemap::new() };
                let r = predicate.threshold;
                if r < 0.0 {
                    return RoaringTreemap::new();
                }
                let (lo, hi) = self.x_range(cx - r, cx + r);
                let r2 = r * r;
                let mut out = RoaringTreemap::new();
                for i in lo..hi {
                    let dx = self.xs[i] - cx;
                    let dy = self.ys[i] - cy;
                    if dx * dx + dy * dy <= r2 {
                        out.insert(self.ids[i]);
                    }
                }
                out
            }
            _ => RoaringTreemap::new(),
        }
    }

    fn memory_estimate_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.xs.capacity() * 8
            + self.ys.capacity() * 8
            + self.ids.capacity() * 8
            + self.by_id.capacity() * (std::mem::size_of::<u64>() + 16 + 8)
    }
}
