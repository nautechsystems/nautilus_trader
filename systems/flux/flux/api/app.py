from __future__ import annotations

import importlib
import json
import logging
import math
import sys
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
from flask import has_request_context
from flask import request

from flux.api._payloads_balances import build_balance_risk_groups
from flux.api._payloads_balances import combine_portfolio_snapshot_rows
from flux.api._payloads_common import tokenmm_trade_rows_require_reset
from flux.api.payloads import ContractCatalogEntry
from flux.api.payloads import StrategyMetadata
from flux.api.payloads import build_alerts_rows
from flux.api.payloads import build_balances_rows
from flux.api.payloads import build_envelope
from flux.api.payloads import build_error
from flux.api.payloads import build_legs_payload
from flux.api.payloads import build_params_payload
from flux.api.payloads import build_signals_payload
from flux.api.payloads import build_trades_rows
from flux.api.payloads import coerce_ts_ms
from flux.api.payloads import collapse_balance_display_rows
from flux.api.payloads import contract_id_for_leg
from flux.api.payloads import decode_text
from flux.api.payloads import enrich_balances_rows
from flux.api.payloads import extract_stream_rows
from flux.api.payloads import filter_balance_rows_for_contract_scope
from flux.api.payloads import load_json
from flux.api.payloads import merge_portfolio_balances_rows
from flux.api.payloads import normalize_symbol_parts
from flux.api.payloads import now_ms
from flux.api.payloads import safe_bool
from flux.api.payloads import safe_float
from flux.api.payloads import safe_int
from flux.api.payloads import select_latest_strategy_row
from flux.api.payloads import strategy_id_from_row
from flux.api.socketio import REALTIME_STANDARD_CONTRACT_VERSION
from flux.api.socketio import build_standard_snapshot_metadata
from flux.api.socketio import create_flux_socket_server
from flux.api.socketio import default_realtime_rollout
from flux.api.socketio import normalize_profile
from flux.common.config import FluxConfig
from flux.common.config import validate_identifier_part
from flux.common.keys import FluxRedisKeys
from flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from flux.common.params import MAKERV3_RUNTIME_PARAM_SCHEMA
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.common.strategy_contracts import execution_account_scope_by_strategy_id
from flux.common.strategy_contracts import shared_observation_group_by_strategy_id
from flux.params.manager import FluxParamsManager
from flux.runners.shared.strategy_set import StrategySetDescriptor
from flux.runners.shared.strategy_set import get_strategy_set_descriptors


if __name__ == "flux.api.app":
    sys.modules.setdefault("nautilus_trader.flux.api.app", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.api.app":
    sys.modules.setdefault("flux.api.app", sys.modules[__name__])


DEFAULT_PARAMS_DEFAULTS: dict[str, Any] = dict(MAKERV3_RUNTIME_PARAM_DEFAULTS)
DEFAULT_PARAMS_SCHEMA: dict[str, dict[str, Any]] = {
    name: dict(spec) for name, spec in MAKERV3_RUNTIME_PARAM_SCHEMA.items()
}
DEFAULT_PARAMS_ORDER: tuple[str, ...] = MAKERV3_RUNTIME_PARAM_REGISTRY.names

_LOG = logging.getLogger(__name__)
TOKENMM_BALANCES_STALE_AFTER_MS = 30_000
PARAMS_RUNNING_STALE_AFTER_MS = 3_000
BALANCES_MAX_LIMIT = 200
STRATEGY_BALANCES_MAX_LIMIT = 5_000


def _tokenmm_trade_rows_require_reset_for_strategies(
    strategy_ids: Sequence[str],
    metadata_resolver: Callable[[str], StrategyMetadata],
    stream_reset_resolver: Callable[[str], bool],
) -> bool:
    if not strategy_ids:
        return False
    for strategy_id in strategy_ids:
        if not _strategy_groups_include_tokenmm(metadata_resolver(strategy_id)):
            continue
        if stream_reset_resolver(strategy_id):
            return True
    return False


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


StrategyRunningResolver = Callable[[Sequence[str]], Mapping[str, bool | None]]
StrategyAlertsResolver = Callable[[Sequence[str]], Mapping[str, Sequence[Mapping[str, Any]]]]


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


def _timestamp_is_fresh(
    ts_ms: Any,
    *,
    now_ms_value: int,
    stale_after_ms: int,
) -> bool:
    parsed = safe_int(ts_ms)
    return parsed is not None and (now_ms_value - parsed) <= stale_after_ms


def _projection_status_is_stale(projection_status: Mapping[str, Any] | None) -> bool:
    if not isinstance(projection_status, Mapping):
        return False
    last_attempt_ts_ms = safe_int(projection_status.get("last_attempt_ts_ms"))
    last_success_ts_ms = safe_int(projection_status.get("last_success_ts_ms"))
    stale_after_ms = safe_int(projection_status.get("stale_after_ms")) or 0
    if last_attempt_ts_ms is None or last_success_ts_ms is None or stale_after_ms <= 0:
        return not bool(projection_status.get("healthy", False))
    return (last_attempt_ts_ms - last_success_ts_ms) > stale_after_ms


def _normalize_scope_status_entries(payload: Any) -> list[dict[str, Any]]:
    if not isinstance(payload, Sequence) or isinstance(payload, str | bytes):
        return []
    return [dict(entry) for entry in payload if isinstance(entry, Mapping)]


def _projection_rows_excluded_from_reconciliation(payload: Any) -> bool:
    if not isinstance(payload, Sequence) or isinstance(payload, str | bytes):
        return False
    for row in payload:
        if not isinstance(row, Mapping):
            continue
        if bool(row.get("stale")) or row.get("include_in_reconciliation") is False:
            return True
    return False


def _merge_scope_status_entries(*groups: Sequence[Mapping[str, Any]] | None) -> list[dict[str, Any]]:
    merged: dict[tuple[str, str], dict[str, Any]] = {}
    ordered_keys: list[tuple[str, str]] = []
    for group in groups:
        if not isinstance(group, Sequence):
            continue
        for entry in group:
            if not isinstance(entry, Mapping):
                continue
            account_scope_id = decode_text(entry.get("account_scope_id")).strip()
            source_scope = decode_text(entry.get("source_scope") or "shared_account").strip() or "shared_account"
            if not account_scope_id:
                continue
            key = (account_scope_id, source_scope)
            if key not in merged:
                ordered_keys.append(key)
            merged[key] = dict(entry)
    return [merged[key] for key in ordered_keys]


def _scope_status_entries_degraded(scope_status: Sequence[Mapping[str, Any]] | None) -> bool:
    if not isinstance(scope_status, Sequence):
        return False
    for entry in scope_status:
        if not isinstance(entry, Mapping):
            continue
        projection_status = entry.get("projection_status")
        healthy = bool(projection_status.get("healthy", False)) if isinstance(projection_status, Mapping) else False
        if not healthy or _projection_status_is_stale(projection_status if isinstance(projection_status, Mapping) else None):
            return True
    return False


def _rows_for_reconciliation(rows: Sequence[Mapping[str, Any]] | None) -> list[dict[str, Any]]:
    if not isinstance(rows, Sequence):
        return []
    filtered: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, Mapping):
            continue
        if bool(row.get("stale")) or row.get("include_in_reconciliation") is False:
            continue
        filtered.append(dict(row))
    return filtered


def _balance_row_source_scope(row: Mapping[str, Any]) -> str:
    return decode_text(row.get("source_scope") or row.get("scope") or "").strip().lower()


def prefer_controller_managed_balance_rows(
    rows: Sequence[Mapping[str, Any]],
    *,
    controller_scope_by_account_scope: Mapping[str, str],
) -> list[dict[str, Any]]:
    normalized_rows = [dict(row) for row in rows if isinstance(row, Mapping)]
    if not normalized_rows or not controller_scope_by_account_scope:
        return normalized_rows

    grouped: dict[tuple[str, str], list[tuple[int, dict[str, Any], str]]] = {}
    for index, row in enumerate(normalized_rows):
        account_scope_id = decode_text(row.get("account_scope_id")).strip()
        controller_scope_id = decode_text(controller_scope_by_account_scope.get(account_scope_id)).strip()
        if not account_scope_id or not controller_scope_id:
            continue
        kind = decode_text(row.get("kind")).strip().lower()
        if kind and kind != "cash":
            continue
        asset = decode_text(row.get("asset")).strip().upper()
        if not asset:
            continue
        grouped.setdefault((account_scope_id, asset), []).append(
            (index, row, controller_scope_id),
        )

    if not grouped:
        return normalized_rows

    replacement_rows: dict[int, dict[str, Any]] = {}
    dropped_indexes: set[int] = set()
    for group_rows in grouped.values():
        shared_rows = [
            (index, row, controller_scope_id)
            for index, row, controller_scope_id in group_rows
            if _balance_row_source_scope(row) == "shared_account"
        ]
        if not shared_rows:
            continue
        winning_index, winning_row, controller_scope_id = max(
            shared_rows,
            key=lambda item: (
                safe_int(item[1].get("ts_ms")) or safe_int(item[1].get("server_ts_ms")) or -1,
                -item[0],
            ),
        )
        authoritative_row = dict(winning_row)
        authoritative_row["controller_scope_id"] = controller_scope_id
        authoritative_row["authority_state"] = "active"
        replacement_rows[winning_index] = authoritative_row
        for index, _row, _scope_id in group_rows:
            if index != winning_index:
                dropped_indexes.add(index)

    result: list[dict[str, Any]] = []
    for index, row in enumerate(normalized_rows):
        if index in dropped_indexes:
            continue
        result.append(replacement_rows.get(index, row))
    return result


def _ordered_params_schema(schema: Mapping[str, Mapping[str, Any]]) -> dict[str, dict[str, Any]]:
    ordered: dict[str, dict[str, Any]] = {}
    for name in DEFAULT_PARAMS_ORDER:
        if name in schema:
            ordered[name] = dict(schema[name])
    for name, spec in schema.items():
        if name not in ordered:
            ordered[str(name)] = dict(spec)
    return ordered


def _strategy_groups_include_tokenmm(metadata: StrategyMetadata) -> bool:
    groups = decode_text(metadata.strategy_groups).strip().lower()
    if not groups:
        return False
    return "tokenmm" in {part.strip() for part in groups.split(",") if part.strip()}


def _component_inventory_is_fresh(component_payload: Mapping[str, Any] | None) -> bool:
    if not isinstance(component_payload, Mapping):
        return False
    return not bool(component_payload.get("stale"))


@dataclass(frozen=True)
class ReadinessSnapshot:
    schema_prefix: str
    required_keys: dict[str, bool]
    schema_ready: bool


@dataclass(frozen=True)
class ParamsContract:
    schema: dict[str, dict[str, Any]]
    defaults: dict[str, Any]
    param_set: str


class FluxApiStore:
    def __init__(
        self,
        *,
        flux_config: FluxConfig,
        redis_client: RedisClientProtocol,
        contract_catalog: Sequence[ContractCatalogEntry],
        contract_catalog_resolver: Callable[[str], Sequence[ContractCatalogEntry]] | None = None,
        strategy_running_resolver: StrategyRunningResolver | None = None,
        strategy_alerts_resolver: StrategyAlertsResolver | None = None,
        params_schema: Mapping[str, Mapping[str, Any]],
        params_defaults: Mapping[str, Any],
        param_set: str = MAKERV3_RUNTIME_PARAM_REGISTRY.param_set,
        params_contract_resolver: Callable[[str], ParamsContract] | None = None,
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
        default_schema = _ordered_params_schema(params_schema)
        default_param_set = param_set.strip()
        default_defaults = FluxParamsManager(
            redis_client=self._redis,
            strategy_id=self._config.identity.strategy_id,
            namespace=self._config.identity.namespace,
            schema_version=self._config.identity.schema_version,
            schema=default_schema,
            defaults=params_defaults,
            param_set=default_param_set,
        ).defaults
        self._default_params_contract = ParamsContract(
            schema=default_schema,
            defaults=default_defaults,
            param_set=default_param_set,
        )
        self._params_contract_resolver = params_contract_resolver
        self._contract_specs = self._validate_contract_catalog(contract_catalog)
        self._contracts = tuple(spec[0] for spec in self._contract_specs)
        self._contract_catalog_resolver = contract_catalog_resolver
        self._strategy_running_resolver = strategy_running_resolver
        self._strategy_alerts_resolver = strategy_alerts_resolver
        self._tokenmm_trade_reset_cache: dict[str, tuple[tuple[int, str], bool]] = {}

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

    def _contract_specs_for_strategy(
        self,
        strategy_id: str,
    ) -> tuple[tuple[ContractCatalogEntry, str, str], ...]:
        if self._contract_catalog_resolver is None:
            return self._contract_specs
        resolved = self._contract_catalog_resolver(strategy_id)
        if not resolved:
            return self._contract_specs
        return self._validate_contract_catalog(resolved)

    def _contracts_for_strategy(self, strategy_id: str) -> tuple[ContractCatalogEntry, ...]:
        return tuple(spec[0] for spec in self._contract_specs_for_strategy(strategy_id))

    def params_contract(self, strategy_id: str) -> ParamsContract:
        if self._params_contract_resolver is None:
            return self._default_params_contract
        return self._params_contract_resolver(strategy_id)

    def _params_manager(self, strategy_id: str) -> FluxParamsManager:
        contract = self.params_contract(strategy_id)
        return FluxParamsManager(
            redis_client=self._redis,
            strategy_id=strategy_id,
            namespace=self._config.identity.namespace,
            schema_version=self._config.identity.schema_version,
            schema=contract.schema,
            defaults=contract.defaults,
            param_set=contract.param_set,
        )

    def _validate_contract_catalog(
        self,
        contract_catalog: Sequence[ContractCatalogEntry],
    ) -> tuple[tuple[ContractCatalogEntry, str, str], ...]:
        keys = self._keys_for_strategy(self._config.identity.strategy_id)
        seen: set[tuple[str, str, str, str]] = set()
        out: list[tuple[ContractCatalogEntry, str, str]] = []
        for index, contract in enumerate(contract_catalog):
            if not isinstance(contract, ContractCatalogEntry):
                raise ContractCatalogValidationError(
                    f"`contract_catalog[{index}]` must be `ContractCatalogEntry`, was {type(contract).__name__}",
                )

            exchange = decode_text(contract.exchange).strip().lower()
            symbol = decode_text(contract.symbol).strip().upper()
            instrument_id = decode_text(contract.instrument_id).strip().upper()
            base, quote = normalize_symbol_parts(symbol=symbol)
            if not base or not quote:
                raise ContractCatalogValidationError(
                    f"Contract symbol did not resolve to base/quote parts: {contract.symbol!r}",
                )

            try:
                keys.market_last(
                    exchange=exchange,
                    base=base,
                    quote=quote,
                    instrument_id=instrument_id or None,
                )
            except (TypeError, ValueError) as e:
                raise ContractCatalogValidationError(
                    f"Invalid contract catalog entry exchange={exchange!r} symbol={symbol!r}: {e}",
                ) from e

            dedupe_key = (
                exchange,
                instrument_id or "",
                "" if instrument_id else base,
                "" if instrument_id else quote,
            )
            if dedupe_key in seen:
                raise ContractCatalogValidationError(
                    "Duplicate contract catalog entry after normalization: "
                    f"exchange={exchange!r} symbol={symbol!r} "
                    f"(instrument_id={instrument_id!r} base={base!r} quote={quote!r})",
                )
            seen.add(dedupe_key)
            out.append(
                (
                    ContractCatalogEntry(
                        exchange=exchange,
                        symbol=symbol,
                        instrument_id=instrument_id,
                    ),
                    base,
                    quote,
                ),
            )

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

    def _running_state_from_strategy_state(self, state: Mapping[str, Any]) -> bool | None:
        if not state:
            return None

        state_name = decode_text(state.get("state")).strip().lower()
        if not state_name:
            return None
        if state_name == "on_stop":
            return False

        ts_ms = coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))
        if ts_ms is not None and now_ms() - ts_ms > PARAMS_RUNNING_STALE_AFTER_MS:
            return False

        return True

    def _load_running_state_from_strategy_state(self, strategy_id: str) -> bool | None:
        keys = self._keys_for_strategy(strategy_id)
        state_raw = self._redis.get(keys.state())
        state_value = load_json(state_raw)
        state = dict(state_value) if isinstance(state_value, dict) else {}
        return self._running_state_from_strategy_state(state)

    def load_running_states(self, strategy_ids: Sequence[str]) -> dict[str, bool | None]:
        deduped_ids: list[str] = []
        seen: set[str] = set()
        for strategy_id in strategy_ids:
            strategy_text = decode_text(strategy_id).strip()
            if not strategy_text or strategy_text in seen:
                continue
            seen.add(strategy_text)
            deduped_ids.append(strategy_text)

        if not deduped_ids:
            return {}

        if self._strategy_running_resolver is None:
            return {
                strategy_id: self._load_running_state_from_strategy_state(strategy_id)
                for strategy_id in deduped_ids
            }

        resolved_raw = dict(self._strategy_running_resolver(deduped_ids))
        resolved: dict[str, bool | None] = {}
        for strategy_id in deduped_ids:
            if strategy_id in resolved_raw:
                value = safe_bool(resolved_raw.get(strategy_id))
                resolved[strategy_id] = value if value is not None else None
            else:
                resolved[strategy_id] = self._load_running_state_from_strategy_state(strategy_id)
        return resolved

    def load_running_state(self, strategy_id: str) -> bool | None:
        return self.load_running_states([strategy_id]).get(strategy_id)

    def load_state_summary(self, strategy_id: str) -> dict[str, Any]:
        keys = self._keys_for_strategy(strategy_id)
        state_raw = self._redis.get(keys.state())
        state_value = load_json(state_raw)
        state = dict(state_value) if isinstance(state_value, dict) else {}
        if not state:
            return {}

        summary: dict[str, Any] = {}
        state_name = decode_text(state.get("state")).strip()
        running = self._running_state_from_strategy_state(state)
        if not state_name or (running is False and state_name.lower() != "on_stop"):
            return {}
        ts_ms = coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))
        if state_name:
            summary["state"] = state_name
        if ts_ms is not None:
            summary["state_ts_ms"] = ts_ms
        for field in (
            "bot_on",
            "effective_bot_on",
            "persisted_bot_on",
            "config_bot_on",
            "startup_bot_off_active",
            "terminal_order_denial_active",
        ):
            parsed = safe_bool(state.get(field))
            if parsed is not None:
                summary[field] = parsed
        reason = decode_text(state.get("bot_on_reason")).strip()
        if reason:
            summary["bot_on_reason"] = reason
        return summary

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

    @staticmethod
    def _instrument_exchange_alias(contract: ContractCatalogEntry) -> str | None:
        contract_exchange = decode_text(contract.exchange).strip().lower()
        instrument_text = decode_text(contract.instrument_id).strip().upper()
        if "." not in instrument_text:
            return None
        venue_text = instrument_text.rsplit(".", maxsplit=1)[1].strip().upper()
        if not venue_text:
            return None
        venue_root = venue_text.split("_", maxsplit=1)[0].lower()
        if not venue_root or venue_root == contract_exchange:
            return None
        return venue_root

    def _market_keys(
        self,
        strategy_id: str,
        *,
        contract_specs: Sequence[tuple[ContractCatalogEntry, str, str]] | None = None,
    ) -> list[tuple[ContractCatalogEntry, list[str]]]:
        keys = self._keys_for_strategy(strategy_id)
        out: list[tuple[ContractCatalogEntry, list[str]]] = []
        legacy_counts: dict[tuple[str, str, str], int] = {}
        active_specs = tuple(contract_specs) if contract_specs is not None else self._contract_specs_for_strategy(strategy_id)
        for contract, base, quote in active_specs:
            legacy_key = (contract.exchange, base, quote)
            legacy_counts[legacy_key] = legacy_counts.get(legacy_key, 0) + 1
        for contract, base, quote in active_specs:
            key_candidates = [
                keys.market_last(
                    exchange=contract.exchange,
                    base=base,
                    quote=quote,
                    instrument_id=contract.instrument_id or None,
                ),
            ]
            alias_exchange = self._instrument_exchange_alias(contract)
            if alias_exchange and contract.instrument_id:
                key_candidates.append(
                    keys.market_last(
                        exchange=alias_exchange,
                        base=base,
                        quote=quote,
                        instrument_id=contract.instrument_id,
                    ),
                )
            if contract.instrument_id and legacy_counts[(contract.exchange, base, quote)] == 1:
                key_candidates.append(
                    keys.market_last(
                        exchange=contract.exchange,
                        base=base,
                        quote=quote,
                    ),
                )
            out.append((contract, key_candidates))
        return out

    @staticmethod
    def _decode_market_rows(raw_values: Sequence[Any]) -> dict[str, Any]:
        decoded_rows = [FluxApiStore._parse_market_row(raw_value) for raw_value in raw_values]
        return FluxApiStore._merge_market_row_values(decoded_rows)

    @staticmethod
    def _parse_market_row(raw_value: Any) -> dict[str, Any]:
        parsed = load_json(raw_value)
        return dict(parsed) if isinstance(parsed, dict) else {}

    @staticmethod
    def _merge_market_row_values(rows: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
        merged: dict[str, Any] = {}
        for row in reversed(list(rows)):
            if not row:
                continue
            for key, value in row.items():
                if value is None and key in merged:
                    continue
                merged[key] = value
        return merged

    @staticmethod
    def _decode_market_row(primary_raw: Any, fallback_raw: Any = None) -> dict[str, Any]:
        return FluxApiStore._decode_market_rows(
            [raw for raw in (primary_raw, fallback_raw) if raw is not None],
        )

    def _decode_market_row_candidates(
        self,
        contract: ContractCatalogEntry,
        raw_values: Sequence[Any],
    ) -> dict[str, Any]:
        decoded_rows = [self._parse_market_row(raw_value) for raw_value in raw_values]
        if not decoded_rows:
            return {}

        canonical_row = decoded_rows[0]
        next_index = 1
        alias_row: dict[str, Any] = {}
        if contract.instrument_id and self._instrument_exchange_alias(contract) and next_index < len(
            decoded_rows
        ):
            alias_row = decoded_rows[next_index]
            next_index += 1
        legacy_row: dict[str, Any] = {}
        if contract.instrument_id and next_index < len(decoded_rows):
            legacy_row = decoded_rows[next_index]

        selected_row = canonical_row or alias_row
        if selected_row:
            return self._merge_market_row_values([selected_row, legacy_row])
        return dict(legacy_row)

    def load_market_rows(self, strategy_id: str) -> dict[str, dict[str, Any]]:
        contract_specs = self._contract_specs_for_strategy(strategy_id)
        market_pairs = self._market_keys(strategy_id, contract_specs=contract_specs)
        pipe = self._redis.pipeline(transaction=False)
        for _, key_candidates in market_pairs:
            for market_key in key_candidates:
                pipe.get(market_key)
        raw = pipe.execute()
        expected_length = sum(len(key_candidates) for _, key_candidates in market_pairs)
        if len(raw) != expected_length:
            raise RuntimeError(
                f"Market pipeline returned {len(raw)} rows, expected {expected_length}",
            )

        market_rows: dict[str, dict[str, Any]] = {}
        raw_index = 0
        for contract, key_candidates in market_pairs:
            parsed = self._decode_market_row_candidates(
                contract,
                raw[raw_index : raw_index + len(key_candidates)],
            )
            raw_index += len(key_candidates)
            contract_id = contract_id_for_leg(
                exchange=contract.exchange,
                symbol=contract.symbol,
                instrument_id=contract.instrument_id,
            )
            market_rows[contract_id] = parsed
        return market_rows

    def load_market_rows_for_strategies(
        self,
        strategy_ids: Sequence[str],
    ) -> dict[str, dict[str, Any]]:
        def _market_row_has_price(row: Mapping[str, Any]) -> bool:
            mid = row.get("mid")
            bid = row.get("bid")
            ask = row.get("ask")
            return any(value is not None for value in (mid, bid, ask))

        def _merge_market_row(existing: Mapping[str, Any], incoming: Mapping[str, Any]) -> dict[str, Any]:
            existing_ts_ms = coerce_ts_ms(existing.get("ts_ms") or existing.get("ts")) or -1
            incoming_ts_ms = coerce_ts_ms(incoming.get("ts_ms") or incoming.get("ts")) or -1
            existing_has_price = _market_row_has_price(existing)
            incoming_has_price = _market_row_has_price(incoming)

            if incoming_has_price and (not existing_has_price or incoming_ts_ms >= existing_ts_ms):
                primary_row = incoming
                fallback_row = existing
            elif existing_has_price:
                primary_row = existing
                fallback_row = incoming
            elif incoming_ts_ms >= existing_ts_ms:
                primary_row = incoming
                fallback_row = existing
            else:
                primary_row = existing
                fallback_row = incoming

            combined = dict(fallback_row)
            for key, value in primary_row.items():
                if value is None and key in combined:
                    continue
                combined[key] = value
            return combined

        merged: dict[str, dict[str, Any]] = {}
        seen: set[str] = set()
        for strategy_id in strategy_ids:
            strategy_text = decode_text(strategy_id).strip()
            if not strategy_text or strategy_text in seen:
                continue
            seen.add(strategy_text)
            strategy_rows = self.load_market_rows(strategy_text)
            for contract_id, row in strategy_rows.items():
                if not isinstance(row, Mapping):
                    continue
                if contract_id not in merged:
                    merged[contract_id] = dict(row)
                    continue
                merged[contract_id] = _merge_market_row(merged[contract_id], row)
        return merged

    def load_portfolio_snapshot(self, portfolio_id: str) -> dict[str, Any] | None:
        key = FluxRedisKeys.portfolio_snapshot(
            portfolio_id=portfolio_id,
            namespace=self._config.identity.namespace,
            schema_version=self._config.identity.schema_version,
        )
        payload = load_json(self._redis.get(key))
        return dict(payload) if isinstance(payload, Mapping) else None

    def load_profile_account_projection_rows(
        self,
        profile_id: str,
        *,
        account_scope_ids: Sequence[str] | None = None,
        limit: int = 200,
    ) -> tuple[list[dict[str, Any]], dict[str, Any], list[dict[str, Any]]]:
        max_items = max(1, min(2_000, int(limit)))
        key_prefix = (
            f"{self._config.identity.namespace}:{self._config.identity.schema_version}:"
            f"profile:account_projection:{validate_identifier_part(profile_id, 'profile_id')}:"
        )
        projection_keys: list[str] = []
        seen_keys: set[str] = set()

        for account_scope_id in account_scope_ids or ():
            key = FluxRedisKeys.profile_account_projection(
                profile_id=profile_id,
                account_scope_id=account_scope_id,
                namespace=self._config.identity.namespace,
                schema_version=self._config.identity.schema_version,
            )
            if key in seen_keys:
                continue
            seen_keys.add(key)
            projection_keys.append(key)
            if len(projection_keys) >= max_items:
                break

        scan_fn = getattr(self._redis, "scan_iter", None)
        scan_succeeded = False
        if not projection_keys and callable(scan_fn):
            try:
                for raw_key in scan_fn(match=f"{key_prefix}*"):
                    if len(projection_keys) >= max_items:
                        break
                    key = decode_text(raw_key).strip()
                    if key.startswith(key_prefix) and key not in seen_keys:
                        seen_keys.add(key)
                        projection_keys.append(key)
                scan_succeeded = True
            except Exception as e:
                _LOG.debug(
                    "Profile-account projection discovery via redis.scan_iter() failed prefix=%s error=%s",
                    key_prefix,
                    type(e).__name__,
                    exc_info=True,
                )

        keys_fn = getattr(self._redis, "keys", None)
        if not projection_keys and not scan_succeeded and callable(keys_fn):
            try:
                for raw_key in keys_fn(f"{key_prefix}*"):
                    if len(projection_keys) >= max_items:
                        break
                    key = decode_text(raw_key).strip()
                    if key.startswith(key_prefix) and key not in seen_keys:
                        seen_keys.add(key)
                        projection_keys.append(key)
            except Exception as e:
                _LOG.debug(
                    "Profile-account projection discovery via redis.keys() failed prefix=%s error=%s",
                    key_prefix,
                    type(e).__name__,
                    exc_info=True,
                )

        strings = getattr(self._redis, "strings", None)
        if isinstance(strings, dict):
            for raw_key in strings:
                if len(projection_keys) >= max_items:
                    break
                key = decode_text(raw_key).strip()
                if key.startswith(key_prefix) and key not in seen_keys:
                    seen_keys.add(key)
                    projection_keys.append(key)

        rows: list[dict[str, Any]] = []
        totals: dict[str, Any] = {}
        scope_status: list[dict[str, Any]] = []
        for key in projection_keys:
            payload = load_json(self._redis.get(key))
            if not isinstance(payload, Mapping):
                continue
            raw_rows = payload.get("rows")
            if isinstance(raw_rows, Sequence) and not isinstance(raw_rows, str | bytes):
                rows.extend(dict(row) for row in raw_rows if isinstance(row, Mapping))
            payload_scope_status = _normalize_scope_status_entries(payload.get("scope_status"))
            if payload_scope_status:
                scope_status = _merge_scope_status_entries(scope_status, payload_scope_status)
            raw_totals = payload.get("totals")
            if (
                isinstance(raw_totals, Mapping)
                and not _scope_status_entries_degraded(payload_scope_status)
                and not _projection_rows_excluded_from_reconciliation(raw_rows)
            ):
                totals = _merge_account_totals(totals, raw_totals)

        return rows, totals, scope_status

    def _tokenmm_inventory_overlay(
        self,
        *,
        strategy_id: str,
        metadata: StrategyMetadata,
    ) -> tuple[dict[str, Any], dict[str, Any] | None] | None:
        if not _strategy_groups_include_tokenmm(metadata):
            return None

        portfolio_snapshot = self.load_portfolio_snapshot("tokenmm")
        if portfolio_snapshot is None:
            return None

        inventory = portfolio_snapshot.get("inventory")
        inventory_payload = dict(inventory) if isinstance(inventory, Mapping) else {}
        request_now_ms = now_ms()
        snapshot_stale_after_ms = (
            safe_int(inventory_payload.get("stale_after_ms"))
            or TOKENMM_BALANCES_STALE_AFTER_MS
        )
        if not (
            _timestamp_is_fresh(
                portfolio_snapshot.get("server_ts_ms"),
                now_ms_value=request_now_ms,
                stale_after_ms=snapshot_stale_after_ms,
            )
            and _timestamp_is_fresh(
                inventory_payload.get("ts_ms"),
                now_ms_value=request_now_ms,
                stale_after_ms=snapshot_stale_after_ms,
            )
        ):
            return None

        components_payload = inventory_payload.get("components")
        if not isinstance(components_payload, list):
            components_payload = portfolio_snapshot.get("components")
        component = next(
            (
                dict(item)
                for item in (components_payload or [])
                if isinstance(item, Mapping)
                and decode_text(item.get("strategy_id")).strip() == strategy_id
            ),
            None,
        )
        return inventory_payload, component

    def _apply_tokenmm_inventory_overlay(
        self,
        *,
        payload: dict[str, Any],
        inventory_payload: Mapping[str, Any],
        component_payload: Mapping[str, Any] | None,
    ) -> None:
        global_qty_base = safe_float(
            inventory_payload.get("global_qty_base") or inventory_payload.get("global_qty"),
        )
        global_qty_complete = safe_bool(
            inventory_payload.get("global_qty_base_complete")
            if inventory_payload.get("global_qty_base_complete") is not None
            else inventory_payload.get("global_qty_complete"),
        )
        aggregation_mode = decode_text(inventory_payload.get("aggregation_mode")).strip() or None
        component_inventory_fresh = _component_inventory_is_fresh(component_payload)

        pricing_adjustments = payload.get("pricing_adjustments")
        if isinstance(pricing_adjustments, list):
            inventory_adjustment_index = next(
                (
                    index
                    for index, item in enumerate(pricing_adjustments)
                    if isinstance(item, Mapping)
                    and decode_text(item.get("type")).strip().lower() == "inventory_skew"
                ),
                None,
            )
            if inventory_adjustment_index is None:
                pricing_adjustments.append({"type": "inventory_skew"})
                inventory_adjustment_index = len(pricing_adjustments) - 1
            inventory_adjustment = dict(pricing_adjustments[inventory_adjustment_index])
            if global_qty_base is not None:
                inventory_adjustment["global_qty_base"] = global_qty_base
                inventory_adjustment["global_qty"] = global_qty_base
            if global_qty_complete is not None:
                inventory_adjustment["global_qty_base_complete"] = global_qty_complete
                inventory_adjustment["global_qty_complete"] = global_qty_complete
            if aggregation_mode is not None:
                inventory_adjustment["aggregation_mode"] = aggregation_mode

            if isinstance(component_payload, Mapping):
                local_qty_base = safe_float(
                    component_payload.get("local_qty_base") or component_payload.get("local_qty"),
                )
                local_position_qty_base = safe_float(component_payload.get("local_position_qty_base"))
                local_position_qty_venue = safe_float(
                    component_payload.get("local_position_qty_venue"),
                )
                qty_conversion_status = (
                    decode_text(component_payload.get("qty_conversion_status")).strip() or None
                )
                qty_conversion_source = (
                    decode_text(component_payload.get("qty_conversion_source")).strip() or None
                )
                existing_local_qty_base = safe_float(
                    inventory_adjustment.get("local_qty_base") or inventory_adjustment.get("local_qty"),
                )
                existing_position_qty_base = safe_float(inventory_adjustment.get("position_qty_base"))
                existing_position_qty_venue = safe_float(
                    inventory_adjustment.get("position_qty_venue"),
                )
                existing_qty_conversion_status = (
                    decode_text(inventory_adjustment.get("qty_conversion_status")).strip() or None
                )
                existing_qty_conversion_source = (
                    decode_text(inventory_adjustment.get("qty_conversion_source")).strip() or None
                )
                if local_qty_base is not None and (
                    component_inventory_fresh or existing_local_qty_base is None
                ):
                    inventory_adjustment["local_qty_base"] = local_qty_base
                    inventory_adjustment["local_qty"] = local_qty_base
                if local_position_qty_base is not None and (
                    component_inventory_fresh or existing_position_qty_base is None
                ):
                    inventory_adjustment["position_qty_base"] = local_position_qty_base
                if local_position_qty_venue is not None and (
                    component_inventory_fresh or existing_position_qty_venue is None
                ):
                    inventory_adjustment["position_qty_venue"] = local_position_qty_venue
                if qty_conversion_status is not None and (
                    component_inventory_fresh or existing_qty_conversion_status is None
                ):
                    inventory_adjustment["qty_conversion_status"] = qty_conversion_status
                if qty_conversion_source is not None and (
                    component_inventory_fresh or existing_qty_conversion_source is None
                ):
                    inventory_adjustment["qty_conversion_source"] = qty_conversion_source

            pricing_adjustments[inventory_adjustment_index] = inventory_adjustment

        if global_qty_base is not None:
            payload["global_qty_base"] = global_qty_base
            payload["global_qty"] = global_qty_base
        if global_qty_complete is not None:
            payload["global_qty_base_complete"] = global_qty_complete
            payload["global_qty_complete"] = global_qty_complete
        if aggregation_mode is not None:
            payload["aggregation_mode"] = aggregation_mode

        if isinstance(component_payload, Mapping):
            local_qty_base = safe_float(
                component_payload.get("local_qty_base") or component_payload.get("local_qty"),
            )
            local_position_qty_base = safe_float(component_payload.get("local_position_qty_base"))
            local_position_qty_venue = safe_float(component_payload.get("local_position_qty_venue"))
            qty_conversion_status = (
                decode_text(component_payload.get("qty_conversion_status")).strip() or None
            )
            qty_conversion_source = (
                decode_text(component_payload.get("qty_conversion_source")).strip() or None
            )
            existing_local_qty_base = safe_float(payload.get("local_qty_base") or payload.get("local_qty"))
            existing_position_qty_base = safe_float(payload.get("position_qty_base"))
            existing_position_qty_venue = safe_float(payload.get("position_qty_venue"))
            existing_qty_conversion_status = (
                decode_text(payload.get("qty_conversion_status")).strip() or None
            )
            existing_qty_conversion_source = (
                decode_text(payload.get("qty_conversion_source")).strip() or None
            )
            if local_qty_base is not None and (
                component_inventory_fresh or existing_local_qty_base is None
            ):
                payload["local_qty_base"] = local_qty_base
                payload["local_qty"] = local_qty_base
            if local_position_qty_base is not None and (
                component_inventory_fresh or existing_position_qty_base is None
            ):
                payload["position_qty_base"] = local_position_qty_base
            if local_position_qty_venue is not None and (
                component_inventory_fresh or existing_position_qty_venue is None
            ):
                payload["position_qty_venue"] = local_position_qty_venue
            if qty_conversion_status is not None and (
                component_inventory_fresh or existing_qty_conversion_status is None
            ):
                payload["qty_conversion_status"] = qty_conversion_status
            if qty_conversion_source is not None and (
                component_inventory_fresh or existing_qty_conversion_source is None
            ):
                payload["qty_conversion_source"] = qty_conversion_source

    def load_signals_payload(
        self,
        strategy_id: str,
        metadata: StrategyMetadata,
        *,
        running: bool | None = None,
    ) -> dict[str, Any]:
        keys = self._keys_for_strategy(strategy_id)
        contract_specs = self._contract_specs_for_strategy(strategy_id)
        strategy_contracts = tuple(spec[0] for spec in contract_specs)

        pipe = self._redis.pipeline(transaction=False)
        pipe.get(keys.state())
        pipe.xrevrange(keys.fv_stream(), count=50)
        pipe.get(keys.balances_snapshot())
        market_pairs = self._market_keys(strategy_id, contract_specs=contract_specs)
        for _, key_candidates in market_pairs:
            for market_key in key_candidates:
                pipe.get(market_key)
        raw = pipe.execute()
        expected_length = 3 + sum(len(key_candidates) for _, key_candidates in market_pairs)
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
        raw_index = 3
        for contract, key_candidates in market_pairs:
            parsed = self._decode_market_row_candidates(
                contract,
                raw[raw_index : raw_index + len(key_candidates)],
            )
            raw_index += len(key_candidates)
            contract_id = contract_id_for_leg(
                exchange=contract.exchange,
                symbol=contract.symbol,
                instrument_id=contract.instrument_id,
            )
            market_rows[contract_id] = parsed
        legs = build_legs_payload(
            contracts=strategy_contracts,
            market_rows=market_rows,
            now_ms_value=now_ms(),
        )

        params = self.load_params(strategy_id)
        payload = build_signals_payload(
            strategy_id=strategy_id,
            metadata=metadata,
            state=state,
            fv_row=fv_row,
            params=params,
            balances=balances,
            legs=legs,
            running=running,
        )
        inventory_overlay = self._tokenmm_inventory_overlay(
            strategy_id=strategy_id,
            metadata=metadata,
        )
        if inventory_overlay is not None:
            inventory_payload, component_payload = inventory_overlay
            self._apply_tokenmm_inventory_overlay(
                payload=payload,
                inventory_payload=inventory_payload,
                component_payload=component_payload,
            )
        payload["running"] = (
            running if running is not None else self._running_state_from_strategy_state(state)
        )
        return payload

    def load_balances_rows(self, strategy_id: str) -> list[dict[str, Any]]:
        rows, _snapshot_present = self.load_balances_rows_with_presence(strategy_id)
        return rows

    def load_balances_rows_with_presence(self, strategy_id: str) -> tuple[list[dict[str, Any]], bool]:
        keys = self._keys_for_strategy(strategy_id)
        contract_specs = self._contract_specs_for_strategy(strategy_id)
        strategy_contracts = tuple(spec[0] for spec in contract_specs)
        pipe = self._redis.pipeline(transaction=False)
        pipe.get(keys.balances_snapshot())
        market_pairs = self._market_keys(strategy_id, contract_specs=contract_specs)
        for _, key_candidates in market_pairs:
            for market_key in key_candidates:
                pipe.get(market_key)
        raw = pipe.execute()
        expected_length = 1 + sum(len(key_candidates) for _, key_candidates in market_pairs)
        if len(raw) != expected_length:
            raise RuntimeError(
                f"Balances pipeline returned {len(raw)} rows, expected {expected_length}",
            )

        raw_snapshot = raw[0]
        balances_raw = load_json(raw_snapshot)
        rows = build_balances_rows(raw_snapshot=balances_raw, strategy_id=strategy_id)

        market_rows: dict[str, dict[str, Any]] = {}
        raw_index = 1
        for contract, key_candidates in market_pairs:
            parsed = self._decode_market_row_candidates(
                contract,
                raw[raw_index : raw_index + len(key_candidates)],
            )
            raw_index += len(key_candidates)
            contract_id = contract_id_for_leg(
                exchange=contract.exchange,
                symbol=contract.symbol,
                instrument_id=contract.instrument_id,
            )
            market_rows[contract_id] = parsed

        return (
            collapse_balance_display_rows(
                enrich_balances_rows(
                    rows,
                    contracts=strategy_contracts,
                    market_rows=market_rows,
                ),
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
        base_first_qty: bool = False,
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
            base_first_qty=base_first_qty,
        )

    def load_all_trades_rows(self, strategy_id: str, *, base_first_qty: bool = False) -> list[dict[str, Any]]:
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
            base_first_qty=base_first_qty,
        )

    def tokenmm_trade_stream_signature(self, strategy_id: str) -> tuple[int, str]:
        keys = self._keys_for_strategy(strategy_id)
        stream_key = keys.trades_stream()
        stream_len = self.trades_stream_len(strategy_id) or 0
        latest_entries = self._redis.xrevrange(stream_key, count=1)
        latest_entry_id = ""
        if latest_entries:
            latest_entry = latest_entries[0]
            if isinstance(latest_entry, Sequence) and not isinstance(latest_entry, str | bytes):
                latest_entry_id = decode_text(latest_entry[0]).strip()
        return stream_len, latest_entry_id

    def tokenmm_trade_stream_requires_reset(self, strategy_id: str) -> bool:
        signature = self.tokenmm_trade_stream_signature(strategy_id)
        cached = self._tokenmm_trade_reset_cache.get(strategy_id)
        if cached is not None and cached[0] == signature:
            return cached[1]
        keys = self._keys_for_strategy(strategy_id)
        entries = self._redis.xrevrange(keys.trades_stream())
        rows = extract_stream_rows(entries)
        filtered = [row for row in rows if strategy_id_from_row(row, strategy_id) == strategy_id]
        requires_reset = tokenmm_trade_rows_require_reset(filtered)
        self._tokenmm_trade_reset_cache[strategy_id] = (signature, requires_reset)
        return requires_reset

    def load_alerts_rows(self, strategy_id: str, *, limit: int) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        fetch_count = max(1, min(2_000, limit * 2))
        entries = self._redis.xrevrange(keys.alerts(), count=fetch_count)
        rows = extract_stream_rows(entries)
        rows.extend(self._resolved_alert_rows([strategy_id]).get(strategy_id, ()))
        return build_alerts_rows(rows=rows, strategy_id=strategy_id, limit=limit)

    def load_all_alerts_rows(self, strategy_id: str) -> list[dict[str, Any]]:
        keys = self._keys_for_strategy(strategy_id)
        entries = self._redis.xrevrange(keys.alerts())
        rows = extract_stream_rows(entries)
        filtered = [row for row in rows if strategy_id_from_row(row, strategy_id) == strategy_id]
        filtered.extend(self._resolved_alert_rows([strategy_id]).get(strategy_id, ()))
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
        extra_count = len(self._resolved_alert_rows([strategy_id]).get(strategy_id, ()))
        xlen_fn = getattr(self._redis, "xlen", None)
        if callable(xlen_fn):
            size = safe_int(xlen_fn(stream_key))
            return max(0, size or 0) + extra_count
        streams = getattr(self._redis, "streams", None)
        if isinstance(streams, dict):
            rows = streams.get(stream_key)
            if isinstance(rows, list):
                return len(rows) + extra_count
        return extra_count or None

    def _resolved_alert_rows(
        self,
        strategy_ids: Sequence[str],
    ) -> dict[str, list[dict[str, Any]]]:
        if self._strategy_alerts_resolver is None:
            return {}

        deduped_ids: list[str] = []
        seen: set[str] = set()
        for strategy_id in strategy_ids:
            strategy_text = decode_text(strategy_id).strip()
            if not strategy_text or strategy_text in seen:
                continue
            seen.add(strategy_text)
            deduped_ids.append(strategy_text)
        if not deduped_ids:
            return {}

        try:
            resolved_raw = dict(self._strategy_alerts_resolver(deduped_ids))
        except Exception:
            _LOG.exception("Flux API supplemental alert resolver failed strategy_ids=%s", deduped_ids)
            return {strategy_id: [] for strategy_id in deduped_ids}

        resolved: dict[str, list[dict[str, Any]]] = {}
        for strategy_id in deduped_ids:
            rows = resolved_raw.get(strategy_id, ())
            normalized_rows: list[dict[str, Any]] = []
            if isinstance(rows, Sequence) and not isinstance(rows, str | bytes):
                for row in rows:
                    if not isinstance(row, Mapping):
                        continue
                    normalized = dict(row)
                    normalized.setdefault("strategy_id", strategy_id)
                    normalized_rows.append(normalized)
            resolved[strategy_id] = normalized_rows
        return resolved

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


def _merge_account_totals(
    current: Mapping[str, Any],
    incoming: Mapping[str, Any],
) -> dict[str, Any]:
    merged = dict(current)
    for key in ("account_equity_raw", "withdrawable_raw"):
        value = _coerce_finite_float(incoming.get(key))
        if value is None:
            continue
        merged[key] = _coerce_finite_float(merged.get(key)) or 0.0
        merged[key] += value
    if "account_equity_raw" in merged:
        merged["account_equity_display"] = _format_money_display(float(merged["account_equity_raw"]))
    if "withdrawable_raw" in merged:
        merged["withdrawable_display"] = _format_money_display(float(merged["withdrawable_raw"]))
    return merged


def _portfolio_snapshot_rows(raw_rows: Any) -> list[dict[str, Any]]:
    if not isinstance(raw_rows, Sequence) or isinstance(raw_rows, str | bytes):
        return []
    return [dict(row) for row in raw_rows if isinstance(row, Mapping)]


def _portfolio_snapshot_inventory_summary(
    portfolio_snapshot: Mapping[str, Any],
) -> dict[str, Any] | None:
    raw_inventory_by_asset = portfolio_snapshot.get("inventory_by_asset")
    if not isinstance(raw_inventory_by_asset, Mapping):
        return None

    inventory_by_asset: dict[str, dict[str, Any]] = {}
    components: list[dict[str, Any]] = []
    missing_required: set[str] = set()
    stale_required: set[str] = set()
    null_qty_required: set[str] = set()
    degraded = False
    stale_after_ms = TOKENMM_BALANCES_STALE_AFTER_MS

    for asset_id, payload in raw_inventory_by_asset.items():
        canonical_asset_id = decode_text(asset_id).strip().upper()
        if not canonical_asset_id or not isinstance(payload, Mapping):
            continue
        normalized_payload = dict(payload)
        normalized_payload["base_currency"] = (
            decode_text(normalized_payload.get("base_currency") or canonical_asset_id).strip().upper()
            or canonical_asset_id
        )
        inventory_by_asset[canonical_asset_id] = normalized_payload
        stale_after_ms = max(
            stale_after_ms,
            safe_int(normalized_payload.get("stale_after_ms")) or TOKENMM_BALANCES_STALE_AFTER_MS,
        )
        degraded = degraded or bool(normalized_payload.get("degraded", False))
        missing_required.update(decode_text(item).strip() for item in normalized_payload.get("missing_required") or [])
        stale_required.update(decode_text(item).strip() for item in normalized_payload.get("stale_required") or [])
        null_qty_required.update(
            decode_text(item).strip() for item in normalized_payload.get("null_qty_required") or []
        )
        for component in normalized_payload.get("components") or []:
            if not isinstance(component, Mapping):
                continue
            component_row = dict(component)
            component_row.setdefault("portfolio_asset_id", canonical_asset_id)
            components.append(component_row)

    return {
        "inventory_by_asset": dict(sorted(inventory_by_asset.items())),
        "components": components,
        "missing_required": sorted(item for item in missing_required if item),
        "stale_required": sorted(item for item in stale_required if item),
        "null_qty_required": sorted(item for item in null_qty_required if item),
        "degraded": degraded or bool(missing_required or stale_required or null_qty_required),
        "stale_after_ms": stale_after_ms,
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
        sorted_rows = [dict(row) for row in rows]
        sorted_rows.sort(key=_trade_replay_sort_key)
        return sorted_rows

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
    contract_catalog_resolver: Callable[[str], Sequence[ContractCatalogEntry]] | None = None,
    strategy_running_resolver: StrategyRunningResolver | None = None,
    strategy_alerts_resolver: StrategyAlertsResolver | None = None,
    strategy_metadata: StrategyMetadata,
    strategy_metadata_resolver: Callable[[str], StrategyMetadata] | None = None,
    profile_strategy_map: Mapping[str, str | Sequence[str]] | None = None,
    profile_required_strategy_map: Mapping[str, str | Sequence[str]] | None = None,
    strategy_contracts: Sequence[Mapping[str, Any]] | None = None,
    params_schema: Mapping[str, Mapping[str, Any]] | None = None,
    params_defaults: Mapping[str, Any] | None = None,
    param_set: str = MAKERV3_RUNTIME_PARAM_REGISTRY.param_set,
    required_readiness_keys: Sequence[str] | None = None,
) -> Flask:
    if not isinstance(flux_config, FluxConfig) and not all(
        hasattr(flux_config, field)
        for field in ("mode", "confirm_live", "identity", "redis", "venues")
    ):
        raise TypeError("`flux_config` must be an instance of `FluxConfig`")
    if not isinstance(strategy_metadata, StrategyMetadata) and not all(
        hasattr(strategy_metadata, field)
        for field in ("strategy_class", "strategy_groups", "base_asset", "quote_asset")
    ):
        raise TypeError("`strategy_metadata` must be an instance of `StrategyMetadata`")
    if strategy_metadata_resolver is not None and not callable(strategy_metadata_resolver):
        raise TypeError("`strategy_metadata_resolver` must be callable when provided")

    schema = params_schema or DEFAULT_PARAMS_SCHEMA
    defaults = params_defaults or DEFAULT_PARAMS_DEFAULTS
    default_param_set = param_set.strip()
    from flux.strategies.registry import get_strategy_spec
    from flux.strategies.registry import resolve_strategy_spec_for_strategy_id

    try:
        default_strategy_spec = get_strategy_spec(default_param_set)
    except Exception:
        default_strategy_spec = None

    default_params_contract = ParamsContract(
        schema=_ordered_params_schema(schema),
        defaults=FluxParamsManager(
            redis_client=redis_client,
            strategy_id=flux_config.identity.strategy_id,
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
            schema=schema,
            defaults=defaults,
            param_set=default_param_set,
        ).defaults,
        param_set=default_param_set,
    )

    params_contract_cache: dict[str, ParamsContract] = {}

    def _metadata_for_contract(strategy_id: str) -> StrategyMetadata:
        metadata = strategy_metadata
        if strategy_metadata_resolver is not None:
            try:
                metadata = strategy_metadata_resolver(strategy_id)
            except LookupError:
                metadata = strategy_metadata
        return metadata

    def _runtime_params_contract_for_param_set(resolved_param_set: str) -> ParamsContract:
        cached = params_contract_cache.get(resolved_param_set)
        if cached is not None:
            return cached
        if resolved_param_set == default_params_contract.param_set:
            params_contract_cache[resolved_param_set] = default_params_contract
            return default_params_contract

        module = importlib.import_module(f"flux.strategies.{resolved_param_set}.runtime_params")
        runtime_schema = getattr(module, "RUNTIME_PARAM_SCHEMA", None)
        runtime_defaults = getattr(module, "RUNTIME_PARAM_DEFAULTS", None)
        runtime_param_set = str(getattr(module, "PARAM_SET", resolved_param_set)).strip()
        if not isinstance(runtime_schema, Mapping) or not isinstance(runtime_defaults, Mapping):
            raise ValueError(
                f"Runtime params module for {resolved_param_set!r} did not expose schema/defaults",
            )
        contract = ParamsContract(
            schema=_ordered_params_schema(runtime_schema),
            defaults=FluxParamsManager(
                redis_client=redis_client,
                strategy_id=flux_config.identity.strategy_id,
                namespace=flux_config.identity.namespace,
                schema_version=flux_config.identity.schema_version,
                schema=runtime_schema,
                defaults=runtime_defaults,
                param_set=runtime_param_set,
            ).defaults,
            param_set=runtime_param_set,
        )
        params_contract_cache[resolved_param_set] = contract
        return contract

    def _params_contract_for_strategy(strategy_id: str) -> ParamsContract:
        metadata = _metadata_for_contract(strategy_id)
        resolved_param_set = decode_text(getattr(metadata, "param_set", "")).strip()
        if not resolved_param_set:
            try:
                resolved_spec = resolve_strategy_spec_for_strategy_id(
                    strategy_id,
                    default=default_strategy_spec,
                )
                resolved_param_set = resolved_spec.param_set
            except Exception:
                resolved_param_set = default_params_contract.param_set
        if not resolved_param_set:
            return default_params_contract
        return _runtime_params_contract_for_param_set(resolved_param_set)

    store = FluxApiStore(
        flux_config=flux_config,
        redis_client=redis_client,
        contract_catalog=contract_catalog,
        contract_catalog_resolver=contract_catalog_resolver,
        strategy_running_resolver=strategy_running_resolver,
        strategy_alerts_resolver=strategy_alerts_resolver,
        params_schema=schema,
        params_defaults=defaults,
        param_set=param_set,
        params_contract_resolver=_params_contract_for_strategy,
        required_readiness_keys=required_readiness_keys,
    )
    default_strategy_id = flux_config.identity.strategy_id
    strategy_set_descriptors = {
        descriptor.profile: descriptor for descriptor in get_strategy_set_descriptors()
    }
    default_unscoped_descriptor = next(
        (
            descriptor
            for descriptor in strategy_set_descriptors.values()
            if descriptor.default_unscoped_api
        ),
        None,
    )

    def _descriptor_for_profile(profile: Any) -> StrategySetDescriptor | None:
        return strategy_set_descriptors.get(normalize_profile(profile))

    def _resolve_strategy_id(
        raw_value: Any,
        *,
        field_name: str,
        explicit: bool | None = None,
    ) -> str:
        text = decode_text(raw_value).strip()
        candidate = text or default_strategy_id
        try:
            strategy_id = validate_identifier_part(candidate, field_name)
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
        is_explicit = explicit if explicit is not None else raw_value is not None
        requested_profile = (
            normalize_profile(request.args.get("profile"))
            if has_request_context()
            else ""
        )
        active_allowlist = strategy_allowlist_by_profile.get(requested_profile, frozenset())
        if not requested_profile and default_unscoped_descriptor is not None:
            active_allowlist = strategy_allowlist_by_profile.get(
                default_unscoped_descriptor.profile,
                frozenset(),
            )
        if active_allowlist and is_explicit and strategy_id not in active_allowlist:
            raise ApiEnvelopeError(
                status=404,
                code="unknown_strategy_id",
                message="Strategy is not configured for this API.",
                details={
                    "field": field_name,
                    "strategy_id": strategy_id,
                },
            )
        return strategy_id

    def _metadata_for_strategy(strategy_id: str) -> StrategyMetadata:
        metadata = strategy_metadata
        if strategy_metadata_resolver is not None:
            try:
                metadata = strategy_metadata_resolver(strategy_id)
            except LookupError:
                metadata = strategy_metadata
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
    app.extensions["flux_realtime_rollout"] = default_realtime_rollout()

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

    resolved_profile_strategy_map: dict[str, list[str]] = {}
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
        descriptor = _descriptor_for_profile(profile_name)
        strategy_ids = _coerce_strategy_ids(resolved_profile_strategy_map.get(profile_name))
        if not strategy_ids and descriptor is not None and descriptor.allow_discovery_without_allowlist:
            strategy_ids = store.discover_strategy_ids_from_params()
        if not strategy_ids:
            raise ValueError(
                f"`profile_required_strategy_map[{profile_name!r}]` requires matching strategy IDs in `profile_strategy_map`",
            )
        missing_ids = sorted(set(required_ids) - set(strategy_ids))
        if missing_ids:
            raise ValueError(
                f"`profile_required_strategy_map[{profile_name!r}]` must be a subset of `profile_strategy_map`; missing={missing_ids}",
            )
    strategy_allowlist_by_profile = {
        profile_name: frozenset(_coerce_strategy_ids(strategy_ids))
        for profile_name, strategy_ids in resolved_profile_strategy_map.items()
    }

    def _strategy_ids_for_profile(profile: str) -> list[str]:
        normalized = normalize_profile(profile)
        return _coerce_strategy_ids(resolved_profile_strategy_map.get(normalized))

    shared_position_groups_cache: dict[str, dict[str, str]] = {}
    profile_projection_scope_ids_cache: dict[str, tuple[str, ...]] = {}
    execution_account_scope_cache: dict[str, dict[str, str]] = {}
    controller_scope_by_account_scope_cache: dict[str, dict[str, str]] = {}

    def _profile_supports_account_projections(profile: str) -> bool:
        return normalize_profile(profile) in {"equities", "tokenmm"}

    def _strategy_allowlist_for_profile(profile: str) -> list[str] | None:
        normalized = normalize_profile(profile)
        strategy_ids = _strategy_ids_for_profile(normalized)
        if strategy_ids:
            return strategy_ids
        descriptor = _descriptor_for_profile(normalized)
        if descriptor is not None and descriptor.allow_discovery_without_allowlist:
            discovered = store.discover_strategy_ids_from_params()
            if discovered:
                return discovered
        return None

    def _shared_position_groups_for_profile(profile: str) -> dict[str, str]:
        normalized = normalize_profile(profile)
        cached = shared_position_groups_cache.get(normalized)
        if cached is not None:
            return cached
        shared_groups = shared_observation_group_by_strategy_id(
            strategy_contracts or (),
            allowlist=_strategy_allowlist_for_profile(normalized),
        )
        shared_position_groups_cache[normalized] = shared_groups
        return shared_groups

    def _profile_projection_scope_ids_for_profile(profile: str) -> tuple[str, ...]:
        normalized = normalize_profile(profile)
        cached = profile_projection_scope_ids_cache.get(normalized)
        if cached is not None:
            return cached
        allowlist = _strategy_allowlist_for_profile(normalized)
        allowlist_set = set(allowlist or ())
        use_allowlist = allowlist is not None
        scope_ids: list[str] = []
        seen: set[str] = set()
        for contract in decode_strategy_contracts(strategy_contracts or ()):
            if use_allowlist and contract.strategy_id not in allowlist_set:
                continue
            for scope_id in (
                contract.execution_account_scope_id,
                contract.reference_account_scope_id,
                contract.hedge_account_scope_id,
            ):
                if scope_id is None or scope_id in seen:
                    continue
                seen.add(scope_id)
                scope_ids.append(scope_id)
        result = tuple(scope_ids)
        profile_projection_scope_ids_cache[normalized] = result
        return result

    def _execution_account_scopes_for_profile(profile: str) -> dict[str, str]:
        normalized = normalize_profile(profile)
        cached = execution_account_scope_cache.get(normalized)
        if cached is not None:
            return cached
        result = execution_account_scope_by_strategy_id(
            strategy_contracts or (),
            allowlist=_strategy_allowlist_for_profile(normalized),
        )
        execution_account_scope_cache[normalized] = result
        return result

    def _controller_scope_by_account_scope_for_profile(profile: str) -> dict[str, str]:
        normalized = normalize_profile(profile)
        cached = controller_scope_by_account_scope_cache.get(normalized)
        if cached is not None:
            return cached
        allowlist = _strategy_allowlist_for_profile(normalized)
        allowlist_set = set(allowlist or ())
        use_allowlist = allowlist is not None
        mapping: dict[str, str] = {}
        for contract in decode_strategy_contracts(strategy_contracts or ()):
            if use_allowlist and contract.strategy_id not in allowlist_set:
                continue
            if contract.controller_scope_id is None:
                continue
            mapping.setdefault(contract.execution_account_scope_id, contract.controller_scope_id)
            if contract.hedge_account_scope_id is not None:
                mapping.setdefault(contract.hedge_account_scope_id, contract.controller_scope_id)
        controller_scope_by_account_scope_cache[normalized] = mapping
        return mapping

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
        descriptor = _descriptor_for_profile(profile)
        strategy_ids = _strategy_ids_for_profile(profile)
        if strategy_ids:
            return strategy_ids[0]
        if descriptor is not None and descriptor.default_unscoped_api:
            return default_strategy_id
        return None

    def _profile_param_sets(profile: str) -> list[str]:
        out: list[str] = []
        seen: set[str] = set()
        for strategy_id in _strategy_ids_for_profile(profile):
            resolved_param_set = decode_text(store.params_contract(strategy_id).param_set).strip()
            if not resolved_param_set or resolved_param_set in seen:
                continue
            seen.add(resolved_param_set)
            out.append(resolved_param_set)
        return out

    def _default_strategy_for_unscoped_request() -> str:
        if default_unscoped_descriptor is None:
            return default_strategy_id
        strategy_ids = _strategy_ids_for_profile(default_unscoped_descriptor.profile)
        if strategy_ids:
            return strategy_ids[0]
        return default_strategy_id

    def _resolve_strategy_id_for_request(
        *,
        field_name: str = "strategy",
        require_unambiguous_profile: bool = False,
    ) -> str:
        strategy_raw = request.args.get("strategy")
        strategy_text = decode_text(strategy_raw).strip()
        if strategy_text:
            return _resolve_strategy_id(strategy_text, field_name=field_name, explicit=True)

        profile_text = decode_text(request.args.get("profile")).strip()
        if profile_text:
            if require_unambiguous_profile:
                profile_param_sets = _profile_param_sets(profile_text)
                if len(profile_param_sets) > 1:
                    raise ApiEnvelopeError(
                        status=400,
                        code="ambiguous_strategy_target",
                        message=(
                            "Profile-scoped params requests require an explicit `strategy` "
                            "when the profile spans multiple param sets."
                        ),
                        details={
                            "profile": profile_text,
                            "strategy_ids": _strategy_ids_for_profile(profile_text),
                            "param_sets": profile_param_sets,
                        },
                    )
            resolved_strategy = _strategy_for_profile(profile_text)
            if resolved_strategy:
                return _resolve_strategy_id(resolved_strategy, field_name=field_name, explicit=False)

        return _resolve_strategy_id(
            _default_strategy_for_unscoped_request(),
            field_name=field_name,
            explicit=False,
        )

    socket_server = create_flux_socket_server(
        app,
        store=store,
        metadata_resolver=_metadata_for_strategy,
        strategy_resolver=_strategy_for_profile,
        strategy_ids_resolver=_strategy_ids_for_profile,
        required_strategy_ids_resolver=_required_strategy_ids_for_profile,
    )
    socket_emitter = socket_server.emitter
    app.extensions["flux_strategy_set_descriptors"] = dict(strategy_set_descriptors)
    app.extensions["flux_profile_strategy_map"] = dict(resolved_profile_strategy_map)
    app.extensions["flux_profile_required_strategy_map"] = dict(resolved_profile_required_strategy_map)

    def _requested_contract_version() -> int | None:
        raw_value = request.args.get("contract_version")
        if raw_value is None or decode_text(raw_value).strip() == "":
            return None
        parsed = safe_int(raw_value)
        if parsed is None:
            raise ApiEnvelopeError(
                status=400,
                code="invalid_contract_version",
                message="`contract_version` must be an integer.",
            )
        if int(parsed) != REALTIME_STANDARD_CONTRACT_VERSION:
            raise ApiEnvelopeError(
                status=400,
                code="unsupported_contract_version",
                message="Requested realtime contract version is not supported.",
                details={"contract_version": int(parsed)},
            )
        return int(parsed)

    def _profile_for_realtime_snapshot(profile_text: str) -> str:
        normalized = normalize_profile(profile_text)
        if normalized:
            return normalized
        if default_unscoped_descriptor is not None:
            descriptor_profile = normalize_profile(default_unscoped_descriptor.profile)
            if descriptor_profile:
                return descriptor_profile
        return ""

    def _realtime_snapshot_metadata(
        *,
        surface: str,
        profile_text: str,
        strategy_ids: Sequence[str],
        last_seq: int,
    ) -> dict[str, Any]:
        normalized_profile = _profile_for_realtime_snapshot(profile_text)
        return build_standard_snapshot_metadata(
            surface=surface,
            profile=normalized_profile,
            strategy_ids=strategy_ids,
            last_seq=last_seq,
            poll_interval_s=socket_emitter.poll_interval_s,
        )

    def _is_canonical_trades_realtime_query(
        *,
        requested_strategy: str,
        profile_text: str,
        requested_limit: int | None,
        offset: int,
        sort_label: str,
        coin_filter: str,
        exchange_filter: str,
        market_type_filter: str,
        side_filter: str,
        signal_id_filter: str,
    ) -> bool:
        if requested_strategy:
            return False
        if not normalize_profile(profile_text):
            return False
        if offset != 0:
            return False
        if requested_limit is not None and int(requested_limit) != 50:
            return False
        if sort_label != "ts_ms_desc":
            return False
        if any(
            (
                coin_filter,
                exchange_filter,
                market_type_filter,
                side_filter,
                signal_id_filter,
            ),
        ):
            return False
        return True

    def _canonical_signals_realtime_metadata(
        *,
        requested_strategy: str,
        profile_text: str,
        strategy_ids: Sequence[str],
    ) -> dict[str, Any] | None:
        normalized_profile = normalize_profile(profile_text)
        if requested_strategy:
            return None
        if not normalized_profile:
            return None
        stream_metadata, _ = socket_emitter.resolve_standard_subscription_descriptor(
            contract_version=REALTIME_STANDARD_CONTRACT_VERSION,
            surface="signal",
            profile=normalized_profile,
        )
        if stream_metadata is None:
            return None
        request_metadata = _realtime_snapshot_metadata(
            surface="signal",
            profile_text=profile_text,
            strategy_ids=strategy_ids,
            last_seq=socket_emitter.current_standard_seq(normalized_profile, "signal"),
        )
        for key in ("surface_query_key", "stream_id", "snapshot_revision"):
            if request_metadata.get(key) != stream_metadata.get(key):
                return None
        return stream_metadata

    def _canonical_trades_realtime_metadata(
        *,
        requested_strategy: str,
        profile_text: str,
        strategy_ids: Sequence[str],
        requested_limit: int | None,
        offset: int,
        sort_label: str,
        coin_filter: str,
        exchange_filter: str,
        market_type_filter: str,
        side_filter: str,
        signal_id_filter: str,
    ) -> dict[str, Any] | None:
        if not _is_canonical_trades_realtime_query(
            requested_strategy=requested_strategy,
            profile_text=profile_text,
            requested_limit=requested_limit,
            offset=offset,
            sort_label=sort_label,
            coin_filter=coin_filter,
            exchange_filter=exchange_filter,
            market_type_filter=market_type_filter,
            side_filter=side_filter,
            signal_id_filter=signal_id_filter,
        ):
            return None
        normalized_profile = normalize_profile(profile_text)
        if not normalized_profile:
            return None
        stream_metadata, _ = socket_emitter.resolve_standard_subscription_descriptor(
            contract_version=REALTIME_STANDARD_CONTRACT_VERSION,
            surface="trades",
            profile=normalized_profile,
        )
        if stream_metadata is None:
            return None
        request_metadata = _realtime_snapshot_metadata(
            surface="trades",
            profile_text=profile_text,
            strategy_ids=strategy_ids,
            last_seq=socket_emitter.current_standard_seq(normalized_profile, "trades"),
        )
        for key in ("surface_query_key", "stream_id", "snapshot_revision"):
            if request_metadata.get(key) != stream_metadata.get(key):
                return None
        return stream_metadata

    def _canonical_alerts_realtime_metadata(
        *,
        requested_strategy: str,
        profile_text: str,
        strategy_ids: Sequence[str],
        limit: int,
        offset: int,
    ) -> dict[str, Any] | None:
        if requested_strategy:
            return None
        if limit != 50:
            return None
        if offset != 0:
            return None
        normalized_profile = normalize_profile(profile_text)
        if not normalized_profile:
            return None
        stream_metadata, _ = socket_emitter.resolve_standard_subscription_descriptor(
            contract_version=REALTIME_STANDARD_CONTRACT_VERSION,
            surface="alerts",
            profile=normalized_profile,
        )
        if stream_metadata is None:
            return None
        request_metadata = _realtime_snapshot_metadata(
            surface="alerts",
            profile_text=profile_text,
            strategy_ids=strategy_ids,
            last_seq=socket_emitter.current_standard_seq(normalized_profile, "alerts"),
        )
        for key in ("surface_query_key", "stream_id", "snapshot_revision"):
            if request_metadata.get(key) != stream_metadata.get(key):
                return None
        return stream_metadata

    def _canonical_balances_realtime_metadata(
        *,
        requested_strategy: str,
        profile_text: str,
        strategy_ids: Sequence[str],
        limit: int,
    ) -> dict[str, Any] | None:
        if requested_strategy:
            return None
        if limit != 50:
            return None
        normalized_profile = _profile_for_realtime_snapshot(profile_text)
        if not normalized_profile:
            return None
        stream_metadata, _ = socket_emitter.resolve_standard_subscription_descriptor(
            contract_version=REALTIME_STANDARD_CONTRACT_VERSION,
            surface="balances",
            profile=normalized_profile,
        )
        if stream_metadata is None:
            return None
        request_metadata = _realtime_snapshot_metadata(
            surface="balances",
            profile_text=profile_text,
            strategy_ids=strategy_ids,
            last_seq=socket_emitter.current_standard_seq(normalized_profile, "balances"),
        )
        for key in ("surface_query_key", "stream_id", "snapshot_revision"):
            if request_metadata.get(key) != stream_metadata.get(key):
                return None
        return stream_metadata

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
        strategy_id = _resolve_strategy_id_for_request(
            field_name="strategy",
            require_unambiguous_profile=True,
        )
        contract = store.params_contract(strategy_id)
        return _ok(
            data={
                "params": _ordered_params_schema(contract.schema),
                "deprecated": {},
                "params_defaults": dict(contract.defaults),
                "param_set": contract.param_set,
            },
        )

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

        running_states = store.load_running_states(strategy_ids)
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
            contract = store.params_contract(strategy_id)
            state_summary = store.load_state_summary(strategy_id)
            state_ts_ms = safe_int(state_summary.get("state_ts_ms"))
            if (
                state_summary.get("state") == "on_stop"
                and state_ts_ms is not None
                and now_ms() - state_ts_ms > PARAMS_RUNNING_STALE_AFTER_MS
            ):
                state_summary = {}
            payload = build_params_payload(
                strategy_id=strategy_id,
                params=params,
                schema=_ordered_params_schema(contract.schema),
                running=running_states.get(strategy_id),
                metadata=_metadata_for_strategy(strategy_id),
            )
            payload["params_defaults"] = dict(contract.defaults)
            payload["param_set"] = contract.param_set
            payload["persisted_bot_on"] = (
                state_summary.get("persisted_bot_on")
                if "persisted_bot_on" in state_summary
                else safe_bool(params.get("bot_on"))
            )
            payload["config_bot_on"] = (
                state_summary.get("config_bot_on")
                if "config_bot_on" in state_summary
                else payload["persisted_bot_on"]
            )
            payload["effective_bot_on"] = (
                state_summary.get("effective_bot_on")
                if "effective_bot_on" in state_summary
                else payload["persisted_bot_on"]
            )
            payload["bot_on_reason"] = decode_text(
                state_summary.get("bot_on_reason")
                or ("running" if payload["effective_bot_on"] else "bot_off"),
            ).strip()
            payload["startup_bot_off_active"] = bool(
                state_summary.get("startup_bot_off_active", False),
            )
            payload["terminal_order_denial_active"] = bool(
                state_summary.get("terminal_order_denial_active", False),
            )
            if "state" in state_summary:
                payload["state"] = state_summary["state"]
            payloads.append(payload)
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
                sid = _resolve_strategy_id(
                    sid_text,
                    field_name=f"updates[{index}].strategy_id",
                    explicit=True,
                )
            except ApiEnvelopeError as e:
                _record_failed(failed, sid_text)
                errors.append(
                    {
                        "index": index,
                        "strategy_id": sid_text,
                        "code": e.code,
                        "message": e.message,
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
            strategy_id = _resolve_strategy_id_for_request(
                field_name="strategy",
                require_unambiguous_profile=True,
            )
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
        contract_version = _requested_contract_version()
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_strategy_ids = _strategy_ids_for_profile(profile_text) if profile_text else []

        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        elif profile_strategy_ids:
            strategy_ids = profile_strategy_ids
        else:
            strategy_ids = [_resolve_strategy_id_for_request(field_name="strategy")]

        running_states = store.load_running_states(strategy_ids)
        strategy_payloads: list[dict[str, Any]] = []
        for strategy_id in strategy_ids:
            try:
                strategy_payload = store.load_signals_payload(
                    strategy_id,
                    _metadata_for_strategy(strategy_id),
                    running=running_states.get(strategy_id),
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

        payload: dict[str, Any] = {
            "server_ts_ms": now_ms(),
            "strategies": strategy_payloads,
        }
        if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
            realtime_metadata = _canonical_signals_realtime_metadata(
                requested_strategy=requested_strategy,
                profile_text=profile_text,
                strategy_ids=strategy_ids,
            )
            if realtime_metadata is not None:
                payload["realtime"] = realtime_metadata

        return _ok(data=payload)

    @app.get("/api/v1/strategies")
    def api_strategies() -> Response:
        strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
        running_state = store.load_running_states([strategy_id]).get(strategy_id)
        try:
            strategy_payload = store.load_signals_payload(
                strategy_id,
                _metadata_for_strategy(strategy_id),
                running=running_state,
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
        contract = store.params_contract(sid)
        payload = {
            "strategy_id": sid,
            "params": params,
            "schema": _ordered_params_schema(contract.schema),
            "params_defaults": dict(contract.defaults),
            "param_set": contract.param_set,
        }
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
        contract = store.params_contract(sid)
        payload = {
            "strategy_id": sid,
            "updated": result["updated"],
            "params": result["params"],
            "schema": _ordered_params_schema(contract.schema),
            "params_defaults": dict(contract.defaults),
            "param_set": contract.param_set,
        }
        return _ok(data=payload)

    @app.get("/api/v1/balances")
    def api_balances() -> Response:
        contract_version = _requested_contract_version()
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        limit = _clamp_limit(
            request.args.get("limit"),
            default=50,
            minimum=1,
            maximum=STRATEGY_BALANCES_MAX_LIMIT if requested_strategy else BALANCES_MAX_LIMIT,
        )
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)
        profile_strategy_ids = _strategy_ids_for_profile(profile_text) if profile_text else []
        if profile_normalized == "tokenmm" and profile_text and not profile_strategy_ids:
            profile_strategy_ids = store.discover_strategy_ids_from_params()

        if requested_strategy:
            strategy_id = _resolve_strategy_id(requested_strategy, field_name="strategy")
            strategy_ids = [strategy_id]
            rows = store.load_balances_rows(strategy_id)
            response_ts_ms = now_ms()
        elif profile_strategy_ids:
            strategy_ids = profile_strategy_ids
            required_strategy_ids = set(
                _required_strategy_ids_for_profile(profile_text, fallback=strategy_ids),
            )
            request_now_ms = now_ms()
            projection_enabled = _profile_supports_account_projections(profile_normalized)
            controller_scope_by_account_scope = (
                _controller_scope_by_account_scope_for_profile(profile_normalized)
                if profile_normalized == "tokenmm"
                else {}
            )
            projection_rows: list[dict[str, Any]] = []
            projection_totals: dict[str, Any] = {}
            projection_scope_status: list[dict[str, Any]] = []
            if projection_enabled:
                (
                    projection_rows,
                    projection_totals,
                    projection_scope_status,
                ) = store.load_profile_account_projection_rows(
                    profile_normalized,
                    account_scope_ids=_profile_projection_scope_ids_for_profile(profile_normalized),
                )
            portfolio_snapshot = (
                store.load_portfolio_snapshot(profile_normalized)
                if profile_text
                else None
            )
            if portfolio_snapshot is not None:
                inventory_summary = _portfolio_snapshot_inventory_summary(portfolio_snapshot)
                if profile_normalized == "equities" and inventory_summary is not None:
                    snapshot_stale_after_ms = inventory_summary["stale_after_ms"]
                    if _timestamp_is_fresh(
                        portfolio_snapshot.get("server_ts_ms"),
                        now_ms_value=request_now_ms,
                        stale_after_ms=snapshot_stale_after_ms,
                    ):
                        snapshot_balances = portfolio_snapshot.get("balances")
                        snapshot_accounts = portfolio_snapshot.get("accounts")
                        snapshot_balance_rows = _portfolio_snapshot_rows(
                            snapshot_balances.get("rows")
                            if isinstance(snapshot_balances, Mapping)
                            else [],
                        )
                        snapshot_account_rows = _portfolio_snapshot_rows(
                            snapshot_accounts.get("rows")
                            if isinstance(snapshot_accounts, Mapping)
                            else [],
                        )
                        snapshot_scope_status = _normalize_scope_status_entries(
                            snapshot_accounts.get("scope_status")
                            if isinstance(snapshot_accounts, Mapping)
                            else [],
                        )
                        snapshot_account_rows_missing = not snapshot_account_rows
                        if projection_rows and snapshot_account_rows_missing:
                            snapshot_account_rows = [*snapshot_account_rows, *projection_rows]
                        scope_status = _merge_scope_status_entries(
                            snapshot_scope_status,
                            projection_scope_status,
                        )
                        snapshot_rows = combine_portfolio_snapshot_rows(
                            balance_rows=snapshot_balance_rows,
                            account_rows=snapshot_account_rows,
                            portfolio_id=decode_text(
                                portfolio_snapshot.get("portfolio_id") or profile_normalized,
                            ),
                        )
                        rows = filter_balance_rows_for_contract_scope(
                            snapshot_rows,
                            contracts=store._contracts,
                            preserve_shared_account_rows=True,
                        )
                        if not rows and snapshot_rows:
                            rows = snapshot_rows
                        if strategy_ids:
                            market_rows = store.load_market_rows_for_strategies(strategy_ids)
                            rows = collapse_balance_display_rows(
                                enrich_balances_rows(
                                    rows,
                                    contracts=store._contracts,
                                    market_rows=market_rows,
                                ),
                            )
                        reconciliation_rows = (
                            _rows_for_reconciliation(rows)
                            if profile_normalized == "equities"
                            else [dict(row) for row in rows]
                        )
                        rows, _ = build_balance_risk_groups(rows)
                        _, risk_groups = build_balance_risk_groups(reconciliation_rows)
                        response_ts_ms = safe_int(portfolio_snapshot.get("server_ts_ms")) or request_now_ms
                        base_currency = decode_text(portfolio_snapshot.get("base_currency")).strip().upper()
                        if not base_currency and len(inventory_summary["inventory_by_asset"]) == 1:
                            base_currency = next(iter(inventory_summary["inventory_by_asset"]))
                        totals = _balances_totals(reconciliation_rows)
                        if isinstance(snapshot_accounts, Mapping):
                            account_totals = snapshot_accounts.get("totals")
                            if isinstance(account_totals, Mapping):
                                totals.update(dict(account_totals))
                            elif projection_totals and snapshot_account_rows_missing:
                                totals.update(dict(projection_totals))
                        total_rows = len(rows)
                        payload = {
                            "source": "portfolio_snapshot_v2",
                            "rows": rows[:limit],
                            "count": total_rows,
                            "total": total_rows,
                            "limit": limit,
                            "totals": totals,
                            "risk_groups": risk_groups,
                            "server_ts_ms": response_ts_ms,
                            "portfolio_id": decode_text(
                                portfolio_snapshot.get("portfolio_id") or profile_normalized,
                            ),
                            "base_currency": base_currency,
                            "inventory_by_asset": inventory_summary["inventory_by_asset"],
                            "components": inventory_summary["components"],
                            "degraded": bool(
                                inventory_summary["degraded"]
                                or _scope_status_entries_degraded(scope_status)
                            ),
                            "missing_required": inventory_summary["missing_required"],
                            "stale_required": inventory_summary["stale_required"],
                            "null_qty_required": inventory_summary["null_qty_required"],
                            "stale_after_ms": snapshot_stale_after_ms,
                            **({"scope_status": scope_status} if scope_status else {}),
                        }
                        if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
                            realtime_metadata = _canonical_balances_realtime_metadata(
                                requested_strategy=requested_strategy,
                                profile_text=profile_text,
                                strategy_ids=strategy_ids,
                                limit=limit,
                            )
                            if realtime_metadata is not None:
                                payload["realtime"] = realtime_metadata
                        return _ok(data=payload)
                elif profile_normalized == "tokenmm":
                    inventory = portfolio_snapshot.get("inventory")
                    inventory_payload = dict(inventory) if isinstance(inventory, Mapping) else {}
                    snapshot_stale_after_ms = (
                        safe_int(inventory_payload.get("stale_after_ms"))
                        or TOKENMM_BALANCES_STALE_AFTER_MS
                    )
                    if (
                        _timestamp_is_fresh(
                            portfolio_snapshot.get("server_ts_ms"),
                            now_ms_value=request_now_ms,
                            stale_after_ms=snapshot_stale_after_ms,
                        )
                        and _timestamp_is_fresh(
                            inventory_payload.get("ts_ms"),
                            now_ms_value=request_now_ms,
                            stale_after_ms=snapshot_stale_after_ms,
                        )
                    ):
                        snapshot_balances = portfolio_snapshot.get("balances")
                        snapshot_accounts = portfolio_snapshot.get("accounts")
                        snapshot_balance_rows = _portfolio_snapshot_rows(
                            snapshot_balances.get("rows")
                            if isinstance(snapshot_balances, Mapping)
                            else [],
                        )
                        snapshot_account_rows = _portfolio_snapshot_rows(
                            snapshot_accounts.get("rows")
                            if isinstance(snapshot_accounts, Mapping)
                            else [],
                        )
                        snapshot_scope_status = _normalize_scope_status_entries(
                            snapshot_accounts.get("scope_status")
                            if isinstance(snapshot_accounts, Mapping)
                            else [],
                        )
                        snapshot_account_rows_missing = not snapshot_account_rows
                        if projection_rows and snapshot_account_rows_missing:
                            snapshot_account_rows = [*snapshot_account_rows, *projection_rows]
                        scope_status = _merge_scope_status_entries(
                            snapshot_scope_status,
                            projection_scope_status,
                        )
                        snapshot_rows = combine_portfolio_snapshot_rows(
                            balance_rows=snapshot_balance_rows,
                            account_rows=snapshot_account_rows,
                            portfolio_id=decode_text(
                                portfolio_snapshot.get("portfolio_id") or profile_normalized,
                            ),
                        )
                        rows = filter_balance_rows_for_contract_scope(
                            snapshot_rows,
                            contracts=store._contracts,
                            preserve_shared_account_rows=projection_enabled,
                        )
                        if not rows and snapshot_rows:
                            rows = snapshot_rows
                        if strategy_ids:
                            market_rows = store.load_market_rows_for_strategies(strategy_ids)
                            rows = collapse_balance_display_rows(
                                enrich_balances_rows(
                                    rows,
                                    contracts=store._contracts,
                                    market_rows=market_rows,
                                ),
                            )
                        if controller_scope_by_account_scope:
                            rows = prefer_controller_managed_balance_rows(
                                rows,
                                controller_scope_by_account_scope=controller_scope_by_account_scope,
                            )
                        reconciliation_rows = (
                            _rows_for_reconciliation(rows)
                            if projection_enabled
                            else [dict(row) for row in rows]
                        )
                        rows, _ = build_balance_risk_groups(rows)
                        _, risk_groups = build_balance_risk_groups(reconciliation_rows)
                        response_ts_ms = (
                            safe_int(portfolio_snapshot.get("server_ts_ms"))
                            or safe_int(inventory_payload.get("ts_ms"))
                            or request_now_ms
                        )
                        components_payload = inventory_payload.get("components")
                        if not isinstance(components_payload, list):
                            components_payload = portfolio_snapshot.get("components")
                        components = [
                            dict(component)
                            for component in (components_payload or [])
                            if isinstance(component, Mapping)
                        ]
                        total_rows = len(rows)
                        totals = _balances_totals(reconciliation_rows)
                        if isinstance(snapshot_accounts, Mapping):
                            account_totals = snapshot_accounts.get("totals")
                            if isinstance(account_totals, Mapping):
                                totals.update(dict(account_totals))
                            elif projection_totals and snapshot_account_rows_missing:
                                totals.update(dict(projection_totals))
                        elif projection_totals and snapshot_account_rows_missing:
                            totals.update(dict(projection_totals))
                        payload = {
                            "source": "portfolio_snapshot",
                            "rows": rows[:limit],
                            "count": total_rows,
                            "total": total_rows,
                            "limit": limit,
                            "totals": totals,
                            "risk_groups": risk_groups,
                            "server_ts_ms": response_ts_ms,
                            "portfolio_id": decode_text(
                                portfolio_snapshot.get("portfolio_id") or profile_normalized,
                            ),
                            "base_currency": decode_text(
                                portfolio_snapshot.get("base_currency")
                                or inventory_payload.get("base_currency"),
                            ).strip().upper(),
                            "components": components,
                            "degraded": bool(
                                inventory_payload.get("degraded", False)
                                or _scope_status_entries_degraded(scope_status)
                            ),
                            "global_qty_base": inventory_payload.get("global_qty_base")
                            or inventory_payload.get("global_qty"),
                            "global_qty": inventory_payload.get("global_qty"),
                            "aggregation_mode": decode_text(
                                inventory_payload.get("aggregation_mode") or "strict",
                            ),
                            "global_qty_base_complete": bool(
                                inventory_payload.get("global_qty_base_complete", True),
                            ),
                            "global_qty_complete": bool(
                                inventory_payload.get("global_qty_complete", True),
                            ),
                            "missing_required": list(inventory_payload.get("missing_required") or []),
                            "stale_required": list(inventory_payload.get("stale_required") or []),
                            "null_qty_required": list(inventory_payload.get("null_qty_required") or []),
                            "stale_after_ms": snapshot_stale_after_ms,
                            **({"scope_status": scope_status} if scope_status else {}),
                        }
                        if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
                            realtime_metadata = _canonical_balances_realtime_metadata(
                                requested_strategy=requested_strategy,
                                profile_text=profile_text,
                                strategy_ids=strategy_ids,
                                limit=limit,
                            )
                            if realtime_metadata is not None:
                                payload["realtime"] = realtime_metadata
                        return _ok(data=payload)

            response_ts_ms = request_now_ms
            rows_by_strategy: dict[str, list[dict[str, Any]]] = {}
            components: list[dict[str, Any]] = []

            for strategy_id in strategy_ids:
                strategy_rows, snapshot_present = store.load_balances_rows_with_presence(strategy_id)
                rows_by_strategy[strategy_id] = strategy_rows
                latest_ts_ms: int | None = None
                for row in strategy_rows:
                    parsed = safe_int(row.get("ts_ms"))
                    if parsed is None:
                        parsed = coerce_ts_ms(row.get("ts") or row.get("timestamp"))
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
                portfolio_id=profile_normalized,
                preserve_product_scope_cash=True,
                execution_account_scope_by_strategy=(
                    _execution_account_scopes_for_profile(profile_normalized)
                    if projection_enabled
                    else None
                ),
                shared_position_groups_by_strategy=(
                    _shared_position_groups_for_profile(profile_normalized)
                    if profile_normalized == "equities"
                    else None
                ),
            )
            if projection_enabled:
                if projection_rows:
                    rows = combine_portfolio_snapshot_rows(
                        balance_rows=rows,
                        account_rows=projection_rows,
                        portfolio_id=profile_normalized,
                    )
            filtered_rows = filter_balance_rows_for_contract_scope(
                rows,
                contracts=store._contracts,
                preserve_shared_account_rows=projection_enabled,
            )
            if filtered_rows:
                rows = filtered_rows
            if strategy_ids:
                market_rows = store.load_market_rows_for_strategies(strategy_ids)
                rows = collapse_balance_display_rows(
                    enrich_balances_rows(
                        rows,
                        contracts=store._contracts,
                        market_rows=market_rows,
                    ),
                )
            if controller_scope_by_account_scope:
                rows = prefer_controller_managed_balance_rows(
                    rows,
                    controller_scope_by_account_scope=controller_scope_by_account_scope,
                )
            reconciliation_rows = (
                _rows_for_reconciliation(rows)
                if projection_enabled
                else [dict(row) for row in rows]
            )
            rows, _ = build_balance_risk_groups(rows)
            _, risk_groups = build_balance_risk_groups(reconciliation_rows)
            missing_required = sorted(
                component["strategy_id"]
                for component in components
                if component["required"] and component["missing"]
            )
            degraded = (
                bool(missing_required)
                or any(component["stale"] for component in components)
                or _scope_status_entries_degraded(projection_scope_status)
            )
            total_rows = len(rows)
            totals = _balances_totals(reconciliation_rows)
            if projection_totals:
                totals.update(projection_totals)
            payload = {
                "rows": rows[:limit],
                "count": total_rows,
                "total": total_rows,
                "limit": limit,
                "totals": totals,
                "risk_groups": risk_groups,
                "server_ts_ms": response_ts_ms,
                "components": components,
                "degraded": degraded,
                "missing_required": missing_required,
                "stale_after_ms": TOKENMM_BALANCES_STALE_AFTER_MS,
                **({"scope_status": projection_scope_status} if projection_scope_status else {}),
            }
            if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
                realtime_metadata = _canonical_balances_realtime_metadata(
                    requested_strategy=requested_strategy,
                    profile_text=profile_text,
                    strategy_ids=strategy_ids,
                    limit=limit,
                )
                if realtime_metadata is not None:
                    payload["realtime"] = realtime_metadata
            return _ok(data=payload)
        else:
            strategy_id = _resolve_strategy_id_for_request(field_name="strategy")
            strategy_ids = [strategy_id]
            rows = store.load_balances_rows(strategy_id)
            response_ts_ms = now_ms()

        rows, risk_groups = build_balance_risk_groups(rows)
        total_rows = len(rows)
        payload = {
            "rows": rows[:limit],
            "count": total_rows,
            "total": total_rows,
            "limit": limit,
            "totals": _balances_totals(rows),
            "risk_groups": risk_groups,
            "server_ts_ms": response_ts_ms,
        }
        if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
            realtime_metadata = _canonical_balances_realtime_metadata(
                requested_strategy=requested_strategy,
                profile_text=profile_text,
                strategy_ids=strategy_ids,
                limit=limit,
            )
            if realtime_metadata is not None:
                payload["realtime"] = realtime_metadata
        return _ok(data=payload)

    @app.get("/api/v1/trades")
    def api_trades() -> Response:
        contract_version = _requested_contract_version()
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_strategy_ids = _strategy_ids_for_profile(profile_text) if profile_text else []
        requested_limit_raw = request.args.get("limit")
        requested_limit = safe_int(requested_limit_raw)
        limit = _clamp_limit(requested_limit_raw, default=50, minimum=1, maximum=200)
        offset = _clamp_offset(request.args.get("offset"), default=0)
        coin_filter = decode_text(request.args.get("coin")).strip().upper()
        exchange_filter = decode_text(request.args.get("exchange")).strip().lower()
        market_type_filter = decode_text(request.args.get("market_type")).strip().lower()
        side_filter = _normalize_trade_side(request.args.get("side"))
        signal_id_filter = decode_text(request.args.get("signal_id")).strip()
        sort_raw = decode_text(request.args.get("sort")).strip().lower()
        sort_ascending = sort_raw in {"asc", "ts_ms_asc"}
        sort_label = "ts_ms_asc" if sort_ascending else "ts_ms_desc"
        has_filters = bool(
            coin_filter or exchange_filter or market_type_filter or side_filter or signal_id_filter
        )

        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        elif profile_strategy_ids:
            strategy_ids = profile_strategy_ids
        else:
            strategy_ids = [_resolve_strategy_id_for_request(field_name="strategy")]

        multi_strategy_profile_fanout = (
            not requested_strategy
            and bool(profile_strategy_ids)
            and len(strategy_ids) > 1
        )

        source_rows: list[dict[str, Any]] = []
        total_count_override: int | None = None
        if has_filters or sort_ascending:
            for strategy_id in strategy_ids:
                base_first_qty = _strategy_groups_include_tokenmm(_metadata_for_strategy(strategy_id))
                strategy_rows = store.load_all_trades_rows(
                    strategy_id,
                    base_first_qty=base_first_qty,
                )
                for row in strategy_rows:
                    normalized_row = dict(row)
                    normalized_row.setdefault("strategy_id", strategy_id)
                    source_rows.append(normalized_row)
        else:
            total_count_override = 0
            page_span = max(1, offset + limit)
            for strategy_id in strategy_ids:
                base_first_qty = _strategy_groups_include_tokenmm(_metadata_for_strategy(strategy_id))
                strategy_rows = store.load_trades_rows(
                    strategy_id,
                    limit=page_span,
                    since_ms=None,
                    since_seq=None,
                    base_first_qty=base_first_qty,
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
            exchange = decode_text(row.get("venue") or row.get("exchange")).strip().lower()
            if exchange_filter and exchange != exchange_filter:
                continue
            market_type = decode_text(row.get("product_type") or row.get("market_type")).strip().lower()
            if market_type_filter and market_type != market_type_filter:
                continue
            side = _normalize_trade_side(row.get("side"))
            if side_filter and side != side_filter:
                continue
            signal_id = decode_text(row.get("signal_id") or row.get("strategy_id")).strip()
            if signal_id_filter and signal_id != signal_id_filter:
                continue
            filtered_rows.append(row)

        compatibility_mode = _tokenmm_trade_rows_require_reset_for_strategies(
            strategy_ids=strategy_ids,
            metadata_resolver=_metadata_for_strategy,
            stream_reset_resolver=store.tokenmm_trade_stream_requires_reset,
        )

        if multi_strategy_profile_fanout:
            filtered_rows.sort(
                key=_trade_sort_key,
                reverse=not sort_ascending,
            )
            # Multi-strategy profile views do not expose a synthetic global sequence cursor.
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
            "reset_required": False,
        }
        if compatibility_mode:
            payload["compatibility_mode"] = True
        if has_more:
            payload["next_offset"] = offset + len(rows)
        if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
            realtime_metadata = _canonical_trades_realtime_metadata(
                requested_strategy=requested_strategy,
                profile_text=profile_text,
                strategy_ids=strategy_ids,
                requested_limit=requested_limit,
                offset=offset,
                sort_label=sort_label,
                coin_filter=coin_filter,
                exchange_filter=exchange_filter,
                market_type_filter=market_type_filter,
                side_filter=side_filter,
                signal_id_filter=signal_id_filter,
            )
            if realtime_metadata is not None:
                payload["realtime"] = realtime_metadata
        return _ok(data=payload)

    @app.get("/api/v1/trades/delta")
    def api_trades_delta() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_strategy_ids = _strategy_ids_for_profile(profile_text) if profile_text else []
        if requested_strategy:
            strategy_ids = [_resolve_strategy_id(requested_strategy, field_name="strategy")]
        elif profile_strategy_ids:
            strategy_ids = profile_strategy_ids
        else:
            strategy_ids = [_resolve_strategy_id_for_request(field_name="strategy")]

        multi_strategy_profile_fanout = (
            not requested_strategy
            and bool(profile_strategy_ids)
            and len(strategy_ids) > 1
        )
        compatibility_mode = _tokenmm_trade_rows_require_reset_for_strategies(
            strategy_ids=strategy_ids,
            metadata_resolver=_metadata_for_strategy,
            stream_reset_resolver=store.tokenmm_trade_stream_requires_reset,
        )

        def _delta_ok(
            *,
            rows: list[dict[str, Any]],
            last_seq: int,
            reset_required: bool,
        ) -> Response:
            payload: dict[str, Any] = {
                "rows": rows,
                "last_seq": int(last_seq),
                "reset_required": reset_required,
            }
            if compatibility_mode:
                payload["compatibility_mode"] = True
            return _ok(data=payload)

        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        since_seq = safe_int(request.args.get("since_seq"))
        since_ms = None if since_seq is not None else coerce_ts_ms(request.args.get("after"))
        after_row_id = decode_text(request.args.get("after_row_id")).strip()
        after_version = safe_int(request.args.get("after_version"))
        fallback_seq = since_seq or 0

        if multi_strategy_profile_fanout:
            if since_seq is not None:
                # Safe Phase 1 behavior: multi-strategy profile delta does not claim
                # a synthetic global cursor; clients should resync to snapshot.
                return _delta_ok(rows=[], last_seq=0, reset_required=since_seq > 0)

            rows: list[dict[str, Any]] = []
            for strategy_id in strategy_ids:
                base_first_qty = _strategy_groups_include_tokenmm(_metadata_for_strategy(strategy_id))
                strategy_rows = _rows_after_trade_replay_cursor(
                    store.load_all_trades_rows(strategy_id, base_first_qty=base_first_qty),
                    after_ms=since_ms,
                    after_row_id=after_row_id,
                    after_version=after_version,
                )
                for row in strategy_rows:
                    normalized_row = dict(row)
                    normalized_row.setdefault("strategy_id", strategy_id)
                    rows.append(normalized_row)
            rows.sort(key=_trade_replay_sort_key)
            return _delta_ok(rows=rows[:limit], last_seq=0, reset_required=False)

        strategy_id = strategy_ids[0]
        base_first_qty = _strategy_groups_include_tokenmm(_metadata_for_strategy(strategy_id))
        if since_seq is not None:
            scan_limit = 2_000
            scanned_rows = store.load_trades_rows(
                strategy_id,
                limit=scan_limit,
                since_ms=None,
                since_seq=None,
                scan_limit=scan_limit,
                base_first_qty=base_first_qty,
            )
            seq_values = [safe_int(row.get("seq")) for row in scanned_rows]
            parsed_seqs = [seq for seq in seq_values if seq is not None]
            if not parsed_seqs:
                reset_required = since_seq > 0
                return _delta_ok(rows=[], last_seq=0, reset_required=reset_required)

            min_seq = min(parsed_seqs)
            max_seq = max(parsed_seqs)
            if since_seq < (min_seq - 1):
                if since_seq <= 0 and (compatibility_mode or len(scanned_rows) < scan_limit):
                    rows = scanned_rows[:limit]
                    return _delta_ok(
                        rows=rows,
                        last_seq=_extract_last_seq(rows, fallback=max_seq),
                        reset_required=False,
                    )
                return _delta_ok(rows=[], last_seq=int(since_seq), reset_required=True)

            if since_seq > max_seq:
                return _delta_ok(rows=[], last_seq=int(max_seq), reset_required=True)

            eligible_rows: list[dict[str, Any]] = []
            for row in scanned_rows:
                seq = safe_int(row.get("seq"))
                if seq is None or seq <= since_seq:
                    continue
                eligible_rows.append(row)
            eligible_rows.sort(key=lambda item: safe_int(item.get("seq")) or 0)
            rows = eligible_rows[:limit]
            last_seq = safe_int(rows[-1].get("seq")) if rows else since_seq
            return _delta_ok(
                rows=rows,
                last_seq=int(last_seq if last_seq is not None else since_seq),
                reset_required=False,
            )

        if since_ms is not None:
            rows = _rows_after_trade_ts(
                store.load_all_trades_rows(strategy_id, base_first_qty=base_first_qty),
                since_ms=since_ms,
            )
            rows = rows[:limit]
            return _delta_ok(
                rows=rows,
                last_seq=_extract_last_seq(rows, fallback=fallback_seq),
                reset_required=False,
            )

        rows = store.load_trades_rows(
            strategy_id,
            limit=limit,
            since_ms=since_ms,
            since_seq=None,
            base_first_qty=base_first_qty,
        )
        return _delta_ok(
            rows=rows,
            last_seq=_extract_last_seq(rows, fallback=fallback_seq),
            reset_required=False,
        )

    @app.get("/api/v1/alerts")
    def api_alerts() -> Response:
        contract_version = _requested_contract_version()
        limit = _clamp_limit(request.args.get("limit"), default=50, minimum=1, maximum=200)
        offset = _clamp_offset(request.args.get("offset"), default=0)
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_strategy_ids = _strategy_ids_for_profile(profile_text) if profile_text else []

        if requested_strategy:
            strategy_id = _resolve_strategy_id(requested_strategy, field_name="strategy")
            strategy_ids = [strategy_id]
            all_rows = store.load_all_alerts_rows(strategy_id)
        elif profile_strategy_ids:
            strategy_ids = profile_strategy_ids
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
            strategy_ids = [strategy_id]
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
        if contract_version == REALTIME_STANDARD_CONTRACT_VERSION:
            realtime_metadata = _canonical_alerts_realtime_metadata(
                requested_strategy=requested_strategy,
                profile_text=profile_text,
                strategy_ids=strategy_ids,
                limit=limit,
                offset=offset,
            )
            if realtime_metadata is not None:
                payload["realtime"] = realtime_metadata
        return _ok(data=payload)

    @app.delete("/api/v1/alerts")
    def api_alerts_delete() -> Response:
        requested_strategy = decode_text(request.args.get("strategy")).strip()
        profile_text = decode_text(request.args.get("profile")).strip()
        profile_normalized = normalize_profile(profile_text)
        profile_strategy_ids = _strategy_ids_for_profile(profile_text) if profile_text else []

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

        if profile_strategy_ids:
            strategy_ids = profile_strategy_ids
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
