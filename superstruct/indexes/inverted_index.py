"""Inverted index. Word level full text search on string attributes.

For every record we tokenize its string value into lower case alpha
numeric words. We map each word to the set of record ids whose value
contains that word. CONTAINS predicates do a single dictionary lookup.

Tokenization is intentionally simple. We split on any non alphanumeric
character and lower case. Real full text engines do stemming, stop
word removal and language aware tokenization, all of which can swap in
here later without touching the rest of the system.
"""
import re
from collections import defaultdict
from sys import getsizeof
from typing import Iterable

from ..primary import Record
from ..query import Predicate, PredicateKind
from .base import Index


# Token splitter. Anything that is not a letter or digit splits.
_TOKEN_RE = re.compile(r"[a-zA-Z0-9]+")


def _tokenize(value: str) -> list[str]:
    """Split a string into lower case alphanumeric tokens."""
    return [m.group(0).lower() for m in _TOKEN_RE.finditer(value)]


class InvertedIndex(Index):
    """Word level full text index. The classic search engine posting list."""

    supported_kinds = {PredicateKind.CONTAINS}

    def __init__(self, attribute: str):
        super().__init__(attribute)

        # word to set of record ids that mention the word at least once.
        # Defaultdict simplifies the insert path.
        self._postings: dict[str, set[int]] = defaultdict(set)

    def build_from_records(self, records: Iterable[Record]) -> None:
        for record in records:
            self.insert(record)

    def insert(self, record: Record) -> None:
        value = record.attrs.get(self.attribute)
        # Only string valued attributes are indexable here. Other types
        # are silently skipped so a heterogenous schema is still safe.
        if not isinstance(value, str):
            return
        for token in _tokenize(value):
            self._postings[token].add(record.id)

    def remove(self, record: Record) -> None:
        value = record.attrs.get(self.attribute)
        if not isinstance(value, str):
            return
        for token in _tokenize(value):
            posting = self._postings.get(token)
            if posting is None:
                continue
            posting.discard(record.id)
            # Drop empty postings to keep the dictionary lean.
            if not posting:
                del self._postings[token]

    def execute(self, predicate: Predicate) -> set[int]:
        # Single word lookup. Lower case the query so it matches the
        # token store. Multi word queries can be expressed as multiple
        # CONTAINS predicates ANDed together at the planner level.
        if not isinstance(predicate.value, str):
            return set()
        word = predicate.value.lower()
        return set(self._postings.get(word, set()))

    def memory_estimate_bytes(self) -> int:
        total = getsizeof(self._postings)
        for k, v in self._postings.items():
            total += getsizeof(k) + getsizeof(v)
        return total
