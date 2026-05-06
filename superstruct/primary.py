"""Primary store. The source of truth for every record.

Every record gets an auto assigned integer id. The store is always
synchronous and always exact. All sub-indexes derive from this store,
so even if every single sub-index is evicted, the data itself is never
lost. This is the property that makes lazy index construction safe.
The worst case after a full eviction is a slow first query while we
rebuild the index. Correctness is never at risk.
"""
from dataclasses import dataclass
from typing import Any, Iterator


@dataclass
class Record:
    """A single record in the structure.

    The id is assigned by the primary store on insert. Attributes are
    a free form dict so different records can hold different fields.
    Indexes that target an attribute simply skip records that lack it.
    """

    id: int
    attrs: dict[str, Any]


class PrimaryStore:
    """Holds the canonical copy of every record. Always synchronous.

    The whole adaptive index machinery sits on top of this. Inserts
    here are cheap. Indexes are notified by the planner so they can
    keep themselves in step.
    """

    def __init__(self):
        # The actual storage. id to Record. A plain dict works fine.
        self._records: dict[int, Record] = {}

        # Counter for assigning new record ids on insert. Monotonic.
        # Reusing ids would invalidate any index that holds them, so we
        # never reuse, even after a delete.
        self._next_id: int = 0

    def insert(self, attrs: dict[str, Any]) -> Record:
        """Add a record. Returns the freshly created Record with its id.

        We copy the attribute dict so later mutations on the caller's
        side do not reach back into our store.
        """
        record_id = self._next_id
        self._next_id += 1
        record = Record(id=record_id, attrs=dict(attrs))
        self._records[record_id] = record
        return record

    def get(self, record_id: int) -> Record | None:
        """O(1) primary key lookup. Returns None when the id is unknown."""
        return self._records.get(record_id)

    def delete(self, record_id: int) -> Record | None:
        """Remove a record. Returns the removed Record or None if absent."""
        return self._records.pop(record_id, None)

    def __iter__(self) -> Iterator[Record]:
        # Iteration order matches insertion order in modern Python which
        # is convenient for deterministic test output.
        return iter(self._records.values())

    def __len__(self) -> int:
        return len(self._records)
