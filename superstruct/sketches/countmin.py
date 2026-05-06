"""Count min sketch.

A two dimensional array of counters with d rows and w columns. To
increment a value we hash it d ways with one hash per row then bump
the counter at row r column hash_r(value) mod w. To estimate the count
we read all d counters and take the minimum. The minimum is an over
estimate of the true count and never an under estimate.
"""
import hashlib
from typing import Any


class CountMinSketch:
    """Approximate frequency sketch.

    Memory is fixed at construction time. Errors scale with width and
    depth. The defaults give roughly two percent error for tens of
    thousands of insertions in a few kilobytes.
    """

    def __init__(self, width: int = 1024, depth: int = 5):
        self._width = width
        self._depth = depth

        # depth rows of width counters. Plain python lists are easier
        # to read than a flat bytearray and the speed cost is fine for
        # a research demo.
        self._table: list[list[int]] = [
            [0] * width for _ in range(depth)
        ]

    def _positions(self, value: Any) -> list[int]:
        """Compute the column index for each row.

        We salt the encoded value with the row index so a single md5
        call gives us d different hash positions. A real implementation
        would use d pairwise independent hash families. This is good
        enough for demonstration.
        """
        encoded = repr(value).encode("utf-8")
        positions = []
        for row in range(self._depth):
            salted = encoded + bytes([row])
            digest = hashlib.md5(salted).digest()
            n = int.from_bytes(digest[:4], "big")
            positions.append(n % self._width)
        return positions

    def add(self, value: Any) -> None:
        """Record one occurrence of value."""
        for row, col in enumerate(self._positions(value)):
            self._table[row][col] += 1

    def estimate(self, value: Any) -> int:
        """Return the estimated count of how many times value was added.

        The minimum across all d rows is an over estimate of the true
        count and a tight one when collisions are sparse. Returns zero
        when the value was definitely never added.
        """
        return min(
            self._table[row][col]
            for row, col in enumerate(self._positions(value))
        )
