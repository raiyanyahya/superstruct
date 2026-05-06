# Superstruct Session Summary — 2026-05-07

## What this project is

Superstruct is an adaptive, in-memory, schema-less, poly-index data structure. You insert hash maps, you run chained queries, and the structure observes your workload — lazily building the right sub-index on first use and evicting cold ones when memory gets tight. You never declare an index.

Originally a Python research project (~3,500 lines). Fully ported to Rust (~2,800 lines lib, 19 source files, 103 tests). Zero false negatives, zero runtime schema, zero boilerplate `CREATE INDEX`.

## Architecture

```
Superstruct facade
  ├── PrimaryStore      — canonical HashMap<u64, Record>, source of truth
  ├── Planner           — lazy index construction, AST walker, memory budget eviction
  ├── WorkloadTracker   — per-index hit counts + recency scores for eviction
  ├── GraphStore        — adjacency, BFS, unweighted shortest path, Dijkstra, weighted shortest path, PageRank
  ├── Indexes (6)
  │   ├── HashIndex     — equality O(1)
  │   ├── SortedIndex   — range + equality, parallel sorted vecs, O(log n + k)
  │   ├── TrieIndex     — prefix + equality, character trie, iterative DFS
  │   ├── InvertedIndex — word-level full text, regex tokenizer, roaring postings
  │   ├── NgramIndex    — fuzzy (Jaccard trigram) + substring (unpadded trigram intersect + verify)
  │   └── SpatialIndex  — within_box, near (circle), parallel xs/ys/ids sorted by x
  └── Sketches (2)
      ├── BloomSketch   — MD5-based, 16384 bits, 5 hashes, no false negatives
      └── CountMinSketch — 5x1024 table, always over-estimates
```

All id sets use `roaring::RoaringTreemap` (compressed bitmaps). The graph stores weighted edges as `HashMap<u64, HashMap<(u64, Option<String>), f64>>`.

Concurrency: each subsystem has its own `RwLock` (primary, planner, blooms, counts, graph). Per-index locking inside the planner — two writers touching different indexes run in parallel. WorkloadTracker uses atomic counters.

## Key files

| File | Role |
|---|---|
| src/core.rs | Superstruct facade + QueryBuilder (290 lines) |
| src/planner.rs | Lazy build, AST decomposition, eviction (260 lines) |
| src/graph.rs | BFS, shortest_path, Dijkstra, shortest_path_weighted, PageRank (340 lines) |
| src/query.rs | AST: Predicate, And, Or, Not, TopK, Query (80 lines) |
| src/value.rs | Value enum: Int, Float, String, List — Eq, Ord, Hash, Serialize, Deserialize |
| src/index/ngram.rs | Fuzzy + substring over roaring postings (150 lines) |
| src/index/spatial.rs | 2D bbox + radius, parallel sorted vecs (170 lines) |
| src/index/sorted.rs | Range/equals via partition_point (95 lines) |
| src/index/trie.rs | Char trie, iterative DFS (105 lines) |
| tests/integration_test.rs | 103 tests covering all paths |
| examples/benchmark.rs | Standard 8-section benchmark (50k records, 4+4 threads) |
| examples/heavy_benchmark.rs | Heavy benchmark (1M records, 16+16 threads) |

## Benchmark results (latest run, Rust 1.95, Linux 7.0)

**Standard:**
- Insert: 121k ops/sec at 50k records (8.3 us/insert)
- Warm queries: equals 1.5ms, range 1.2ms, prefix 2.4ms, contains 1.2ms, fuzzy 3.2ms
- Compound query vs scan: 23x (0.36ms vs 8.48ms)
- Concurrency: 5,700 ops/sec (4W+4R)
- Memory: 2.3 MB for 5 indexes on 20k records
- Spatial: within_box warm 3.7ms, near warm 47us
- Substring vs scan: 2x (4.4ms vs 8.4ms), contains correctly returns 0 (word boundary)
- Graph: Dijkstra 1.1ms, shortest_path_weighted 0.9ms, PageRank 15ms

**Heavy:**
- Insert: 206k ops/sec at 1M records
- Compound vs scan at 500k: 31x (2.8ms vs 87.8ms)
- Concurrency: 14,408 ops/sec (16W+16R)
- Read-only concurrency: 763 ops/sec (16 readers, bottleneck is HashMap cloning, not locking)
- Memory: 19.7 MB at 200k records (99 bytes/record)

## Commit history

```
349f7fd Add README for Rust port matching original Python style
7e56f49 Add Rust benchmark matching the original Python benchmark suite
00bfd86 Port entire project from Python to Rust
a5ce133 Initial commit: Python Superstruct v0.1.0
```

Head is at `d9d62de` with use case docs committed.

## What was added beyond the pure Python port

1. **Roaring bitmaps** — Replaced HashSet<u64> with RoaringTreemap across all indexes. 5-7x memory reduction, faster intersection.
2. **Substring search** — PredicateKind::Substring in NgramIndex. Unpadded trigram intersection + literal verify pass. Finds "cat" inside "concatenation" (contains() correctly does not).
3. **SpatialIndex** — PredicateKind::Within + Near. Parallel sorted xs/ys/ids vectors. Binary search on x, linear y scan.
4. **Weighted graph** — add_weighted_edge, Dijkstra, shortest_path_weighted, PageRank. OrderedF64 newtype for BinaryHeap. Backward compatible: add_edge() still works with weight 1.0.
5. **Finer-grained locking** — Split from single RwLock to per-subsystem locks. Per-index locks in planner.
6. **Heavy benchmark** — 1M records, 16+16 threads, top_k-only timing section.
7. **Use case docs** — docs/use_cases/ with three integration patterns + README overview.

## What's NOT done (future work, not yet started)

- Packed columnar primary store layout (per-record HashMaps dominate memory at scale)
- Incremental-only index building (first build is still O(n) full scan)
- Result streaming or id-only return paths (currently clones all HashMaps)
- Disk-backed primary store for cold fraction
- Cost-based query planner (selectivity estimation, reorder AND chain)
- Learned indexes as a 7th index type
- Versioned snapshots / write-ahead log
- Count() / exists() methods (no-hydration queries)
- Cached warm result sets
- Lock-free posting lists (two writers to same index still serialize)

## How to run

```bash
cargo test                          # 103 tests
cargo run --example benchmark --release        # standard benchmark
cargo run --example heavy_benchmark --release  # heavy benchmark (~3 min)
cargo build --release               # optimized lib
```
