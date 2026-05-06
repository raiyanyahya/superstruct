"""Probabilistic sketches. Tiny always on data structures.

Sketches answer questions that the heavy indexes either cannot answer
cheaply or do not answer at all. A bloom filter answers "is this value
probably absent" in microseconds with no false negatives. A count min
sketch answers "approximately how many times has this value been seen"
in similarly small memory.

In Superstruct sketches are auto attached to every attribute as records
are inserted. They never require explicit construction, never get
evicted under memory pressure and cost on the order of a few kilobytes
per attribute.
"""
from .bloom import BloomSketch
from .countmin import CountMinSketch

__all__ = ["BloomSketch", "CountMinSketch"]
