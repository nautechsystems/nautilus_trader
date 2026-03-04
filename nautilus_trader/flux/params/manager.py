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

import hashlib
import json
import math
import time
from collections.abc import Mapping
from typing import Any
from typing import Protocol

from nautilus_trader.flux.common.config import FLUX_DEFAULT_NAMESPACE
from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY


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
        param_set: str = MAKERV3_RUNTIME_PARAM_REGISTRY.param_set,
    ) -> None:
        if not schema:
            raise ValueError("`schema` must not be empty")
        if not isinstance(param_set, str) or not param_set.strip():
            raise ValueError("`param_set` must be a non-empty string")

        safe_param_set = param_set.strip()

        self._redis = redis_client
        self._schema = {str(name): dict(spec) for name, spec in schema.items()}
        self._defaults = self._coerce_defaults(defaults)
        self._schema_version = schema_version
        self._param_set = safe_param_set
        self._schema_digest = self._digest_schema_metadata(
            schema=self._schema,
            defaults=self._defaults,
            schema_version=self._schema_version,
            param_set=self._param_set,
        )
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

    @property
    def defaults(self) -> dict[str, Any]:
        return dict(self._defaults)

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
            "digest": self._schema_digest,
            "param_set": self._param_set,
            "schema_version": self._schema_version,
            "strategy_id": self.strategy_id,
            "updates": coerced_updates,
            "ts_ms": int(ts_ms),
        }
        encoded = json.dumps(payload, separators=(",", ":"), sort_keys=True, allow_nan=False)
        self._redis.publish(self._keys.global_params_pubsub_channel(), encoded)
        self._redis.publish(self._keys.params_pubsub_channel(), encoded)
        return payload

    def _coerce_updates(self, updates: Mapping[str, Any]) -> dict[str, Any]:
        out: dict[str, Any] = {}
        for name, raw in updates.items():
            out[name] = self._coerce_value(name, raw)
        return out

    def _coerce_defaults(self, defaults: Mapping[str, Any]) -> dict[str, Any]:
        missing = [name for name in self._schema if name not in defaults]
        if missing:
            raise ValueError(f"Missing default values for parameters: {sorted(missing)}")

        out: dict[str, Any] = {}
        for name in self._schema:
            out[name] = self._coerce_value(name, defaults[name])
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
                parsed = float(self._decode(value))
            except (TypeError, ValueError):
                raise ValueError(f"Invalid number value for parameter {name!r}: {value!r}") from None
            if not math.isfinite(parsed):
                raise ValueError(f"Invalid number value for parameter {name!r}: {value!r} (must be finite)")
            return parsed

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
        if isinstance(value, float):
            if not math.isfinite(value):
                raise ValueError(f"Invalid float value for Redis serialization: {value!r} (must be finite)")
            if value.is_integer():
                return str(int(value))
        return str(value)

    @staticmethod
    def _digest_schema_metadata(
        *,
        schema: Mapping[str, Mapping[str, Any]],
        defaults: Mapping[str, Any],
        schema_version: str,
        param_set: str,
    ) -> str:
        canonical_payload = FluxParamsManager._normalize_for_digest(
            {
                "schema_version": schema_version,
                "param_set": param_set,
                "schema": schema,
                "defaults": defaults,
            },
        )
        canonical = json.dumps(
            canonical_payload,
            separators=(",", ":"),
            sort_keys=True,
            allow_nan=False,
        )
        return hashlib.sha256(canonical.encode("utf-8")).hexdigest()

    @staticmethod
    def _normalize_for_digest(value: Any) -> Any:
        if value is None or isinstance(value, bool | int | str):
            return value
        if isinstance(value, bytes):
            return value.decode("utf-8", errors="replace")
        if isinstance(value, float):
            if math.isnan(value):
                return "NaN"
            if math.isinf(value):
                return "Infinity" if value > 0 else "-Infinity"
            return value
        if isinstance(value, Mapping):
            return {
                str(key): FluxParamsManager._normalize_for_digest(item)
                for key, item in sorted(value.items(), key=lambda pair: str(pair[0]))
            }
        if isinstance(value, tuple | list):
            return [FluxParamsManager._normalize_for_digest(item) for item in value]
        if isinstance(value, set | frozenset):
            return [
                FluxParamsManager._normalize_for_digest(item)
                for item in sorted(value, key=lambda item: f"{type(item).__name__}:{item!s}")
            ]
        return {
            "__type__": f"{type(value).__module__}.{type(value).__qualname__}",
            "value": str(value),
        }

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
