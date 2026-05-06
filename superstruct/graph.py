"""Graph store. A side car relationship layer.

Records are nodes. Edges are stored separately by record id and can
carry an optional label so users can model multiple relationship
types in the same store. The graph offers neighbors, breadth first
search and shortest path methods.

The graph is independent of the index zoo. It does not need a planner
or a workload tracker because graph traversal patterns are uniform
enough that a single adjacency representation handles every operation
the API exposes.
"""
from collections import deque, defaultdict
from typing import Optional


class GraphStore:
    """Adjacency lists with optional edge labels.

    Edges are bidirectional by default. Pass directed=True to add and
    remove a one way edge instead. Self loops are allowed.
    """

    def __init__(self):
        # node id to set of (neighbor id, label). We use a set of tuples
        # so duplicate edges are silently deduplicated and so removal
        # is cheap.
        self._adj: dict[int, set[tuple[int, Optional[str]]]] = defaultdict(set)

    def add_edge(
        self,
        a: int,
        b: int,
        label: Optional[str] = None,
        directed: bool = False,
    ) -> None:
        """Connect a and b. Bidirectional unless directed is True."""
        self._adj[a].add((b, label))
        if not directed:
            self._adj[b].add((a, label))

    def remove_edge(
        self,
        a: int,
        b: int,
        label: Optional[str] = None,
        directed: bool = False,
    ) -> None:
        """Remove the link between a and b. Both ends unless directed."""
        self._adj.get(a, set()).discard((b, label))
        if not directed:
            self._adj.get(b, set()).discard((a, label))

    def remove_node(self, node_id: int) -> None:
        """Remove every edge that touches node_id.

        Called when a record is deleted from the primary store so that
        the graph stays consistent with the data.
        """
        # Drop the node's own adjacency entry then walk every neighbor
        # and remove any reverse edge that mentions node_id.
        outgoing = self._adj.pop(node_id, set())
        for neighbor, _label in outgoing:
            kept = {(n, l) for (n, l) in self._adj[neighbor] if n != node_id}
            if kept:
                self._adj[neighbor] = kept
            else:
                # Avoid leaving an empty entry behind.
                del self._adj[neighbor]

    def neighbors(
        self, node_id: int, label: Optional[str] = None
    ) -> set[int]:
        """Immediate neighbors of node_id.

        If label is given only edges with that label are followed.
        Pass label=None to ignore labels and traverse every edge.
        """
        if label is None:
            return {n for (n, _l) in self._adj.get(node_id, set())}
        return {
            n for (n, l) in self._adj.get(node_id, set()) if l == label
        }

    def bfs(
        self,
        start: int,
        max_depth: Optional[int] = None,
        label: Optional[str] = None,
    ) -> dict[int, int]:
        """Breadth first search from start.

        Returns a map of node id to the depth at which it was first
        reached. Useful for queries like "every friend within two hops"
        which becomes a bfs with max_depth equal to two.
        """
        depths: dict[int, int] = {start: 0}
        frontier = deque([start])
        while frontier:
            node = frontier.popleft()
            current_depth = depths[node]
            if max_depth is not None and current_depth >= max_depth:
                continue
            for neighbor in self.neighbors(node, label):
                if neighbor in depths:
                    continue
                depths[neighbor] = current_depth + 1
                frontier.append(neighbor)
        return depths

    def shortest_path(
        self,
        source: int,
        target: int,
        label: Optional[str] = None,
    ) -> Optional[list[int]]:
        """Shortest path from source to target as a list of node ids.

        Returns None if no path exists. Both endpoints are included
        in the returned list. Edges are unweighted so plain breadth
        first search gives the optimal path.
        """
        if source == target:
            return [source]

        # Standard BFS with predecessor tracking so we can reconstruct
        # the path once we reach the target.
        predecessor: dict[int, int] = {source: source}
        frontier = deque([source])
        while frontier:
            node = frontier.popleft()
            for neighbor in self.neighbors(node, label):
                if neighbor in predecessor:
                    continue
                predecessor[neighbor] = node
                if neighbor == target:
                    # Walk predecessors back to the source then reverse.
                    path = [target]
                    while path[-1] != source:
                        path.append(predecessor[path[-1]])
                    return list(reversed(path))
                frontier.append(neighbor)
        return None

    def edges(self) -> list[tuple[int, int, Optional[str]]]:
        """Return every directed edge as a (from, to, label) triple.

        Used by the persistence layer to dump the graph. Bidirectional
        edges show up twice, once in each direction, which is what the
        graph actually stores.
        """
        out = []
        for node, neighbors in self._adj.items():
            for (neighbor, label) in neighbors:
                out.append((node, neighbor, label))
        return out
