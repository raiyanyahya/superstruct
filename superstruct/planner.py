"""Query planner. The brain of Superstruct.

Three responsibilities sit in this module.

One. Lazy index construction. When a predicate arrives and no built
index can answer it the planner picks the right index type, builds it
from the primary store and registers it.

Two. Cross-structure query decomposition. A query tree of And, Or, Not
and Predicate leaves gets walked. Each leaf is routed to whichever
index can answer it. Inner nodes combine the resulting id sets with
intersection, union or set difference.

Three. Memory budget enforcement. After every build the planner sums
estimated memory across all materialized indexes. If the total exceeds
the budget it evicts the lowest scoring indexes until the total fits.
The workload tracker provides the scores.
"""
import time

from .primary import PrimaryStore, Record
from .query import (
    Query,
    Predicate,
    PredicateKind,
    And,
    Or,
    Not,
    Node,
)
from .indexes.base import Index
from .indexes.hash_index import HashIndex
from .indexes.sorted_index import SortedIndex
from .indexes.trie_index import TrieIndex
from .indexes.inverted_index import InvertedIndex
from .indexes.ngram_index import NgramIndex
from .workload import WorkloadTracker


# Default mapping from predicate kind to the index type best suited to
# answer it. The planner consults this when no built index exists yet
# and it needs to materialize one. EQUALS goes to hash because hash is
# strictly faster than scanning a sorted list. RANGE goes to sorted.
# PREFIX goes to trie. CONTAINS goes to a word level inverted index.
# FUZZY goes to a trigram n gram index.
_DEFAULT_INDEX_FOR_KIND: dict = {
    PredicateKind.EQUALS: HashIndex,
    PredicateKind.RANGE: SortedIndex,
    PredicateKind.PREFIX: TrieIndex,
    PredicateKind.CONTAINS: InvertedIndex,
    PredicateKind.FUZZY: NgramIndex,
}


class Planner:
    """Routes queries to indexes, builds indexes lazily, evicts when full."""

    def __init__(
        self,
        primary: PrimaryStore,
        workload: WorkloadTracker,
        memory_budget_bytes: int,
    ):
        self._primary = primary
        self._workload = workload
        self._memory_budget_bytes = memory_budget_bytes

        # Materialized indexes keyed by (index type name, attribute).
        # This shape means at most one trie per attribute and one
        # sorted index per attribute, which is the right invariant.
        self._indexes: dict[tuple[str, str], Index] = {}

    # -----------------------------------------------------------------
    # Configuration
    # -----------------------------------------------------------------

    def set_memory_budget(self, bytes_limit: int) -> None:
        """Change the memory budget. Triggers an eviction pass right
        away if the current footprint exceeds the new limit."""
        self._memory_budget_bytes = bytes_limit
        self._enforce_memory_budget()

    # -----------------------------------------------------------------
    # Lazy index construction
    # -----------------------------------------------------------------

    def _ensure_index_for(self, predicate: Predicate) -> Index | None:
        """Return an index that can answer this predicate.

        Builds a new index on demand if no existing one can satisfy
        the predicate. Returns None if no index type knows how to
        answer this predicate kind, in which case the caller falls back
        to a primary store scan.
        """
        # Look for an existing materialized index that can answer this
        # predicate. We loop because several index types may be valid
        # and any one of them will do.
        for idx in self._indexes.values():
            if idx.can_answer(predicate):
                return idx

        # No matching index. Pick the default type for this predicate
        # kind. If we have no registered type we return None and let
        # the caller scan the primary store.
        cls = _DEFAULT_INDEX_FOR_KIND.get(predicate.kind)
        if cls is None:
            return None

        # Build and register. We time the build so the workload tracker
        # can give the new index a small loyalty bonus that protects it
        # from immediate eviction right after construction.
        idx = cls(predicate.attribute)
        start = time.monotonic()
        idx.build_from_records(iter(self._primary))
        elapsed = time.monotonic() - start

        key = (cls.__name__, predicate.attribute)
        self._indexes[key] = idx
        self._workload.record_build(key, elapsed)

        # Memory pressure check. If this build pushed us over the budget
        # we drop the cheapest indexes by score until we fit again.
        self._enforce_memory_budget()
        return idx

    def _enforce_memory_budget(self):
        """Drop the lowest scoring indexes until total memory is back
        under the configured budget."""
        total = sum(i.memory_estimate_bytes() for i in self._indexes.values())
        if total <= self._memory_budget_bytes:
            return

        # Sort keys ascending by score. Lowest score evicts first. We
        # snapshot the keys list because we mutate the dict in the loop.
        keys_sorted = sorted(
            list(self._indexes.keys()),
            key=lambda k: self._workload.score(k),
        )
        for key in keys_sorted:
            if total <= self._memory_budget_bytes:
                return
            idx = self._indexes.pop(key)
            total -= idx.memory_estimate_bytes()
            # Forget the stats too. A future rebuild starts from cold
            # and earns its place again.
            self._workload.forget(key)

    # -----------------------------------------------------------------
    # Query execution. Cross-structure decomposition lives here.
    # -----------------------------------------------------------------

    def execute(self, query: Query) -> list[Record]:
        """Run a Query end to end. Returns matching records."""
        # Empty query means every record. Skip predicate routing.
        if query.where is None and query.top_k is None:
            return list(self._primary)

        # Resolve the candidate id set.
        if query.where is None:
            # No predicates but a top_k step exists. Universe is every
            # record in the primary store.
            candidate_ids = {r.id for r in self._primary}
        else:
            candidate_ids = self._evaluate_node(query.where)

        if query.top_k is not None:
            # Final ordering. We hydrate to records here so we can sort
            # by the requested attribute. Skip records that lack the
            # ordering attribute since None and numbers cannot compare.
            attr = query.top_k.attribute
            records = [self._primary.get(rid) for rid in candidate_ids]
            records = [
                r for r in records
                if r is not None and attr in r.attrs
            ]
            records.sort(
                key=lambda r: r.attrs[attr],
                reverse=query.top_k.descending,
            )
            return records[: query.top_k.k]

        # No top-k. Hydrate ids to records in stable id order so test
        # output is deterministic.
        records = [self._primary.get(rid) for rid in sorted(candidate_ids)]
        return [r for r in records if r is not None]

    def _evaluate_node(self, node: Node) -> set[int]:
        """Recursively walk the AST and return the set of matching ids.

        Predicate is the leaf case and dispatches to an index. And, Or
        and Not combine child results with intersection, union and
        complement against the universe of all primary record ids.
        """
        if isinstance(node, Predicate):
            return self._evaluate_predicate(node)
        if isinstance(node, And):
            return self._evaluate_and(node)
        if isinstance(node, Or):
            return self._evaluate_or(node)
        if isinstance(node, Not):
            return self._evaluate_not(node)

        # Defensive default. The Node type alias should keep this
        # branch unreachable in well formed queries.
        return set()

    def _evaluate_predicate(self, pred: Predicate) -> set[int]:
        """Route a single predicate to its index and return ids."""
        idx = self._ensure_index_for(pred)
        if idx is None:
            # No index type registered for this predicate kind. Fall
            # back to a primary store scan. Slow but always correct.
            return self._scan(pred)
        ids = idx.execute(pred)
        self._workload.record_hit((type(idx).__name__, idx.attribute))
        return ids

    def _evaluate_and(self, node: And) -> set[int]:
        """Intersect child id sets. Empty child list means every record."""
        if not node.children:
            return {r.id for r in self._primary}

        result: set[int] | None = None
        for child in node.children:
            child_ids = self._evaluate_node(child)
            if result is None:
                result = child_ids
            else:
                result &= child_ids
            # Empty intersection short circuits. The conjunctive AND
            # cannot grow back from empty.
            if not result:
                return set()
        return result or set()

    def _evaluate_or(self, node: Or) -> set[int]:
        """Union child id sets. Empty child list means no records."""
        out: set[int] = set()
        for child in node.children:
            out |= self._evaluate_node(child)
        return out

    def _evaluate_not(self, node: Not) -> set[int]:
        """Complement of the child against the universe of all records."""
        if node.child is None:
            # Empty NOT means nothing is excluded so every record matches.
            return {r.id for r in self._primary}
        universe = {r.id for r in self._primary}
        excluded = self._evaluate_node(node.child)
        return universe - excluded

    def _scan(self, predicate: Predicate) -> set[int]:
        """Fallback for predicates whose kind has no registered index."""
        out: set[int] = set()
        for r in self._primary:
            v = r.attrs.get(predicate.attribute)
            if v is None:
                continue
            if predicate.kind is PredicateKind.EQUALS and v == predicate.value:
                out.add(r.id)
        return out

    # -----------------------------------------------------------------
    # Maintenance
    # -----------------------------------------------------------------

    def on_insert(self, record: Record) -> None:
        """Tell every materialized index about a new record.

        Indexes that have not been built yet do not need notification.
        They will pick the record up the next time they are built.
        """
        for idx in self._indexes.values():
            idx.insert(record)

    def on_delete(self, record: Record) -> None:
        """Tell every materialized index about a removed record."""
        for idx in self._indexes.values():
            idx.remove(record)

    def index_inventory(self) -> list[tuple[str, str, int]]:
        """Snapshot of currently materialized indexes.

        One tuple per index containing the type name, the attribute
        and an estimated byte size. Useful for the demo and for any
        introspective UI.
        """
        return [
            (type(i).__name__, i.attribute, i.memory_estimate_bytes())
            for i in self._indexes.values()
        ]
