"""Large scale and stress tests.

These tests run on bigger datasets and compare Superstruct results to
brute force scans computed manually. They catch bugs that only show
up at scale and verify that adaptive eviction is making the right
choices.
"""
import random
import unittest

from superstruct import Superstruct


class LargeScaleCorrectnessTests(unittest.TestCase):
    """Compare Superstruct results to brute force scans."""

    def setUp(self):
        random.seed(42)
        self.ss = Superstruct()
        self.records = []
        for _ in range(2000):
            r = {
                "name": random.choice(["alice", "bob", "carol", "dave"]),
                "age": random.randint(0, 99),
                "city": random.choice(["NYC", "SF", "LA"]),
            }
            self.records.append(r)
            self.ss.insert(r)

    def test_random_equality_queries_match_brute_force(self):
        for city in ["NYC", "SF", "LA"]:
            expected = [r for r in self.records if r["city"] == city]
            out = self.ss.find().equals("city", city).execute()
            self.assertEqual(len(out), len(expected))

    def test_random_range_queries_match_brute_force(self):
        for lo, hi in [(0, 9), (20, 40), (50, 99), (10, 10)]:
            expected = [r for r in self.records if lo <= r["age"] <= hi]
            out = self.ss.find().range("age", lo, hi).execute()
            self.assertEqual(len(out), len(expected))

    def test_compound_query_matches_brute_force(self):
        expected = [
            r for r in self.records
            if r["name"].startswith("a")
            and 20 <= r["age"] <= 40
            and r["city"] == "NYC"
        ]
        out = (
            self.ss.find()
                  .prefix("name", "a")
                  .range("age", 20, 40)
                  .equals("city", "NYC")
                  .execute()
        )
        self.assertEqual(len(out), len(expected))

    def test_top_k_descending_matches_sort(self):
        expected = sorted(
            (r["age"] for r in self.records), reverse=True,
        )[:5]
        out = self.ss.find().top_k("age", 5).execute()
        self.assertEqual([r["age"] for r in out], expected)


class MemoryBudgetTests(unittest.TestCase):
    def test_zero_budget_keeps_no_indexes(self):
        ss = Superstruct(memory_budget_bytes=0)
        for i in range(100):
            ss.insert({"n": i})
        # Each query rebuilds and immediately gets evicted.
        for i in range(5):
            out = ss.find().equals("n", i).execute()
            self.assertEqual(len(out), 1)
        self.assertEqual(ss.index_inventory(), [])

    def test_eviction_picks_lowest_score_first(self):
        ss = Superstruct()
        for i in range(100):
            ss.insert({"a": i, "b": i})

        # Build two indexes. Hammer index a so it scores higher than b.
        for _ in range(20):
            ss.find().equals("a", 1).execute()
        ss.find().equals("b", 1).execute()  # Cold by comparison.

        sizes = {attr: size for (_, attr, size) in ss.index_inventory()}
        # Tighten the budget to fit only one index.
        max_size = max(sizes.values())
        ss.set_memory_budget(max_size + 1024)

        survivors = ss.index_inventory()
        attrs_left = {attr for (_, attr, _) in survivors}
        # The hot index on a survives.
        self.assertIn("a", attrs_left)


class WorkloadTrackerCorrectnessTests(unittest.TestCase):
    def test_repeated_queries_reuse_the_same_index(self):
        ss = Superstruct()
        for i in range(100):
            ss.insert({"x": i})
        ss.find().equals("x", 1).execute()
        ss.find().equals("x", 2).execute()
        ss.find().equals("x", 3).execute()
        # Inventory still shows only one hash index for x.
        types = [t for t, attr, _ in ss.index_inventory()]
        self.assertEqual(types.count("HashIndex"), 1)


if __name__ == "__main__":
    unittest.main()
