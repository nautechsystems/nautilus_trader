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
import logging
import math
import uuid
from collections.abc import Callable
from collections.abc import Mapping
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Any
from typing import Protocol

import redis
from flask import Flask
from flask import Response
from flask import g
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
from nautilus_trader.flux.api.payloads import contract_id_for_leg
from nautilus_trader.flux.api.payloads import decode_text
from nautilus_trader.flux.api.payloads import enrich_balances_rows
from nautilus_trader.flux.api.payloads import extract_stream_rows
from nautilus_trader.flux.api.payloads import filter_balance_rows_for_contract_scope
from nautilus_trader.flux.api.payloads import load_json
from nautilus_trader.flux.api.payloads import merge_portfolio_balances_rows
from nautilus_trader.flux.api.payloads import normalize_symbol_parts
from nautilus_trader.flux.api.payloads import now_ms
from nautilus_trader.flux.api.payloads import safe_int
from nautilus_trader.flux.api.payloads import select_latest_strategy_row
from nautilus_trader.flux.api.payloads import strategy_id_from_row
from nautilus_trader.flux.api.socketio import create_flux_socket_server
from nautilus_trader.flux.api.socketio import normalize_profile
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import validate_identifier_part
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_SCHEMA
from nautilus_trader.flux.params.manager import FluxParamsManager


DEFAULT_PARAMS_DEFAULTS: dict[str, Any] = dict(MAKERV3_RUNTIME_PARAM_DEFAULTS)
DEFAULT_PARAMS_SCHEMA: dict[str, dict[str, Any]] = {
    name: dict(spec) for name, spec in MAKERV3_RUNTIME_PARAM_SCHEMA.items()
}
DEFAULT_PARAMS_ORDER: tuple[str, ...] = MAKERV3_RUNTIME_PARAM_REGISTRY.names

_LOG = logging.getLogger(__name__)
TOKENMM_BALANCES_STALE_AFTER_MS = 30_000


class RedisPipelineProtocol(Protocol):
    def get(self, key: str) -> Any: ...
    def exists(self, key: str) -> Any: ...
    def xrevrange(
        self,
        key: str,
        max: str = "+",
        min: str = "-",
        count: int | None = None,
    ) -> Any: ...
    def execute(self) -> list[Any]: ...


class RedisClientProtocol(Protocol):
    def ping(self) -> Any: ...
    def get(self, key: str) -> Any: ...
    def xrevrange(
        self,
        key: str,
        max: str = "+",
        min: str = "-",
        count: int | None = None,
    ) -> Any: ...
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
                raise ContractCatalogValidationError(
                    "`required_readiness_keys` must contain non-empty strings",
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
                    f"`contract_catalog[{index}]` must be `ContractCatalogEntry`, was {type(contract).__name__}",
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
            except (TypeError, ValueError) as e:
                raise ContractCatalogValidationError(
                    f"Invalid contract catalog entry exchange={exchange!r} symbol={symbol!r}: {e}",
                ) from e

            dedupe_key = (exchange, base, quote)
            if dedupe_key in seen:
                raise ContractCatalogValidationError(
                    "Duplicate contract catalog entry after normalization: "
                    f"exchange={exchange!r} symbol={symbol!r} (base={base!r} quote={quote!r})",
                )
            seen.add(dedupe_key)
            out.append((ContractCatalogEntry(exchange=exchange, symbol=symbol), base, quote))

        if not out:
            raise ContractCatalogValidationError(
                "`contract_catalog` produced no valid contract entries",
            )
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
            for key, value in zip(self._required_readiness_keys, exists_raw, strict=True)
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
        except ValueError as e:
            raise ParamsStoreValidationError(str(e)) from e

    def _strategy_id_from_params_key(self, raw_key: Any, *, key_prefix: str) -> str | None:
        key = decode_text(raw_key).strip()
        if not key.startswith(key_prefix):
            return None
        strategy_text = key[len(key_prefix) :].strip()
        if not strategy_text:
            return None
        try:
            return validate_identifier_part(strategy_text, "strategy_id")
        except ValueError:
            return None

    def discover_strategy_ids_from_params(  # noqa: C901
        self,
        *,
        limit: int = 200,
        include_default: bool = True,
    ) -> list[str]:
        max_items = max(1, min(2_000, int(limit)))
        key_prefix = (
            f"{self._config.identity.namespace}:{self._config.identity.schema_version}:params:"
        )
        discovered: set[str] = set()

        keys_fn = getattr(self._redis, "keys", None)
        if callable(keys_fn):
            try:
                for raw_key in keys_fn(f"{key_prefix}*"):
                    if len(discovered) >= max_items:
                        break
                    strategy_id = self._strategy_id_from_params_key(raw_key, key_prefix=key_prefix)
                    if strategy_id:
                        discovered.add(strategy_id)
            except Exception as e:
                _LOG.debug(
                    "Strategy-id discovery via redis.keys() failed prefix=%s error=%s",
                    key_prefix,
                    type(e).__name__,
                    exc_info=True,
                )

        hashes = getattr(self._redis, "hashes", None)
        if isinstance(hashes, dict):
            for raw_key in hashes:
                if len(discovered) >= max_items:
                    break
                strategy_id = self._strategy_id_from_params_key(raw_key, key_prefix=key_prefix)
                if strategy_id:
                    discovered.add(strategy_id)

        default_strategy_id = self._config.identity.strategy_id
        if include_default and len(discovered) < max_items:
            discovered.add(default_strategy_id)

        return sorted(discovered)

    def update_params(self, strategy_id: str, updates: Mapping[str, Any]) -> dict[str, Any]:
        manager = self._params_manager(strategy_id)
        if not updates:
            params = self.load_params(strategy_id)
            return {"updated": [], "params": params}

        try:
            applied_updates = manager.update(updates)
        except ValueError as e:
            raise ParamsUpdateValidationError(str(e)) from e
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
        for (contract, _), market_raw in zip(market_pairs, raw[3:], strict=True):
            parsed = load_json(market_raw)
            contract_id = contract_id_for_leg(exchange=contract.exchange, symbol=contract.symbol)
            market_rows[contract_id] = dict(parsed) if isinstance(parsed, dict) else {}
        legs = build_legs_payload(
            contracts=self._contracts,
            market_rows=market_rows,
            now_ms_value=now_ms(),
        )

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
        rows, _snapshot_present = self.load_balances_rows_with_presence(strategy_id)
        return rows

    def load_balances_rows_with_presence(self, strategy_id: str) -> tuple[list[dict[str, Any]], bool]:
        keys = self._keys_for_strategy(strategy_id)
        market_pairs = self._market_keys(strategy_id)
        pipe = self._redis.pipeline(transaction=False)
        pipe.get(keys.balances_snapshot())
        for _, market_key in market_pairs:
            pipe.get(market_key)
        raw = pipe.execute()
        expected_length = 1 + len(market_pairs)
        if len(raw) != expected_length:
            raise RuntimeError(
                f"Balances pipeline returned {len(raw)} rows, expected {expected_length}",
            )

        raw_snapshot = raw[0]
        balances_raw = load_json(raw_snapshot)
        rows = build_balances_rows(raw_snapshot=balances_raw, strategy_id=strategy_id)

        market_rows: dict[str, dict[str, Any]] = {}
        for (contract, _), market_raw in zip(market_pairs, raw[1:], strict=True):
            parsed = load_json(market_raw)
            contract_id = contract_id_for_leg(exchange=contract.exchange, symbol=contract.symbol)
            market_rows[contract_id] = dict(parsed) if isinstance(parsed, dict) else {}

        return (
            enrich_balances_rows(
                rows,
                contracts=self._contracts,
                market_rows=market_rows,
            ),
            raw_snapshot is not None,
        )

    def load_trades_rows(
        self,
        strategy_id: str,
        *,
        limit: int,
        since_ms: int | None,
        since_seq: int | None = None,
        scan_limit: int | None = None,
    ) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        if scan_limit is not None:
            fetch_count = max(1, min(2_000, scan_limit))
        else:
            fetch_count = max(
                1,
                min(
                    2_000,
                    (limit * 4) if (since_ms is not None or since_seq is not None) else limit,
                ),
            )
        entries = self._redis.xrevrange(keys.trades_stream(), count=fetch_count)
        rows = extract_stream_rows(entries)
        return build_trades_rows(
            rows=rows,
            strategy_id=strategy_id,
            limit=limit,
            since_ms=since_ms,
            since_seq=since_seq,
        )

    def load_all_trades_rows(self, strategy_id: str) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        entries = self._redis.xrevrange(keys.trades_stream())
        rows = extract_stream_rows(entries)
        filtered = [row for row in rows if strategy_id_from_row(row, strategy_id) == strategy_id]
        return build_trades_rows(
            rows=filtered,
            strategy_id=strategy_id,
            limit=max(1, len(filtered)),
            since_ms=None,
            since_seq=None,
        )

    def load_alerts_rows(self, strategy_id: str, *, limit: int) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        fetch_count = max(1, min(2_000, limit * 2))
        entries = self._redis.xrevrange(keys.alerts(), count=fetch_count)
        rows = extract_stream_rows(entries)
        return build_alerts_rows(rows=rows, strategy_id=strategy_id, limit=limit)

    def load_all_alerts_rows(self, strategy_id: str) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        entries = self._redis.xrevrange(keys.alerts())
        rows = extract_stream_rows(entries)
        filtered = [row for row in rows if strategy_id_from_row(row, strategy_id) == strategy_id]
        return build_alerts_rows(
            rows=filtered,
            strategy_id=strategy_id,
            limit=max(1, len(filtered)),
        )

    def trades_stream_len(self, strategy_id: str) -> int | None:
        keys = self._keys_for_strategy(strategy_id)
        stream_key = keys.trades_stream()
        xlen_fn = getattr(self._redis, "xlen", None)
        if callable(xlen_fn):
            size = safe_int(xlen_fn(stream_key))
            return max(0, size or 0)
        streams = getattr(self._redis, "streams", None)
        if isinstance(streams, dict):
            rows = streams.get(stream_key)
            if isinstance(rows, list):
                return len(rows)
        return None

    def alerts_stream_len(self, strategy_id: str) -> int | None:
        keys = self._keys_for_strategy(strategy_id)
        stream_key = keys.alerts()
        xlen_fn = getattr(self._redis, "xlen", None)
        if callable(xlen_fn):
            size = safe_int(xlen_fn(stream_key))
            return max(0, size or 0)
        streams = getattr(self._redis, "streams", None)
        if isinstance(streams, dict):
            rows = streams.get(stream_key)
            if isinstance(rows, list):
                return len(rows)
        return None

    def clear_alerts(self, strategy_id: str) -> int:
        keys = self._keys_for_strategy(strategy_id)
        alerts_key = keys.alerts()

        pre_count: int | None = None
        xlen_fn = getattr(self._redis, "xlen", None)
        if callable(xlen_fn):
            size = safe_int(xlen_fn(alerts_key))
            pre_count = max(0, size or 0)
        streams = getattr(self._redis, "streams", None)
        if pre_count is None and isinstance(streams, dict):
            existing_rows = streams.get(alerts_key)
            if isinstance(existing_rows, list):
                pre_count = len(existing_rows)

        delete_fn = getattr(self._redis, "delete", None)
        if callable(delete_fn):
            deleted = safe_int(delete_fn(alerts_key))
            if (deleted or 0) <= 0:
                return 0
            if pre_count is not None:
                return pre_count
            return 1

        if isinstance(streams, dict):
            removed_rows = streams.pop(alerts_key, [])
            if isinstance(removed_rows, list):
                return len(removed_rows)
            return 1 if removed_rows else 0

        return 0


def _request_id() -> str:
    value = getattr(g, "request_id", "")
    return value if isinstance(value, str) and value else uuid.uuid4().hex


def _clamp_limit(value: Any, *, default: int = 50, minimum: int = 1, maximum: int = 200) -> int:
    try:
        out = int(str(value))
    except (TypeError, ValueError):
        out = default
    return max(minimum, min(maximum, out))


def _clamp_offset(value: Any, *, default: int = 0) -> int:
    try:
        out = int(str(value))
    except (TypeError, ValueError):
        out = default
    return max(0, out)


def _coerce_finite_float(value: Any) -> float | None:
    if value is None or isinstance(value, bool):
        return None
    if isinstance(value, int | float):
        out = float(value)
        return out if math.isfinite(out) else None
    text = decode_text(value).strip()
    if not text:
        return None
    try:
        out = float(text)
    except ValueError:
        return None
    return out if math.isfinite(out) else None


def _format_money_display(value: float) -> str:
    return f"{'-$' if value < 0 else '$'}{abs(value):.2f}"


def _balances_totals(rows: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    total_mv = 0.0
    for row in rows:
        mv = _coerce_finite_float(
            row.get("mv_raw")
            or row.get("mv")
            or row.get("notional")
            or row.get("notional_quote")
            or row.get("notional_usd"),
        )
        if mv is not None:
            total_mv += mv
    return {
        "mv_raw": total_mv,
        "mv_display": _format_money_display(total_mv),
    }


def _normalize_trade_side(value: Any) -> str:
    side = decode_text(value).strip().lower()
    if side in {"1", "buy", "bid"}:
        return "buy"
    if side in {"2", "sell", "ask"}:
        return "sell"
    return side


def _extract_last_seq(rows: Sequence[Mapping[str, Any]], *, fallback: int = 0) -> int:
    best = fallback
    for row in rows:
        seq = safe_int(row.get("seq"))
        if seq is not None and seq > best:
            best = seq
    return best


def _trade_replay_cursor(row: Mapping[str, Any]) -> tuple[int, str, int] | None:
    ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
    row_id = decode_text(row.get("row_id")).strip()
    version = safe_int(row.get("version")) or 1
    if ts_ms is None or not row_id:
        return None
    return (ts_ms, row_id, version)


def _trade_replay_sort_key(row: Mapping[str, Any]) -> tuple[int, str, int, str]:
    cursor = _trade_replay_cursor(row)
    if cursor is None:
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp")) or 0
        return (
            ts_ms,
            decode_text(row.get("row_id")).strip(),
            safe_int(row.get("version")) or 1,
            decode_text(row.get("strategy_id")).strip(),
        )
    return (*cursor, decode_text(row.get("strategy_id")).strip())


def _trade_sort_key(row: Mapping[str, Any]) -> tuple[int, int, str, str]:
    return (
        coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp")) or 0,
        safe_int(row.get("seq")) or 0,
        decode_text(row.get("strategy_id")).strip(),
        decode_text(row.get("row_id")).strip(),
    )


def _rows_after_trade_ts(
    rows: Sequence[Mapping[str, Any]],
    *,
    since_ms: int | None,
) -> list[dict[str, Any]]:
    if since_ms is None:
        return [dict(row) for row in rows]
    out: list[dict[str, Any]] = []
    for row in rows:
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        if ts_ms is None or ts_ms <= since_ms:
            continue
        out.append(dict(row))
    out.sort(key=_trade_sort_key)
    return out


def _rows_after_trade_replay_cursor(
    rows: Sequence[Mapping[str, Any]],
    *,
    after_ms: int | None,
    after_row_id: str = "",
    after_version: int | None = None,
) -> list[dict[str, Any]]:
    if after_ms is None:
        out = [dict(row) for row in rows]
        out.sort(key=_trade_replay_sort_key)
        return out

    cursor_after = (
        (after_ms, after_row_id, int(after_version or 1))
        if after_row_id
        else None
    )
    out: list[dict[str, Any]] = []
    for row in rows:
        replay_cursor = _trade_replay_cursor(row)
        if cursor_after is not None and replay_cursor is not None:
            if replay_cursor <= cursor_after:
                continue
        else:
            ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
            if ts_ms is None or ts_ms <= after_ms:
                continue
        out.append(dict(row))
    out.sort(key=_trade_replay_sort_key)
    return out


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
        return {str(key): _strict_json_value(item) for key, item in value.items()}
    if isinstance(value, tuple | list | set | frozenset):
        return [_strict_json_value(item) for item in value]
    return str(value)


def create_flux_api_app(  # noqa: C901
    flux_config: FluxConfig,
    redis_client: RedisClientProtocol,
    *,
    contract_catalog: Sequence[ContractCatalogEntry],
    strategy_metadata: StrategyMetadata,
    strategy_metadata_resolver: Callable[[str], StrategyMetadata] | None = None,
    profile_strategy_map: Mapping[str, str | Sequence[str]] | None = None,
    profile_required_strategy_map: Mapping[str, str | Sequence[str]] | None = None,
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
        except ValueError as e:
            raise ApiEnvelopeError(
                status=400,
                code="invalid_strategy_id",
                message=str(e),
                details={
                    "field": field_name,
                    "strategy_id": text or candidate,
                },
            ) from e

    def _metadata_for_strategy(strategy_id: str) -> StrategyMetadata:
        metadata = strategy_metadata
        if strategy_metadata_resolver is not None:
            try:
                metadata = strategy_metadata_resolver(strategy_id)
            except Exception as e:
                raise ApiEnvelopeError(
                    status=500,
                    code="config_validation_error",
                    message="Strategy metadata resolver failed.",
                    details={
                        "strategy_id": strategy_id,
                        "error_type": type(e).__name__,
                    },
                ) from e
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

    def _coerce_strategy_ids(raw_value: Any) -> list[str]:
        values: list[Any]
        if isinstance(raw_value, str):
            values = [raw_value]
        elif isinstance(raw_value, Sequence) and not isinstance(raw_value, bytes):
            values = list(raw_value)
        else:
            return []
        out: list[str] = []
        seen: set[str] = set()
        for value in values:
            text = decode_text(value).strip()
            if not text:
                continue
            try:
                strategy_id = validate_identifier_part(text, "strategy_id")
            except ValueError:
                continue
            if strategy_id in seen:
                continue
            seen.add(strategy_id)
            out.append(strategy_id)
        return out

    resolved_profile_strategy_map: dict[str, list[str]] = {
        "tokenmm": [default_strategy_id],
    }
    if profile_strategy_map is not None:
        for profile_name, raw_ids in profile_strategy_map.items():
            normalized = normalize_profile(profile_name)
            ids = _coerce_strategy_ids(raw_ids)
            if not ids:
                continue
            resolved_profile_strategy_map[normalized] = ids

    resolved_profile_required_strategy_map: dict[str, list[str]] = {}
    if profile_required_strategy_map is not None:
        for profile_name, raw_ids in profile_required_strategy_map.items():
            normalized = normalize_profile(profile_name)
            ids = _coerce_strategy_ids(raw_ids)
            if ids:
                resolved_profile_required_strategy_map[normalized] = ids

    for profile_name, required_ids in resolved_profile_required_strategy_map.items():
        strategy_ids = _coerce_strategy_ids(resolved_profile_strategy_map.get(profile_name))
        if not strategy_ids:
            raise ValueError(
                f"`profile_required_strategy_map[{profile_name!r}]` requires matching strategy IDs in `profile_strategy_map`",
            )
        missing_ids = sorted(set(required_ids) - set(strategy_ids))
        if missing_ids:
            raise ValueError(
                f"`profile_required_strategy_map[{profile_name!r}]` must be a subset of `profile_strategy_map`; missing={missing_ids}",
            )

    def _strategy_ids_for_profile(profile: str) -> list[str]:
        normalized = normalize_profile(profile)
        return _coerce_strategy_ids(resolved_profile_strategy_map.get(normalized))

    def _required_strategy_ids_for_profile(
        profile: str,
        *,
        fallback: Sequence[str],
    ) -> list[str]:
        normalized = normalize_profile(profile)
        mapped_ids = _coerce_strategy_ids(resolved_profile_required_strategy_map.get(normalized))
        if mapped_ids:
            return mapped_ids
        return list(fallback)

    def _strategy_for_profile(profile: str) -> str | None:
        strategy_ids = _strategy_ids_for_profile(profile)
        return strategy_ids[0] if strategy_ids else None

    def _resolve_strategy_id_for_request(*, field_name: str = "strategy") -> str:
        strategy_raw = request.args.get("strategy")
        strategy_text = decode_text(strategy_raw).strip()
        if strategy_text:
            return _resolve_strategy_id(strategy_text, field_name=field_name)

        profile_text = decode_text(request.args.get("profile")).strip()
        if profile_text:
            resolved_strategy = _strategy_for_profile(profile_text)
            if resolved_strategy:
                return _resolve_strategy_id(resolved_strategy, field_name=field_name)

        return _resolve_strategy_id(None, field_name=field_name)

    create_flux_socket_server(
        app,
        store=store,
        metadata_resolver=_metadata_for_strategy,
        strategy_resolver=_strategy_for_profile,
        strategy_ids_resolver=_strategy_ids_for_profile,
    )
    app.extensions["flux_profile_strategy_map"] = dict(resolved_profile_strategy_map)
    app.extensions["flux_profile_required_strategy_map"] = dict(resolved_profile_required_strategy_map)

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
        except Exception as e:
            return _error(
                status=503,
                code="readiness_probe_failed",
                message="Readiness probe failed during health check.",
                details={
                    "schema_prefix": store.schema_prefix,
                    "reason": "internal_error",
                    "error_type": type(e).__name__,
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
        except Exception as e:
            return _error(
                status=503,
                code="service_not_ready",
                message="Readiness probe failed.",
                details={
                    "schema_prefix": store.schema_prefix,
                    "reason": "internal_error",
                    "error_type": type(e).__name__,
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
        ordered_schema = _ordered_params_schema(schema)
        return _ok(data={"params": ordered_schema, "deprecated": {}})

    @app.get("/api/v1/params")
    def api_params() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        else:
            profile = decode_text(request.args.get("profile")).strip()
            strategy_ids = _strategy_ids_for_profile(profile) if profile else []
            if not strategy_ids:
                strategy_ids = [default_strategy_id]

        payloads: list[dict[str, Any]] = []
        for strategy_id in strategy_ids:
            try:
                params = store.load_params(strategy_id)
            except ParamsStoreValidationError as e:
                return _error(
                    status=500,
                    code="params_store_invalid",
                    message=str(e),
                    details={"strategy_id": strategy_id},
                )
            payloads.append(
                build_params_payload(
                    strategy_id=strategy_id,
                    params=params,
                    schema=_ordered_params_schema(schema),
                ),
            )
        return _ok(data=payloads)

    def _record_failed(failed: list[str], strategy_id: str) -> None:
        if strategy_id not in failed:
            failed.append(strategy_id)

    def _parse_updates_list(
        updates_raw: Sequence[Any],
        *,
        failed: list[str],
        errors: list[dict[str, Any]],
    ) -> list[tuple[int, str, dict[str, Any]]]:
        updates_batch: list[tuple[int, str, dict[str, Any]]] = []
        for index, item in enumerate(updates_raw):
            if not isinstance(item, dict):
                errors.append(
                    {
                        "index": index,
                        "strategy_id": "",
                        "code": "missing_payload",
                        "message": "Each `updates` item must be an object.",
                    },
                )
                continue
            sid_text = decode_text(item.get("strategy_id")).strip()
            if not sid_text:
                _record_failed(failed, "")
                errors.append(
                    {
                        "index": index,
                        "strategy_id": "",
                        "code": "invalid_strategy_id",
                        "message": f"`updates[{index}].strategy_id` must be a non-empty string.",
                    },
                )
                continue
            try:
                sid = validate_identifier_part(sid_text, f"updates[{index}].strategy_id")
            except ValueError as e:
                _record_failed(failed, sid_text)
                errors.append(
                    {
                        "index": index,
                        "strategy_id": sid_text,
                        "code": "invalid_strategy_id",
                        "message": str(e),
                    },
                )
                continue
            params = item.get("params")
            if not isinstance(params, dict):
                _record_failed(failed, sid)
                errors.append(
                    {
                        "index": index,
                        "strategy_id": sid,
                        "code": "missing_payload",
                        "message": "Each `updates` item must include a `params` mapping.",
                    },
                )
                continue
            updates_batch.append((index, sid, dict(params)))
        return updates_batch

    def _apply_updates_batch(
        updates_batch: Sequence[tuple[int, str, dict[str, Any]]],
        *,
        failed: list[str],
        errors: list[dict[str, Any]],
    ) -> list[dict[str, Any]]:
        success: list[dict[str, Any]] = []
        for index, strategy_id, updates in updates_batch:
            try:
                result = store.update_params(strategy_id, updates)
            except ParamsUpdateValidationError as e:
                _record_failed(failed, strategy_id)
                errors.append(
                    {
                        "index": index,
                        "strategy_id": strategy_id,
                        "code": "invalid_params_update",
                        "message": str(e),
                    },
                )
            except ParamsStoreValidationError as e:
                _record_failed(failed, strategy_id)
                errors.append(
                    {
                        "index": index,
                        "strategy_id": strategy_id,
                        "code": "params_store_invalid",
                        "message": str(e),
                    },
                )
            except Exception as e:
                _record_failed(failed, strategy_id)
                errors.append(
                    {
                        "index": index,
                        "strategy_id": strategy_id,
                        "code": "internal_error",
                        "message": "Internal server error.",
                        "details": {"error_type": type(e).__name__},
                    },
                )
            else:
                success.append(
                    {
                        "strategy_id": strategy_id,
                        "updated": result["updated"],
                        "params": result["params"],
                    },
                )
        return success

    @app.post("/api/v1/params")
    @app.patch("/api/v1/params")
    def api_params_update() -> Response:
        payload = request.get_json(silent=True)
        failed: list[str] = []
        errors: list[dict[str, Any]] = []
        updates_batch: list[tuple[int, str, dict[str, Any]]] = []

        if isinstance(payload, dict) and isinstance(payload.get("updates"), list):
            updates_batch = _parse_updates_list(
                payload.get("updates") or [],
                failed=failed,
                errors=errors,
            )
        else:
            strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
            updates = _params_request_payload()
            if not updates:
                return _error(
                    status=400,
                    code="missing_payload",
                    message="Request JSON must include `params` mapping.",
                    details={"strategy_id": strategy_id},
                )
            updates_batch = [(0, strategy_id, updates)]

        if not updates_batch and errors:
            return _ok(data={"success": [], "failed": failed, "errors": errors}, status=200)

        success = _apply_updates_batch(updates_batch, failed=failed, errors=errors)
        return _ok(data={"success": success, "failed": failed, "errors": errors})

    @app.get("/api/v1/signals")
    def api_signals() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)

        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        elif profile_normalized == "tokenmm":
            strategy_ids = _strategy_ids_for_profile(profile_text)
            if not strategy_ids:
                strategy_ids = [default_strategy_id]
        else:
            strategy_ids = [_resolve_strategy_id_for_request(field_name="strategy")]

        strategy_payloads: list[dict[str, Any]] = []
        for strategy_id in strategy_ids:
            try:
                strategy_payload = store.load_signals_payload(
                    strategy_id,
                    _metadata_for_strategy(strategy_id),
                )
            except ParamsStoreValidationError as e:
                return _error(
                    status=500,
                    code="params_store_invalid",
                    message=str(e),
                    details={"strategy_id": strategy_id},
                )
            except redis.RedisError as e:
                return _error(
                    status=503,
                    code="store_unavailable",
                    message="Data store unavailable.",
                    details={"strategy_id": strategy_id, "error_type": type(e).__name__},
                )
            strategy_payloads.append(strategy_payload)

        return _ok(data={"server_ts_ms": now_ms(), "strategies": strategy_payloads})

    @app.get("/api/v1/strategies")
    def api_strategies() -> Response:
        strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
        try:
            strategy_payload = store.load_signals_payload(
                strategy_id,
                _metadata_for_strategy(strategy_id),
            )
        except ParamsStoreValidationError as e:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(e),
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
        except ParamsStoreValidationError as e:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(e),
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
        except ParamsUpdateValidationError as e:
            return _error(
                status=400,
                code="invalid_params_update",
                message=str(e),
                details={"strategy_id": sid},
            )
        except ParamsStoreValidationError as e:
            return _error(
                status=500,
                code="params_store_invalid",
                message=str(e),
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
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)

        if requested_strategy:
            strategy_id = _resolve_strategy_id(requested_strategy, field_name="strategy")
            rows = store.load_balances_rows(strategy_id)
            response_ts_ms = now_ms()
        elif profile_normalized == "tokenmm":
            strategy_ids = _strategy_ids_for_profile(profile_text)
            if not strategy_ids:
                strategy_ids = [default_strategy_id]
            required_strategy_ids = set(
                _required_strategy_ids_for_profile(profile_text, fallback=strategy_ids),
            )
            response_ts_ms = now_ms()
            rows_by_strategy: dict[str, list[dict[str, Any]]] = {}
            components: list[dict[str, Any]] = []

            for strategy_id in strategy_ids:
                strategy_rows, snapshot_present = store.load_balances_rows_with_presence(strategy_id)
                rows_by_strategy[strategy_id] = strategy_rows
                latest_ts_ms: int | None = None
                for row in strategy_rows:
                    parsed = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
                    if parsed is None:
                        continue
                    if latest_ts_ms is None or parsed > latest_ts_ms:
                        latest_ts_ms = parsed
                age_ms = (response_ts_ms - latest_ts_ms) if latest_ts_ms is not None else None
                stale = (
                    not snapshot_present
                    or latest_ts_ms is None
                    or (age_ms is not None and age_ms > TOKENMM_BALANCES_STALE_AFTER_MS)
                )
                missing = (not snapshot_present) or not strategy_rows
                components.append(
                    {
                        "strategy_id": strategy_id,
                        "snapshot_present": snapshot_present,
                        "rows": len(strategy_rows),
                        "latest_ts_ms": latest_ts_ms,
                        "age_ms": age_ms,
                        "stale": stale,
                        "required": strategy_id in required_strategy_ids,
                        "missing": missing,
                    },
                )

            rows = merge_portfolio_balances_rows(
                rows_by_strategy=rows_by_strategy,
                portfolio_id="tokenmm",
            )
            rows = filter_balance_rows_for_contract_scope(
                rows,
                contracts=store._contracts,
            )
            missing_required = sorted(
                component["strategy_id"]
                for component in components
                if component["required"] and component["missing"]
            )
            degraded = bool(missing_required) or any(component["stale"] for component in components)
            total_rows = len(rows)
            return _ok(
                data={
                    "rows": rows[:limit],
                    "count": total_rows,
                    "total": total_rows,
                    "limit": limit,
                    "totals": _balances_totals(rows),
                    "server_ts_ms": response_ts_ms,
                    "components": components,
                    "degraded": degraded,
                    "missing_required": missing_required,
                    "stale_after_ms": TOKENMM_BALANCES_STALE_AFTER_MS,
                },
            )
        else:
            strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
            rows = store.load_balances_rows(strategy_id)
            response_ts_ms = now_ms()

        total_rows = len(rows)
        return _ok(
            data={
                "rows": rows[:limit],
                "count": total_rows,
                "total": total_rows,
                "limit": limit,
                "totals": _balances_totals(rows),
                "server_ts_ms": response_ts_ms,
            },
        )

    @app.get("/api/v1/trades")
    def api_trades() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)
        requested_limit_raw = request.args.get("limit")
        requested_limit = safe_int(requested_limit_raw)
        limit = _clamp_limit(requested_limit_raw, default=50, minimum=1, maximum=200)
        offset = _clamp_offset(request.args.get("offset"), default=0)
        coin_filter = decode_text(request.args.get("coin")).strip().upper()
        exchange_filter = decode_text(request.args.get("exchange")).strip().lower()
        side_filter = _normalize_trade_side(request.args.get("side"))
        signal_id_filter = decode_text(request.args.get("signal_id")).strip()
        sort_raw = decode_text(request.args.get("sort")).strip().lower()
        sort_ascending = sort_raw in {"asc", "ts_ms_asc"}
        sort_label = "ts_ms_asc" if sort_ascending else "ts_ms_desc"
        has_filters = bool(coin_filter or exchange_filter or side_filter or signal_id_filter)

        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        elif profile_normalized == "tokenmm":
            strategy_ids = _strategy_ids_for_profile(profile_text)
            if not strategy_ids:
                strategy_ids = [default_strategy_id]
        else:
            strategy_ids = [_resolve_strategy_id_for_request(field_name="strategy")]

        tokenmm_fanout = (
            not requested_strategy
            and profile_normalized == "tokenmm"
            and len(strategy_ids) > 1
        )

        source_rows: list[dict[str, Any]] = []
        total_count_override: int | None = None
        if has_filters or sort_ascending:
            for strategy_id in strategy_ids:
                strategy_rows = store.load_all_trades_rows(strategy_id)
                for row in strategy_rows:
                    normalized_row = dict(row)
                    normalized_row.setdefault("strategy_id", strategy_id)
                    source_rows.append(normalized_row)
        else:
            total_count_override = 0
            page_span = max(1, offset + limit)
            for strategy_id in strategy_ids:
                strategy_rows = store.load_trades_rows(
                    strategy_id,
                    limit=page_span,
                    since_ms=None,
                    since_seq=None,
                )
                for row in strategy_rows:
                    normalized_row = dict(row)
                    normalized_row.setdefault("strategy_id", strategy_id)
                    source_rows.append(normalized_row)
                strategy_total = store.trades_stream_len(strategy_id)
                if strategy_total is None:
                    total_count_override = None
                elif total_count_override is not None:
                    total_count_override += int(strategy_total)

        filtered_rows: list[dict[str, Any]] = []
        for row in source_rows:
            coin = decode_text(row.get("coin") or row.get("asset")).strip().upper()
            if coin_filter and coin != coin_filter:
                continue
            exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
            if exchange_filter and exchange != exchange_filter:
                continue
            side = _normalize_trade_side(row.get("side"))
            if side_filter and side != side_filter:
                continue
            signal_id = decode_text(row.get("signal_id") or row.get("strategy_id")).strip()
            if signal_id_filter and signal_id != signal_id_filter:
                continue
            filtered_rows.append(row)

        if tokenmm_fanout:
            filtered_rows.sort(
                key=_trade_sort_key,
                reverse=not sort_ascending,
            )
            # Multi-strategy TokenMM trades do not expose a synthetic global sequence cursor.
            last_seq = 0
        else:
            filtered_rows.sort(
                key=_trade_sort_key,
                reverse=not sort_ascending,
            )
            last_seq = _extract_last_seq(filtered_rows, fallback=0)

        total = total_count_override if total_count_override is not None else len(filtered_rows)
        rows = filtered_rows[offset : offset + limit]
        has_more = (offset + len(rows)) < total
        payload: dict[str, Any] = {
            "rows": rows,
            "total": total,
            "limit": limit,
            "requested_limit": int(requested_limit if requested_limit is not None else limit),
            "effective_limit": limit,
            "max_limit": 200,
            "offset": offset,
            "has_more": has_more,
            "last_seq": last_seq,
            "sort": sort_label,
        }
        if has_more:
            payload["next_offset"] = offset + len(rows)
        return _ok(data=payload)

    @app.get("/api/v1/trades/delta")
    def api_trades_delta() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)
        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        elif profile_normalized == "tokenmm":
            strategy_ids = _strategy_ids_for_profile(profile_text)
            if not strategy_ids:
                strategy_ids = [default_strategy_id]
        else:
            strategy_ids = [_resolve_strategy_id_for_request(field_name="strategy")]

        tokenmm_fanout = (
            not requested_strategy
            and profile_normalized == "tokenmm"
            and len(strategy_ids) > 1
        )
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        since_seq = safe_int(request.args.get("since_seq"))
        since_ms = None if since_seq is not None else coerce_ts_ms(request.args.get("after"))
        after_row_id = decode_text(request.args.get("after_row_id")).strip()
        after_version = safe_int(request.args.get("after_version"))
        fallback_seq = since_seq or 0

        if tokenmm_fanout:
            if since_seq is not None:
                # Safe Phase 1 behavior: multi-strategy TokenMM delta does not claim
                # a synthetic global cursor; clients should resync to snapshot.
                return _ok(
                    data={
                        "rows": [],
                        "last_seq": 0,
                        "reset_required": since_seq > 0,
                    },
                )

            rows: list[dict[str, Any]] = []
            for strategy_id in strategy_ids:
                strategy_rows = _rows_after_trade_replay_cursor(
                    store.load_all_trades_rows(strategy_id),
                    after_ms=since_ms,
                    after_row_id=after_row_id,
                    after_version=after_version,
                )
                for row in strategy_rows:
                    normalized_row = dict(row)
                    normalized_row.setdefault("strategy_id", strategy_id)
                    rows.append(normalized_row)
            rows.sort(key=_trade_replay_sort_key)
            return _ok(
                data={
                    "rows": rows[:limit],
                    "last_seq": 0,
                    "reset_required": False,
                },
            )

        strategy_id = strategy_ids[0]
        if since_seq is not None:
            scan_limit = 2_000
            scanned_rows = store.load_trades_rows(
                strategy_id,
                limit=scan_limit,
                since_ms=None,
                since_seq=None,
                scan_limit=scan_limit,
            )
            seq_values = [safe_int(row.get("seq")) for row in scanned_rows]
            parsed_seqs = [seq for seq in seq_values if seq is not None]
            if not parsed_seqs:
                reset_required = since_seq > 0
                return _ok(
                    data={
                        "rows": [],
                        "last_seq": 0,
                        "reset_required": reset_required,
                    },
                )

            min_seq = min(parsed_seqs)
            max_seq = max(parsed_seqs)
            if since_seq < (min_seq - 1):
                return _ok(
                    data={
                        "rows": [],
                        "last_seq": int(since_seq),
                        "reset_required": True,
                    },
                )

            if since_seq > max_seq:
                return _ok(
                    data={
                        "rows": [],
                        "last_seq": int(max_seq),
                        "reset_required": True,
                    },
                )

            eligible_rows: list[dict[str, Any]] = []
            for row in scanned_rows:
                seq = safe_int(row.get("seq"))
                if seq is None or seq <= since_seq:
                    continue
                eligible_rows.append(row)
            eligible_rows.sort(key=lambda item: safe_int(item.get("seq")) or 0)
            rows = eligible_rows[:limit]
            last_seq = safe_int(rows[-1].get("seq")) if rows else since_seq
            return _ok(
                data={
                    "rows": rows,
                    "last_seq": int(last_seq if last_seq is not None else since_seq),
                    "reset_required": False,
                },
            )

        if since_ms is not None:
            rows = _rows_after_trade_ts(store.load_all_trades_rows(strategy_id), since_ms=since_ms)
            rows = rows[:limit]
            return _ok(
                data={
                    "rows": rows,
                    "last_seq": _extract_last_seq(rows, fallback=fallback_seq),
                    "reset_required": False,
                },
            )

        rows = store.load_trades_rows(strategy_id, limit=limit, since_ms=since_ms, since_seq=None)
        return _ok(
            data={
                "rows": rows,
                "last_seq": _extract_last_seq(rows, fallback=fallback_seq),
                "reset_required": False,
            },
        )

    @app.get("/api/v1/alerts")
    def api_alerts() -> Response:
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        offset = _clamp_offset(request.args.get("offset"), default=0)
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)

        if requested_strategy:
            strategy_id = _resolve_strategy_id(requested_strategy, field_name="strategy")
            all_rows = store.load_all_alerts_rows(strategy_id)
        elif profile_normalized == "tokenmm":
            strategy_ids = _strategy_ids_for_profile(profile_text)
            if not strategy_ids:
                strategy_ids = [default_strategy_id]
            all_rows = []
            for strategy_id in strategy_ids:
                all_rows.extend(store.load_all_alerts_rows(strategy_id))
            all_rows.sort(
                key=lambda row: (
                    coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp")) or 0
                ),
                reverse=True,
            )
        else:
            strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
            all_rows = store.load_all_alerts_rows(strategy_id)

        total = len(all_rows)
        rows = all_rows[offset : offset + limit]
        has_more = (offset + len(rows)) < total
        payload: dict[str, Any] = {
            "rows": rows,
            "total": total,
            "limit": limit,
            "offset": offset,
            "has_more": has_more,
        }
        if has_more:
            payload["next_offset"] = offset + len(rows)
        return _ok(data=payload)

    @app.delete("/api/v1/alerts")
    def api_alerts_delete() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)

        if requested_strategy:
            strategy_id = _resolve_strategy_id(requested_strategy, field_name="strategy")
            deleted = store.clear_alerts(strategy_id)
            remaining = len(store.load_all_alerts_rows(strategy_id))
            payload: dict[str, Any] = {
                "success": True,
                "strategy_id": strategy_id,
                "deleted": deleted,
                "remaining": remaining,
                "server_ts_ms": now_ms(),
            }
            return _ok(data=payload)

        if profile_normalized == "tokenmm":
            strategy_ids = _strategy_ids_for_profile(profile_text)
            if not strategy_ids:
                strategy_ids = [default_strategy_id]
            deleted_total = 0
            remaining_by_strategy: dict[str, int] = {}
            for strategy_id in strategy_ids:
                deleted_total += store.clear_alerts(strategy_id)
                remaining_by_strategy[strategy_id] = len(store.load_all_alerts_rows(strategy_id))

            payload = {
                "success": True,
                "profile": profile_normalized,
                "strategy_ids": strategy_ids,
                "deleted": deleted_total,
                "remaining": sum(remaining_by_strategy.values()),
                "remaining_by_strategy": remaining_by_strategy,
                "server_ts_ms": now_ms(),
            }
            return _ok(data=payload)

        strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
        deleted = store.clear_alerts(strategy_id)
        remaining = len(store.load_all_alerts_rows(strategy_id))
        return _ok(
            data={
                "success": True,
                "strategy_id": strategy_id,
                "deleted": deleted,
                "remaining": remaining,
                "server_ts_ms": now_ms(),
            },
        )

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
        return _error(
            status=503,
            code="store_unavailable",
            message="Data store unavailable.",
            details={"error_type": type(exc).__name__},
        )

    @app.errorhandler(Exception)
    def _handle_uncaught(exc: Exception) -> Response:
        return _error(
            status=500,
            code="internal_error",
            message="Internal server error.",
            details={"error_type": type(exc).__name__},
        )

    return app


__all__ = [
    "DEFAULT_PARAMS_DEFAULTS",
    "DEFAULT_PARAMS_SCHEMA",
    "ApiEnvelopeError",
    "ContractCatalogValidationError",
    "FluxApiStore",
    "ParamsStoreValidationError",
    "ParamsUpdateValidationError",
    "ReadinessSnapshot",
    "RedisClientProtocol",
    "create_flux_api_app",
]
