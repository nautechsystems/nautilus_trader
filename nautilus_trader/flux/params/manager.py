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
import time
from collections.abc import Mapping
from typing import Any
from typing import Protocol

from nautilus_trader.flux.common.config import FLUX_DEFAULT_NAMESPACE
from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.common.keys import FluxRedisKeys


class RedisHashPubSubClient(Protocol):
    """
    Minimal Redis client contract used by `FluxParamsManager`.
    """

    def hmget(self, key: str, fields: list[str]) -> list[Any]: ...

    def hkeys(self, key: str) -> list[Any]: ...

    def hset(self, key: str, mapping: dict[str, str]) -> int: ...

    def publish(self, channel: str, message: str) -> int: ...


class FluxParamsManager:
    """
    Redis-backed manager for strategy runtime params.
    """

    def __init__(
        self,
        *,
        redis_client: RedisHashPubSubClient,
        strategy_id: str,
        schema: Mapping[str, Mapping[str, Any]],
        defaults: Mapping[str, Any],
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> None:
        if not schema:
            raise ValueError("`schema` must not be empty")
        self._redis = redis_client
        self._schema = {str(name): dict(spec) for name, spec in schema.items()}
        self._defaults = dict(defaults)
        self._keys = FluxRedisKeys(
            strategy_id=strategy_id,
            namespace=namespace,
            schema_version=schema_version,
        )

    @property
    def strategy_id(self) -> str:
        return self._keys.strategy_id

    @property
    def hash_key(self) -> str:
        return self._keys.params_hash_key()

    def load(self) -> dict[str, Any]:
        fields = list(self._schema.keys())
        unknown_fields = sorted(set(self._unknown_hash_fields()))
        if unknown_fields:
            raise ValueError(
                f"Unknown params keys in {self.hash_key}: {unknown_fields}",
            )

        raw_values = self._redis.hmget(self.hash_key, fields)
        if len(raw_values) != len(fields):
            raise RuntimeError(
                f"Redis HMGET returned {len(raw_values)} values for {len(fields)} fields",
            )

        params: dict[str, Any] = {}
        for name, raw in zip(fields, raw_values):
            if raw is None:
                params[name] = self._defaults.get(name)
                continue
            params[name] = self._coerce_value(name, raw)
        return params

    def update(self, updates: Mapping[str, Any]) -> dict[str, Any]:
        coerced_updates = self._coerce_updates(updates)
        if not coerced_updates:
            return {}

        mapping = {
            name: self._to_redis_text(value)
            for name, value in coerced_updates.items()
        }
        self._redis.hset(self.hash_key, mapping=mapping)
        return coerced_updates

    def publish_update(self, updates: Mapping[str, Any], *, ts_ms: int | None = None) -> dict[str, Any]:
        coerced_updates = self._coerce_updates(updates)
        if ts_ms is None:
            ts_ms = self._now_ms()

        payload = {
            "strategy_id": self.strategy_id,
            "updates": coerced_updates,
            "ts_ms": int(ts_ms),
        }
        encoded = json.dumps(payload, separators=(",", ":"), sort_keys=True)
        self._redis.publish(self._keys.global_params_pubsub_channel(), encoded)
        self._redis.publish(self._keys.params_pubsub_channel(), encoded)
        return payload

    def _coerce_updates(self, updates: Mapping[str, Any]) -> dict[str, Any]:
        out: dict[str, Any] = {}
        for name, raw in updates.items():
            out[name] = self._coerce_value(name, raw)
        return out

    def _coerce_value(self, name: str, value: Any) -> Any:
        schema = self._schema.get(name)
        if schema is None:
            raise ValueError(f"Unknown parameter {name!r} for strategy {self.strategy_id!r}")

        schema_type = str(schema.get("type", "number"))
        if schema_type == "boolean":
            parsed = self._parse_bool(value)
            if parsed is None:
                raise ValueError(f"Invalid boolean value for parameter {name!r}: {value!r}")
            return parsed

        if schema_type == "integer":
            try:
                return int(self._decode(value))
            except (TypeError, ValueError):
                raise ValueError(f"Invalid integer value for parameter {name!r}: {value!r}") from None

        if schema_type == "number":
            try:
                return float(self._decode(value))
            except (TypeError, ValueError):
                raise ValueError(f"Invalid number value for parameter {name!r}: {value!r}") from None

        return self._decode(value)

    @staticmethod
    def _decode(value: Any) -> str:
        if isinstance(value, bytes):
            return value.decode("utf-8", errors="replace")
        if value is None:
            return ""
        return str(value)

    @staticmethod
    def _parse_bool(value: Any) -> bool | None:
        if isinstance(value, bool):
            return value
        if isinstance(value, (int, float)):
            if value in (0, 0.0):
                return False
            if value in (1, 1.0):
                return True
        text = FluxParamsManager._decode(value).strip().lower()
        if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
            return True
        if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
            return False
        return None

    @staticmethod
    def _to_redis_text(value: Any) -> str:
        if isinstance(value, bool):
            return "1" if value else "0"
        if isinstance(value, float) and value.is_integer():
            return str(int(value))
        return str(value)

    def _unknown_hash_fields(self) -> list[str]:
        known = set(self._schema.keys())
        unknown: list[str] = []
        for field in self._redis.hkeys(self.hash_key):
            name = self._decode(field)
            if name and name not in known:
                unknown.append(name)
        return unknown

    @staticmethod
    def _now_ms() -> int:
        return int(time.time() * 1_000)

