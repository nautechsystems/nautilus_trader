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

import asyncio
import json
from typing import Any

import pytest

from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import StrategyMetadata
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig


class FakeRedisPipeline:
    def __init__(self, redis_client: FakeRedis) -> None:
        self._redis = redis_client
        self._commands: list[tuple[str, Any]] = []

    def get(self, key: str) -> FakeRedisPipeline:
        self._commands.append(("get", key))
        return self

    def exists(self, key: str) -> FakeRedisPipeline:
        self._commands.append(("exists", key))
        return self

    def xrevrange(
        self,
        key: str,
        max: str = "+",  # noqa: A002
        min: str = "-",  # noqa: A002
        count: int | None = None,
    ) -> FakeRedisPipeline:
        _ = max, min
        self._commands.append(("xrevrange", key, count))
        return self

    def execute(self) -> list[Any]:
        self._redis.pipeline_exec_count += 1
        self._redis.pipeline_batches.append(list(self._commands))

        out: list[Any] = []
        for command in self._commands:
            op = command[0]
            if op == "get":
                out.append(self._redis._get_value(command[1]))
            elif op == "exists":
                out.append(self._redis._exists_value(command[1]))
            elif op == "xrevrange":
                out.append(self._redis._xrevrange_value(command[1], count=command[2]))
        return out


class FakeRedis:
    def __init__(self) -> None:
        self.ping_result = True
        self.strings: dict[str, bytes] = {}
        self.hashes: dict[str, dict[str, bytes]] = {}
        self.streams: dict[str, list[dict[str, Any]]] = {}
        self.exists_overrides: dict[str, bool] = {}
        self.publish_calls: list[tuple[str, str]] = []
        self.direct_get_calls: list[str] = []
        self.direct_xrevrange_calls: list[tuple[str, int | None]] = []
        self.pipeline_exec_count = 0
        self.pipeline_batches: list[list[tuple[str, Any]]] = []

    def set_json(self, key: str, value: Any) -> None:
        self.strings[key] = json.dumps(value, separators=(",", ":"), sort_keys=True).encode("utf-8")

    def set_hash_json(self, key: str, mapping: dict[str, Any]) -> None:
        target: dict[str, bytes] = {}
        for field, value in mapping.items():
            if isinstance(value, bytes):
                target[field] = value
            elif isinstance(value, str):
                target[field] = value.encode("utf-8")
            elif isinstance(value, bool):
                target[field] = b"1" if value else b"0"
            elif isinstance(value, int | float):
                target[field] = str(value).encode("utf-8")
            else:
                target[field] = json.dumps(value, separators=(",", ":"), sort_keys=True).encode("utf-8")
        self.hashes[key] = target

    def add_stream_rows(self, key: str, rows: list[dict[str, Any]]) -> None:
        self.streams[key] = list(rows)

    def ping(self) -> bool:
        return bool(self.ping_result)

    def _get_value(self, key: str) -> bytes | None:
        return self.strings.get(key)

    def get(self, key: str) -> bytes | None:
        self.direct_get_calls.append(key)
        return self._get_value(key)

    def _exists_value(self, key: str) -> int:
        if key in self.exists_overrides:
            return 1 if self.exists_overrides[key] else 0
        present = key in self.strings or key in self.hashes or key in self.streams
        return 1 if present else 0

    def exists(self, key: str) -> int:
        return self._exists_value(key)

    def _xrevrange_value(self, key: str, *, count: int | None) -> list[tuple[bytes, dict[bytes, bytes]]]:
        rows = list(reversed(self.streams.get(key, [])))
        if count is not None:
            rows = rows[:count]
        out: list[tuple[bytes, dict[bytes, bytes]]] = []
        for index, row in enumerate(rows):
            encoded = json.dumps(row, separators=(",", ":"), sort_keys=True).encode("utf-8")
            out.append((f"{index}-0".encode("utf-8"), {b"payload": encoded}))
        return out

    def xrevrange(
        self,
        key: str,
        max: str = "+",  # noqa: A002
        min: str = "-",  # noqa: A002
        count: int | None = None,
    ) -> list[tuple[bytes, dict[bytes, bytes]]]:
        _ = max, min
        self.direct_xrevrange_calls.append((key, count))
        return self._xrevrange_value(key, count=count)

    def hmget(self, key: str, fields: list[str]) -> list[bytes | None]:
        mapping = self.hashes.get(key, {})
        return [mapping.get(field) for field in fields]

    def hkeys(self, key: str) -> list[str]:
        return list(self.hashes.get(key, {}).keys())

    def hset(self, key: str, mapping: dict[str, str]) -> int:
        target = self.hashes.setdefault(key, {})
        for field, value in mapping.items():
            target[field] = value.encode("utf-8")
        return len(mapping)

    def publish(self, channel: str, message: str) -> int:
        self.publish_calls.append((channel, message))
        return 1

    def pipeline(self, transaction: bool = False) -> FakeRedisPipeline:
        _ = transaction
        return FakeRedisPipeline(self)


@pytest.fixture
def contract_catalog() -> tuple[ContractCatalogEntry, ...]:
    return (
        ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),
        ContractCatalogEntry(exchange="venue_b", symbol="ABC/USDT"),
    )


@pytest.fixture
def strategy_metadata() -> StrategyMetadata:
    return StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )


@pytest.fixture
def flux_config() -> FluxConfig:
    return FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id="strategy_01",
            strategy_instance_id="strategy_01",
            trader_id="trader_01",
            external_strategy_id="strategy_01",
        ),
        redis=FluxRedisConfig(
            host="127.0.0.1",
            port=6380,
            db=0,
        ),
        venues=FluxVenuesConfig(
            execution_venue="venue_a",
            reference_venue="venue_b",
            execution_symbol="ABC/USDT",
            reference_symbol="ABC/USDT",
        ),
    )


@pytest.fixture
def params_schema() -> dict[str, dict[str, Any]]:
    return {
        "qty": {"type": "number"},
        "bot_on": {"type": "boolean"},
        "max_age_ms": {"type": "integer"},
    }


@pytest.fixture
def params_defaults() -> dict[str, Any]:
    return {
        "qty": 1.0,
        "bot_on": False,
        "max_age_ms": 10_000,
    }


@pytest.fixture
def redis_client() -> FakeRedis:
    return FakeRedis()


@pytest.fixture
def event_loop():
    loop = asyncio.new_event_loop()
    try:
        yield loop
    finally:
        if not loop.is_closed():
            loop.close()
