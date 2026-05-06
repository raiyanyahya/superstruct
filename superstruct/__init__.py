"""Superstruct. Adaptive in-memory polyindex.

The user instantiates Superstruct, inserts records and runs queries.
They never declare an index. The structure observes the workload and
lazily builds whatever sub-indexes pay for themselves, evicting the
cold ones when memory pressure rises.

The package also ships a graph layer for relationship queries and two
always on probabilistic sketches per attribute for very fast
membership and frequency checks.
"""
from .core import Superstruct, QueryBuilder
from .query import Query, Predicate, PredicateKind, TopK, And, Or, Not
from .primary import Record
from .sketches import BloomSketch, CountMinSketch
from .graph import GraphStore

__all__ = [
    "Superstruct",
    "QueryBuilder",
    "Query",
    "Predicate",
    "PredicateKind",
    "TopK",
    "And",
    "Or",
    "Not",
    "Record",
    "BloomSketch",
    "CountMinSketch",
    "GraphStore",
]
