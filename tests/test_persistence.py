"""Save and load tests."""
import os
import tempfile
import unittest

from superstruct import Superstruct


class PersistenceTests(unittest.TestCase):
    def test_round_trip_preserves_records_and_edges(self):
        ss = Superstruct()
        a = ss.insert({"name": "Alice", "age": 30})
        b = ss.insert({"name": "Bob", "age": 25})
        ss.add_edge(a, b)

        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "snapshot.json")
            ss.save(path)
            loaded = Superstruct.load(path)

            self.assertEqual(loaded.get(a)["name"], "Alice")
            self.assertEqual(loaded.get(b)["name"], "Bob")
            self.assertIn(b, loaded.neighbors(a))

    def test_load_then_query_triggers_rebuild(self):
        ss = Superstruct()
        for name in ["Alice", "Bob", "Carol"]:
            ss.insert({"name": name})

        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "snapshot.json")
            ss.save(path)
            loaded = Superstruct.load(path)

            self.assertEqual(loaded.index_inventory(), [])
            out = loaded.find().prefix("name", "A").execute()
            self.assertEqual([r["name"] for r in out], ["Alice"])
            types = [t for t, _, _ in loaded.index_inventory()]
            self.assertIn("TrieIndex", types)


class PersistenceEdgeCaseTests(unittest.TestCase):
    def test_save_load_empty_store(self):
        ss = Superstruct()
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "empty.json")
            ss.save(path)
            loaded = Superstruct.load(path)
            self.assertEqual(len(loaded), 0)

    def test_heterogeneous_schemas_survive(self):
        ss = Superstruct()
        ss.insert({"a": 1})
        ss.insert({"b": "hello"})
        ss.insert({"c": [1, 2, 3]})

        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "het.json")
            ss.save(path)
            loaded = Superstruct.load(path)
            self.assertEqual(len(loaded), 3)
            self.assertEqual(loaded.get(0), {"a": 1})
            self.assertEqual(loaded.get(1), {"b": "hello"})
            self.assertEqual(loaded.get(2), {"c": [1, 2, 3]})

    def test_graph_labels_survive_round_trip(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        b = ss.insert({"x": 2})
        ss.add_edge(a, b, label="friend")
        ss.add_edge(a, b, label="rival")

        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "graph.json")
            ss.save(path)
            loaded = Superstruct.load(path)
            self.assertEqual(loaded.neighbors(a, label="friend"), {b})
            self.assertEqual(loaded.neighbors(a, label="rival"), {b})

    def test_sketches_rebuild_after_load(self):
        ss = Superstruct()
        for _ in range(10):
            ss.insert({"city": "NYC"})

        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "snap.json")
            ss.save(path)
            loaded = Superstruct.load(path)
            self.assertTrue(loaded.maybe_contains("city", "NYC"))
            self.assertGreaterEqual(loaded.estimate_count("city", "NYC"), 10)

    def test_loaded_id_counter_does_not_collide_with_new_inserts(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        b = ss.insert({"x": 2})
        with tempfile.TemporaryDirectory() as tmpdir:
            path = os.path.join(tmpdir, "ids.json")
            ss.save(path)
            loaded = Superstruct.load(path)
            c = loaded.insert({"x": 3})
            self.assertNotIn(c, [a, b])


if __name__ == "__main__":
    unittest.main()
