"""End to end demo of every feature in Superstruct.

The output walks through, in order.

  1. Lazy index construction. Indexes appear as queries demand them.
  2. Cross-structure decomposition. A multi predicate query runs
     against multiple indexes and intersects the results.
  3. Boolean composition. OR and NOT via the QueryBuilder.
  4. Full text search via CONTAINS.
  5. Fuzzy match via FUZZY.
  6. Always on sketches. maybe_contains and estimate_count.
  7. Graph queries. Neighbors, BFS, shortest path.
  8. Persistence. Save to disk and load it back.
  9. Concurrency. Many threads hitting the structure at once.
 10. Adaptive eviction. Cap memory and watch indexes get evicted.
"""
import os
import random
import tempfile
import threading
import time

from superstruct import Superstruct


NAMES = [
    "Alice", "Anya", "Andre", "Bea", "Ben",
    "Cara", "Carl", "Diana", "Erin", "Fred",
]
CITIES = ["NYC", "SF", "LA", "Boston", "Austin"]
BIOS = [
    "loves cats and long walks",
    "dog person all the way",
    "cat owner who also walks dogs",
    "runs marathons every weekend",
    "indie game developer and coffee fan",
]


def section(title: str) -> None:
    print("\n" + "=" * 60)
    print(title)
    print("=" * 60)


def main():
    random.seed(0)
    ss = Superstruct()

    section("Step 1. Insert 5000 records")
    for _ in range(5000):
        ss.insert({
            "name": random.choice(NAMES),
            "age": random.randint(18, 80),
            "score": random.randint(0, 100),
            "city": random.choice(CITIES),
            "bio": random.choice(BIOS),
        })
    print(f"  records: {len(ss)}")
    print(f"  indexes built so far: {ss.index_inventory()}")

    section("Step 2. Lazy index construction")
    print("First range query triggers a SortedIndex on age.")
    t0 = time.monotonic()
    out = ss.find().range("age", 25, 35).execute()
    print(f"  range query 25..35: {len(out)} matches in {(time.monotonic() - t0) * 1000:.2f} ms")
    print(f"  inventory: {ss.index_inventory()}")

    print("\nFirst prefix query triggers a TrieIndex on name.")
    t0 = time.monotonic()
    out = ss.find().prefix("name", "A").execute()
    print(f"  prefix query A: {len(out)} matches in {(time.monotonic() - t0) * 1000:.2f} ms")
    print(f"  inventory: {ss.index_inventory()}")

    section("Step 3. Cross-structure decomposition")
    print("Compound query splits across three indexes plus a top-k step.")
    t0 = time.monotonic()
    out = (
        ss.find()
          .range("age", 25, 35)
          .prefix("name", "A")
          .equals("city", "SF")
          .top_k("score", 5)
          .execute()
    )
    print(f"  age 25..35 AND name prefix A AND city = SF, top 5 by score:")
    print(f"  {len(out)} matches in {(time.monotonic() - t0) * 1000:.2f} ms")
    for r in out:
        print(f"    {r['name']:6} age={r['age']} city={r['city']} score={r['score']}")
    print(f"  inventory: {ss.index_inventory()}")

    section("Step 4. Boolean composition. OR and NOT")
    print("People in NYC OR top scorers, excluding anyone named Alice.")
    out = (
        ss.find()
          .any_of(
              ss.find().equals("city", "NYC"),
              ss.find().range("score", 95, 100),
          )
          .exclude(ss.find().equals("name", "Alice"))
          .top_k("score", 5)
          .execute()
    )
    print(f"  {len(out)} top results:")
    for r in out:
        print(f"    {r['name']:6} city={r['city']} score={r['score']}")

    section("Step 5. Full text search")
    print("Find people whose bio contains the word 'cat'.")
    out = ss.find().contains("bio", "cat").top_k("score", 3).execute()
    print(f"  {len(out)} sample results:")
    for r in out:
        print(f"    {r['name']:6} bio={r['bio']!r}")

    section("Step 6. Fuzzy match")
    print("Find names approximately equal to 'Alise' with threshold 0.4.")
    out = ss.find().fuzzy("name", "Alise", threshold=0.4).execute()
    seen_names = sorted({r["name"] for r in out})
    print(f"  unique near matches: {seen_names}")

    section("Step 7. Always on sketches")
    print(f"  maybe_contains city='NYC':       {ss.maybe_contains('city', 'NYC')}")
    print(f"  maybe_contains city='Atlantis':  {ss.maybe_contains('city', 'Atlantis')}")
    print(f"  estimate_count city='NYC':       {ss.estimate_count('city', 'NYC')}")
    print(f"  estimate_count city='SF':        {ss.estimate_count('city', 'SF')}")
    print(f"  estimate_count city='Atlantis':  {ss.estimate_count('city', 'Atlantis')}")

    section("Step 8. Graph queries")
    # Wire up a tiny social graph between the first few records.
    ss.add_edge(0, 1)
    ss.add_edge(1, 2)
    ss.add_edge(2, 3)
    ss.add_edge(0, 4, label="block")
    print(f"  neighbors of record 0: {ss.neighbors(0)}")
    print(f"  bfs from record 0 within 2 hops: {dict(sorted(ss.bfs(0, max_depth=2).items()))}")
    print(f"  shortest path 0 to 3: {ss.shortest_path(0, 3)}")
    print(f"  neighbors of 0 ignoring 'block' label: {ss.neighbors(0, label=None)}")

    section("Step 9. Persistence")
    with tempfile.TemporaryDirectory() as tmpdir:
        path = os.path.join(tmpdir, "snapshot.json")
        ss.save(path)
        size = os.path.getsize(path)
        print(f"  saved to {path}, {size} bytes")

        loaded = Superstruct.load(path)
        print(f"  loaded records: {len(loaded)}")
        print(f"  loaded inventory before any query: {loaded.index_inventory()}")
        out = loaded.find().equals("city", "NYC").top_k("score", 1).execute()
        if out:
            print(f"  highest scoring NYC after reload: {out[0]['name']} score={out[0]['score']}")
        print(f"  inventory after the reload query: {loaded.index_inventory()}")

    section("Step 10. Concurrency")
    parallel = Superstruct()

    def writer():
        for i in range(1000):
            parallel.insert({"city": "NYC", "n": i})

    def reader():
        for _ in range(1000):
            parallel.find().equals("city", "NYC").execute()

    threads = [
        threading.Thread(target=writer),
        threading.Thread(target=writer),
        threading.Thread(target=reader),
        threading.Thread(target=reader),
    ]
    t0 = time.monotonic()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    elapsed = time.monotonic() - t0
    print(f"  4 threads, 2000 inserts and 2000 queries finished in {elapsed * 1000:.0f} ms")
    print(f"  final record count: {len(parallel)}")

    section("Step 11. Adaptive eviction under memory pressure")
    print(f"  before: {ss.index_inventory()}")
    ss.set_memory_budget(256)
    print(f"  after a 256 byte budget: {ss.index_inventory()}")

    print("\nDemo complete.")


if __name__ == "__main__":
    main()
