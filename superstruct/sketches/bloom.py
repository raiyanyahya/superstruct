"""Bloom filter sketch.

A bit array of m bits and k independent hash functions. To insert a
value we hash it k ways and set those bits. To check membership we
hash the same way and read those bits. If any bit is zero the value
was definitely never inserted. If all bits are one the value is
probably inserted with a tunable false positive rate.

We do not implement deletion. Standard bloom filters cannot delete
without tracking counters which would double the memory cost. The
false positive rate slowly rises as records churn through the sketch
over time. For a research demo this trade is fine.
"""
import hashlib
from typing import Any


class BloomSketch:
    """Probabilistic membership filter.

    Tiny memory footprint. False positives possible. False negatives
    never. Ideal as a fast gate in front of an expensive lookup.
    """

    def __init__(self, bit_size: int = 1 << 14, num_hashes: int = 5):
        # Default 16384 bits is 2 KB which is plenty for tens of
        # thousands of distinct values at a few percent false positive
        # rate. The user can dial this up for larger universes.
        self._bit_size = bit_size
        self._num_hashes = num_hashes

        # bytearray of bit_size / 8 bytes. All zeros initially.
        self._bits = bytearray(bit_size // 8)

    def _positions(self, value: Any) -> list[int]:
        """Compute the k bit positions for a given value.

        We hash repr(value) with md5 then chop the digest into k slices
        of four bytes each, taking each slice as a bit position modulo
        the filter size. Slow compared to a hand tuned bloom but works
        for any value the user might insert.
        """
        encoded = repr(value).encode("utf-8")
        digest = hashlib.md5(encoded).digest()
        # Double the digest so we can take overlapping windows when k
        # is larger than the digest can support naturally.
        doubled = digest + digest

        positions = []
        for i in range(self._num_hashes):
            start = (i * 4) % len(digest)
            chunk = doubled[start:start + 4]
            n = int.from_bytes(chunk, "big")
            positions.append(n % self._bit_size)
        return positions

    def add(self, value: Any) -> None:
        """Mark value as present in the filter."""
        for p in self._positions(value):
            self._bits[p // 8] |= (1 << (p % 8))

    def maybe_contains(self, value: Any) -> bool:
        """Return False if value was definitely never added.

        Return True when value was probably added with a small false
        positive rate. Never returns False on a value that was added.
        """
        for p in self._positions(value):
            if not (self._bits[p // 8] & (1 << (p % 8))):
                return False
        return True
