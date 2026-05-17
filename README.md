# Superstruct

> The data structure that watches how you use it then quietly becomes the right one.

> This project was built for fun, for learning and as a research sandbox. It is not a production system and does not try to be one. Everything here exists because it was interesting to build.

Superstruct is an in-memory Rust data structure that holds your records in one place and answers your questions through a zoo of classical sub-structures it builds on demand. You insert hash maps. You ask questions. The structure observes your workload and decides which sub-structure to consult, builds it lazily the first time it is needed and evicts it when memory gets tight. **You never declare an index.**

Hash map, sorted index, trie, inverted index, trigram fuzzy and substring index, 2D spatial index, bloom filter, count-min sketch and a weighted graph layer with shortest path, Dijkstra and PageRank all sit inside one Rust struct. A small chained query DSL routes across them.

```rust
use superstruct::*;
use std::collections::HashMap;

let ss = Superstruct::new(None);
ss.insert(HashMap::from([
    ("name".into(), Value::String("Alice".into())),
    ("age".into(),  Value::Int(30)),
    ("city".into(), Value::String("NYC".into())),
    ("bio".into(),  Value::String("loves cats".into())),
]));
ss.insert(HashMap::from([
    ("name".into(), Value::String("Anya".into())),
    ("age".into(),  Value::Int(25)),
    ("city".into(), Value::String("SF".into())),
    ("bio".into(),  Value::String("dog person".into())),
]));
// ... thousands more

// This single chained query splits across three indexes plus a sort.
// None of them existed before this line ran.
let results = ss.find()
    .range("age", Value::Int(25), Value::Int(35))
    .prefix("name", "A")
    .contains("bio", "cat")
    .top_k("score", 10, true)
    .execute();
```

---

### Explain it to a five-year-old

Imagine you have a giant box of LEGOs. Every time you put a new piece in, the box secretly watches. When you ask "show me all the red ones," the box quickly sorts them for you. When you ask "show me the tall ones," it does that too. It never asks you to organize anything first. It just figures out what you want and makes it fast. And when the box gets too full, it quietly puts away the organizers you stopped using.

That is Superstruct. A box you throw data into, and it turns itself into whatever shape you need next.

### Explain it to an engineer

You know that moment when you are prototyping and you think "I should put a hash index on this field, but wait, I will also need range queries on that field, and maybe full-text on this other one, and fuzzy matching might be useful too"? And then you spend twenty minutes wiring up five different data structures, syncing them on every insert and delete, and half of them turn out to be unused.

Superstruct eliminates that entire decision tree. You call `insert(some_hashmap)` and `find().equals(...).range(...).execute()`. It observes every query, lazily builds the exact index the query needs, and evicts cold ones under a memory cap. Six index types, two sketches, a graph layer, all behind a chained builder API. No `CREATE INDEX`, no schema, no upfront design.

### What was discovered here

The insight worth porting from the original Python version is: **a database planner does not require a database**. Strip away SQL, strip away the storage engine, strip away the client/server boundary, and what is left is a routing layer that maps predicate shapes to index types. That routing layer is about two hundred lines of code. Everything else (the five indexes, the sketches and the graph) are standard textbook data structures. The innovation is putting them all under one roof with a lazy-adaptive policy and making the whole thing feel like a single object.

The Rust port sharpened this with roaring bitmaps for id sets (six times less memory, faster intersections), per-index locking so writers to different indexes run in parallel and three new index types (spatial, substring, weighted graph) that plugged into the planner routing pattern with zero API changes.

### When would you actually use this

The honest answer: right now, for prototyping, exploratory data work and single-node in-memory workloads where you do not want to run a database. The compound query speedup is real enough that it earns its keep on any dataset over about ten thousand records. The laziness means you pay zero upfront. No indexes exist until someone asks a question that needs one. And when you save and load, indexes rebuild on first access, so the persisted state is tiny.

It is not a database replacement. It will not beat Postgres for OLTP or DuckDB for analytics. But for that narrow-yet-pervasive use case of "I have a pile of hash maps, I need to search them in several different ways, I do not want to schema-design or index-design anything," it occupies a spot nothing else fills.

---

## Table of contents

- [The big idea](#the-big-idea)
- [Quick start](#quick-start)
- [Architecture](#architecture)
- [The cast of characters](#the-cast-of-characters)
- [Feature tour](#feature-tour)
- [Inside the nucleus](#inside-the-nucleus)
- [Live benchmarks](#live-benchmarks)
- [How it compares to other things](#how-it-compares-to-other-things)
- [Full API reference](#full-api-reference)
- [Limitations and honest caveats](#limitations-and-honest-caveats)
- [Future directions](#future-directions)
- [Project layout](#project-layout)
- [Run it yourself](#run-it-yourself)

---

## The big idea

The classical way to make queries fast is to build the right data structure ahead of time. Hash map for equality. B-tree for ranges. Trie for prefixes. Inverted index for full text. Bloom filter for probable absence. Each one is excellent at one thing.

The price is that the developer has to predict in advance which structures they will need and pay the cost of building and maintaining all of them. This is why every database asks you to write `CREATE INDEX` and why most in-memory libraries focus on exactly one structure. The trade is correctness and performance against schema-design effort.

Superstruct flips the trade. Inside one struct lives every classical structure. The user never declares any of them. Every insert is held in a primary store as the canonical truth. When a query first arrives that would benefit from a particular sub-structure, the structure is built lazily by walking the primary store. Subsequent queries of that kind reuse the warm structure. When memory pressure rises, the least useful structures are evicted by a recency weighted frequency score. They get rebuilt the next time someone asks the right question.

The thesis in one sentence:

> The user stores data and asks questions. The structure watches the workload and self-organizes its internal indexes to minimize total query latency under a memory budget.

This is the same idea database planners use behind a SQL parser, but lifted out and turned into an embeddable Rust data structure with a small chained API.

---

## Quick start

Rust 1.70 or newer.

```bash
git clone <wherever this lives>
cd superstruct

# Run the test suite. Should be green.
cargo test

# Run the benchmark suite.
cargo run --example benchmark --release
```

Then open a Rust project.

```rust
use superstruct::*;
use std::collections::HashMap;

let ss = Superstruct::new(None);
ss.insert(HashMap::from([
    ("name".into(), Value::String("Alice".into())),
    ("age".into(),  Value::Int(30)),
    ("city".into(), Value::String("NYC".into())),
    ("score".into(), Value::Int(88)),
]));
ss.find().equals("city", Value::String("NYC".into())).execute();
ss.find()
    .range("age", Value::Int(20), Value::Int(40))
    .top_k("score", 5, true)
    .execute();
```

That is the whole getting started flow. Every other feature plugs in through methods on the same `ss` object or chained on `ss.find()`.

---

## Architecture

```
                        ┌───────────────────────────────┐
                        │        Superstruct facade     │
                        │  insert  delete  get  find    │
                        │  save    load    add_edge ... │
                        └────────────┬──────────────────┘
                                     │
              ┌──────────────────────┼──────────────────────┐
              │                      │                      │
       ┌──────▼──────┐        ┌──────▼─────┐        ┌───────▼──────┐
       │ PrimaryStore│        │   Planner  │        │ GraphStore   │
       │  source of  │        │ lazy build │        │  adjacency   │
       │   truth     │        │ AST walker │        │  bfs / paths │
       └──────┬──────┘        │ eviction   │        └──────────────┘
              │               └──────┬─────┘
              │                      │
              │             ┌────────┴───────┬───────┬───────┬───────┐
              │             ▼                ▼       ▼       ▼       ▼
              │        ┌────────┐  ┌──────────┐  ┌─────┐ ┌────────┐ ┌──────┐
              │        │  Hash  │  │  Sorted  │  │Trie │ │Inverted│ │Ngram │
              │        │  index │  │  index   │  │     │ │ index  │ │ index│
              │        └────────┘  └──────────┘  └─────┘ └────────┘ └──────┘
              │
              │             ┌──────────────┬──────────────┐
              │             ▼              ▼              ▼
              │        ┌────────┐    ┌──────────┐
              │        │ Bloom  │    │ CountMin │   (auto attached per attribute,
              │        │ sketch │    │  sketch  │    never evicted)
              │        └────────┘    └──────────┘
              │
              └──────  WorkloadTracker (per index hit counts and recency)
```

Every record goes into the **PrimaryStore** synchronously and stays there forever until you delete it. The **Planner** decides which **Index** to build and consult for each query, asking the **WorkloadTracker** for help when it has to choose what to evict. Sketches are auto attached per attribute and answer cheap probable-membership and approximate-frequency questions. The **GraphStore** is independent and operates on record ids.

The user only ever talks to the **Superstruct facade**. Everything else is internal.

---

## The cast of characters

Same architecture, told as a small office.

- **The Vault** is the primary store. One locked room with every record in it. Always correct. Walking it end to end is slow but always works.
- **The Specialists** are the indexes. Each one is brilliant at one specific kind of question.
  - The **Hash Specialist** is a phone book. Give them a name, they hand you the number.
  - The **Sorted Specialist** keeps cards in order. Great at "everyone aged 25 to 35".
  - The **Trie Specialist** is obsessed with how words start.
  - The **Inverted Index Specialist** has read every bio and remembers every word.
  - The **N-gram Specialist** is the one to ask when you misspell things, and when you ask "find anything containing this exact substring".
  - The **Cartographer of Places** is the spatial index. Hand them a bounding box or a point and a radius, they give you everyone inside.
- **The Sketches** are two clerks at the front desk. One says "I am pretty sure I have not seen this before, do not bother going to the Vault." The other says "I have seen this name about 47 times this week."
- **The Manager** is the planner. Hires a Specialist when a new kind of question shows up. Fires the laziest Specialist when the office gets crowded. Splits compound questions across multiple Specialists and combines their answers.
- **HR** is the workload tracker. Keeps a tally of which Specialist gets used a lot and which one is just sitting around.
- **The Cartographer of People** is the graph store. Knows who is friends with whom, with what weight, and can answer "shortest path", "PageRank" and "BFS depth from here". Independent of the Specialists.

The user only knocks on the front desk.

---

## Feature tour

### Inserting and deleting

```rust
let ss = Superstruct::new(None);
let alice = ss.insert(HashMap::from([
    ("name".into(), Value::String("Alice".into())),
    ("age".into(),  Value::Int(30)),
    ("city".into(), Value::String("NYC".into())),
]));                             // returns id
ss.delete(alice);               // returns bool
ss.len();                       // record count
ss.get(alice);                  // Option<HashMap<String, Value>>
```

Records are arbitrary maps. Different records can have different keys. There is no schema.

### Equality, range, prefix

```rust
ss.find().equals("city", Value::String("NYC".into())).execute();
ss.find().range("age", Value::Int(25), Value::Int(35)).execute();  // both ends inclusive
ss.find().prefix("name", "A").execute();
```

The first call of each kind builds the right index lazily. Later calls reuse it.

### Compound queries

```rust
ss.find()
  .range("age", Value::Int(25), Value::Int(35))
  .prefix("name", "A")
  .equals("city", Value::String("SF".into()))
  .top_k("score", 10, true)
  .execute();
```

Each predicate runs against its own sub-index. The id sets are intersected. Top-k is the final ordering step.

### Boolean composition

```rust
// OR group
ss.find().any_of(vec![
    ss.find().equals("city", Value::String("NYC".into())).to_node().unwrap(),
    ss.find().range("score", Value::Int(90), Value::Int(100)).to_node().unwrap(),
]).execute();

// NOT
ss.find().exclude(
    ss.find().equals("name", Value::String("Alice".into())).to_node().unwrap()
).execute();
```

### Full text search

```rust
ss.find().contains("bio", "cats").execute();
```

Tokenization is lowercase and alphanumeric. Multiple `contains` predicates AND together because they sit at the top level of the implicit AND.

### Fuzzy match

```rust
ss.find().fuzzy("name", "Alise", 0.4).execute();
```

Trigram Jaccard similarity. `threshold=1.0` is exact, `0.5` is fairly strict, `0.3` is permissive.

### Sketches

Always on per attribute. No build, no eviction.

```rust
ss.maybe_contains("city", &Value::String("NYC".into()));  // bool, microseconds
ss.estimate_count("city", &Value::String("NYC".into()));  // over-estimate, never below truth
```

### Graph layer

```rust
ss.add_edge(alice, bob, None, false);
ss.add_edge(alice, carol, Some("follows".into()), false);
ss.neighbors(alice, None);
ss.shortest_path(alice, dave, None);
ss.bfs(alice, None, None);
```

Deleting a record clears every edge that touched it.

### Persistence

```rust
ss.save("snap.json").unwrap();
let loaded = Superstruct::load("snap.json", None).unwrap();
```

Records and edges round-trip. Indexes rebuild on first query against the loaded instance. Sketches rebuild from the replayed inserts.

### Memory budget

```rust
ss.set_memory_budget(2_000_000);   // 2 MB cap on indexes
ss.index_inventory();              // which ones are alive right now: Vec<(String, String, usize)>
```

Tightening the budget triggers immediate eviction. Loosening it does nothing until a new build happens.

### Concurrency

```rust
let ss = Arc::new(Superstruct::new(None));
// threads can hammer ss with insert and find calls without coordination
```

The struct is always thread-safe. Internally each piece of state lives behind
its own lock so concurrent reads of the primary store and existing indexes do
not serialize on a single mutex. The workload tracker uses atomic counters so
recording a hit is lock-free on the read path. Queries that hit the warm
fast path, where every needed index already exists, never take a write lock.

---

## Inside the nucleus

This section walks through what is actually happening inside the structure when you call its methods. Reading this is optional. Skip to [Live benchmarks](#live-benchmarks) if you only care about numbers.

### Storage: PrimaryStore

The primary store is a `HashMap<u64, Record>`. Each record gets a monotonic auto-assigned id at insert time. Ids are never reused, even after delete, so any sub-index that holds an id can never accidentally point at a different record after a deletion.

```rust
pub struct Record {
    pub id: u64,
    pub attrs: HashMap<String, Value>,
}
```

Attribute maps are cloned on insert so the user cannot accidentally mutate stored state from outside.

### Query language: a tiny AST

```
Query
├── where: Option<Node>          (root of the predicate tree)
└── top_k: Option<TopK>          (final ordering step)

Node = Predicate | And | Or | Not

Predicate = { kind, attribute, value, threshold }
And       = Vec<Node>
Or        = Vec<Node>
Not       = Option<Box<Node>>
```

The QueryBuilder is sugar that builds an implicit AND. `.equals(...)` appends a Predicate leaf. `.any_of(...)` wraps nodes in an Or. `.exclude(...)` wraps a node in a Not. `.execute()` collapses the implicit AND list into a single root and hands the Query to the Planner.

### The Planner

The planner has three responsibilities.

**1. Lazy index construction.** When `evaluate_predicate` is called, the planner looks for any existing index that can answer the predicate. If none exists, it picks the right index type from a static map and builds it by walking the primary store once.

```rust
fn build_index_for(kind: PredicateKind) -> Box<dyn Index> {
    match kind {
        Equals   => HashIndex,
        Range    => SortedIndex,
        Prefix   => TrieIndex,
        Contains => InvertedIndex,
        Fuzzy    => NgramIndex,
    }
}
```

The build is timed and that time is recorded on the workload tracker as a small loyalty bonus that protects the new index from immediate eviction.

**2. Cross-structure decomposition.** The planner walks the AST recursively.

| Node      | Action                                              |
|-----------|-----------------------------------------------------|
| Predicate | Route to an index. Get back a set of ids.           |
| And       | Intersect children. Short-circuit on empty.         |
| Or        | Union children.                                     |
| Not       | Universe minus child.                               |

The intersection pass is incremental, so as soon as the running result is empty no further children are evaluated.

**3. Memory budget enforcement.** After every build, the planner sums `memory_estimate_bytes()` across all live indexes. If the total exceeds the budget, indexes are evicted in ascending order of their workload score.

```
score(idx) = (hit_count / age_in_seconds) + (build_cost_seconds * 0.1)
```

The `age_in_seconds` term decays the score as time passes since the last hit, so an index that was hot last week and cold this week will be the first to go. The `build_cost_seconds` term gives an expensive-to-build index a small loyalty bonus.

### The five indexes

Each index implements the same trait plus a `supports_kind` method so the planner knows when to route to it.

```rust
pub trait Index: Send + Sync {
    fn attribute(&self) -> &str;
    fn supports_kind(&self, kind: PredicateKind) -> bool;
    fn build_from_records(&mut self, records: &[Record]);
    fn insert(&mut self, record: &Record);
    fn remove(&mut self, record: &Record);
    fn execute(&self, predicate: &Predicate) -> HashSet<u64>;
    fn memory_estimate_bytes(&self) -> usize;
}
```

**HashIndex** uses `HashMap<Value, HashSet<u64>>`. Equality lookup is O(1). Insert and remove are O(1).

**SortedIndex** uses two parallel `Vec`s, `values` sorted and `ids` in lockstep. Range queries use `partition_point` (the Rust `bisect`) to find the slice in O(log n) then read off the ids in O(k) where k is the result size. Bulk build sorts once in O(n log n). Incremental insert is O(n) due to vector shifting.

**TrieIndex** is a classical character trie. Each node stores `children: HashMap<char, TrieNode>` and `ids: HashSet<u64>`. Prefix queries walk down to the prefix node in O(k) where k is the prefix length, then collect ids from the subtree by iterative DFS. Equality is the same walk without the DFS.

**InvertedIndex** is the classic search-engine posting list. Tokenizes string values into lowercase alphanumeric words via a regex. Maps each word to the set of record ids that mention it. CONTAINS predicates do a single hash lookup.

**NgramIndex** powers fuzzy match. Each string value is converted to its set of trigrams, that is every contiguous three character window after lowercasing and padding both ends with two spaces. The padding ensures that even short strings have at least one trigram and that matching beginnings and endings is rewarded. Two structures are kept: `postings: HashMap<trigram, HashSet<id>>` for finding candidates, and `record_trigrams: HashMap<id, HashSet<trigram>>` so that Jaccard similarity can be computed at query time without re-reading the primary store.

A FUZZY predicate runs in two phases.

```
Phase 1 (broad net): collect every record sharing at least one trigram
                     with the target.
Phase 2 (score):     for each candidate, compute Jaccard similarity
                     between target trigrams and record trigrams.
                     Keep records with similarity >= threshold.
```

### The two sketches

Auto attached per attribute as records are inserted. Tiny memory, never evicted.

**BloomSketch** is a bit array of `m` bits and `k` hash functions. The default is 16384 bits and 5 hashes which sits at well under one percent false positive rate for tens of thousands of distinct values. Hash positions are derived from MD5 of the value display representation so any value works. **No false negatives.** False positive rate slowly rises with occupancy.

**CountMinSketch** is a 2D table of `d` rows by `w` columns. Each `add(value)` increments `d` counters, one per row, at columns chosen by `d` independent hashes. `estimate(value)` returns the minimum across those `d` counters. The minimum is **always an over-estimate** of the true count, never an under-estimate, and is tight when collisions are sparse.

### The graph store

Adjacency lists keyed by record id. Each entry is a `HashSet<(neighbor_id, Option<String>)>`. Edges are bidirectional by default. `remove_node` walks every neighbor and discards reverse edges so deleting a record cleans up the whole local neighborhood. BFS uses a `VecDeque` and a depths map. Shortest path is BFS with predecessor tracking.

### The workload tracker

`HashMap<(index_type_name, attribute), IndexStats>`. `IndexStats` has `hit_count`, `last_used: Instant` and `build_cost_secs`. `record_hit`, `record_build` and `forget` are the three mutators. `score` rolls hits and recency and build cost into a single comparable number that the planner sorts by during eviction.

### Persistence on disk

```json
{
  "version": 1,
  "next_id": 5000,
  "records": [{"id": 0, "attrs": {...}}, ...],
  "edges":   [{"from": 0, "to": 1, "label": "friend"}, ...]
}
```

JSON only via serde. No indexes, no sketches. The load path replays records through the normal insert path which preserves their original ids and re-pushes them through sketches. Edges are added back as `directed=true` because the saved format already contains both directions of any bidirectional edge. The planner starts cold after load.

### Concurrency model

State is split across multiple locks rather than a single coarse one. The primary store, the planner index map, the bloom map, the count-min map and the graph each sit behind their own outer `RwLock`. Reads of any of these pieces run in parallel across threads. The workload tracker uses atomic counters internally so hit recording is lock-free on the query fast path.

The planner takes the design one step further: the index map is `HashMap<Key, Arc<RwLock<Box<dyn Index>>>>`. Each individual index has its own per-index `RwLock`. The planner-level `RwLock` is only taken in write mode when the map itself changes, namely when a new index is built or an evicted one is removed. Updating an existing index (an insert or delete propagating to a posting list) only takes a brief per-index write lock, so a writer touching the HashIndex on `city` does not block a writer touching the SortedIndex on `age`.

Query execution has two paths. If every predicate in the query already has a matching index, the fast path takes a planner read lock plus per-index read locks and runs in full parallel with other readers. If at least one index has to be built, a one-time planner write lock serializes the build phase, then the query falls back to read locks for execution.

Acquisition order is fixed (primary, planner, blooms, counts, graph) so deadlocks are impossible.

---

## Live benchmarks

Numbers below come from running `cargo run --example benchmark --release` on Rust 1.95, Linux 7.0. Recompute on your own hardware to compare relative costs.

### Insert throughput

| Records | Total time | Per insert | Throughput |
|---:|---:|---:|---:|
| 1,000   | 7.6 ms     | 7.6 us | 131,000 ops/sec |
| 10,000  | 85.3 ms    | 8.5 us | 117,000 ops/sec |
| 50,000  | 414.8 ms   | 8.3 us | 121,000 ops/sec |

Throughput holds steady because per-insert overhead stays constant regardless of how many records are already in the store. Lazy index construction means inserts only touch the primary store and sketches.

### Query latency. Cold first call vs warm reuse

Run on a 20,000 record store. Cold is the first time the predicate kind ever runs. Warm is the average of fifty subsequent calls.

| Query | Cold | Warm | Speedup |
|---|---:|---:|---:|
| `equals("city", "NYC")` | 30.4 ms | 1479 us | 21x |
| `range("age", 25, 35)` | 19.9 ms | 1179 us | 17x |
| `prefix("name", "a")` | 25.1 ms | 2445 us | 10x |
| `contains("bio", "cat")` | 30.0 ms | 1236 us | 24x |
| `fuzzy("name", "alise")` | 39.5 ms | 3215 us | 12x |

The cold cost is index build cost. The warm cost is the actual lookup. The 24x cold-to-warm ratio for full text reflects how much the inverted index costs to build versus how trivially fast a single posting list lookup is.

### Compound query speedup

| Method | Average over 5 runs |
|---|---:|
| Indexed compound | 0.36 ms |
| Rust iterator scan | 8.48 ms |
| Speedup | 23x |

The compound query splits across SortedIndex(age) + TrieIndex(name) + HashIndex(city). At 50,000 records the indexed path is 23x faster than an equivalent Rust iterator scan. Most of the lift comes from doing AND across posting lists as a roaring bitmap intersection rather than a hash-by-hash set intersection.

### Concurrency

4 writer threads plus 4 reader threads, 2,000 ops each. 16,000 total ops. Throughput is around 5,700 ops/sec under contention. Each live index sits behind its own RwLock, so writer A updating the HashIndex on `city` does not block writer B updating the SortedIndex on `age`, and a reader hitting a warm index never blocks at all once that index exists. The remaining serialization is per-index: two writers updating the same index still take that one lock in turn.

### Memory footprint

After running every query type once on 20,000 records the inventory looks like:

| Index | Attribute | Bytes |
|---:|---:|---:|
| NgramIndex | name | 1,236,739 |
| SortedIndex | age | 800,000 |
| InvertedIndex | bio | 195,194 |
| TrieIndex | name | 45,432 |
| HashIndex | city | 40,660 |
| **Total** | | **2,318,025** |

Roaring bitmaps for posting lists are the big lift here. The inverted index dropped 7.6x (1.49 MB to 0.20 MB), the trie dropped 6.4x, the hash index dropped 7.1x. NgramIndex went from caching a HashSet of trigrams per record to storing the lowercased value once and recomputing on demand, which combined with roaring postings is now down to 1.24 MB. The default 64 MB budget comfortably holds the full set even at much higher record counts. Updated memory numbers come from running on Rust 1.85.

### Spatial, substring and weighted graph (50k records)

| Operation | Result |
|---|---|
| `within_box` 500x500 in a 1000x1000 plane | warm 3.7 ms (31k matches) |
| `near` radius 50 | warm 47 us |
| `substring("cat")` (matches inside words) | warm 4.4 ms (12.5k matches), 2x faster than Rust scan |
| `contains("cat")` (word boundary only, for contrast) | warm 0.2 us, 0 matches in this dataset |
| `dijkstra` on 5k nodes, 25k edges | 1.1 ms per call |
| `shortest_path_weighted` on the same graph | 0.9 ms per call |
| `pagerank(0.85, 30 iterations)` | 15 ms per call |

The substring vs contains split is intentional. Inverted index splits on word boundaries so it cannot match the substring `cat` inside `concatenate`. Substring search finds it via roaring trigram intersection plus a literal verification pass. The two work side by side and you pick which semantic you want per query.

### Scale out. Numbers from `cargo run --example heavy_benchmark --release`

The heavy benchmark stresses the structure at 10x to 50x the standard scale: 1M records for ingest, 200k for query latency, 500k for the compound vs scan, 16 writer threads + 16 reader threads.

| Metric | Standard (50k / 4+4) | Heavy (1M / 16+16) |
|---|---:|---:|
| Insert per record | 8.3 us | 4.9 us at 1M |
| Insert throughput | 121k ops/s | 206k ops/s at 1M |
| Compound vs scan | 23x | **31x at 500k** |
| Mixed read+write throughput | 5,700 ops/s | **14,408 ops/s** |
| Memory | 2.3 MB at 20k | 19.7 MB at 200k |
| Memory per record | 116 B | **99 B** |

Three things stand out:

1. **Insert throughput rises with scale.** At 1,000 records we see 131k ops/s, at 1M records it is 206k ops/s. Larger batches amortize fixed overhead better and the hash map pre-allocation inside `populate` gives denser layouts.
2. **Compound speedup grows with N.** 23x at 50k becomes 31x at 500k. The more data, the more the polyindex pays for itself relative to a tight Rust scan.
3. **Per-record memory drops as N grows.** From 116 bytes per record at 20k to 99 bytes at 200k, because fixed overhead amortizes and roaring bitmaps compress denser posting lists better.

The one place we hit a real ceiling is read-only concurrency at 16 threads doing top_k queries that hydrate thousands of records per call. Total throughput there is 763 ops/s across 160k queries. The warm single-threaded top_k at this scale runs around 5-26 ms per query. Under 16 concurrent readers the slowdown is allocator pressure (each query allocates a Vec of scored tuples) plus CPU-cache thrashing on the shared `HashMap<u64, Record>` primary store. Both are general systems-engineering ceilings rather than flaws in the polyindex design. Further lift there would come from a packed columnar layout for primary store hot fields, or caching warm result sets.

---

## How it compares to other things

| Tool | What it offers | Where Superstruct differs |
|---|---|---|
| Postgres / SQLite / DuckDB | Full SQL, multiple index types, query planner, persistence | Server or query engine, schema required, you write `CREATE INDEX`. Superstruct is a single in-process Rust struct with no schema. |
| Database cracking (Idreos, CWI/Harvard) | Build sorted indexes incrementally from query results | Same lazy spirit but only one structure type. Superstruct does it across a zoo. |
| Self-tuning DBs (Oracle, Snowflake) | Auto recommend or create indexes from observed workload | Server side, offline analysis, SQL-based. Superstruct does it in-process and on the very first query. |
| Learned indexes (Kraska et al, 2018) | ML model in place of an index | Single structure flavor. Could plug into Superstruct as a sixth index choice. |
| Redis / KeyDB | Multiple in-memory data types per key | You pick the type at write time. No cross-structure decomposition. No adaptive build. |
| Rust-specific crates (hashbrown, bitvec, tantivy) | Each one nails one structure | Single purpose. No router across multiple structures. No workload adaptation. |

The packaging is the speciality. An embeddable, in-process, schema-less Rust struct that secretly contains a database planner, a structure zoo and a memory budget, all behind a small chained API.

---

## Full API reference

Around thirty callable surfaces. You can do real work with six.

### Lifecycle

| Function | Returns | Notes |
|---|---|---|
| `Superstruct::new(memory_budget_bytes)` | instance | Default budget 64 MiB. |
| `Superstruct::load(path, memory_budget_bytes)` | Result | Replays records and edges from a JSON snapshot. |

### Mutations

| Function | Returns | Notes |
|---|---|---|
| `insert(attrs)` | u64 | Auto-assigned monotonic id. Updates indexes and sketches. |
| `delete(id)` | bool | Cleans up edges that touched the record. |
| `add_edge(a, b, label, directed)` | () | Bidirectional unless `directed`. Weight defaults to 1.0. |
| `add_weighted_edge(a, b, weight, label, directed)` | () | Same as `add_edge` with an explicit edge weight. |
| `remove_edge(a, b, label, directed)` | () | Symmetric to `add_edge`. |

### Direct lookups

| Function | Returns | Notes |
|---|---|---|
| `get(id)` | Option&lt;Attrs&gt; | O(1) primary key lookup. |
| `len()` | usize | Live record count. |
| `maybe_contains(attr, value)` | bool | Bloom-backed. False positives possible, false negatives never. |
| `estimate_count(attr, value)` | u64 | CountMin-backed. Over-estimate, never below truth. |

### Query builder

All chain off `ss.find()` and end with `.execute()`.

| Method | Returns | Notes |
|---|---|---|
| `find()` | builder | Fresh implicit-AND builder. |
| `equals(attr, value)` | builder | Hash index. |
| `range(attr, low, high)` | builder | Sorted index. Both ends inclusive. |
| `prefix(attr, prefix)` | builder | Trie index. |
| `contains(attr, word)` | builder | Inverted index. Lowercase tokens. |
| `fuzzy(attr, target, threshold)` | builder | N-gram index. Jaccard similarity. |
| `substring(attr, query)` | builder | N-gram index. Literal substring anywhere in the value, including across word boundaries. |
| `within_box(attr, min_x, min_y, max_x, max_y)` | builder | Spatial index. 2D bounding-box filter. |
| `near(attr, x, y, radius)` | builder | Spatial index. Records inside a 2D circle. |
| `any_of(nodes)` | builder | OR group. Takes `Vec<Node>`. |
| `exclude(node)` | builder | NOT. Takes a `Node`. |
| `top_k(attr, k, descending)` | builder | Final ordering step. |
| `execute()` | Vec&lt;Attrs&gt; | Run and hydrate to attribute maps. |
| `to_node()` | Option&lt;Node&gt; | Materialize the implicit AND into an AST node. |

### Graph

| Function | Returns | Notes |
|---|---|---|
| `neighbors(id, label)` | HashSet&lt;u64&gt; | Optional label filter. |
| `bfs(start, max_depth, label)` | HashMap&lt;u64, usize&gt; | Unweighted. Map of node to depth. |
| `shortest_path(src, tgt, label)` | Option&lt;Vec&lt;u64&gt;&gt; | Unweighted. Inclusive of both endpoints. |
| `dijkstra(src, label)` | HashMap&lt;u64, f64&gt; | Single-source shortest weighted distance to every reachable node. |
| `shortest_path_weighted(src, tgt, label)` | Option&lt;(Vec&lt;u64&gt;, f64)&gt; | Weighted shortest path plus total cost. |
| `pagerank(damping, iterations)` | HashMap&lt;u64, f64&gt; | PageRank over weighted out-edges. Typical args: 0.85, 30. |

### Persistence and config

| Function | Returns | Notes |
|---|---|---|
| `save(path)` | Result | JSON snapshot of records and edges. |
| `set_memory_budget(bytes)` | () | Triggers immediate eviction if over. |
| `index_inventory()` | Vec&lt;(String, String, usize)&gt; | Currently materialized indexes. |

---

## Limitations and honest caveats

- **In memory only.** Persistence is JSON snapshot and reload, not a write-ahead log. Crash mid-insert and the record is lost.
- **JSON-friendly attribute values only.** If you `save` a record whose attribute contains an unserializable type, it will fail. The `Value` enum supports `Int`, `Float`, `String`, and `List` variants, all of which serialize cleanly.
- **Bloom filters cannot delete.** False positive rate rises slowly with churn. Wipe and rebuild for a long-running process if precision matters.
- **N-gram index doubles memory.** It stores per-record trigram sets so Jaccard can be computed without a primary store walk.
- **Two writers updating the same index serialize on that index per-index RwLock.** Writers touching different indexes run in parallel, but if every record carries the same attribute (e.g. every insert sets `city`), all writers contend on the HashIndex for that attribute. A lock-free posting list would lift this further; not yet implemented.
- **Compound speedup is dataset dependent.** Indexes shine when predicates are selective and base costs are nontrivial. On small in-memory datasets a Rust iterator scan is hard to beat.
- **No SQL.** No JOIN. No window functions. The query language is intentionally tiny and conjunctive plus boolean composition.
- **No type system on attributes.** Mixing ints and strings under the same attribute will work for some indexes and break for others (the sorted index will refuse to compare them).

---

## Future directions

The architecture has clean places to plug each of these in.

- **Learned indexes** as a sixth index type. Train a tiny model per attribute and let the planner consult it for range queries.
- **Disk-backed primary store** so the structure can hold more than RAM and only materialize the working set in memory.
- **Cost-based query planner** that estimates selectivity of each predicate and reorders the AND chain so the most selective runs first.
- **Finer-grained locking** with per-index `RwLock` for better concurrent read/write throughput.
- **Pluggable tokenizers** for the inverted index. Stemming, stop words, language-aware splits.
- **Versioned snapshots** with a write-ahead log so the structure becomes recoverable.
- **Distributed mode** with consistent hashing over a cluster.
- **Algorithm-as-view** materializations. Pre-computed PageRank, topological sort, k-shortest-paths, all served as cached views with incremental update.

---

## Project layout

```
superstruct/
├── Cargo.toml                         crate manifest
├── README.md                          this file
├── src/
│   ├── lib.rs                         public exports
│   ├── core.rs                        Superstruct facade and QueryBuilder
│   ├── primary.rs                     source of truth record store
│   ├── query.rs                       AST: Predicate, And, Or, Not, TopK
│   ├── value.rs                       Value enum: Int, Float, String, List
│   ├── planner.rs                     lazy build, decomposition, eviction
│   ├── workload.rs                    per-index hit counts and recency score
│   ├── graph.rs                       adjacency, BFS, weighted shortest path, Dijkstra, PageRank
│   ├── index/
│   │   ├── mod.rs                     module declarations
│   │   ├── base.rs                    Index trait
│   │   ├── hash.rs                    equality lookup
│   │   ├── sorted.rs                  range and equality on sorted arrays
│   │   ├── trie.rs                    character trie for prefix
│   │   ├── inverted.rs                word-level full text
│   │   ├── ngram.rs                   trigram fuzzy match and substring search
│   │   └── spatial.rs                 2D spatial bbox and radius queries
│   └── sketch/
│       ├── mod.rs                     module declarations
│       ├── bloom.rs                   probable-membership filter
│       └── countmin.rs                approximate frequency counter
├── examples/
│   ├── benchmark.rs                   live throughput, latency, memory at 50k records
│   └── heavy_benchmark.rs             stress run at 1M records and 16+16 threads
└── tests/
    └── integration_test.rs            103 tests: core, text, sketches, graph,
                                        persistence, concurrency, stress, indexes,
                                        spatial, substring, Dijkstra, PageRank
```

103 tests, all green at the time of writing.

---

## Run it yourself

```bash
# All tests
cargo test

# Verbose output
cargo test -- --nocapture

# Standard benchmark (50k records, 4+4 thread concurrency, ~10s wall)
cargo run --example benchmark --release

# Heavy benchmark (up to 1M records, 16+16 thread concurrency, ~3 min wall)
cargo run --example heavy_benchmark --release

# Build optimized
cargo build --release
```

If anything fails, please open an issue on whichever platform you are reading this from.

---

## License

MIT or Apache-2.0. The spirit is "use it, learn from it, build something cooler with it".

---

> Built as a thought experiment about what a database planner looks like when you strip the database away. Originally written in Python, ported to Rust. Comments throughout the code avoid Oxford commas and dashes by explicit author preference.
