# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

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
