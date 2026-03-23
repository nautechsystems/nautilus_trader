from __future__ import annotations

import hashlib
import json
import math
import time
import uuid
from collections.abc import Mapping
from typing import Any
from typing import Protocol

from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.common.keys import FluxRedisKeys
from flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY


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
        self._aliases_by_name = {
            name: self._parse_aliases(spec.get("aliases"))
            for name, spec in self._schema.items()
        }
        self._canonical_by_alias = self._build_canonical_by_alias(self._aliases_by_name)
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

        requested_fields: list[str] = list(fields)
        for aliases in self._aliases_by_name.values():
            requested_fields.extend(alias for alias in aliases if alias not in requested_fields)

        raw_values = self._redis.hmget(self.hash_key, requested_fields)
        if len(raw_values) != len(requested_fields):
            raise RuntimeError(
                f"Redis HMGET returned {len(raw_values)} values for {len(requested_fields)} fields",
            )
        raw_by_field = dict(zip(requested_fields, raw_values, strict=True))

        params: dict[str, Any] = {}
        for name in fields:
            raw = raw_by_field.get(name)
            if raw is None:
                for alias in self._aliases_by_name.get(name, ()):
                    raw = raw_by_field.get(alias)
                    if raw is not None:
                        break
            if raw is None:
                params[name] = self._defaults.get(name)
                continue
            params[name] = self._coerce_value(name, raw)
        return params

    def update(self, updates: Mapping[str, Any]) -> dict[str, Any]:
        coerced_updates = self._coerce_updates(updates)
        if not coerced_updates:
            return {}

        mapping = {name: self._to_redis_text(value) for name, value in coerced_updates.items()}
        self._redis.hset(self.hash_key, mapping=mapping)
        if "bot_on" in coerced_updates:
            self._redis.hset(
                self._keys.params_metadata_key(),
                mapping={"bot_on_control_revision": self._new_control_revision()},
            )
        return coerced_updates

    def load_bot_on_control_revision(self) -> str:
        raw_values = self._redis.hmget(
            self._keys.params_metadata_key(),
            ["bot_on_control_revision"],
        )
        if not raw_values:
            return ""
        return self._decode(raw_values[0]).strip()

    def publish_update(
        self,
        updates: Mapping[str, Any],
        *,
        ts_ms: int | None = None,
    ) -> dict[str, Any]:
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
            canonical_name = self._canonical_name(name)
            out[canonical_name] = self._coerce_value(canonical_name, raw)
        return out

    def _coerce_defaults(self, defaults: Mapping[str, Any]) -> dict[str, Any]:
        missing = [name for name in self._schema if name not in defaults]
        if missing:
            raise ValueError(f"Missing default values for parameters: {sorted(missing)}")

        out: dict[str, Any] = {}
        for name in self._schema:
            out[name] = self._coerce_value(name, defaults[name])
        return out

    def _canonical_name(self, name: Any) -> str:
        text = str(name)
        return self._canonical_by_alias.get(text, text)

    def _coerce_value(self, name: str, value: Any) -> Any:
        schema = self._schema.get(name)
        if schema is None:
            raise ValueError(f"Unknown parameter {name!r} for strategy {self.strategy_id!r}")

        schema_type = str(schema.get("type", "number"))
        if schema_type == "boolean":
            parsed_bool = self._parse_bool(value)
            if parsed_bool is None:
                raise ValueError(f"Invalid boolean value for parameter {name!r}: {value!r}")
            return parsed_bool

        if schema_type == "integer":
            try:
                return int(self._decode(value))
            except (TypeError, ValueError):
                raise ValueError(
                    f"Invalid integer value for parameter {name!r}: {value!r}",
                ) from None

        if schema_type == "number":
            try:
                parsed_num = float(self._decode(value))
            except (TypeError, ValueError):
                raise ValueError(
                    f"Invalid number value for parameter {name!r}: {value!r}",
                ) from None
            if not math.isfinite(parsed_num):
                raise ValueError(
                    f"Invalid number value for parameter {name!r}: {value!r} (must be finite)",
                )
            return parsed_num

        if schema_type == "select":
            parsed_text = self._decode(value).strip()
            raw_options = schema.get("options")
            if isinstance(raw_options, (list, tuple)):
                valid_values = {
                    str(option[0]).strip()
                    for option in raw_options
                    if isinstance(option, (list, tuple)) and len(option) >= 1
                }
                if valid_values and parsed_text not in valid_values:
                    raise ValueError(
                        f"Invalid option value for parameter {name!r}: {value!r}",
                    )
            if not parsed_text:
                raise ValueError(
                    f"Invalid option value for parameter {name!r}: {value!r}",
                )
            return parsed_text

        return self._decode(value)

    @staticmethod
    def _decode(value: Any) -> str:
        if isinstance(value, bytes):
            return value.decode("utf-8", errors="replace")
        if value is None:
            return ""
        return str(value)

    @staticmethod
    def _parse_aliases(raw_aliases: Any) -> tuple[str, ...]:
        if raw_aliases is None:
            return ()
        if not isinstance(raw_aliases, (list, tuple)):
            return ()
        aliases: list[str] = []
        for raw_alias in raw_aliases:
            if isinstance(raw_alias, (list, tuple)) and raw_alias:
                alias = str(raw_alias[0]).strip()
            else:
                alias = str(raw_alias).strip()
            if alias:
                aliases.append(alias)
        return tuple(aliases)

    @staticmethod
    def _build_canonical_by_alias(
        aliases_by_name: Mapping[str, tuple[str, ...]],
    ) -> dict[str, str]:
        canonical_by_alias: dict[str, str] = {}
        for canonical_name, aliases in aliases_by_name.items():
            for alias in aliases:
                existing = canonical_by_alias.get(alias)
                if existing is not None and existing != canonical_name:
                    raise ValueError(
                        f"Duplicate runtime param alias {alias!r} for {canonical_name!r} and {existing!r}",
                    )
                canonical_by_alias[alias] = canonical_name
        return canonical_by_alias

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
                raise ValueError(
                    f"Invalid float value for Redis serialization: {value!r} (must be finite)",
                )
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
        known = set(self._schema.keys()) | set(self._canonical_by_alias.keys())
        unknown: list[str] = []
        for field in self._redis.hkeys(self.hash_key):
            name = self._decode(field)
            if name and name not in known:
                unknown.append(name)
        return unknown

    @staticmethod
    def _now_ms() -> int:
        return int(time.time() * 1_000)

    @staticmethod
    def _new_control_revision() -> str:
        return f"{time.time_ns()}-{uuid.uuid4().hex}"
