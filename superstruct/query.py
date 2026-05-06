"""Query language for Superstruct.

The query AST is a small tree. Leaves are Predicates. Inner nodes are
And, Or or Not which combine sub trees. Top level is the where field
of a Query plus an optional final TopK ordering step.

The QueryBuilder builds an implicit And of predicates by default. The
any_of and exclude methods introduce Or and Not nodes so users can
express richer queries without ever touching the AST directly.
"""
from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Optional, Union


class PredicateKind(Enum):
    """The kinds of leaf predicate the planner knows how to route.

    Each kind implies a different access pattern. EQUALS wants a hash
    map. RANGE wants a sorted structure. PREFIX wants a trie. CONTAINS
    wants a word level inverted index. FUZZY wants an n gram similarity
    index. New kinds plug in by adding an entry here and registering an
    index type in the planner default map.
    """

    EQUALS = "equals"
    RANGE = "range"
    PREFIX = "prefix"
    CONTAINS = "contains"
    FUZZY = "fuzzy"


@dataclass
class Predicate:
    """A leaf condition on a single attribute.

    The shape of value depends on kind. EQUALS takes the literal value
    to match. RANGE takes a (low, high) tuple, both ends inclusive.
    PREFIX takes a string prefix. CONTAINS takes a single word that
    must appear in the tokenized value. FUZZY takes a target string
    and uses the threshold field as the minimum similarity score.
    """

    kind: PredicateKind
    attribute: str
    value: Any = None

    # Only used by FUZZY. Defaults to a permissive 0.5 Jaccard similarity.
    threshold: float = 0.5


@dataclass
class And:
    """Conjunction. Match records that satisfy every child."""
    children: list = field(default_factory=list)


@dataclass
class Or:
    """Disjunction. Match records that satisfy at least one child."""
    children: list = field(default_factory=list)


@dataclass
class Not:
    """Negation. Match records that do not satisfy the child."""
    child: Any = None


# Type alias for any node in the AST. Helps keep the planner type hints
# readable. Python does not enforce this at runtime but it documents
# intent and lets editors flag mistakes.
Node = Union[Predicate, And, Or, Not]


@dataclass
class TopK:
    """Final ordering step. Sort by attribute then take k items.

    Defaults to descending so top_k("score", 10) returns the ten
    highest scoring records. Pass descending=False for the lowest.
    """

    attribute: str
    k: int
    descending: bool = True


@dataclass
class Query:
    """A whole query.

    where is the root of the predicate tree. None means every record.
    top_k is an optional final ordering step applied to the result of
    evaluating where.
    """
    where: Optional[Node] = None
    top_k: Optional[TopK] = None
