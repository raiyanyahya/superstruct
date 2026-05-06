"""Sorted index. Range and equality lookups on a single attribute.

Internally two parallel arrays. _values holds the attribute values in
sorted order. _ids holds the corresponding record ids at the same
positions. Range queries use bisect to find the matching slice in
O(log n) then read off the ids. Bulk build is O(n log n). Incremental
inserts are O(n) in the worst case because of list shifting which is
acceptable for a research prototype.

We could swap the parallel arrays for a balanced BST or a B-tree later
if write throughput becomes a concern. The Index interface stays the
same.
"""
import bisect
from sys import getsizeof
from typing import Iterable

from ..primary import Record
from ..query import Predicate, PredicateKind
from .base import Index


class SortedIndex(Index):
    """Range and equality lookups using sorted parallel arrays."""

    # Sorted indexes can satisfy equality too. The planner will normally
    # prefer a hash index for pure equality, but if only a sorted index
    # exists for an attribute we use it rather than building a second.
    supported_kinds = {PredicateKind.RANGE, PredicateKind.EQUALS}

    def __init__(self, attribute: str):
        super().__init__(attribute)

        # Sorted in step with each other. _values[i] is the indexed
        # value of the record whose id is _ids[i]. Duplicate values are
        # allowed and form a contiguous run in the array.
        self._values: list = []
        self._ids: list[int] = []

    def build_from_records(self, records: Iterable[Record]) -> None:
        # Collect every (value, id) pair then sort once. A single sort
        # is cheaper than n bisect insorts, which would shift the list
        # n times.
        pairs = []
        for record in records:
            if self.attribute in record.attrs:
                pairs.append((record.attrs[self.attribute], record.id))
        pairs.sort(key=lambda p: p[0])

        # Split into parallel arrays. Two list comprehensions are simple
        # and the cost is negligible compared to the sort.
        self._values = [p[0] for p in pairs]
        self._ids = [p[1] for p in pairs]

    def insert(self, record: Record) -> None:
        if self.attribute not in record.attrs:
            return
        value = record.attrs[self.attribute]

        # bisect_left places the new value at the leftmost legal spot
        # for its sort order. We then splice both parallel arrays at
        # the same position so they stay in step.
        pos = bisect.bisect_left(self._values, value)
        self._values.insert(pos, value)
        self._ids.insert(pos, record.id)

    def remove(self, record: Record) -> None:
        if self.attribute not in record.attrs:
            return
        value = record.attrs[self.attribute]

        # Find the run of equal values then locate the exact id within
        # that run. There may be duplicates so a single bisect would
        # not be enough.
        lo = bisect.bisect_left(self._values, value)
        hi = bisect.bisect_right(self._values, value)
        for i in range(lo, hi):
            if self._ids[i] == record.id:
                del self._values[i]
                del self._ids[i]
                return

    def execute(self, predicate: Predicate) -> set[int]:
        if predicate.kind is PredicateKind.RANGE:
            low, high = predicate.value
            # Closed interval on both ends. bisect_left for the lower
            # bound and bisect_right for the upper bound gives every
            # value v with low <= v <= high.
            left = bisect.bisect_left(self._values, low)
            right = bisect.bisect_right(self._values, high)
            return set(self._ids[left:right])

        if predicate.kind is PredicateKind.EQUALS:
            value = predicate.value
            left = bisect.bisect_left(self._values, value)
            right = bisect.bisect_right(self._values, value)
            return set(self._ids[left:right])

        # Should not reach here because can_answer would have rejected
        # the predicate. Returning an empty set is the safe default.
        return set()

    def memory_estimate_bytes(self) -> int:
        # Two parallel python lists. We are not following the pointers
        # to the values themselves because the planner only needs a
        # rough comparison between indexes.
        return getsizeof(self._values) + getsizeof(self._ids)
