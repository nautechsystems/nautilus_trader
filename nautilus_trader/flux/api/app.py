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

from collections.abc import Mapping
from collections.abc import Sequence
from dataclasses import dataclass
import json
import math
import uuid
from typing import Any
from typing import Callable
from typing import Protocol

import redis
from flask import Flask
from flask import Response
from flask import g
from flask import request

from nautilus_trader.flux.api.payloads import ContractCatalogEntry
from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.api.payloads import build_alerts_rows
from nautilus_trader.flux.api.payloads import build_contract_id
from nautilus_trader.flux.api.payloads import build_balances_rows
from nautilus_trader.flux.api.payloads import build_envelope
from nautilus_trader.flux.api.payloads import build_error
from nautilus_trader.flux.api.payloads import build_legs_payload
from nautilus_trader.flux.api.payloads import build_params_payload
from nautilus_trader.flux.api.payloads import build_signals_payload
from nautilus_trader.flux.api.payloads import build_trades_rows
from nautilus_trader.flux.api.payloads import coerce_ts_ms
from nautilus_trader.flux.api.payloads import decode_text
from nautilus_trader.flux.api.payloads import extract_stream_rows
from nautilus_trader.flux.api.payloads import load_json
from nautilus_trader.flux.api.payloads import normalize_symbol_parts
from nautilus_trader.flux.api.payloads import now_ms
from nautilus_trader.flux.api.payloads import select_latest_strategy_row
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import validate_identifier_part
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_SCHEMA
from nautilus_trader.flux.params.manager import FluxParamsManager


DEFAULT_PARAMS_DEFAULTS: dict[str, Any] = dict(MAKERV3_RUNTIME_PARAM_DEFAULTS)
DEFAULT_PARAMS_SCHEMA: dict[str, dict[str, Any]] = {
    name: dict(spec)
    for name, spec in MAKERV3_RUNTIME_PARAM_SCHEMA.items()
}
DEFAULT_PARAMS_ORDER: tuple[str, ...] = MAKERV3_RUNTIME_PARAM_REGISTRY.names


class RedisPipelineProtocol(Protocol):
    def get(self, key: str) -> Any: ...
    def exists(self, key: str) -> Any: ...
    def xrevrange(self, key: str, max: str = "+", min: str = "-", count: int | None = None) -> Any: ...
    def execute(self) -> list[Any]: ...


class RedisClientProtocol(Protocol):
    def ping(self) -> Any: ...
    def get(self, key: str) -> Any: ...
    def xrevrange(self, key: str, max: str = "+", min: str = "-", count: int | None = None) -> Any: ...
    def hmget(self, key: str, fields: list[str]) -> list[Any]: ...
    def hkeys(self, key: str) -> list[Any]: ...
    def hset(self, key: str, mapping: dict[str, str]) -> int: ...
    def publish(self, channel: str, message: str) -> int: ...
    def pipeline(self, transaction: bool = ...) -> RedisPipelineProtocol: ...


class ParamsStoreValidationError(ValueError):
    """
    Raised when stored parameter hash content is invalid for the expected schema.
    """


class ParamsUpdateValidationError(ValueError):
    """
    Raised when inbound parameter update payload fails coercion/validation.
    """


class ContractCatalogValidationError(ValueError):
    """
    Raised when contract catalog input is invalid for Flux key building.
    """


class ApiEnvelopeError(ValueError):
    """
    Value error carrying response status/code/details for explicit API envelopes.
    """

    def __init__(
        self,
        *,
        status: int,
        code: str,
        message: str,
        details: Mapping[str, Any] | None = None,
    ) -> None:
        super().__init__(message)
        self.status = int(status)
        self.code = code
        self.message = message
        self.details = dict(details) if details is not None else None


def _ordered_params_schema(schema: Mapping[str, Mapping[str, Any]]) -> dict[str, dict[str, Any]]:
    ordered: dict[str, dict[str, Any]] = {}
    for name in DEFAULT_PARAMS_ORDER:
        if name in schema:
            ordered[name] = dict(schema[name])
    for name, spec in schema.items():
        if name not in ordered:
            ordered[str(name)] = dict(spec)
    return ordered


@dataclass(frozen=True)
class ReadinessSnapshot:
    schema_prefix: str
    required_keys: dict[str, bool]
    schema_ready: bool


class FluxApiStore:
    def __init__(
        self,
        *,
        flux_config: FluxConfig,
        redis_client: RedisClientProtocol,
        contract_catalog: Sequence[ContractCatalogEntry],
        params_schema: Mapping[str, Mapping[str, Any]],
        params_defaults: Mapping[str, Any],
        param_set: str = MAKERV3_RUNTIME_PARAM_REGISTRY.param_set,
        required_readiness_keys: Sequence[str] | None = None,
    ) -> None:
        if not contract_catalog:
            raise ValueError("`contract_catalog` must not be empty")
        if not params_schema:
            raise ValueError("`params_schema` must not be empty")
        if not params_defaults:
            raise ValueError("`params_defaults` must not be empty")
        if not isinstance(param_set, str) or not param_set.strip():
            raise ValueError("`param_set` must be a non-empty string")

        self._config = flux_config
        self._redis = redis_client
        self._params_schema = _ordered_params_schema(params_schema)
        self._param_set = param_set.strip()
        self._params_defaults = FluxParamsManager(
            redis_client=self._redis,
            strategy_id=self._config.identity.strategy_id,
            namespace=self._config.identity.namespace,
            schema_version=self._config.identity.schema_version,
            schema=self._params_schema,
            defaults=params_defaults,
            param_set=self._param_set,
        ).defaults
        self._contract_specs = self._validate_contract_catalog(contract_catalog)
        self._contracts = tuple(spec[0] for spec in self._contract_specs)

        base_keys = self._keys_for_strategy(self._config.identity.strategy_id)
        self._required_readiness_keys = tuple(
            required_readiness_keys
            or (
                base_keys.state(),
                base_keys.params_hash_key(),
                base_keys.balances_snapshot(),
                base_keys.fv_stream(),
            ),
        )
        for key in self._required_readiness_keys:
            if not isinstance(key, str) or not key.strip():
                raise ContractCatalogValidationError("`required_readiness_keys` must contain non-empty strings")

    @property
    def schema_version(self) -> str:
        return self._config.identity.schema_version

    @property
    def schema_prefix(self) -> str:
        return f"{self._config.identity.namespace}:{self._config.identity.schema_version}"

    @property
    def required_readiness_keys(self) -> tuple[str, ...]:
        return self._required_readiness_keys

    def _keys_for_strategy(self, strategy_id: str) -> FluxRedisKeys:
        return FluxRedisKeys(
            strategy_id=strategy_id,
            namespace=self._config.identity.namespace,
            schema_version=self._config.identity.schema_version,
        )

    def _params_manager(self, strategy_id: str) -> FluxParamsManager:
        return FluxParamsManager(
            redis_client=self._redis,
            strategy_id=strategy_id,
            namespace=self._config.identity.namespace,
            schema_version=self._config.identity.schema_version,
            schema=self._params_schema,
            defaults=self._params_defaults,
            param_set=self._param_set,
        )

    def _validate_contract_catalog(
        self,
        contract_catalog: Sequence[ContractCatalogEntry],
    ) -> tuple[tuple[ContractCatalogEntry, str, str], ...]:
        keys = self._keys_for_strategy(self._config.identity.strategy_id)
        seen: set[tuple[str, str, str]] = set()
        out: list[tuple[ContractCatalogEntry, str, str]] = []
        for index, contract in enumerate(contract_catalog):
            if not isinstance(contract, ContractCatalogEntry):
                raise ContractCatalogValidationError(
                    f"`contract_catalog[{index}]` must be `ContractCatalogEntry`, got {type(contract).__name__}",
                )

            exchange = decode_text(contract.exchange).strip().lower()
            symbol = decode_text(contract.symbol).strip().upper()
            base, quote = normalize_symbol_parts(symbol=symbol)
            if not base or not quote:
                raise ContractCatalogValidationError(
                    f"Contract symbol did not resolve to base/quote parts: {contract.symbol!r}",
                )

            try:
                keys.market_last(exchange=exchange, base=base, quote=quote)
            except (TypeError, ValueError) as exc:
                raise ContractCatalogValidationError(
                    f"Invalid contract catalog entry exchange={exchange!r} symbol={symbol!r}: {exc}",
                ) from exc

            dedupe_key = (exchange, base, quote)
            if dedupe_key in seen:
                continue
            seen.add(dedupe_key)
            out.append((ContractCatalogEntry(exchange=exchange, symbol=symbol), base, quote))

        if not out:
            raise ContractCatalogValidationError("`contract_catalog` produced no valid contract entries")
        return tuple(out)

    def redis_available(self) -> bool:
        try:
            return bool(self._redis.ping())
        except redis.RedisError:
            return False

    def readiness_snapshot(self) -> ReadinessSnapshot:
        pipe = self._redis.pipeline(transaction=False)
        for key in self._required_readiness_keys:
            pipe.exists(key)
        exists_raw = pipe.execute()
        if len(exists_raw) != len(self._required_readiness_keys):
            raise RuntimeError(
                f"Readiness pipeline returned {len(exists_raw)} rows, expected {len(self._required_readiness_keys)}",
            )
        key_map = {
            key: bool(value)
            for key, value in zip(self._required_readiness_keys, exists_raw)
        }
        return ReadinessSnapshot(
            schema_prefix=self.schema_prefix,
            required_keys=key_map,
            schema_ready=all(key_map.values()),
        )

    def load_params(self, strategy_id: str) -> dict[str, Any]:
        manager = self._params_manager(strategy_id)
        try:
            return manager.load()
        except ValueError as exc:
            raise ParamsStoreValidationError(str(exc)) from exc

    def update_params(self, strategy_id: str, updates: Mapping[str, Any]) -> dict[str, Any]:
        manager = self._params_manager(strategy_id)
        if not updates:
            params = self.load_params(strategy_id)
            return {"updated": [], "params": params}

        try:
            applied_updates = manager.update(updates)
        except ValueError as exc:
            raise ParamsUpdateValidationError(str(exc)) from exc
        if applied_updates:
            manager.publish_update(applied_updates, ts_ms=now_ms())
        params = self.load_params(strategy_id)
        return {"updated": sorted(applied_updates), "params": params}

    def _market_keys(self, strategy_id: str) -> list[tuple[ContractCatalogEntry, str]]:
        keys = self._keys_for_strategy(strategy_id)
        out: list[tuple[ContractCatalogEntry, str]] = []
        for contract, base, quote in self._contract_specs:
            out.append(
                (
                    contract,
                    keys.market_last(exchange=contract.exchange, base=base, quote=quote),
                ),
            )
        return out

    def load_signals_payload(self, strategy_id: str, metadata: StrategyMetadata) -> dict[str, Any]:
        keys = self._keys_for_strategy(strategy_id)
        market_pairs = self._market_keys(strategy_id)

        pipe = self._redis.pipeline(transaction=False)
        pipe.get(keys.state())
        pipe.xrevrange(keys.fv_stream(), count=50)
        pipe.get(keys.balances_snapshot())
        for _, market_key in market_pairs:
            pipe.get(market_key)
        raw = pipe.execute()
        expected_length = 3 + len(market_pairs)
        if len(raw) != expected_length:
            raise RuntimeError(
                f"Signals pipeline returned {len(raw)} rows, expected {expected_length}",
            )

        state_value = load_json(raw[0])
        state = dict(state_value) if isinstance(state_value, dict) else {}

        fv_rows = extract_stream_rows(raw[1])
        fv_row = select_latest_strategy_row(fv_rows, strategy_id)

        balances_raw = load_json(raw[2])
        balances = build_balances_rows(raw_snapshot=balances_raw, strategy_id=strategy_id)

        market_rows: dict[str, dict[str, Any]] = {}
        for (contract, _), market_raw in zip(market_pairs, raw[3:]):
            parsed = load_json(market_raw)
            market_rows[build_contract_id(exchange=contract.exchange, symbol=contract.symbol)] = (
                dict(parsed) if isinstance(parsed, dict) else {}
            )
        legs = build_legs_payload(contracts=self._contracts, market_rows=market_rows, now_ms_value=now_ms())

        params = self.load_params(strategy_id)
        return build_signals_payload(
            strategy_id=strategy_id,
            metadata=metadata,
            state=state,
            fv_row=fv_row,
            params=params,
            balances=balances,
            legs=legs,
        )

    def load_balances_rows(self, strategy_id: str) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        raw = load_json(self._redis.get(keys.balances_snapshot()))
        return build_balances_rows(raw_snapshot=raw, strategy_id=strategy_id)

    def load_trades_rows(self, strategy_id: str, *, limit: int, since_ms: int | None) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        fetch_count = max(1, min(2_000, (limit * 4) if since_ms is not None else limit))
        entries = self._redis.xrevrange(keys.trades_stream(), count=fetch_count)
        rows = extract_stream_rows(entries)
        return build_trades_rows(rows=rows, strategy_id=strategy_id, limit=limit, since_ms=since_ms)

    def load_alerts_rows(self, strategy_id: str, *, limit: int) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        fetch_count = max(1, min(2_000, limit * 2))
        entries = self._redis.xrevrange(keys.alerts(), count=fetch_count)
        rows = extract_stream_rows(entries)
        return build_alerts_rows(rows=rows, strategy_id=strategy_id, limit=limit)


def _request_id() -> str:
    value = getattr(g, "request_id", "")
    return value if isinstance(value, str) and value else uuid.uuid4().hex


def _clamp_limit(value: Any, *, default: int = 50, minimum: int = 1, maximum: int = 200) -> int:
    try:
        out = int(str(value))
    except (TypeError, ValueError):
        out = default
    return max(minimum, min(maximum, out))


def _params_request_payload() -> dict[str, Any]:
    payload = request.get_json(silent=True)
    if not isinstance(payload, dict):
        return {}
    nested = payload.get("params")
    if isinstance(nested, dict):
        return dict(nested)
    return {key: value for key, value in payload.items() if key != "source"}


def _strict_json_value(value: Any) -> Any:
    if value is None or isinstance(value, bool | int | str):
        return value
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    if isinstance(value, float):
        return value if math.isfinite(value) else None
    if isinstance(value, Mapping):
        return {
            str(key): _strict_json_value(item)
            for key, item in value.items()
        }
    if isinstance(value, tuple | list | set | frozenset):
        return [_strict_json_value(item) for item in value]
    return str(value)


def create_flux_api_app(
    flux_config: FluxConfig,
    redis_client: RedisClientProtocol,
    *,
    contract_catalog: Sequence[ContractCatalogEntry],
    strategy_metadata: StrategyMetadata,
    strategy_metadata_resolver: Callable[[str], StrategyMetadata] | None = None,
    params_schema: Mapping[str, Mapping[str, Any]] | None = None,
    params_defaults: Mapping[str, Any] | None = None,
    required_readiness_keys: Sequence[str] | None = None,
) -> Flask:
    if not isinstance(flux_config, FluxConfig):
        raise TypeError("`flux_config` must be an instance of `FluxConfig`")
    if not isinstance(strategy_metadata, StrategyMetadata):
        raise TypeError("`strategy_metadata` must be an instance of `StrategyMetadata`")
    if strategy_metadata_resolver is not None and not callable(strategy_metadata_resolver):
        raise TypeError("`strategy_metadata_resolver` must be callable when provided")

    schema = params_schema or DEFAULT_PARAMS_SCHEMA
    defaults = params_defaults or DEFAULT_PARAMS_DEFAULTS
    store = FluxApiStore(
        flux_config=flux_config,
        redis_client=redis_client,
        contract_catalog=contract_catalog,
        params_schema=schema,
        params_defaults=defaults,
        required_readiness_keys=required_readiness_keys,
    )
    default_strategy_id = flux_config.identity.strategy_id

    def _resolve_strategy_id(raw_value: Any, *, field_name: str) -> str:
        text = decode_text(raw_value).strip()
        candidate = text or default_strategy_id
        try:
            return validate_identifier_part(candidate, field_name)
        except ValueError as exc:
            raise ApiEnvelopeError(
                status=400,
                code="invalid_strategy_id",
                message=str(exc),
                details={
                    "field": field_name,
                    "strategy_id": text or candidate,
                },
            ) from exc

    def _metadata_for_strategy(strategy_id: str) -> StrategyMetadata:
        metadata = strategy_metadata
        if strategy_metadata_resolver is not None:
            try:
                metadata = strategy_metadata_resolver(strategy_id)
            except Exception as exc:  # noqa: BLE001
                raise ApiEnvelopeError(
                    status=500,
                    code="config_validation_error",
                    message="Strategy metadata resolver failed.",
                    details={
                        "strategy_id": strategy_id,
                        "reason": str(exc),
                    },
                ) from exc
        if not isinstance(metadata, StrategyMetadata):
            raise ApiEnvelopeError(
                status=500,
                code="config_validation_error",
                message="Strategy metadata resolver returned invalid metadata type.",
                details={
                    "strategy_id": strategy_id,
                    "type": type(metadata).__name__,
                },
            )
        return metadata

    app = Flask(__name__)
    app.config["JSON_SORT_KEYS"] = False
    app.json.sort_keys = False

    def _response(
        *,
        ok: bool,
        data: Any,
        error: Mapping[str, Any] | None,
        status: int,
    ) -> Response:
        body = build_envelope(
            ok=ok,
            api_version=store.schema_version,
            request_id=_request_id(),
            timestamp_ms=now_ms(),
            data=data,
            error=error,
        )
        strict_body = _strict_json_value(body)
        encoded = json.dumps(strict_body, separators=(",", ":"), sort_keys=False, allow_nan=False)
        return Response(encoded, status=status, mimetype="application/json")

    def _error(
        *,
        status: int,
        code: str,
        message: str,
        details: Mapping[str, Any] | None = None,
    ) -> Response:
        return _response(
            ok=False,
            data=None,
            error=build_error(code=code, message=message, details=details),
            status=status,
        )

    def _ok(*, data: Any, status: int = 200) -> Response:
        return _response(ok=True, data=data, error=None, status=status)

    @app.before_request
    def _install_request_context() -> None:
        incoming = (
            request.headers.get("X-Request-Id")
            or request.headers.get("X-Request-ID")
            or request.headers.get("x-request-id")
        )
        value = decode_text(incoming).strip()
        g.request_id = value or uuid.uuid4().hex

    @app.get("/")
    def root() -> Response:
        return _ok(data={"service": "flux-api", "schema_prefix": store.schema_prefix}, status=200)

    @app.get("/api/v1/healthz")
    def healthz() -> Response:
        if not store.redis_available():
            return _error(
                status=503,
                code="redis_unavailable",
                message="Redis ping failed.",
                details={"schema_prefix": store.schema_prefix},
            )

        try:
            snapshot = store.readiness_snapshot()
        except Exception as exc:  # noqa: BLE001
            return _error(
                status=503,
                code="readiness_probe_failed",
                message="Readiness probe failed during health check.",
                details={
                    "schema_prefix": store.schema_prefix,
                    "reason": str(exc),
                },
            )

        return _ok(
            data={
                "redis_available": True,
                "schema_prefix": snapshot.schema_prefix,
                "schema_ready": snapshot.schema_ready,
                "required_keys": snapshot.required_keys,
            },
        )

    @app.get("/api/v1/readyz")
    def readyz() -> Response:
        if not store.redis_available():
            return _error(
                status=503,
                code="service_not_ready",
                message="Redis is unavailable.",
                details={"schema_prefix": store.schema_prefix},
            )

        try:
            snapshot = store.readiness_snapshot()
        except Exception as exc:  # noqa: BLE001
            return _error(
                status=503,
                code="service_not_ready",
                message="Readiness probe failed.",
                details={
                    "schema_prefix": store.schema_prefix,
                    "reason": str(exc),
                },
            )

        if not snapshot.schema_ready:
            missing = sorted(key for key, present in snapshot.required_keys.items() if not present)
            return _error(
                status=503,
                code="service_not_ready",
                message="Flux schema keys are not ready.",
                details={
                    "schema_prefix": snapshot.schema_prefix,
                    "required_keys": snapshot.required_keys,
                    "missing_keys": missing,
                },
            )

        return _ok(
            data={
                "redis_available": True,
                "schema_prefix": snapshot.schema_prefix,
                "schema_ready": True,
                "required_keys": snapshot.required_keys,
            },
            status=200,
        )

    @app.get("/api/v1/param-schema")
    def api_param_schema() -> Response:
        return _ok(data={"schema": _ordered_params_schema(schema)})

    @app.get("/api/v1/params")
    def api_params() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        try:
            params = store.load_params(strategy_id)
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        payload = build_params_payload(strategy_id=strategy_id, params=params, schema=_ordered_params_schema(schema))
        return _ok(data={"strategies": [payload]})

    @app.post("/api/v1/params")
    @app.patch("/api/v1/params")
    def api_params_update() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        updates = _params_request_payload()
        if not updates:
            return _error(
                status=400,
                code="missing_payload",
                message="Request JSON must include `params` mapping.",
                details={"strategy_id": strategy_id},
            )
        try:
            result = store.update_params(strategy_id, updates)
        except ParamsUpdateValidationError as exc:
            return _error(
                status=400,
                code="invalid_params_update",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        payload = {
            "strategy_id": strategy_id,
            "updated": result["updated"],
            "params": result["params"],
            "schema": _ordered_params_schema(schema),
        }
        return _ok(data=payload)

    @app.get("/api/v1/signals")
    def api_signals() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        try:
            strategy_payload = store.load_signals_payload(strategy_id, _metadata_for_strategy(strategy_id))
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        except redis.RedisError as exc:
            return _error(
                status=503,
                code="store_unavailable",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        return _ok(data={"server_ts_ms": now_ms(), "strategies": [strategy_payload]})

    @app.get("/api/v1/strategies")
    def api_strategies() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        try:
            strategy_payload = store.load_signals_payload(strategy_id, _metadata_for_strategy(strategy_id))
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        return _ok(
            data={
                "strategies": [strategy_payload],
                "count": 1,
            },
        )

    @app.get("/api/v1/strategies/<string:strategy_id>/parameters")
    def api_strategy_parameters(strategy_id: str) -> Response:
        sid = _resolve_strategy_id(strategy_id, field_name="strategy_id")
        try:
            params = store.load_params(sid)
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": sid},
            )
        payload = {"strategy_id": sid, "params": params, "schema": _ordered_params_schema(schema)}
        return _ok(data=payload)

    @app.post("/api/v1/strategies/<string:strategy_id>/parameters")
    @app.patch("/api/v1/strategies/<string:strategy_id>/parameters")
    def api_strategy_parameters_update(strategy_id: str) -> Response:
        sid = _resolve_strategy_id(strategy_id, field_name="strategy_id")
        updates = _params_request_payload()
        if not updates:
            return _error(
                status=400,
                code="missing_payload",
                message="Request JSON must include `params` mapping.",
                details={"strategy_id": sid},
            )
        try:
            result = store.update_params(sid, updates)
        except ParamsUpdateValidationError as exc:
            return _error(
                status=400,
                code="invalid_params_update",
                message=str(exc),
                details={"strategy_id": sid},
            )
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": sid},
            )
        payload = {
            "strategy_id": sid,
            "updated": result["updated"],
            "params": result["params"],
            "schema": _ordered_params_schema(schema),
        }
        return _ok(data=payload)

    @app.get("/api/v1/balances")
    def api_balances() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        rows = store.load_balances_rows(strategy_id)
        return _ok(
            data={
                "rows": rows[:limit],
                "count": len(rows),
                "server_ts_ms": now_ms(),
            },
        )

    @app.get("/api/v1/trades")
    def api_trades() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        rows = store.load_trades_rows(strategy_id, limit=limit, since_ms=None)
        return _ok(data={"rows": rows, "count": len(rows), "server_ts_ms": now_ms()})

    @app.get("/api/v1/trades/delta")
    def api_trades_delta() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        since_ms = coerce_ts_ms(request.args.get("after"))
        rows = store.load_trades_rows(strategy_id, limit=limit, since_ms=since_ms)
        return _ok(
            data={
                "rows": rows,
                "count": len(rows),
                "server_ts_ms": now_ms(),
                "after": since_ms,
            },
        )

    @app.get("/api/v1/alerts")
    def api_alerts() -> Response:
        strategy_id = _resolve_strategy_id(request.args.get("strategy"), field_name="strategy")
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        rows = store.load_alerts_rows(strategy_id, limit=limit)
        return _ok(data={"rows": rows, "count": len(rows), "server_ts_ms": now_ms()})

    @app.errorhandler(ApiEnvelopeError)
    def _handle_envelope_error(exc: ApiEnvelopeError) -> Response:
        return _error(
            status=exc.status,
            code=exc.code,
            message=exc.message,
            details=exc.details,
        )

    @app.errorhandler(redis.RedisError)
    def _handle_redis_error(exc: redis.RedisError) -> Response:
        return _error(status=503, code="store_unavailable", message=str(exc))

    @app.errorhandler(Exception)
    def _handle_uncaught(exc: Exception) -> Response:
        return _error(status=500, code="internal_error", message=str(exc))

    return app


__all__ = [
    "ApiEnvelopeError",
    "ContractCatalogValidationError",
    "DEFAULT_PARAMS_DEFAULTS",
    "DEFAULT_PARAMS_SCHEMA",
    "FluxApiStore",
    "ParamsStoreValidationError",
    "ParamsUpdateValidationError",
    "RedisClientProtocol",
    "ReadinessSnapshot",
    "create_flux_api_app",
]
