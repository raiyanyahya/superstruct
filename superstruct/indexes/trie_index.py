"""Trie index. Prefix lookups on string attributes.

Classical prefix tree. Each node stores its children by character and a
set of record ids whose value ended at that node. Prefix queries walk
to the prefix node then collect every id in the subtree below it.

The index only handles string values. Records whose value is not a
string are skipped, mirroring the design choice that records are not
required to share a schema.
"""
from sys import getsizeof
from typing import Iterable

from ..primary import Record
from ..query import Predicate, PredicateKind
from .base import Index


class _TrieNode:
    """A single node in the trie.

    children maps a character to the next node. ids is the set of
    record ids whose indexed value ended exactly at this node, which
    lets us answer equality queries in addition to prefix queries.
    """

    __slots__ = ("children", "ids")

    def __init__(self):
        self.children: dict[str, "_TrieNode"] = {}
        self.ids: set[int] = set()


class TrieIndex(Index):
    """Prefix lookups using a classical character trie."""

    # Tries answer prefix and equality. Both walks start from the root
    # and only differ in whether we collect the subtree at the end.
    supported_kinds = {PredicateKind.PREFIX, PredicateKind.EQUALS}

    def __init__(self, attribute: str):
        super().__init__(attribute)
        self._root = _TrieNode()

    def build_from_records(self, records: Iterable[Record]) -> None:
        # No bulk path that beats the per record insert here. Tries are
        # built one character at a time.
        for record in records:
            self.insert(record)

    def _walk_or_create(self, value: str) -> _TrieNode:
        # Walk down the trie creating any missing nodes. Returns the
        # terminal node for the supplied string.
        node = self._root
        for ch in value:
            nxt = node.children.get(ch)
            if nxt is None:
                nxt = _TrieNode()
                node.children[ch] = nxt
            node = nxt
        return node

    def insert(self, record: Record) -> None:
        value = record.attrs.get(self.attribute)
        # Tries only make sense for strings. Other types skip silently.
        if not isinstance(value, str):
            return
        terminal = self._walk_or_create(value)
        terminal.ids.add(record.id)

    def remove(self, record: Record) -> None:
        value = record.attrs.get(self.attribute)
        if not isinstance(value, str):
            return

        # Walk to the terminal node. If any link is missing the record
        # was not in the trie and there is nothing to remove.
        node = self._root
        for ch in value:
            node = node.children.get(ch)
            if node is None:
                return
        node.ids.discard(record.id)

        # We do not prune empty subtrees here. Pruning is fiddly book
        # keeping and the cost is small. The promoter can reclaim that
        # wasted memory by evicting the whole index when it goes cold.

    def _collect_subtree(self, node: _TrieNode) -> set[int]:
        # Iterative DFS. Recursive would be cleaner but blows the stack
        # for very long string keys.
        out: set[int] = set()
        stack = [node]
        while stack:
            current = stack.pop()
            out.update(current.ids)
            stack.extend(current.children.values())
        return out

    def execute(self, predicate: Predicate) -> set[int]:
        target = predicate.value
        if not isinstance(target, str):
            return set()

        # Walk to the prefix node. Missing link means zero matches.
        node = self._root
        for ch in target:
            node = node.children.get(ch)
            if node is None:
                return set()

        if predicate.kind is PredicateKind.PREFIX:
            # Everything in this subtree shares the prefix. Collect.
            return self._collect_subtree(node)

        if predicate.kind is PredicateKind.EQUALS:
            # Only ids that ended exactly at this node, not the subtree.
            return set(node.ids)

        return set()

    def memory_estimate_bytes(self) -> int:
        # Walk the trie summing node sizes. Acceptable since the
        # promoter calls this rarely. A live counter would be faster
        # but more bookkeeping.
        total = 0
        stack = [self._root]
        while stack:
            node = stack.pop()
            total += getsizeof(node.children) + getsizeof(node.ids)
            stack.extend(node.children.values())
        return total
