"""Real time benchmarks for Superstruct.

Five sections.

  1. Insert throughput across data sizes.
  2. Query latency for each verb. Cold first call versus warm reuse.
  3. Compound query speedup over a primary store scan.
  4. Concurrency throughput with mixed readers and writers.
  5. Memory footprint of materialized indexes.

Each section prints results immediately so the output reads as a live
log. Numbers depend heavily on the host machine. Re run on the same
hardware to compare relative costs across changes.
"""
import os
import random
import sys
import threading
import time

# Make the package importable when this script is run directly.
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from superstruct import Superstruct


# ---------------------------------------------------------------------
# Pretty printing helpers
# ---------------------------------------------------------------------

def section(title: str) -> None:
    """Print a banner that separates benchmark sections clearly."""
    print("\n" + "=" * 68)
    print(title)
    print("=" * 68)


def fmt_us(seconds: float) -> str:
    return f"{seconds * 1_000_000:.2f} us"


def fmt_ms(seconds: float) -> str:
    return f"{seconds * 1000:.3f} ms"


def fmt_per_sec(count: int, seconds: float) -> str:
    if seconds <= 0:
        return "infinite"
    return f"{count / seconds:,.0f} ops/sec"


def time_block(fn) -> float:
    """Run fn once and return how long it took in seconds."""
    t0 = time.perf_counter()
    fn()
    return time.perf_counter() - t0


# ---------------------------------------------------------------------
# Synthetic data generation
# ---------------------------------------------------------------------

def populate(n: int) -> Superstruct:
    """Build a Superstruct with n synthetic records.

    Thread safety is off because the benchmark is single threaded for
    most sections and we want to measure the structure itself, not the
    lock overhead.
    """
    ss = Superstruct(thread_safe=False)
    cities = ["NYC", "SF", "LA", "Boston", "Austin"]
    names = [
        "alice", "anya", "andre", "bea", "ben",
        "cara", "carl", "diana", "erin", "fred",
    ]
    bios = [
        "loves cats and long walks",
        "dog person all the way",
        "cat owner who also walks dogs",
        "runs marathons every weekend",
        "indie game developer and coffee fan",
    ]
    rng = random.Random(0)
    for _ in range(n):
        ss.insert({
            "name": rng.choice(names),
            "age": rng.randint(18, 80),
            "score": rng.randint(0, 100),
            "city": rng.choice(cities),
            "bio": rng.choice(bios),
        })
    return ss


# ---------------------------------------------------------------------
# Section 1. Insert throughput
# ---------------------------------------------------------------------

def bench_insert_throughput():
    section("1. Insert throughput")
    print(
        f"{'records':>10} {'total':>14} "
        f"{'per insert':>14} {'rate':>22}"
    )
    for n in [1_000, 10_000, 50_000]:
        elapsed = time_block(lambda: populate(n))
        per = elapsed / n
        print(
            f"{n:>10,} {fmt_ms(elapsed):>14} "
            f"{fmt_us(per):>14} {fmt_per_sec(n, elapsed):>22}"
        )


# ---------------------------------------------------------------------
# Section 2. Query latency. Cold versus warm.
# ---------------------------------------------------------------------

def bench_query_latency():
    section("2. Query latency. Cold first call versus warm reuse")
    ss = populate(20_000)

    queries = {
        "equals city = NYC": lambda: ss.find().equals("city", "NYC").execute(),
        "range age 25..35":  lambda: ss.find().range("age", 25, 35).execute(),
        "prefix name = a":   lambda: ss.find().prefix("name", "a").execute(),
        "contains bio cat":  lambda: ss.find().contains("bio", "cat").execute(),
        "fuzzy name alise":  lambda: ss.find().fuzzy("name", "alise", threshold=0.4).execute(),
    }

    print(f"{'query':>22} {'cold':>14} {'warm avg':>14} {'speedup':>10}")
    for label, q in queries.items():
        cold = time_block(q)
        # Run the query several times. Warm latency is the average.
        warm_runs = [time_block(q) for _ in range(50)]
        warm_avg = sum(warm_runs) / len(warm_runs)
        speedup = cold / warm_avg if warm_avg > 0 else 0
        print(
            f"{label:>22} {fmt_ms(cold):>14} "
            f"{fmt_us(warm_avg):>14} {speedup:>9.0f}x"
        )


# ---------------------------------------------------------------------
# Section 3. Compound query versus a primary store scan
# ---------------------------------------------------------------------

def bench_compound_vs_scan():
    section("3. Compound query. Indexed versus naive scan")
    ss = populate(50_000)

    # Warm every index used by the compound query.
    for _ in range(2):
        ss.find().range("age", 25, 35).execute()
        ss.find().prefix("name", "a").execute()
        ss.find().equals("city", "SF").execute()

    def compound():
        return (
            ss.find()
              .range("age", 25, 35)
              .prefix("name", "a")
              .equals("city", "SF")
              .execute()
        )

    def scan():
        # Naive python scan over the primary store. Equivalent semantics
        # to the indexed compound query above so the comparison is fair.
        out = []
        for record in ss._primary:
            attrs = record.attrs
            name = attrs.get("name")
            if (
                25 <= attrs.get("age", -1) <= 35
                and isinstance(name, str)
                and name.startswith("a")
                and attrs.get("city") == "SF"
            ):
                out.append(attrs)
        return out

    runs = 5
    compound_avg = sum(time_block(compound) for _ in range(runs)) / runs
    scan_avg = sum(time_block(scan) for _ in range(runs)) / runs
    print(f"  compound (indexed): {fmt_ms(compound_avg)}")
    print(f"  naive python scan:  {fmt_ms(scan_avg)}")
    if compound_avg > 0:
        print(f"  speedup:            {scan_avg / compound_avg:.0f}x")


# ---------------------------------------------------------------------
# Section 4. Concurrency
# ---------------------------------------------------------------------

def bench_concurrency():
    section("4. Concurrency. Mixed readers and writers")
    parallel = Superstruct()

    n_writers = 4
    n_readers = 4
    ops_per_thread = 2_000

    def writer(seed: int):
        rng = random.Random(seed)
        for _ in range(ops_per_thread):
            parallel.insert({"city": "NYC", "n": rng.randint(0, 999)})

    def reader():
        for _ in range(ops_per_thread):
            parallel.find().equals("city", "NYC").execute()

    threads = []
    for i in range(n_writers):
        threads.append(threading.Thread(target=writer, args=(i,)))
    for _ in range(n_readers):
        threads.append(threading.Thread(target=reader))

    t0 = time.perf_counter()
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    elapsed = time.perf_counter() - t0
    total_ops = (n_writers + n_readers) * ops_per_thread
    print(f"  {n_writers} writers + {n_readers} readers, {ops_per_thread:,} ops each")
    print(f"  total {total_ops:,} operations in {fmt_ms(elapsed)}")
    print(f"  throughput: {fmt_per_sec(total_ops, elapsed)}")
    print(f"  final record count: {len(parallel):,}")


# ---------------------------------------------------------------------
# Section 5. Memory footprint of materialized indexes
# ---------------------------------------------------------------------

def bench_memory_inventory():
    section("5. Memory footprint of materialized indexes")
    ss = populate(20_000)
    ss.find().equals("city", "NYC").execute()
    ss.find().range("age", 25, 35).execute()
    ss.find().prefix("name", "a").execute()
    ss.find().contains("bio", "cat").execute()
    ss.find().fuzzy("name", "alise", threshold=0.4).execute()
    print(f"  {'type':>15} {'attribute':>12} {'bytes':>14}")
    inventory = sorted(ss.index_inventory(), key=lambda x: -x[2])
    for (kind, attr, size) in inventory:
        print(f"  {kind:>15} {attr:>12} {size:>14,}")
    total = sum(size for (_, _, size) in inventory)
    print(f"  {'total':>15} {'':>12} {total:>14,}")


def main():
    bench_insert_throughput()
    bench_query_latency()
    bench_compound_vs_scan()
    bench_concurrency()
    bench_memory_inventory()
    print("\nDone.")


if __name__ == "__main__":
    main()
