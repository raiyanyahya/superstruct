"""Core query API tests.

Equality, range, prefix, compound queries, top-k ordering, lazy index
construction, eviction, propagation of inserts and deletes through
materialized indexes, plus boolean composition via any_of and exclude
and a battery of edge cases.
"""
import unittest

from superstruct import Superstruct


class CoreQueryTests(unittest.TestCase):
    """Original feature set. Equality, range, prefix, top-k, eviction."""

    def setUp(self):
        # Small fixed dataset. Picked so compound queries produce
        # predictable non trivial intersections.
        self.ss = Superstruct()
        self.records = [
            {"name": "Alice", "age": 30, "city": "NYC", "score": 88},
            {"name": "Anya",  "age": 25, "city": "SF",  "score": 92},
            {"name": "Andre", "age": 41, "city": "NYC", "score": 70},
            {"name": "Bea",   "age": 30, "city": "SF",  "score": 85},
            {"name": "Ben",   "age": 22, "city": "LA",  "score": 60},
        ]
        self.ids = [self.ss.insert(r) for r in self.records]

    def test_get_returns_record_by_id(self):
        self.assertEqual(self.ss.get(self.ids[0])["name"], "Alice")

    def test_get_returns_none_for_unknown_id(self):
        self.assertIsNone(self.ss.get(9999))

    def test_equals_query(self):
        out = self.ss.find().equals("city", "NYC").execute()
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Andre"])

    def test_range_query_inclusive_bounds(self):
        out = self.ss.find().range("age", 25, 30).execute()
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Anya", "Bea"])

    def test_prefix_query(self):
        out = self.ss.find().prefix("name", "A").execute()
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Andre", "Anya"])

    def test_prefix_query_no_match(self):
        out = self.ss.find().prefix("name", "Z").execute()
        self.assertEqual(out, [])

    def test_compound_intersection(self):
        out = (
            self.ss.find()
                  .range("age", 25, 50)
                  .prefix("name", "A")
                  .equals("city", "NYC")
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Andre"])

    def test_compound_intersection_empty(self):
        out = (
            self.ss.find()
                  .range("age", 25, 30)
                  .prefix("name", "A")
                  .equals("city", "LA")
                  .execute()
        )
        self.assertEqual(out, [])

    def test_top_k_descending(self):
        out = self.ss.find().top_k("score", 2).execute()
        scores = [r["score"] for r in out]
        self.assertEqual(scores, [92, 88])

    def test_top_k_ascending(self):
        out = self.ss.find().top_k("score", 2, descending=False).execute()
        scores = [r["score"] for r in out]
        self.assertEqual(scores, [60, 70])

    def test_top_k_after_filter(self):
        out = (
            self.ss.find()
                  .equals("city", "NYC")
                  .top_k("score", 1)
                  .execute()
        )
        self.assertEqual(out[0]["name"], "Alice")

    def test_no_indexes_built_until_first_query(self):
        self.assertEqual(self.ss.index_inventory(), [])
        self.ss.find().equals("city", "NYC").execute()
        types = [t for t, _, _ in self.ss.index_inventory()]
        self.assertIn("HashIndex", types)

    def test_insert_after_build_propagates_to_index(self):
        self.ss.find().equals("city", "NYC").execute()
        self.ss.insert({"name": "Zed", "age": 99, "city": "NYC", "score": 1})
        out = self.ss.find().equals("city", "NYC").execute()
        names = sorted(r["name"] for r in out)
        self.assertIn("Zed", names)

    def test_delete_propagates_to_index(self):
        self.ss.find().equals("city", "NYC").execute()
        self.ss.delete(self.ids[0])
        out = self.ss.find().equals("city", "NYC").execute()
        names = sorted(r["name"] for r in out)
        self.assertNotIn("Alice", names)

    def test_eviction_under_tight_budget(self):
        self.ss.find().equals("city", "NYC").execute()
        self.ss.find().range("age", 20, 40).execute()
        self.ss.find().prefix("name", "A").execute()
        self.assertGreater(len(self.ss.index_inventory()), 0)
        self.ss.set_memory_budget(0)
        self.assertEqual(self.ss.index_inventory(), [])

    def test_correctness_survives_eviction(self):
        before = self.ss.find().equals("city", "NYC").execute()
        self.ss.set_memory_budget(0)
        after = self.ss.find().equals("city", "NYC").execute()
        self.assertEqual(
            sorted(r["name"] for r in before),
            sorted(r["name"] for r in after),
        )


class BooleanCompositionTests(unittest.TestCase):
    """OR via any_of and NOT via exclude on the QueryBuilder."""

    def setUp(self):
        self.ss = Superstruct()
        self.records = [
            {"name": "Alice", "city": "NYC", "score": 88},
            {"name": "Bob",   "city": "SF",  "score": 92},
            {"name": "Carol", "city": "NYC", "score": 70},
            {"name": "Dave",  "city": "LA",  "score": 60},
        ]
        for r in self.records:
            self.ss.insert(r)

    def test_or_group_unions(self):
        out = (
            self.ss.find()
                  .any_of(
                      self.ss.find().equals("city", "NYC"),
                      self.ss.find().range("score", 90, 100),
                  )
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Bob", "Carol"])

    def test_exclude_subtracts(self):
        out = (
            self.ss.find()
                  .exclude(self.ss.find().equals("city", "NYC"))
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Bob", "Dave"])

    def test_combine_or_and_not(self):
        out = (
            self.ss.find()
                  .any_of(
                      self.ss.find().equals("city", "NYC"),
                      self.ss.find().range("score", 90, 100),
                  )
                  .exclude(self.ss.find().equals("name", "Alice"))
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Bob", "Carol"])

    def test_nested_or_via_subbuilder(self):
        # An any_of can take a sub builder that itself has an any_of so
        # users can express deeper boolean trees.
        inner = self.ss.find().any_of(
            self.ss.find().equals("city", "NYC"),
            self.ss.find().equals("city", "SF"),
        )
        out = (
            self.ss.find()
                  .any_of(inner, self.ss.find().equals("name", "Dave"))
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Bob", "Carol", "Dave"])


class EdgeCaseTests(unittest.TestCase):
    """Boundary conditions and unusual inputs."""

    def test_query_on_empty_store_returns_empty(self):
        ss = Superstruct()
        self.assertEqual(ss.find().equals("city", "NYC").execute(), [])
        self.assertEqual(ss.find().range("age", 0, 100).execute(), [])
        self.assertEqual(ss.find().prefix("name", "A").execute(), [])

    def test_insert_empty_dict_yields_id(self):
        ss = Superstruct()
        rid = ss.insert({})
        self.assertEqual(ss.get(rid), {})

    def test_query_on_attribute_no_record_has(self):
        ss = Superstruct()
        ss.insert({"a": 1})
        # Attribute "b" exists nowhere. Query returns empty cleanly.
        self.assertEqual(ss.find().equals("b", 1).execute(), [])

    def test_range_with_lo_greater_than_hi_returns_empty(self):
        ss = Superstruct()
        for i in range(5):
            ss.insert({"n": i})
        self.assertEqual(ss.find().range("n", 4, 1).execute(), [])

    def test_range_with_lo_equal_hi_acts_as_point(self):
        ss = Superstruct()
        for i in range(5):
            ss.insert({"n": i})
        out = ss.find().range("n", 2, 2).execute()
        self.assertEqual([r["n"] for r in out], [2])

    def test_prefix_with_empty_string_matches_all_strings(self):
        ss = Superstruct()
        ss.insert({"name": "Alice"})
        ss.insert({"name": "Bob"})
        ss.insert({"name": "Carol"})
        out = ss.find().prefix("name", "").execute()
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice", "Bob", "Carol"])

    def test_records_without_attribute_skip_silently(self):
        ss = Superstruct()
        ss.insert({"name": "Alice"})
        ss.insert({"age": 30})  # No name attribute on this one.
        out = ss.find().prefix("name", "A").execute()
        self.assertEqual(len(out), 1)
        self.assertEqual(out[0]["name"], "Alice")

    def test_delete_unknown_id_returns_false(self):
        ss = Superstruct()
        self.assertFalse(ss.delete(0))
        ss.insert({"x": 1})
        self.assertFalse(ss.delete(99))

    def test_insert_returns_monotonic_ids(self):
        ss = Superstruct()
        ids = [ss.insert({"n": i}) for i in range(5)]
        self.assertEqual(ids, [0, 1, 2, 3, 4])

    def test_deleted_id_is_not_reused(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        ss.delete(a)
        b = ss.insert({"x": 2})
        self.assertNotEqual(a, b)

    def test_query_matches_all_when_no_predicates(self):
        ss = Superstruct()
        for i in range(3):
            ss.insert({"n": i})
        out = ss.find().execute()
        self.assertEqual(len(out), 3)

    def test_top_k_alone_returns_ordered_universe(self):
        ss = Superstruct()
        for i in [3, 1, 4, 1, 5, 9, 2, 6]:
            ss.insert({"v": i})
        out = ss.find().top_k("v", 3).execute()
        self.assertEqual([r["v"] for r in out], [9, 6, 5])


class TopKEdgeCaseTests(unittest.TestCase):
    def setUp(self):
        self.ss = Superstruct()
        for i in range(5):
            self.ss.insert({"score": i})

    def test_k_zero_returns_empty(self):
        self.assertEqual(self.ss.find().top_k("score", 0).execute(), [])

    def test_k_larger_than_result_returns_all(self):
        out = self.ss.find().top_k("score", 100).execute()
        self.assertEqual(len(out), 5)

    def test_top_k_skips_records_missing_attribute(self):
        # Insert a record with no score field. It must not appear in
        # the ordered output because comparing None to ints would fail.
        self.ss.insert({"other": 99})
        out = self.ss.find().top_k("score", 100).execute()
        for r in out:
            self.assertIn("score", r)


if __name__ == "__main__":
    unittest.main()
