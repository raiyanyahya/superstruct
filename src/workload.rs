use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

// One stats cell per (index_type, attribute). Hit counts and last-used time
// are atomic so the read path can update them without grabbing an exclusive
// lock. The map of cells itself sits behind an RwLock: read access for the
// common case (cell already exists), write access only when a new key is
// inserted or one is forgotten on eviction.
#[derive(Debug)]
struct IndexStatsCell {
    hit_count: AtomicU64,
    last_used_micros: AtomicU64,
    build_cost_micros: AtomicU64,
}

pub struct WorkloadTracker {
    stats: RwLock<HashMap<(String, String), Arc<IndexStatsCell>>>,
    epoch: Instant,
}

impl Default for WorkloadTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkloadTracker {
    pub fn new() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
            epoch: Instant::now(),
        }
    }

    fn now_micros(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }

    fn cell_for(&self, key: (String, String)) -> Arc<IndexStatsCell> {
        // Fast path: cell already exists, return a clone of the Arc.
        {
            let stats = self.stats.read().unwrap();
            if let Some(cell) = stats.get(&key) {
                return cell.clone();
            }
        }
        // Slow path: insert. Use entry() so two threads racing the same key
        // both end up with the same Arc.
        let mut stats = self.stats.write().unwrap();
        stats
            .entry(key)
            .or_insert_with(|| {
                Arc::new(IndexStatsCell {
                    hit_count: AtomicU64::new(0),
                    last_used_micros: AtomicU64::new(self.now_micros()),
                    build_cost_micros: AtomicU64::new(0),
                })
            })
            .clone()
    }

    pub fn record_build(&self, key: (String, String), duration_secs: f64) {
        let micros = (duration_secs * 1_000_000.0) as u64;
        let cell = self.cell_for(key);
        cell.build_cost_micros.store(micros, Ordering::Relaxed);
    }

    pub fn record_hit(&self, key: (String, String)) {
        let cell = self.cell_for(key);
        cell.hit_count.fetch_add(1, Ordering::Relaxed);
        cell.last_used_micros
            .store(self.now_micros(), Ordering::Relaxed);
    }

    pub fn forget(&self, key: &(String, String)) {
        self.stats.write().unwrap().remove(key);
    }

    pub fn score(&self, key: &(String, String)) -> f64 {
        let stats = self.stats.read().unwrap();
        match stats.get(key) {
            None => 0.0,
            Some(cell) => {
                let hits = cell.hit_count.load(Ordering::Relaxed) as f64;
                let last_used = cell.last_used_micros.load(Ordering::Relaxed);
                let now = self.now_micros();
                let age_micros = now.saturating_sub(last_used).max(1_000_000);
                let age_secs = age_micros as f64 / 1_000_000.0;
                let build_cost_secs =
                    cell.build_cost_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0;
                (hits / age_secs) + (build_cost_secs * 0.1)
            }
        }
    }
}
