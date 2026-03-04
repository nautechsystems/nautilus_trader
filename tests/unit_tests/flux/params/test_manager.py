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

import json

import pytest

from nautilus_trader.flux.params.manager import FluxParamsManager


class _FakeRedis:
    def __init__(self) -> None:
        self.hashes: dict[str, dict[str, bytes]] = {}
        self.hmget_calls: list[tuple[str, tuple[str, ...]]] = []
        self.hset_calls: list[tuple[str, dict[str, str]]] = []
        self.publish_calls: list[tuple[str, str]] = []

    def hmget(self, key: str, fields: list[str]) -> list[bytes | None]:
        self.hmget_calls.append((key, tuple(fields)))
        mapping = self.hashes.get(key, {})
        return [mapping.get(field) for field in fields]

    def hkeys(self, key: str) -> list[str]:
        return list(self.hashes.get(key, {}).keys())

    def hset(self, key: str, mapping: dict[str, str]) -> int:
        self.hset_calls.append((key, dict(mapping)))
        target = self.hashes.setdefault(key, {})
        for field, value in mapping.items():
            target[field] = value.encode("utf-8")
        return len(mapping)

    def publish(self, channel: str, payload: str) -> int:
        self.publish_calls.append((channel, payload))
        return 1


@pytest.fixture
def schema() -> dict[str, dict[str, str]]:
    return {
        "qty": {"type": "number"},
        "max_age_ms": {"type": "integer"},
        "bot_on": {"type": "boolean"},
    }


@pytest.fixture
def defaults() -> dict[str, object]:
    return {
        "qty": 1.0,
        "max_age_ms": 1_000,
        "bot_on": False,
    }


def _manager(redis_client: _FakeRedis, schema: dict[str, dict[str, str]], defaults: dict[str, object]) -> FluxParamsManager:
    return FluxParamsManager(
        redis_client=redis_client,
        strategy_id="maker_v3_01",
        schema=schema,
        defaults=defaults,
    )


def test_load_uses_hmget_and_coerces_values(schema: dict[str, dict[str, str]], defaults: dict[str, object]) -> None:
    redis_client = _FakeRedis()
    redis_client.hashes["flux:v1:params:maker_v3_01"] = {
        "qty": b"2.5",
        "max_age_ms": b"2500",
        "bot_on": b"1",
    }
    manager = _manager(redis_client, schema, defaults)

    loaded = manager.load()

    assert loaded == {"qty": 2.5, "max_age_ms": 2500, "bot_on": True}
    assert redis_client.hmget_calls == [
        ("flux:v1:params:maker_v3_01", ("qty", "max_age_ms", "bot_on")),
    ]


def test_load_rejects_unknown_hash_fields(schema: dict[str, dict[str, str]], defaults: dict[str, object]) -> None:
    redis_client = _FakeRedis()
    redis_client.hashes["flux:v1:params:maker_v3_01"] = {
        "qty": b"2.5",
        "unexpected": b"x",
    }
    manager = _manager(redis_client, schema, defaults)

    with pytest.raises(ValueError, match="Unknown params keys"):
        manager.load()


def test_update_writes_coerced_hset_mapping(schema: dict[str, dict[str, str]], defaults: dict[str, object]) -> None:
    redis_client = _FakeRedis()
    manager = _manager(redis_client, schema, defaults)

    applied = manager.update({"qty": "3.25", "max_age_ms": "250", "bot_on": "true"})

    assert applied == {"qty": 3.25, "max_age_ms": 250, "bot_on": True}
    assert redis_client.hset_calls == [
        ("flux:v1:params:maker_v3_01", {"qty": "3.25", "max_age_ms": "250", "bot_on": "1"}),
    ]


def test_update_rejects_unknown_param_keys(schema: dict[str, dict[str, str]], defaults: dict[str, object]) -> None:
    redis_client = _FakeRedis()
    manager = _manager(redis_client, schema, defaults)

    with pytest.raises(ValueError, match="Unknown parameter"):
        manager.update({"unknown": 1})


def test_publish_update_targets_flux_v1_params_channels(
    schema: dict[str, dict[str, str]],
    defaults: dict[str, object],
) -> None:
    redis_client = _FakeRedis()
    manager = _manager(redis_client, schema, defaults)

    payload = manager.publish_update({"qty": "4.5", "bot_on": "false"}, ts_ms=123)

    assert payload == {
        "strategy_id": "maker_v3_01",
        "updates": {"qty": 4.5, "bot_on": False},
        "ts_ms": 123,
    }
    assert [channel for channel, _ in redis_client.publish_calls] == [
        "flux:v1:params:global",
        "flux:v1:params:maker_v3_01",
    ]
    parsed_payloads = [json.loads(encoded) for _, encoded in redis_client.publish_calls]
    assert parsed_payloads == [payload, payload]
