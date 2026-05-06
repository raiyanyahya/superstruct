"""Workload tracker. Drives the adaptive promotion and demotion logic.

Every time the planner serves a predicate from an index we record a
hit. Every time the planner builds an index for the first time we
record the build cost. The score function rolls these into a single
number that the planner uses to choose eviction victims when memory
pressure rises.

Higher score means more valuable to keep. Lowest score evicts first.
"""
import time
from dataclasses import dataclass


@dataclass
class IndexStats:
    """Per-index access statistics. Updated by the planner on every hit."""

    # Cumulative number of times the planner pulled an answer from this
    # index. Resets only when the index is evicted and rebuilt.
    hit_count: int = 0

    # Monotonic clock timestamp of the most recent hit. Used to age out
    # stale indexes that were popular long ago but no longer used.
    last_used_ts: float = 0.0

    # How long the initial bulk build took in seconds. We use this as a
    # small loyalty bonus so an expensive to build index does not get
    # immediately evicted right after a costly construction.
    build_cost_seconds: float = 0.0


class WorkloadTracker:
    """Holds IndexStats keyed by the same key the planner uses internally.

    The key is a (index_type_name, attribute) tuple. This means a hash
    index on age and a sorted index on age have separate statistics
    even though they share the attribute, which is the right granularity
    for the eviction policy.
    """

    def __init__(self):
        self._stats: dict[tuple[str, str], IndexStats] = {}

    def record_build(self, key: tuple[str, str], duration_seconds: float):
        """Remember how long it cost to build this index."""
        s = self._stats.setdefault(key, IndexStats())
        s.build_cost_seconds = duration_seconds

    def record_hit(self, key: tuple[str, str]):
        """Bump the hit count and freshen the timestamp on this index."""
        s = self._stats.setdefault(key, IndexStats())
        s.hit_count += 1
        s.last_used_ts = time.monotonic()

    def forget(self, key: tuple[str, str]):
        """Drop stats. Called when an index is evicted so a future
        rebuild starts from a clean slate."""
        self._stats.pop(key, None)

    def score(self, key: tuple[str, str]) -> float:
        """Compute a value score for this index. Higher is better.

        Two terms. Hits per second of age gives a recency weighted
        frequency. Build cost gives a small bonus that protects an
        expensive index from being evicted seconds after it was built.
        """
        s = self._stats.get(key)
        if s is None:
            return 0.0

        # Age in seconds since last use. We floor at 1 second so we do
        # not divide by zero and so the score does not explode for an
        # index that was just touched.
        age = max(time.monotonic() - s.last_used_ts, 1.0)

        return (s.hit_count / age) + (s.build_cost_seconds * 0.1)
