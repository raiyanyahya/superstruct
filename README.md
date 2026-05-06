# Superstruct

> The data structure that watches how you use it then quietly becomes the right one.

Superstruct is an in-memory Python data structure that holds your records in one place and answers your questions through a zoo of classical sub-structures it builds on demand. You insert dictionaries. You ask questions. The structure observes your workload and decides which sub-structure to consult, builds it lazily the first time it is needed and evicts it when memory gets tight. **You never declare an index.**

Hash map, sorted index, trie, inverted index, trigram fuzzy index, bloom filter, count-min sketch and a graph layer all sit inside one Python object. A small chained query DSL routes across them. The whole thing fits behind 25 public functions.

```python
from superstruct import Superstruct

ss = Superstruct()
ss.insert({"name": "Alice", "age": 30, "city": "NYC", "bio": "loves cats"})
ss.insert({"name": "Anya",  "age": 25, "city": "SF",  "bio": "dog person"})
# ... thousands more

# This single chained query splits across three indexes plus a sort.
# None of them existed before this line ran.
results = (
    ss.find()
      .range("age", 25, 35)
      .prefix("name", "A")
      .contains("bio", "cat")
      .top_k("score", 10)
      .execute()
)
```

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

Superstruct flips the trade. Inside one object lives every classical structure. The user never declares any of them. Every insert is held in a primary store as the canonical truth. When a query first arrives that would benefit from a particular sub-structure, the structure is built lazily by walking the primary store. Subsequent queries of that kind reuse the warm structure. When memory pressure rises, the least useful structures are evicted by a recency weighted frequency score. They get rebuilt the next time someone asks the right question.

The thesis in one sentence:

> The user stores data and asks questions. The structure watches the workload and self-organizes its internal indexes to minimize total query latency under a memory budget.

This is the same idea database planners use behind a SQL parser, but lifted out and turned into an embeddable Python data structure with a small chained API.

---

## Quick start

Python 3.10 or newer. No external dependencies.

```bash
git clone <wherever this lives>
cd superstruct

# Run the test suite. Should be green.
python3 -m unittest discover -s tests

# Run the end to end demo.
PYTHONPATH=. python3 examples/demo.py

# Run the live benchmarks.
PYTHONPATH=. python3 benchmarks/run_benchmarks.py
```

Then open a Python REPL.

```python
from superstruct import Superstruct

ss = Superstruct()
ss.insert({"name": "Alice", "age": 30, "city": "NYC", "score": 88})
ss.find().equals("city", "NYC").execute()
ss.find().range("age", 20, 40).top_k("score", 5).execute()
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
  - The **N-gram Specialist** is the one to ask when you misspell things.
- **The Sketches** are two clerks at the front desk. One says "I am pretty sure I have not seen this before, do not bother going to the Vault." The other says "I have seen this name about 47 times this week."
- **The Manager** is the planner. Hires a Specialist when a new kind of question shows up. Fires the laziest Specialist when the office gets crowded. Splits compound questions across multiple Specialists and combines their answers.
- **HR** is the workload tracker. Keeps a tally of which Specialist gets used a lot and which one is just sitting around.
- **The Cartographer** is the graph store. Knows who is friends with whom. Independent of the Specialists.

The user only knocks on the front desk.

---

## Feature tour

### Inserting and deleting

```python
ss = Superstruct()
alice = ss.insert({"name": "Alice", "age": 30, "city": "NYC"})  # returns id
ss.delete(alice)                                                 # returns bool
len(ss)                                                          # record count
ss.get(alice)                                                    # dict or None
```

Records are arbitrary dicts. Different records can have different keys. There is no schema.

### Equality, range, prefix

```python
ss.find().equals("city", "NYC").execute()
ss.find().range("age", 25, 35).execute()      # both ends inclusive
ss.find().prefix("name", "A").execute()
```

The first call of each kind builds the right index lazily. Later calls reuse it.

### Compound queries

```python
ss.find()                       \
  .range("age", 25, 35)         \
  .prefix("name", "A")          \
  .equals("city", "SF")         \
  .top_k("score", 10)           \
  .execute()
```

Each predicate runs against its own sub-index. The id sets are intersected. Top-k is the final ordering step.

### Boolean composition

```python
# OR group
ss.find().any_of(
    ss.find().equals("city", "NYC"),
    ss.find().range("score", 90, 100),
).execute()

# NOT
ss.find().exclude(ss.find().equals("name", "Alice")).execute()

# Nest by passing a sub-builder that itself contains an any_of.
```

### Full text search

```python
ss.find().contains("bio", "cats").execute()
```

Tokenization is lowercase and alphanumeric. Multiple `contains` predicates AND together because they sit at the top level of the implicit AND.

### Fuzzy match

```python
ss.find().fuzzy("name", "Alise", threshold=0.4).execute()
```

Trigram Jaccard similarity. `threshold=1.0` is exact, `0.5` is fairly strict, `0.3` is permissive.

### Sketches

Always on per attribute. No build, no eviction.

```python
ss.maybe_contains("city", "NYC")    # True or False, microseconds
ss.estimate_count("city", "NYC")    # over-estimate, never below the truth
```

### Graph layer

```python
ss.add_edge(alice, bob)
ss.add_edge(alice, carol, label="follows")
ss.neighbors(alice)
ss.shortest_path(alice, dave)
ss.bfs(alice, max_depth=2)
```

Deleting a record clears every edge that touched it.

### Persistence

```python
ss.save("snap.json")
loaded = Superstruct.load("snap.json")
```

Records and edges round-trip. Indexes rebuild on first query against the loaded instance. Sketches rebuild from the replayed inserts.

### Memory budget

```python
ss.set_memory_budget(2_000_000)   # 2 MB cap on indexes
ss.index_inventory()              # which ones are alive right now
```

Tightening the budget triggers immediate eviction. Loosening it does nothing until a new build happens.

### Concurrency

```python
ss = Superstruct(thread_safe=True)   # default
# threads can hammer ss with insert and find calls without coordination
```

A re-entrant lock wraps every public method. Pass `thread_safe=False` for the lowest possible single-thread overhead.

---

## Inside the nucleus

This section walks through what is actually happening inside the structure when you call its methods. Reading this is optional. Skip to [Live benchmarks](#live-benchmarks) if you only care about numbers.

### Storage: PrimaryStore

The primary store is a plain `dict[int, Record]`. Each record gets a monotonic auto-assigned id at insert time. Ids are never reused, even after delete, so any sub-index that holds an id can never accidentally point at a different record after a deletion.

```python
class Record:
    id: int
    attrs: dict[str, Any]
```

Attribute dicts are copied on insert so the user cannot accidentally mutate stored state from outside.

### Query language: a tiny AST

```
Query
├── where: Node                  (root of the predicate tree, optional)
└── top_k: TopK                  (final ordering step, optional)

Node = Predicate | And | Or | Not

Predicate = (kind, attribute, value, threshold)
And       = list of Node
Or        = list of Node
Not       = single Node
```

The QueryBuilder is sugar that builds an implicit AND. `.equals(...)` appends a Predicate leaf. `.any_of(...)` wraps sub-builders in an Or. `.exclude(...)` wraps a sub-builder in a Not. `.execute()` collapses the implicit AND list into a single root and hands the Query to the Planner.

### The Planner

The planner has three responsibilities.

**1. Lazy index construction.** When `_evaluate_predicate` is called, the planner looks for any existing index that can answer the predicate. If none exists, it picks the right index type from a static map and builds it by walking the primary store once.

```python
_DEFAULT_INDEX_FOR_KIND = {
    PredicateKind.EQUALS:   HashIndex,
    PredicateKind.RANGE:    SortedIndex,
    PredicateKind.PREFIX:   TrieIndex,
    PredicateKind.CONTAINS: InvertedIndex,
    PredicateKind.FUZZY:    NgramIndex,
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

Each index implements the same five-method interface plus a `supported_kinds` set so the planner knows when to route to it.

```python
class Index(ABC):
    supported_kinds: set[PredicateKind]
    def build_from_records(self, records): ...
    def insert(self, record): ...
    def remove(self, record): ...
    def execute(self, predicate) -> set[int]: ...
    def memory_estimate_bytes(self) -> int: ...
```

**HashIndex** uses `dict[value, set[int]]`. Equality lookup is O(1). Insert and remove are O(1).

**SortedIndex** uses two parallel lists, `_values` sorted and `_ids` in lockstep. Range queries do `bisect_left` and `bisect_right` to find the slice in O(log n) then read off the ids in O(k) where k is the result size. Bulk build sorts once in O(n log n). Incremental insert is O(n) due to list shifting which is fine for moderate sizes. Could be swapped for a B-tree later without changing the interface.

**TrieIndex** is a classical character trie. Each node stores `children: dict[char, _TrieNode]` and `ids: set[int]`. Prefix queries walk down to the prefix node in O(k) where k is the prefix length, then collect ids from the subtree by iterative DFS. Equality is the same walk without the DFS.

**InvertedIndex** is the classic search-engine posting list. Tokenizes string values into lowercase alphanumeric words via a regex. Maps each word to the set of record ids that mention it. CONTAINS predicates do a single dictionary lookup.

**NgramIndex** powers fuzzy match. Each string value is converted to its set of trigrams, that is every contiguous three character window after lowercasing and padding both ends with two spaces. The padding ensures that even short strings have at least one trigram and that matching beginnings and endings is rewarded. Two structures are kept: `_postings: dict[trigram, set[int]]` for finding candidates, and `_record_trigrams: dict[id, set[trigram]]` so that Jaccard similarity can be computed at query time without re-reading the primary store.

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

**BloomSketch** is a bit array of `m` bits and `k` hash functions. The default is 16384 bits and 5 hashes which sits at well under one percent false positive rate for tens of thousands of distinct values. Hash positions are derived from md5 of `repr(value)` so any value the user might insert works. **No false negatives.** False positive rate slowly rises with occupancy. We do not implement deletion since standard bloom filters cannot subtract without doubling the memory cost via counting.

**CountMinSketch** is a 2D table of `d` rows by `w` columns. Each `add(value)` increments `d` counters, one per row, at columns chosen by `d` independent hashes. `estimate(value)` returns the minimum across those `d` counters. The minimum is **always an over-estimate** of the true count, never an under-estimate, and is tight when collisions are sparse.

### The graph store

Adjacency lists keyed by record id. Each entry is a `set[(neighbor_id, label)]` so duplicate edges deduplicate naturally. Edges are bidirectional by default. `remove_node` walks every neighbor and discards reverse edges so deleting a record cleans up the whole local neighborhood. BFS uses a deque and a depths dict. Shortest path is BFS with predecessor tracking, then a walk back from the target.

### The workload tracker

`dict[(index_type_name, attribute), IndexStats]`. IndexStats has `hit_count`, `last_used_ts` and `build_cost_seconds`. `record_hit`, `record_build` and `forget` are the three mutators. `score` rolls hits and recency and build cost into a single comparable number that the planner sorts by during eviction.

### Persistence on disk

```json
{
  "version": 1,
  "next_id": 5000,
  "records": [{"id": 0, "attrs": {...}}, ...],
  "edges":   [{"from": 0, "to": 1, "label": "friend"}, ...]
}
```

JSON only. No indexes, no sketches. The load path replays records through `_restore_record` which preserves their original ids and re-pushes them through every materialized index plus the sketches. Edges are added back as `directed=True` because the saved format already contains both directions of any bidirectional edge.

### Concurrency model

A `threading.RLock` wraps every public Superstruct method. Re-entrant means a method that calls another method on the same instance does not deadlock. With Python's GIL the lock mostly serializes work, which is what we want because every mutation touches multiple sub-structures and they need to stay consistent. Users who run single-threaded can pass `thread_safe=False` to skip the lock entirely and shave a microsecond or two per operation.

---

## Live benchmarks

Numbers below come from running `benchmarks/run_benchmarks.py` on a Linux 7.0 host with Python 3.14. Recompute on your own hardware to compare relative costs across changes.

### Insert throughput

| Records | Total time | Per insert | Throughput |
|---:|---:|---:|---:|
| 1,000   | 31 ms     | 31 us | 32,000 ops/sec |
| 10,000  | 276 ms    | 28 us | 36,000 ops/sec |
| 50,000  | 1131 ms   | 23 us | 44,000 ops/sec |

Throughput rises slightly with scale because the per-insert overhead of the auto-attached sketches amortizes better as the dict warms up.

### Query latency. Cold first call vs warm reuse

Run on a 20,000 record store. Cold is the first time the predicate kind ever runs. Warm is the average of fifty subsequent calls.

| Query | Cold | Warm | Speedup |
|---|---:|---:|---:|
| `equals("city", "NYC")` | 2.78 ms | 312 us | 9x |
| `range("age", 25, 35)` | 3.34 ms | 291 us | 12x |
| `prefix("name", "a")` | 4.62 ms | 504 us | 9x |
| `contains("bio", "cat")` | 26.09 ms | 329 us | 79x |
| `fuzzy("name", "alise")` | 30.88 ms | 2947 us | 10x |

The cold cost is index build cost. The warm cost is the actual lookup. The 79x cold-to-warm ratio for full text reflects how much the inverted index costs to build versus how trivially fast a single posting list lookup is.

### Compound query speedup

| Method | Average over 5 runs |
|---|---:|
| Indexed compound | 0.98 ms |
| Naive Python scan | 3.13 ms |
| Speedup | 3x |

The compound query splits across SortedIndex(age) + TrieIndex(name) + HashIndex(city). At 50,000 records on this dataset the win over a single-pass python scan is modest. The crossover where indexes really pull ahead grows with selectivity (more selective predicates extract bigger wins) and with the cost of each predicate evaluated naively (a regex per record, for instance, costs much more than `attrs.get('age')` and indexes win bigger). Try the same benchmark with a fuzzy or contains predicate as one of the conjuncts and the gap widens dramatically.

### Concurrency

4 writer threads plus 4 reader threads, 2,000 ops each. 16,000 total ops in 4.7 seconds, about 3,400 ops/sec under contention. The lock serializes most of the real work because the GIL is in play either way, so the throughput here is roughly what you would see from a careful single-threaded run with the lock overhead included.

### Memory footprint

After running every query type once on 20,000 records the inventory looks like:

| Index | Attribute | Bytes |
|---|---|---:|
| NgramIndex | name | 22,241,872 |
| InvertedIndex | bio | 3,939,278 |
| TrieIndex | name | 1,323,208 |
| HashIndex | city | 656,856 |
| SortedIndex | age | 346,032 |
| **Total** | | **28,507,246** |

The n-gram index dominates because trigram sets are large per record. The default 64 MB budget comfortably holds the full set. Tightening it triggers eviction in workload-score order.

---

## How it compares to other things

| Tool | What it offers | Where Superstruct differs |
|---|---|---|
| Postgres / SQLite / DuckDB | Full SQL, multiple index types, query planner, persistence | Server or query engine, schema required, you write `CREATE INDEX`. Superstruct is a single in-process Python object with no schema. |
| Database cracking (Idreos, CWI/Harvard) | Build sorted indexes incrementally from query results | Same lazy spirit but only one structure type. Superstruct does it across a zoo. |
| Self-tuning DBs (Oracle, Snowflake, SQL Server tuning advisor) | Auto recommend or create indexes from observed workload | Server side, offline analysis, SQL-based. Superstruct does it in-process and on the very first query. |
| Learned indexes (Kraska et al, 2018) | ML model in place of an index | Single structure flavor. Could plug into Superstruct as a sixth index choice. |
| Redis / KeyDB | Multiple in-memory data types per key | You pick the type at write time. No cross-structure decomposition. No adaptive build. |
| Pandas DataFrame | Multi-index optional, columnar | One structure flavor (sorted). No prefix or full text or fuzzy or graph. No auto-indexing. |
| Specialized libraries (sortedcontainers, marisa-trie, Whoosh, pyroaring) | Each one nails one structure | Single purpose. No router across multiple structures. No workload adaptation. |

The packaging is the speciality. An embeddable, in-process, schema-less Python object that secretly contains a database planner, a structure zoo and a memory budget, all behind a small chained API.

---

## Full API reference

Twenty-five callable surfaces. You can do real work with six.

### Lifecycle

| Function | Returns | Notes |
|---|---|---|
| `Superstruct(memory_budget_bytes=None, thread_safe=True)` | instance | Default budget 64 MiB. Pass `thread_safe=False` for solo use. |
| `Superstruct.load(path, memory_budget_bytes=None, thread_safe=True)` | instance | Replays records and edges from a JSON snapshot. |

### Mutations

| Function | Returns | Notes |
|---|---|---|
| `insert(attrs)` | id | Auto-assigned monotonic id. Updates indexes and sketches. |
| `delete(id)` | bool | Cleans up edges that touched the record. |
| `add_edge(a, b, label=None, directed=False)` | None | Bidirectional unless `directed`. |
| `remove_edge(a, b, label=None, directed=False)` | None | Symmetric to `add_edge`. |

### Direct lookups

| Function | Returns | Notes |
|---|---|---|
| `get(id)` | dict or None | O(1) primary key lookup. |
| `len(ss)` | int | Live record count. |
| `maybe_contains(attribute, value)` | bool | Bloom-backed. False positives possible, false negatives never. |
| `estimate_count(attribute, value)` | int | CountMin-backed. Over-estimate, never below truth. |

### Query builder

All chain off `ss.find()` and end with `.execute()`.

| Method | Returns | Notes |
|---|---|---|
| `find()` | builder | Fresh implicit-AND builder. |
| `equals(attr, value)` | builder | Hash index. |
| `range(attr, low, high)` | builder | Sorted index. Both ends inclusive. |
| `prefix(attr, prefix)` | builder | Trie index. |
| `contains(attr, word)` | builder | Inverted index. Lowercase tokens. |
| `fuzzy(attr, target, threshold=0.5)` | builder | N-gram index. Jaccard similarity. |
| `any_of(*sub_builders)` | builder | OR group. |
| `exclude(sub_builder)` | builder | NOT. |
| `top_k(attr, k, descending=True)` | builder | Final ordering step. |
| `execute()` | list[dict] | Run and hydrate to attribute dicts. |

### Graph

| Function | Returns | Notes |
|---|---|---|
| `neighbors(id, label=None)` | set[int] | Optional label filter. |
| `bfs(start, max_depth=None, label=None)` | dict[int, int] | Map of node to depth. |
| `shortest_path(src, tgt, label=None)` | list[int] or None | Inclusive of both endpoints. |

### Persistence and config

| Function | Returns | Notes |
|---|---|---|
| `save(path)` | None | JSON snapshot of records and edges. |
| `set_memory_budget(bytes)` | None | Triggers immediate eviction if over. |
| `index_inventory()` | list of (type, attribute, bytes) | Currently materialized indexes. |

---

## Limitations and honest caveats

- **In memory only.** Persistence is JSON snapshot and reload, not a write-ahead log. Crash mid-insert and the record is lost.
- **JSON-friendly attribute values only.** If you `save` a record whose attribute is a Python set or a custom class, it will fail to serialize. Stick to JSON-native types.
- **Bloom filters cannot delete.** False positive rate rises slowly with churn. Wipe and rebuild for a long-running process if precision matters.
- **N-gram index doubles memory.** It stores per-record trigram sets so Jaccard can be computed without a primary store walk.
- **Not concurrent inside.** The lock is coarse and serializes all work. The Python GIL would have done much the same anyway. For real parallel throughput across cores you would need a multi-process design or a different language.
- **Compound speedup is dataset dependent.** Indexes shine when predicates are selective and base costs are nontrivial. On small in-memory datasets a Python scan is hard to beat.
- **No SQL.** No JOIN. No window functions. The query language is intentionally tiny and conjunctive plus boolean composition.
- **No type system on attributes.** Mixing ints and strings under the same attribute will work for some indexes and break for others (the sorted index will refuse to compare them).

---

## Future directions

The architecture has clean places to plug each of these in.

- **Roaring bitmaps** for the id sets, especially in the inverted index and the n-gram candidate phase.
- **Learned indexes** as a sixth index type. Train a tiny model per attribute and let the planner consult it for range queries.
- **Disk-backed primary store** so the structure can hold more than RAM and only materialize the working set in memory.
- **Cost-based query planner** that estimates selectivity of each predicate and reorders the AND chain so the most selective runs first.
- **Real read/write lock** for concurrent reads. Today the lock is coarse.
- **Pluggable tokenizers** for the inverted index. Stemming, stop words, language-aware splits.
- **Versioned snapshots** with a write-ahead log so the structure becomes recoverable.
- **Distributed mode** with consistent hashing over a cluster.
- **Algorithm-as-view** materializations. Pre-computed PageRank, topological sort, k-shortest-paths, all served as cached views with incremental update.

---

## Project layout

```
superstruct/
├── pyproject.toml
├── README.md                          this file
├── superstruct/
│   ├── __init__.py                    public exports
│   ├── core.py                        Superstruct facade and QueryBuilder
│   ├── primary.py                     source of truth record store
│   ├── query.py                       AST: Predicate, And, Or, Not, TopK
│   ├── planner.py                     lazy build, decomposition, eviction
│   ├── workload.py                    per-index hit counts and recency score
│   ├── graph.py                       adjacency, neighbors, BFS, shortest path
│   ├── indexes/
│   │   ├── base.py                    abstract Index interface
│   │   ├── hash_index.py              equality lookup
│   │   ├── sorted_index.py            range and equality on sorted arrays
│   │   ├── trie_index.py              character trie for prefix
│   │   ├── inverted_index.py          word-level full text
│   │   └── ngram_index.py             trigram fuzzy match
│   └── sketches/
│       ├── bloom.py                   probable-membership filter
│       └── countmin.py                approximate frequency counter
├── examples/
│   └── demo.py                        end-to-end walkthrough of every feature
├── benchmarks/
│   └── run_benchmarks.py              live throughput, latency, memory
└── tests/
    ├── test_core.py                   query API, edge cases, top-k
    ├── test_text.py                   contains and fuzzy
    ├── test_sketches.py               facade and quality bounds
    ├── test_graph.py                  neighbors, BFS, shortest path, edges
    ├── test_persistence.py            save/load round trip
    ├── test_concurrency.py            threaded inserts and queries
    ├── test_indexes.py                per-index unit tests
    └── test_stress.py                 large-scale, brute-force comparison
```

103 tests, all green at the time of writing.

---

## Run it yourself

```bash
# Tests
python3 -m unittest discover -s tests
python3 -m unittest discover -s tests -v   # verbose

# Single test file
python3 -m unittest tests.test_indexes -v

# Demo (requires PYTHONPATH for the script entry)
PYTHONPATH=. python3 examples/demo.py

# Benchmarks
PYTHONPATH=. python3 benchmarks/run_benchmarks.py
```

If anything fails, please open an issue on whichever platform you are reading this from.

---

## License

Pick one that fits. The code is small and was written for fun and research. MIT or Apache-2.0 are both fine choices. The spirit is "use it, learn from it, build something cooler with it".

---

> Built as a thought experiment about what a database planner looks like when you strip the database away. Comments throughout the code avoid Oxford commas and dashes by explicit author preference.
