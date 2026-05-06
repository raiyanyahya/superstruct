use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub hit_count: u64,
    pub last_used: Instant,
    pub build_cost_secs: f64,
}

impl Default for IndexStats {
    fn default() -> Self {
        Self {
            hit_count: 0,
            last_used: Instant::now(),
            build_cost_secs: 0.0,
        }
    }
}

#[derive(Debug, Default)]
pub struct WorkloadTracker {
    stats: HashMap<(String, String), IndexStats>,
}

impl WorkloadTracker {
    pub fn new() -> Self {
        Self { stats: HashMap::new() }
    }

    pub fn record_build(&mut self, key: (String, String), duration_secs: f64) {
        let s = self.stats.entry(key).or_default();
        s.build_cost_secs = duration_secs;
    }

    pub fn record_hit(&mut self, key: (String, String)) {
        let s = self.stats.entry(key).or_default();
        s.hit_count += 1;
        s.last_used = Instant::now();
    }

    pub fn forget(&mut self, key: &(String, String)) {
        self.stats.remove(key);
    }

    pub fn score(&self, key: &(String, String)) -> f64 {
        match self.stats.get(key) {
            None => 0.0,
            Some(s) => {
                let age = s.last_used.elapsed().as_secs_f64().max(1.0);
                (s.hit_count as f64 / age) + (s.build_cost_secs * 0.1)
            }
        }
    }
}
