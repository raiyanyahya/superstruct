"""Hash index. Equality lookups in O(1).

Maps an attribute value to the set of record ids that hold that value.
Many records can share a value so the buckets are sets. Constructed
in O(n) by walking the primary store once.
"""
from collections import defaultdict
from sys import getsizeof
from typing import Iterable

from ..primary import Record
from ..query import Predicate, PredicateKind
from .base import Index


class HashIndex(Index):
    """Equality lookup on a single attribute. The classical hash map."""

    # We only answer equality. Range and prefix queries route to other
    # index types even when an attribute also has a hash index.
    supported_kinds = {PredicateKind.EQUALS}

    def __init__(self, attribute: str):
        super().__init__(attribute)

        # value to set of ids. Multiple records can share the same value
        # so each bucket is a set rather than a single id.
        self._buckets: dict = defaultdict(set)

    def build_from_records(self, records: Iterable[Record]) -> None:
        # Single pass over the primary store. Each record drops into its
        # value bucket. O(n) total.
        for record in records:
            self.insert(record)

    def insert(self, record: Record) -> None:
        # Records are not required to share a schema. If this record
        # does not have the indexed attribute we simply skip it.
        if self.attribute not in record.attrs:
            return
        value = record.attrs[self.attribute]
        # Hashable values only. If the user inserted a list or dict the
        # TypeError will surface here so they know to fix their data.
        self._buckets[value].add(record.id)

    def remove(self, record: Record) -> None:
        if self.attribute not in record.attrs:
            return
        value = record.attrs[self.attribute]
        bucket = self._buckets.get(value)
        if bucket is None:
            return
        bucket.discard(record.id)
        # Empty bucket. Drop the key so the index does not slowly leak
        # memory as records churn through values that briefly existed.
        if not bucket:
            del self._buckets[value]

    def execute(self, predicate: Predicate) -> set[int]:
        # Return a fresh copy because callers will mutate the result
        # during intersection with other predicate results.
        return set(self._buckets.get(predicate.value, set()))

    def memory_estimate_bytes(self) -> int:
        # Sum the dict overhead plus every key and bucket. Approximate
        # since python sets contain pointers we are not following.
        total = getsizeof(self._buckets)
        for key, bucket in self._buckets.items():
            total += getsizeof(key) + getsizeof(bucket)
        return total
