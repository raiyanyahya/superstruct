"""N gram index. Approximate string matching by trigram overlap.

For every string value we extract its set of trigrams, that is every
contiguous three character substring after lower casing and padding.
Each trigram maps to the set of record ids whose value contains the
trigram. To answer a fuzzy query we trigram the target string then
compute Jaccard similarity between target trigrams and each candidate
record's trigrams. Records above the predicate threshold are returned.

This trades precision for recall. It cheaply finds candidates that are
plausibly similar then scores them. The threshold parameter on the
predicate controls how strict the match has to be. A threshold of 0.5
is permissive, 0.7 is fairly strict and 0.9 is near exact.
"""
from collections import defaultdict
from sys import getsizeof
from typing import Iterable

from ..primary import Record
from ..query import Predicate, PredicateKind
from .base import Index


def _trigrams(value: str) -> set[str]:
    """Return the set of trigrams of value.

    Pads both ends of the string with spaces so even short strings
    have at least one trigram and so the matching rewards aligning
    beginnings and endings of words.
    """
    padded = "  " + value.lower() + "  "
    return {padded[i:i + 3] for i in range(len(padded) - 2)}


class NgramIndex(Index):
    """Trigram index for fuzzy string matching."""

    supported_kinds = {PredicateKind.FUZZY}

    def __init__(self, attribute: str):
        super().__init__(attribute)

        # trigram to set of record ids whose value contains it.
        self._postings: dict[str, set[int]] = defaultdict(set)

        # Per record trigram set. We keep this so we can compute the
        # exact Jaccard similarity at query time without having to walk
        # the primary store again. Doubles the memory cost but keeps
        # queries cheap.
        self._record_trigrams: dict[int, set[str]] = {}

    def build_from_records(self, records: Iterable[Record]) -> None:
        for record in records:
            self.insert(record)

    def insert(self, record: Record) -> None:
        value = record.attrs.get(self.attribute)
        if not isinstance(value, str):
            return
        grams = _trigrams(value)
        self._record_trigrams[record.id] = grams
        for g in grams:
            self._postings[g].add(record.id)

    def remove(self, record: Record) -> None:
        grams = self._record_trigrams.pop(record.id, None)
        if grams is None:
            return
        for g in grams:
            posting = self._postings.get(g)
            if posting is None:
                continue
            posting.discard(record.id)
            if not posting:
                del self._postings[g]

    def execute(self, predicate: Predicate) -> set[int]:
        target = predicate.value
        if not isinstance(target, str):
            return set()
        target_grams = _trigrams(target)

        # Step one. Find candidate records that share at least one
        # trigram with the target. Pure unions across all target
        # trigrams. This is the "broad net" phase.
        candidates: set[int] = set()
        for g in target_grams:
            candidates.update(self._postings.get(g, set()))

        # Step two. Score each candidate by Jaccard similarity against
        # the target trigrams. Records above the threshold are kept.
        threshold = predicate.threshold
        out: set[int] = set()
        for rid in candidates:
            rec_grams = self._record_trigrams.get(rid)
            if rec_grams is None:
                continue
            overlap = len(target_grams & rec_grams)
            union_size = len(target_grams | rec_grams)
            if union_size == 0:
                continue
            similarity = overlap / union_size
            if similarity >= threshold:
                out.add(rid)
        return out

    def memory_estimate_bytes(self) -> int:
        total = getsizeof(self._postings)
        for k, v in self._postings.items():
            total += getsizeof(k) + getsizeof(v)
        total += getsizeof(self._record_trigrams)
        for v in self._record_trigrams.values():
            total += getsizeof(v)
        return total
