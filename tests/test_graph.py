"""Graph layer tests."""
import unittest

from superstruct import Superstruct


class GraphTests(unittest.TestCase):
    def setUp(self):
        self.ss = Superstruct()
        # Build a small social graph. Alice knows Bob and Carol. Bob
        # knows Dave. Eve is intentionally disconnected.
        self.alice = self.ss.insert({"name": "Alice"})
        self.bob = self.ss.insert({"name": "Bob"})
        self.carol = self.ss.insert({"name": "Carol"})
        self.dave = self.ss.insert({"name": "Dave"})
        self.eve = self.ss.insert({"name": "Eve"})

        self.ss.add_edge(self.alice, self.bob)
        self.ss.add_edge(self.alice, self.carol)
        self.ss.add_edge(self.bob, self.dave)

    def test_neighbors(self):
        self.assertEqual(
            self.ss.neighbors(self.alice),
            {self.bob, self.carol},
        )

    def test_bfs_with_max_depth(self):
        depths = self.ss.bfs(self.alice, max_depth=2)
        self.assertEqual(depths[self.alice], 0)
        self.assertEqual(depths[self.bob], 1)
        self.assertEqual(depths[self.dave], 2)
        self.assertNotIn(self.eve, depths)

    def test_shortest_path_finds_two_hop_route(self):
        path = self.ss.shortest_path(self.alice, self.dave)
        self.assertEqual(path, [self.alice, self.bob, self.dave])

    def test_shortest_path_none_when_unreachable(self):
        self.assertIsNone(self.ss.shortest_path(self.alice, self.eve))

    def test_delete_node_drops_edges(self):
        self.ss.delete(self.bob)
        self.assertIsNone(self.ss.shortest_path(self.alice, self.dave))


class GraphEdgeCaseTests(unittest.TestCase):
    def test_self_loop(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        ss.add_edge(a, a)
        self.assertIn(a, ss.neighbors(a))

    def test_directed_edge_does_not_go_reverse(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        b = ss.insert({"x": 2})
        ss.add_edge(a, b, directed=True)
        self.assertIn(b, ss.neighbors(a))
        self.assertNotIn(a, ss.neighbors(b))

    def test_labels_segregate_neighbors(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        b = ss.insert({"x": 2})
        c = ss.insert({"x": 3})
        ss.add_edge(a, b, label="friend")
        ss.add_edge(a, c, label="block")
        self.assertEqual(ss.neighbors(a, label="friend"), {b})
        self.assertEqual(ss.neighbors(a, label="block"), {c})
        # No label argument means every edge counts.
        self.assertEqual(ss.neighbors(a), {b, c})

    def test_bfs_no_max_depth_reaches_everything_connected(self):
        ss = Superstruct()
        nodes = [ss.insert({"n": i}) for i in range(10)]
        # Chain 0 -> 1 -> 2 -> ... -> 9
        for i in range(9):
            ss.add_edge(nodes[i], nodes[i + 1])
        depths = ss.bfs(nodes[0])
        self.assertEqual(len(depths), 10)
        self.assertEqual(depths[nodes[9]], 9)

    def test_shortest_path_same_source_and_target(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        self.assertEqual(ss.shortest_path(a, a), [a])

    def test_remove_edge_breaks_path(self):
        ss = Superstruct()
        a = ss.insert({"x": 1})
        b = ss.insert({"x": 2})
        ss.add_edge(a, b)
        self.assertIn(b, ss.neighbors(a))
        ss.remove_edge(a, b)
        self.assertNotIn(b, ss.neighbors(a))
        self.assertNotIn(a, ss.neighbors(b))

    def test_dense_graph_traversal(self):
        # Fully connected graph of 8 nodes. Every pair is one hop away.
        ss = Superstruct()
        nodes = [ss.insert({"i": i}) for i in range(8)]
        for i in range(8):
            for j in range(i + 1, 8):
                ss.add_edge(nodes[i], nodes[j])
        for i in range(8):
            depths = ss.bfs(nodes[i])
            for j in range(8):
                if i == j:
                    continue
                self.assertEqual(depths[nodes[j]], 1)


if __name__ == "__main__":
    unittest.main()
