from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any


type JSONScalar = None | bool | int | float | str
type JSONValue = JSONScalar | list["JSONValue"] | dict[str, "JSONValue"]
type JSONRow = dict[str, JSONValue]


@dataclass(frozen=True)
class CorrelationContext:
    strategy_id: str
    topic: str
    entry_id: str
    ts_ms: int


@dataclass(frozen=True)
class SetJSONOp:
    key: str
    value: JSONValue
    ttl_seconds: int | None = None


@dataclass(frozen=True)
class StreamJSONOp:
    key: str
    row: JSONRow
    maxlen: int


@dataclass(frozen=True)
class ReplaceHashJSONOp:
    key: str
    mapping: dict[str, JSONRow]
    ttl_seconds: int | None = None


type WriteOp = SetJSONOp | StreamJSONOp | ReplaceHashJSONOp


type HandlerFn = Callable[[Any, CorrelationContext], list[WriteOp]]


__all__ = [
    "CorrelationContext",
    "HandlerFn",
    "JSONRow",
    "JSONValue",
    "ReplaceHashJSONOp",
    "SetJSONOp",
    "StreamJSONOp",
    "WriteOp",
]
