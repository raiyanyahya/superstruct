"""Abstract base class for every sub-structure.

A concrete Index has two jobs. One, declare which predicate kinds it
can answer so the planner knows when to route to it. Two, actually
answer those predicates by returning the set of matching record ids.

Indexes are constructed lazily. The first query that needs a given
index triggers a one shot bulk build from the primary store via
build_from_records. Once built, the planner keeps the index in step
with subsequent inserts and deletes by calling insert and remove.
"""
from abc import ABC, abstractmethod
from typing import Iterable

from ..primary import Record
from ..query import Predicate, PredicateKind


class Index(ABC):
    """Common interface every sub-structure must implement.

    Each subclass advertises which predicate kinds it can answer via
    the supported_kinds class attribute. The attribute argument names
    the record field this index is built on. A SortedIndex on "age"
    indexes the value of the age key in each record.
    """

    # Subclasses override this. Tells the planner which predicate kinds
    # this index supports. For example a TrieIndex sets PREFIX.
    supported_kinds: set[PredicateKind] = set()

    def __init__(self, attribute: str):
        # The record attribute this index is built on. Stored for the
        # planner so it can match predicates by attribute name.
        self.attribute = attribute

    @abstractmethod
    def build_from_records(self, records: Iterable[Record]) -> None:
        """One shot bulk build from the primary store.

        Called exactly once when the index is first materialized. Should
        be the cheapest path possible, typically a single sort or a
        single linear pass.
        """

    @abstractmethod
    def insert(self, record: Record) -> None:
        """Incremental insert. Called after the index has been built.

        If the record does not have the indexed attribute the index
        should silently skip. Records without the attribute are not
        a schema error in this design.
        """

    @abstractmethod
    def remove(self, record: Record) -> None:
        """Incremental remove. Called when a record is deleted."""

    @abstractmethod
    def execute(self, predicate: Predicate) -> set[int]:
        """Run a single predicate. Return the set of matching record ids.

        The returned set is owned by the caller. Implementations should
        return a fresh copy because the planner intersects sets in place.
        """

    @abstractmethod
    def memory_estimate_bytes(self) -> int:
        """Rough memory footprint.

        The planner uses this to decide which index to evict when the
        memory budget is exceeded. Cheap approximations are fine. The
        planner does not depend on the number being exact.
        """

    def can_answer(self, predicate: Predicate) -> bool:
        """True when this index is a valid match for this predicate.

        The planner walks every materialized index calling can_answer.
        The first match wins. If none match the planner builds a fresh
        index based on the predicate kind.
        """
        return (
            predicate.kind in self.supported_kinds
            and predicate.attribute == self.attribute
        )
