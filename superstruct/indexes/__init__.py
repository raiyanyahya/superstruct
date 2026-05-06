"""Concrete index implementations.

Each module here implements one classical data structure as an Index
subclass. The planner picks among them when it needs to materialize an
index for a freshly seen predicate kind. Adding a new index type means
dropping a new module here and registering its predicate kinds in the
planner default map.
"""
from .base import Index
from .hash_index import HashIndex
from .sorted_index import SortedIndex
from .trie_index import TrieIndex
from .inverted_index import InvertedIndex
from .ngram_index import NgramIndex

__all__ = [
    "Index",
    "HashIndex",
    "SortedIndex",
    "TrieIndex",
    "InvertedIndex",
    "NgramIndex",
]
