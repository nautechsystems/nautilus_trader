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
import uuid
from typing import Any
from typing import Protocol

import redis
from flask import Flask
from flask import Response
from flask import g
from flask import jsonify
from flask import request

from nautilus_trader.flux.api.payloads import ContractCatalogEntry
from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.api.payloads import build_alerts_rows
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
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.params.manager import FluxParamsManager


DEFAULT_PARAMS_DEFAULTS: dict[str, Any] = {
    "qty": 1_000.0,
    "des_qty_global": 0.0,
    "max_qty_global": 40_000.0,
    "max_skew_bps_global": 20.0,
    "des_qty_local": 0.0,
    "max_qty_local": 0.0,
    "max_skew_bps_local": 0.0,
    "linear_offset_bps": 0.0,
    "n_orders1": 5,
    "distance1": 2.0,
    "bid_edge1": 10.0,
    "ask_edge1": 10.0,
    "place_edge1": 2.0,
    "n_orders2": 0,
    "distance2": 5.0,
    "bid_edge2": 25.0,
    "ask_edge2": 25.0,
    "place_edge2": 2.0,
    "n_orders3": 0,
    "distance3": 5.0,
    "bid_edge3": 50.0,
    "ask_edge3": 50.0,
    "place_edge3": 2.0,
    "quote_fail_critical_after_count": 3,
    "quote_fail_critical_after_s": 60.0,
    "max_age_ms": 10_000,
    "bot_on": False,
}

DEFAULT_PARAMS_SCHEMA: dict[str, dict[str, Any]] = {
    "qty": {"type": "number", "description": "Target base quantity per quote/hedge cycle."},
    "des_qty_global": {"type": "number", "description": "Global desired inventory target in base units."},
    "max_qty_global": {"type": "number", "description": "Global hard inventory cap in base units."},
    "max_skew_bps_global": {"type": "number", "description": "Global maker/hedge skew cap in bps."},
    "des_qty_local": {"type": "number", "description": "Local desired inventory target in base units."},
    "max_qty_local": {"type": "number", "description": "Local hard inventory cap in base units."},
    "max_skew_bps_local": {"type": "number", "description": "Local maker skew cap in bps."},
    "linear_offset_bps": {"type": "number", "description": "Linear inventory offset in bps."},
    "n_orders1": {"type": "integer", "description": "Band 1 order depth per side."},
    "distance1": {"type": "number", "description": "Band 1 spacing increment in bps."},
    "bid_edge1": {"type": "number", "description": "Band 1 bid edge in bps."},
    "ask_edge1": {"type": "number", "description": "Band 1 ask edge in bps."},
    "place_edge1": {"type": "number", "description": "Band 1 placement edge in bps."},
    "n_orders2": {"type": "integer", "description": "Band 2 order depth per side."},
    "distance2": {"type": "number", "description": "Band 2 spacing increment in bps."},
    "bid_edge2": {"type": "number", "description": "Band 2 bid edge in bps."},
    "ask_edge2": {"type": "number", "description": "Band 2 ask edge in bps."},
    "place_edge2": {"type": "number", "description": "Band 2 placement edge in bps."},
    "n_orders3": {"type": "integer", "description": "Band 3 order depth per side."},
    "distance3": {"type": "number", "description": "Band 3 spacing increment in bps."},
    "bid_edge3": {"type": "number", "description": "Band 3 bid edge in bps."},
    "ask_edge3": {"type": "number", "description": "Band 3 ask edge in bps."},
    "place_edge3": {"type": "number", "description": "Band 3 placement edge in bps."},
    "quote_fail_critical_after_count": {"type": "integer", "description": "Escalation count for quote failures."},
    "quote_fail_critical_after_s": {"type": "number", "description": "Escalation window for quote failures."},
    "max_age_ms": {"type": "integer", "description": "Replace managed orders older than this age."},
    "bot_on": {"type": "boolean", "description": "Enable quote publishing and management."},
}

DEFAULT_PARAMS_ORDER: tuple[str, ...] = (
    "qty",
    "des_qty_global",
    "max_qty_global",
    "max_skew_bps_global",
    "des_qty_local",
    "max_qty_local",
    "max_skew_bps_local",
    "linear_offset_bps",
    "n_orders1",
    "distance1",
    "bid_edge1",
    "ask_edge1",
    "place_edge1",
    "n_orders2",
    "distance2",
    "bid_edge2",
    "ask_edge2",
    "place_edge2",
    "n_orders3",
    "distance3",
    "bid_edge3",
    "ask_edge3",
    "place_edge3",
    "quote_fail_critical_after_count",
    "quote_fail_critical_after_s",
    "max_age_ms",
    "bot_on",
)


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
        required_readiness_keys: Sequence[str] | None = None,
    ) -> None:
        if not contract_catalog:
            raise ValueError("`contract_catalog` must not be empty")
        if not params_schema:
            raise ValueError("`params_schema` must not be empty")
        if not params_defaults:
            raise ValueError("`params_defaults` must not be empty")

        self._config = flux_config
        self._redis = redis_client
        self._contracts = tuple(contract_catalog)
        self._params_schema = _ordered_params_schema(params_schema)
        self._params_defaults = dict(params_defaults)

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
        )

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
        for contract in self._contracts:
            base, quote = normalize_symbol_parts(symbol=contract.symbol)
            if not base or not quote:
                raise ValueError(
                    f"Contract symbol did not resolve to base/quote parts: {contract.symbol!r}",
                )
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
            market_rows[contract.exchange] = dict(parsed) if isinstance(parsed, dict) else {}
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


def create_flux_api_app(
    flux_config: FluxConfig,
    redis_client: RedisClientProtocol,
    *,
    contract_catalog: Sequence[ContractCatalogEntry],
    strategy_metadata: StrategyMetadata,
    params_schema: Mapping[str, Mapping[str, Any]] | None = None,
    params_defaults: Mapping[str, Any] | None = None,
    required_readiness_keys: Sequence[str] | None = None,
) -> Flask:
    if not isinstance(flux_config, FluxConfig):
        raise TypeError("`flux_config` must be an instance of `FluxConfig`")

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
        return jsonify(body), status

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
        redis_available = store.redis_available()
        required_keys = {key: False for key in store.required_readiness_keys}
        schema_ready = False
        error = None
        if redis_available:
            try:
                snapshot = store.readiness_snapshot()
                required_keys = snapshot.required_keys
                schema_ready = snapshot.schema_ready
            except Exception as exc:  # noqa: BLE001
                error = build_error(
                    code="readiness_probe_failed",
                    message=str(exc),
                    details={"schema_prefix": store.schema_prefix},
                )
        else:
            error = build_error(
                code="redis_unavailable",
                message="Redis ping failed.",
                details={"schema_prefix": store.schema_prefix},
            )

        return _response(
            ok=redis_available,
            data={
                "redis_available": redis_available,
                "schema_prefix": store.schema_prefix,
                "schema_ready": schema_ready,
                "required_keys": required_keys,
            },
            error=error,
            status=200,
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
                message=str(exc),
                details={"schema_prefix": store.schema_prefix},
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
        try:
            strategy_payload = store.load_signals_payload(strategy_id, strategy_metadata)
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        except ValueError as exc:
            return _error(
                status=500,
                code="store_validation_error",
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
        try:
            strategy_payload = store.load_signals_payload(strategy_id, strategy_metadata)
        except ParamsStoreValidationError as exc:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(exc),
                details={"strategy_id": strategy_id},
            )
        except ValueError as exc:
            return _error(
                status=500,
                code="store_validation_error",
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
        sid = strategy_id or flux_config.identity.strategy_id
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
        sid = strategy_id or flux_config.identity.strategy_id
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        rows = store.load_trades_rows(strategy_id, limit=limit, since_ms=None)
        return _ok(data={"rows": rows, "count": len(rows), "server_ts_ms": now_ms()})

    @app.get("/api/v1/trades/delta")
    def api_trades_delta() -> Response:
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
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
        strategy_id = request.args.get("strategy") or flux_config.identity.strategy_id
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        rows = store.load_alerts_rows(strategy_id, limit=limit)
        return _ok(data={"rows": rows, "count": len(rows), "server_ts_ms": now_ms()})

    @app.errorhandler(redis.RedisError)
    def _handle_redis_error(exc: redis.RedisError) -> Response:
        return _error(status=503, code="store_unavailable", message=str(exc))

    @app.errorhandler(Exception)
    def _handle_uncaught(exc: Exception) -> Response:
        return _error(status=500, code="internal_error", message=str(exc))

    return app


__all__ = [
    "DEFAULT_PARAMS_DEFAULTS",
    "DEFAULT_PARAMS_SCHEMA",
    "FluxApiStore",
    "ParamsStoreValidationError",
    "ParamsUpdateValidationError",
    "RedisClientProtocol",
    "ReadinessSnapshot",
    "create_flux_api_app",
]
