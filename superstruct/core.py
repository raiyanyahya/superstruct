"""Public Superstruct facade. The class users actually instantiate.

Holds every piece of the system and exposes a single front desk API.

Pieces.
    Primary store. The canonical record table.
    Workload tracker. Per index hit counts and recency.
    Planner. Lazy index construction and query routing.
    Sketches. Always on Bloom and CountMin per attribute.
    Graph. Optional relationship layer between records.
    Lock. RLock that serializes mutations and queries.

The user inserts records and runs queries. They never declare an index.
They optionally call sketch helpers, save and load to disk and add
edges between records. The structure observes incoming queries and
lazily builds whatever indexes pay for themselves, evicting cold ones
when memory pressure rises.
"""
import json
import threading
from typing import Any, Optional

from .primary import PrimaryStore
from .query import Query, Predicate, PredicateKind, And, Or, Not, TopK, Node
from .planner import Planner
from .workload import WorkloadTracker
from .sketches import BloomSketch, CountMinSketch
from .graph import GraphStore


class Superstruct:
    """The all in one adaptive in-memory data structure.

    Example:

        ss = Superstruct()
        ss.insert({"name": "Alice", "age": 30, "score": 88, "bio": "loves cats"})
        results = (
            ss.find()
              .range("age", 20, 40)
              .prefix("name", "A")
              .contains("bio", "cats")
              .top_k("score", 10)
              .execute()
        )
    """

    # Sensible default for a research demo. The user can pass their own
    # budget into the constructor or change it later via set_memory_budget.
    DEFAULT_MEMORY_BUDGET_BYTES = 64 * 1024 * 1024  # 64 MiB

    def __init__(
        self,
        memory_budget_bytes: Optional[int] = None,
        thread_safe: bool = True,
    ):
        # Core pieces. Primary store holds the canonical data. Workload
        # tracker gathers usage stats. Planner runs the show.
        self._primary = PrimaryStore()
        self._workload = WorkloadTracker()
        # Explicit None check rather than `or` so that a budget of zero
        # is honored. With `or` a zero would fall through to the default.
        if memory_budget_bytes is None:
            memory_budget_bytes = self.DEFAULT_MEMORY_BUDGET_BYTES
        self._planner = Planner(
            self._primary,
            self._workload,
            memory_budget_bytes,
        )

        # Auto attached sketches. One bloom and one count min per
        # attribute, created lazily the first time we see an attribute
        # on any inserted record. They are never evicted.
        self._blooms: dict[str, BloomSketch] = {}
        self._counts: dict[str, CountMinSketch] = {}

        # Side car relationship layer. Edges reference primary record
        # ids. When a record is deleted the graph drops every edge
        # that touches it.
        self._graph = GraphStore()

        # Concurrency. RLock so a method that calls another method on
        # the same instance does not deadlock. The user can disable
        # locking via the constructor for single threaded workloads
        # that want the absolute lowest overhead.
        if thread_safe:
            self._lock = threading.RLock()
        else:
            # Sentinel context manager that does nothing. Lets the
            # method bodies use the same with self._lock pattern in
            # both modes.
            self._lock = _NullLock()

    # -----------------------------------------------------------------
    # Mutation API
    # -----------------------------------------------------------------

    def insert(self, attrs: dict[str, Any]) -> int:
        """Add a record. Returns the assigned id.

        The record is stored synchronously in the primary store, pushed
        into every currently materialized index and registered into
        the always on sketches for each of its attributes. Indexes
        that have not been built yet will pick the record up the next
        time they are built.
        """
        with self._lock:
            record = self._primary.insert(attrs)
            self._planner.on_insert(record)
            self._update_sketches(record.attrs)
            return record.id

    def delete(self, record_id: int) -> bool:
        """Remove a record by id. Returns True if the record existed.

        Sketches are intentionally not updated on delete. Bloom filters
        cannot subtract. The slowly rising false positive rate over time
        is the documented tradeoff.
        """
        with self._lock:
            record = self._primary.delete(record_id)
            if record is None:
                return False
            self._planner.on_delete(record)
            # Drop any graph edges that touch this node so the graph
            # stays consistent with the primary store.
            self._graph.remove_node(record_id)
            return True

    def _update_sketches(self, attrs: dict[str, Any]) -> None:
        """Auto attach a bloom and count min sketch per attribute then
        record the value in both. Cheap because sketches are tiny."""
        for attr, value in attrs.items():
            bloom = self._blooms.get(attr)
            if bloom is None:
                bloom = BloomSketch()
                self._blooms[attr] = bloom
            bloom.add(value)

            cm = self._counts.get(attr)
            if cm is None:
                cm = CountMinSketch()
                self._counts[attr] = cm
            cm.add(value)

    # -----------------------------------------------------------------
    # Query API
    # -----------------------------------------------------------------

    def get(self, record_id: int) -> Optional[dict[str, Any]]:
        """Primary key lookup. Always synchronous, always exact."""
        with self._lock:
            record = self._primary.get(record_id)
            return None if record is None else record.attrs

    def find(self) -> "QueryBuilder":
        """Start a chained query. Returns a fresh QueryBuilder."""
        return QueryBuilder(self)

    def execute(self, query: Query) -> list[dict[str, Any]]:
        """Run a Query directly. Most users will use find() instead.

        Returns a list of attribute dicts. We strip the internal Record
        wrapper so callers do not see the auto assigned id unless they
        need it.
        """
        with self._lock:
            records = self._planner.execute(query)
            return [r.attrs for r in records]

    # -----------------------------------------------------------------
    # Sketch helpers
    # -----------------------------------------------------------------

    def maybe_contains(self, attribute: str, value: Any) -> bool:
        """Bloom backed probable membership check.

        Returns False if the value was definitely never inserted on
        this attribute. Returns True if the value was probably inserted
        with a small false positive rate. Microsecond level fast.
        """
        with self._lock:
            bloom = self._blooms.get(attribute)
            if bloom is None:
                return False
            return bloom.maybe_contains(value)

    def estimate_count(self, attribute: str, value: Any) -> int:
        """Count min backed approximate frequency.

        Returns the estimated number of times the value has been
        inserted on this attribute. Always an over estimate of the
        true count, never an under estimate. Returns zero when the
        attribute has never been seen.
        """
        with self._lock:
            cm = self._counts.get(attribute)
            if cm is None:
                return 0
            return cm.estimate(value)

    # -----------------------------------------------------------------
    # Graph helpers. Thin pass through to the GraphStore.
    # -----------------------------------------------------------------

    def add_edge(
        self,
        a: int,
        b: int,
        label: Optional[str] = None,
        directed: bool = False,
    ) -> None:
        """Add an edge between two record ids."""
        with self._lock:
            self._graph.add_edge(a, b, label=label, directed=directed)

    def remove_edge(
        self,
        a: int,
        b: int,
        label: Optional[str] = None,
        directed: bool = False,
    ) -> None:
        with self._lock:
            self._graph.remove_edge(a, b, label=label, directed=directed)

    def neighbors(
        self, record_id: int, label: Optional[str] = None
    ) -> set[int]:
        """Immediate neighbors of a record."""
        with self._lock:
            return self._graph.neighbors(record_id, label=label)

    def bfs(
        self,
        start: int,
        max_depth: Optional[int] = None,
        label: Optional[str] = None,
    ) -> dict[int, int]:
        """Breadth first search returning depth for every reached node."""
        with self._lock:
            return self._graph.bfs(start, max_depth=max_depth, label=label)

    def shortest_path(
        self,
        source: int,
        target: int,
        label: Optional[str] = None,
    ) -> Optional[list[int]]:
        """Shortest path between two records by hop count."""
        with self._lock:
            return self._graph.shortest_path(source, target, label=label)

    # -----------------------------------------------------------------
    # Memory budget controls
    # -----------------------------------------------------------------

    def set_memory_budget(self, bytes_limit: int) -> None:
        """Change the memory budget. Forces an immediate eviction pass
        if the new budget is smaller than the current footprint."""
        with self._lock:
            self._planner.set_memory_budget(bytes_limit)

    # -----------------------------------------------------------------
    # Persistence. Save and load the canonical state to JSON.
    # -----------------------------------------------------------------

    def save(self, path: str) -> None:
        """Write primary records and graph edges to a JSON file.

        Indexes are not persisted. They rebuild lazily on first query
        after load. Sketches are not persisted either since reinserting
        every record on load reconstructs them faithfully.
        """
        with self._lock:
            payload = {
                # Format version so future changes to the on disk shape
                # can be detected and migrated.
                "version": 1,
                "next_id": self._primary._next_id,
                "records": [
                    {"id": r.id, "attrs": r.attrs}
                    for r in self._primary
                ],
                "edges": [
                    {"from": a, "to": b, "label": label}
                    for (a, b, label) in self._graph.edges()
                ],
            }
            with open(path, "w", encoding="utf-8") as f:
                json.dump(payload, f, indent=2)

    @classmethod
    def load(
        cls,
        path: str,
        memory_budget_bytes: Optional[int] = None,
        thread_safe: bool = True,
    ) -> "Superstruct":
        """Read a JSON file written by save and return a fresh instance.

        We rehydrate by replaying every record through the normal
        insert path and every edge through add_edge. This means
        sketches and any later built index see consistent state.
        """
        with open(path, "r", encoding="utf-8") as f:
            payload = json.load(f)

        ss = cls(
            memory_budget_bytes=memory_budget_bytes,
            thread_safe=thread_safe,
        )

        # Replay records. We bypass the auto assigned id so that ids
        # in the loaded file are preserved. This matters for the graph
        # edges which reference those ids.
        for entry in payload["records"]:
            ss._restore_record(entry["id"], entry["attrs"])

        # Restore the next id counter so future inserts do not collide
        # with restored ids.
        ss._primary._next_id = payload["next_id"]

        # Replay edges. The graph stores both directions for non
        # directed edges. We pass directed=True here so a saved pair
        # of forward and reverse edges does not double up.
        for edge in payload["edges"]:
            ss._graph.add_edge(
                edge["from"],
                edge["to"],
                label=edge["label"],
                directed=True,
            )

        return ss

    def _restore_record(self, record_id: int, attrs: dict[str, Any]) -> None:
        """Insert a record at a specific id during load.

        Bypasses the primary store's auto increment counter so that
        record ids match the saved file exactly. Updates sketches and
        any currently materialized indexes the same way insert does.
        """
        # Build the Record manually then drop it into the primary store.
        from .primary import Record  # Local import avoids a cycle.
        record = Record(id=record_id, attrs=dict(attrs))
        self._primary._records[record_id] = record
        self._planner.on_insert(record)
        self._update_sketches(record.attrs)

    # -----------------------------------------------------------------
    # Introspection. Useful for the research demo.
    # -----------------------------------------------------------------

    def index_inventory(self) -> list[tuple[str, str, int]]:
        """Currently materialized indexes."""
        with self._lock:
            return self._planner.index_inventory()

    def __len__(self) -> int:
        with self._lock:
            return len(self._primary)


class QueryBuilder:
    """Chainable query construction.

    Each method returns self so calls can be strung together. execute
    builds a Query from the accumulated tree and hands it to the
    Superstruct planner.

    The top level of the builder is an implicit AND of every predicate
    added via equals, range, prefix, contains and fuzzy. The any_of
    method introduces an OR group built from sub builders. The exclude
    method introduces a NOT around a sub builder.
    """

    def __init__(self, owner: Superstruct):
        self._owner = owner

        # Implicit AND. Each entry can be a Predicate leaf, an Or node
        # or a Not node. The execute path wraps them in an And if the
        # list contains more than one entry.
        self._and_children: list = []

        self._top_k: Optional[TopK] = None

    # -----------------------------------------------------------------
    # Leaf predicates
    # -----------------------------------------------------------------

    def equals(self, attribute: str, value: Any) -> "QueryBuilder":
        """Match records where attribute equals value."""
        self._and_children.append(
            Predicate(PredicateKind.EQUALS, attribute, value)
        )
        return self

    def range(self, attribute: str, low: Any, high: Any) -> "QueryBuilder":
        """Match records where low <= attribute <= high."""
        self._and_children.append(
            Predicate(PredicateKind.RANGE, attribute, (low, high))
        )
        return self

    def prefix(self, attribute: str, prefix: str) -> "QueryBuilder":
        """Match records where attribute starts with prefix."""
        self._and_children.append(
            Predicate(PredicateKind.PREFIX, attribute, prefix)
        )
        return self

    def contains(self, attribute: str, word: str) -> "QueryBuilder":
        """Match records whose attribute text contains word.

        Tokenization is lower case and alphanumeric only so a word
        like "Cat!" inserts as the token cat which is what this query
        will look for.
        """
        self._and_children.append(
            Predicate(PredicateKind.CONTAINS, attribute, word)
        )
        return self

    def fuzzy(
        self,
        attribute: str,
        value: str,
        threshold: float = 0.5,
    ) -> "QueryBuilder":
        """Match records whose attribute is approximately equal to value.

        Uses trigram Jaccard similarity. Threshold is the minimum
        similarity score in the unit interval. 0.5 is permissive,
        0.7 is fairly strict and 0.9 is near exact.
        """
        self._and_children.append(
            Predicate(
                PredicateKind.FUZZY,
                attribute,
                value,
                threshold=threshold,
            )
        )
        return self

    # -----------------------------------------------------------------
    # Boolean composition
    # -----------------------------------------------------------------

    def any_of(self, *builders: "QueryBuilder") -> "QueryBuilder":
        """Add an OR group containing the supplied sub builders.

        Each sub builder is converted to its tree node and the whole
        OR is appended to the implicit top level AND. A record matches
        the group if it matches at least one sub builder.
        """
        nodes = [b._to_node() for b in builders]
        # Filter out empty sub builders so the resulting Or does not
        # contain stray None nodes that would behave strangely.
        nodes = [n for n in nodes if n is not None]
        self._and_children.append(Or(children=nodes))
        return self

    def exclude(self, builder: "QueryBuilder") -> "QueryBuilder":
        """Exclude records matching the supplied sub builder.

        The sub builder is converted to a tree node, wrapped in Not
        and appended to the implicit top level AND. A record passes
        only if it does not match the sub builder.
        """
        node = builder._to_node()
        self._and_children.append(Not(child=node))
        return self

    def top_k(
        self, attribute: str, k: int, descending: bool = True
    ) -> "QueryBuilder":
        """Final ordering step. Sort by attribute and take k items.

        Defaults to descending so top_k("score", 10) returns the ten
        highest scores. Pass descending=False for the lowest.
        """
        self._top_k = TopK(attribute=attribute, k=k, descending=descending)
        return self

    # -----------------------------------------------------------------
    # Execution
    # -----------------------------------------------------------------

    def _to_node(self) -> Optional[Node]:
        """Collapse the implicit AND list into a single AST node.

        Returns None if no predicates have been added. Returns the
        sole child as is if exactly one has been added. Wraps in And
        otherwise.
        """
        if not self._and_children:
            return None
        if len(self._and_children) == 1:
            return self._and_children[0]
        return And(children=list(self._and_children))

    def execute(self) -> list[dict[str, Any]]:
        """Run the accumulated query."""
        query = Query(where=self._to_node(), top_k=self._top_k)
        return self._owner.execute(query)


class _NullLock:
    """Stand in for a real lock when the user opts out of thread safety.

    Implements just the context manager protocol so the with self._lock
    pattern works either way without branching in every method.
    """

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False
