"""Full text and fuzzy match tests."""
import unittest

from superstruct import Superstruct


class FullTextTests(unittest.TestCase):
    def setUp(self):
        self.ss = Superstruct()
        self.ids = [
            self.ss.insert({"bio": "loves cats and long walks"}),
            self.ss.insert({"bio": "dog person all the way"}),
            self.ss.insert({"bio": "cat owner who also walks dogs"}),
        ]

    def test_single_word_match(self):
        out = self.ss.find().contains("bio", "cats").execute()
        bios = [r["bio"] for r in out]
        self.assertIn("loves cats and long walks", bios)

    def test_case_insensitive(self):
        out = self.ss.find().contains("bio", "DOG").execute()
        self.assertEqual(len(out), 1)
        self.assertIn("dog", out[0]["bio"])

    def test_two_words_anded(self):
        out = (
            self.ss.find()
                  .contains("bio", "walks")
                  .contains("bio", "dogs")
                  .execute()
        )
        self.assertEqual(len(out), 1)
        self.assertIn("walks dogs", out[0]["bio"])

    def test_punctuation_does_not_block_match(self):
        ss = Superstruct()
        ss.insert({"bio": "hello, world! how are you?"})
        self.assertEqual(len(ss.find().contains("bio", "hello").execute()), 1)
        self.assertEqual(len(ss.find().contains("bio", "world").execute()), 1)

    def test_word_not_present_returns_empty(self):
        out = self.ss.find().contains("bio", "elephant").execute()
        self.assertEqual(out, [])


class FuzzyTests(unittest.TestCase):
    def setUp(self):
        self.ss = Superstruct()
        for name in ["Alice", "Alicia", "Alyce", "Bob", "Charlie"]:
            self.ss.insert({"name": name})

    def test_fuzzy_finds_near_matches(self):
        out = (
            self.ss.find()
                  .fuzzy("name", "Alise", threshold=0.3)
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertIn("Alice", names)

    def test_fuzzy_strict_threshold_rules_out_far_matches(self):
        out = (
            self.ss.find()
                  .fuzzy("name", "Alice", threshold=0.5)
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertNotIn("Bob", names)
        self.assertNotIn("Charlie", names)

    def test_fuzzy_threshold_one_only_matches_exact(self):
        # Threshold 1 means trigram set equality which is exact match
        # after lower casing.
        out = (
            self.ss.find()
                  .fuzzy("name", "Alice", threshold=1.0)
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertEqual(names, ["Alice"])

    def test_fuzzy_lower_threshold_widens_net(self):
        # Threshold 0.1 should pull in Alyce and Alicia comfortably.
        out = (
            self.ss.find()
                  .fuzzy("name", "Alice", threshold=0.1)
                  .execute()
        )
        names = sorted(r["name"] for r in out)
        self.assertIn("Alyce", names)
        self.assertIn("Alicia", names)


if __name__ == "__main__":
    unittest.main()
