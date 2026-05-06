"""Direct unit tests for each Index implementation.

We instantiate concrete index classes directly and exercise their
build, insert, remove and execute paths. This catches bugs that the
facade level tests would only see indirectly.
"""
import unittest

from superstruct.primary import Record
from superstruct.query import Predicate, PredicateKind
from superstruct.indexes import (
    HashIndex, SortedIndex, TrieIndex, InvertedIndex, NgramIndex,
)


def _records(*items):
    """Helper. Wrap a series of attribute dicts into Record objects."""
    return [Record(id=i, attrs=attrs) for i, attrs in enumerate(items)]


class HashIndexTests(unittest.TestCase):
    def test_build_and_query(self):
        idx = HashIndex("city")
        idx.build_from_records(_records(
            {"city": "NYC"}, {"city": "SF"}, {"city": "NYC"},
        ))
        ids = idx.execute(Predicate(PredicateKind.EQUALS, "city", "NYC"))
        self.assertEqual(ids, {0, 2})

    def test_skips_records_without_attribute(self):
        idx = HashIndex("city")
        idx.build_from_records(_records({"city": "NYC"}, {"name": "x"}))
        self.assertEqual(
            idx.execute(Predicate(PredicateKind.EQUALS, "city", "NYC")),
            {0},
        )

    def test_remove_clears_id(self):
        idx = HashIndex("city")
        idx.build_from_records(_records({"city": "NYC"}, {"city": "NYC"}))
        idx.remove(Record(id=0, attrs={"city": "NYC"}))
        self.assertEqual(
            idx.execute(Predicate(PredicateKind.EQUALS, "city", "NYC")),
            {1},
        )

    def test_can_answer_equality_only(self):
        idx = HashIndex("x")
        self.assertTrue(
            idx.can_answer(Predicate(PredicateKind.EQUALS, "x", 1))
        )
        self.assertFalse(
            idx.can_answer(Predicate(PredicateKind.RANGE, "x", (0, 5)))
        )
        # Same kind but different attribute. Index says no.
        self.assertFalse(
            idx.can_answer(Predicate(PredicateKind.EQUALS, "y", 1))
        )

    def test_unknown_value_returns_empty(self):
        idx = HashIndex("x")
        idx.build_from_records(_records({"x": 1}, {"x": 2}))
        self.assertEqual(
            idx.execute(Predicate(PredicateKind.EQUALS, "x", 99)),
            set(),
        )


class SortedIndexTests(unittest.TestCase):
    def test_range_includes_both_bounds(self):
        idx = SortedIndex("n")
        idx.build_from_records(_records(*[{"n": i} for i in range(5)]))
        ids = idx.execute(Predicate(PredicateKind.RANGE, "n", (1, 3)))
        self.assertEqual(ids, {1, 2, 3})

    def test_range_handles_duplicates(self):
        idx = SortedIndex("n")
        idx.build_from_records(_records({"n": 1}, {"n": 1}, {"n": 2}))
        ids = idx.execute(Predicate(PredicateKind.RANGE, "n", (1, 1)))
        self.assertEqual(ids, {0, 1})

    def test_insert_keeps_sorted_order(self):
        idx = SortedIndex("n")
        idx.build_from_records(_records({"n": 3}, {"n": 1}))
        idx.insert(Record(id=2, attrs={"n": 2}))
        ids = idx.execute(Predicate(PredicateKind.RANGE, "n", (0, 5)))
        self.assertEqual(ids, {0, 1, 2})

    def test_remove_finds_correct_id_among_duplicates(self):
        idx = SortedIndex("n")
        idx.build_from_records(_records({"n": 5}, {"n": 5}, {"n": 5}))
        idx.remove(Record(id=1, attrs={"n": 5}))
        ids = idx.execute(Predicate(PredicateKind.RANGE, "n", (5, 5)))
        self.assertEqual(ids, {0, 2})

    def test_equals_via_sorted_index(self):
        idx = SortedIndex("n")
        idx.build_from_records(_records({"n": 1}, {"n": 2}, {"n": 2}, {"n": 3}))
        ids = idx.execute(Predicate(PredicateKind.EQUALS, "n", 2))
        self.assertEqual(ids, {1, 2})


class TrieIndexTests(unittest.TestCase):
    def test_prefix_returns_subtree(self):
        idx = TrieIndex("name")
        idx.build_from_records(_records(
            {"name": "Alice"}, {"name": "Anya"}, {"name": "Bob"},
        ))
        ids = idx.execute(Predicate(PredicateKind.PREFIX, "name", "A"))
        self.assertEqual(ids, {0, 1})

    def test_exact_via_equals(self):
        idx = TrieIndex("name")
        idx.build_from_records(_records({"name": "Alice"}, {"name": "Anya"}))
        ids = idx.execute(Predicate(PredicateKind.EQUALS, "name", "Alice"))
        self.assertEqual(ids, {0})

    def test_skips_non_string_values(self):
        idx = TrieIndex("v")
        idx.build_from_records(_records({"v": "abc"}, {"v": 123}))
        ids = idx.execute(Predicate(PredicateKind.PREFIX, "v", "a"))
        self.assertEqual(ids, {0})

    def test_remove_clears_id(self):
        idx = TrieIndex("name")
        idx.build_from_records(_records({"name": "Alice"}, {"name": "Anya"}))
        idx.remove(Record(id=0, attrs={"name": "Alice"}))
        ids = idx.execute(Predicate(PredicateKind.PREFIX, "name", "A"))
        self.assertEqual(ids, {1})

    def test_empty_prefix_returns_all_strings(self):
        idx = TrieIndex("name")
        idx.build_from_records(_records({"name": "Alice"}, {"name": "Bob"}))
        ids = idx.execute(Predicate(PredicateKind.PREFIX, "name", ""))
        self.assertEqual(ids, {0, 1})


class InvertedIndexTests(unittest.TestCase):
    def test_finds_word_anywhere(self):
        idx = InvertedIndex("bio")
        idx.build_from_records(_records(
            {"bio": "loves cats"},
            {"bio": "dogs are great"},
            {"bio": "cats and dogs both"},
        ))
        cats = idx.execute(Predicate(PredicateKind.CONTAINS, "bio", "cats"))
        self.assertEqual(cats, {0, 2})

    def test_lowercases_query(self):
        idx = InvertedIndex("bio")
        idx.build_from_records(_records({"bio": "Hello World"}))
        ids = idx.execute(Predicate(PredicateKind.CONTAINS, "bio", "HELLO"))
        self.assertEqual(ids, {0})

    def test_remove_drops_postings(self):
        idx = InvertedIndex("bio")
        idx.build_from_records(_records({"bio": "alpha beta"}))
        idx.remove(Record(id=0, attrs={"bio": "alpha beta"}))
        ids = idx.execute(Predicate(PredicateKind.CONTAINS, "bio", "alpha"))
        self.assertEqual(ids, set())


class NgramIndexTests(unittest.TestCase):
    def test_finds_near_match(self):
        idx = NgramIndex("name")
        idx.build_from_records(_records(
            {"name": "Alice"}, {"name": "Bob"},
        ))
        pred = Predicate(
            PredicateKind.FUZZY, "name", "Alise", threshold=0.3,
        )
        self.assertIn(0, idx.execute(pred))

    def test_remove_drops_postings(self):
        idx = NgramIndex("name")
        idx.build_from_records(_records({"name": "Alice"}, {"name": "Anya"}))
        idx.remove(Record(id=0, attrs={"name": "Alice"}))
        pred = Predicate(
            PredicateKind.FUZZY, "name", "Alice", threshold=0.5,
        )
        self.assertNotIn(0, idx.execute(pred))

    def test_threshold_one_only_returns_exact(self):
        idx = NgramIndex("name")
        idx.build_from_records(_records(
            {"name": "Alice"}, {"name": "Alyce"}, {"name": "Alice"},
        ))
        pred = Predicate(
            PredicateKind.FUZZY, "name", "Alice", threshold=1.0,
        )
        ids = idx.execute(pred)
        self.assertEqual(ids, {0, 2})


if __name__ == "__main__":
    unittest.main()
