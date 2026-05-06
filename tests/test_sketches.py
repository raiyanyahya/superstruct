"""Probabilistic sketch tests.

Wraps the Superstruct facade tests for the auto attached sketches and
direct unit tests of the BloomSketch and CountMinSketch classes that
verify quality bounds.
"""
import unittest

from superstruct import Superstruct
from superstruct.sketches import BloomSketch, CountMinSketch


class FacadeSketchTests(unittest.TestCase):
    """Sketches accessed through Superstruct.maybe_contains and estimate_count."""

    def setUp(self):
        self.ss = Superstruct()
        for _ in range(50):
            self.ss.insert({"city": "NYC"})
        for _ in range(10):
            self.ss.insert({"city": "SF"})

    def test_bloom_says_yes_for_inserted(self):
        self.assertTrue(self.ss.maybe_contains("city", "NYC"))
        self.assertTrue(self.ss.maybe_contains("city", "SF"))

    def test_bloom_says_no_for_never_inserted(self):
        self.assertFalse(self.ss.maybe_contains("city", "Atlantis"))

    def test_count_min_estimates_frequency(self):
        self.assertGreaterEqual(self.ss.estimate_count("city", "NYC"), 50)
        self.assertGreaterEqual(self.ss.estimate_count("city", "SF"), 10)

    def test_count_min_zero_for_never_seen(self):
        self.assertEqual(self.ss.estimate_count("city", "Atlantis"), 0)

    def test_unknown_attribute_returns_safe_defaults(self):
        # Attribute never seen returns False for bloom and zero for count.
        self.assertFalse(self.ss.maybe_contains("ghost", "x"))
        self.assertEqual(self.ss.estimate_count("ghost", "x"), 0)


class BloomQualityTests(unittest.TestCase):
    """Bloom must never have false negatives and the false positive
    rate must stay low at moderate occupancy."""

    def test_no_false_negatives(self):
        bf = BloomSketch(bit_size=2048, num_hashes=3)
        for i in range(200):
            bf.add(f"item-{i}")
        for i in range(200):
            self.assertTrue(bf.maybe_contains(f"item-{i}"))

    def test_false_positive_rate_is_low_at_default_size(self):
        bf = BloomSketch()
        for i in range(200):
            bf.add(f"item-{i}")
        false_positives = sum(
            1 for i in range(10_000)
            if bf.maybe_contains(f"never-seen-{i}")
        )
        # Default 16384 bits, 5 hashes, 200 items. Expected FP rate is
        # well under one percent. Allow up to two percent for safety.
        self.assertLess(false_positives / 10_000, 0.02)


class CountMinQualityTests(unittest.TestCase):
    def test_zero_for_never_seen(self):
        cm = CountMinSketch()
        self.assertEqual(cm.estimate("nope"), 0)

    def test_estimate_is_at_least_true_count(self):
        # Count min always over estimates, so the result is at least
        # the true count and usually exact when collisions are sparse.
        cm = CountMinSketch(width=4096, depth=5)
        true_counts = {}
        for i in range(50):
            true_counts[f"item-{i}"] = i + 1
            for _ in range(i + 1):
                cm.add(f"item-{i}")
        for key, true_count in true_counts.items():
            self.assertGreaterEqual(cm.estimate(key), true_count)


if __name__ == "__main__":
    unittest.main()
