#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from collections.abc import Mapping
from dataclasses import dataclass
from datetime import datetime
from datetime import time as dt_time
from datetime import timezone
from decimal import Decimal
from pathlib import Path
from typing import Any
from urllib.parse import urlencode
from urllib.request import urlopen

try:
    from zoneinfo import ZoneInfo
except ImportError:  # pragma: no cover - stdlib on supported runtimes
    ZoneInfo = None  # type: ignore[assignment]

from flux.api.payloads import contract_id_for_leg
from flux.common.account_projection import decode_profile_account_snapshot
from flux.common.account_scopes import AccountScopeConfig
from flux.common.account_scopes import decode_account_scopes
from flux.common.controller_scopes import ControllerScopeConfig
from flux.common.controller_scopes import decode_controller_scopes
from flux.common.keys import FluxRedisKeys
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import decode_component
from flux.common.strategy_contracts import StrategyContractEntry
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.runners.shared.bootstrap import build_redis_client
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies.shared.quote_health import evaluate_quote_health


DEFAULT_REQUEST_TIMEOUT_SECS = 5.0
DEFAULT_PROJECTION_MAX_AGE_MS = 120_000
DEFAULT_REQUIRED_BALANCE_SOURCE = "portfolio_snapshot_v2"
DEFAULT_SIGNAL_MAX_AGE_MS = 10_000
PROJECTION_PROVIDERS = frozenset({"binance", "ibkr"})
UNSPECIFIED_BIND_HOSTS = frozenset({"0.0.0.0", "::"})  # noqa: S104
US_EQUITIES_REGULAR_TZ = "America/New_York"
US_EQUITIES_REGULAR_START = dt_time(hour=9, minute=30)
US_EQUITIES_REGULAR_END = dt_time(hour=16, minute=0)
EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")
if EQUITIES_DESCRIPTOR is None:  # pragma: no cover - static descriptor contract
    raise RuntimeError("Equities strategy-set descriptor is not registered")


@dataclass(frozen=True, slots=True)
class EquitiesReadinessThresholds:
    max_stale_signal_legs: int = 0
    max_unhealthy_strategies: int = 0
    projection_max_age_ms: int = DEFAULT_PROJECTION_MAX_AGE_MS
    required_balance_source: str | None = DEFAULT_REQUIRED_BALANCE_SOURCE
    ignore_reference_freshness_outside_regular_session: bool = False
    expected_projection_scope_ids: tuple[str, ...] = ()

    def __post_init__(self) -> None:
        if self.max_stale_signal_legs < 0:
            raise ValueError("`max_stale_signal_legs` must be >= 0")
        if self.max_unhealthy_strategies < 0:
            raise ValueError("`max_unhealthy_strategies` must be >= 0")
        if self.projection_max_age_ms < 0:
            raise ValueError("`projection_max_age_ms` must be >= 0")


@dataclass(frozen=True, slots=True)
class ReadinessCheck:
    name: str
    ok: bool
    summary: str
    details: dict[str, Any]

    def as_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "ok": self.ok,
            "summary": self.summary,
            "details": dict(self.details),
        }


@dataclass(frozen=True, slots=True)
class EquitiesReadinessResult:
    ok: bool
    checks: dict[str, ReadinessCheck]
    summary: dict[str, Any]

    def as_dict(self) -> dict[str, Any]:
        return {
            "ok": self.ok,
            "summary": dict(self.summary),
            "checks": {
                name: check.as_dict()
                for name, check in self.checks.items()
            },
        }


@dataclass(frozen=True, slots=True)
class ProjectionHealthSnapshot:
    check: ReadinessCheck
    missing_config_scope_ids: list[str]
    missing_scope_ids: list[str]
    empty_scope_ids: list[str]
    stale_scope_ids: list[str]


@dataclass(frozen=True, slots=True)
class SignalHealthSnapshot:
    check: ReadinessCheck
    healthy_strategy_count: int
    reference_unhealthy_strategy_ids: list[str]


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _safe_int(value: Any) -> int | None:
    if isinstance(value, bool) or value is None:
        return None
    try:
        return int(value)
    except Exception:
        return None


def _sorted_unique_texts(values: Any) -> list[str]:
    if not isinstance(values, list):
        return []
    return sorted(
        {
            text
            for value in values
            if (text := _optional_text(value)) is not None
        },
    )


def _mapping(value: Any) -> Mapping[str, Any]:
    return value if isinstance(value, Mapping) else {}


def _decode_json_mapping(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        text = raw.decode("utf-8")
    elif isinstance(raw, str):
        text = raw
    else:
        return None
    payload = json.loads(text)
    return dict(payload) if isinstance(payload, Mapping) else None


def _expected_reference_account_scope_id(
    strategy_contracts: tuple[StrategyContractEntry, ...],
) -> str | None:
    scope_ids = sorted(
        {
            scope_id
            for contract in strategy_contracts
            if (scope_id := _optional_text(contract.reference_account_scope_id)) is not None
        },
    )
    return scope_ids[0] if len(scope_ids) == 1 else None


def _signal_state_name(payload: Mapping[str, Any]) -> str:
    state = _mapping(payload.get("state"))
    return (_optional_text(state.get("state")) or "").lower()


def _signal_stale_legs(payload: Mapping[str, Any]) -> list[str]:
    debug = _mapping(payload.get("debug"))
    md_health = _mapping(debug.get("md_health"))
    return _sorted_unique_texts(md_health.get("stale_legs"))


def _signal_role_map(payload: Mapping[str, Any]) -> Mapping[str, Any]:
    role_map = _mapping(payload.get("maker_role_map"))
    if role_map:
        return role_map
    state = _mapping(payload.get("state"))
    return _mapping(state.get("maker_role_map"))


def _signal_state_stale(payload: Mapping[str, Any]) -> bool:
    debug = _mapping(payload.get("debug"))
    md_health = _mapping(debug.get("md_health"))
    return bool(md_health.get("state_stale"))


def _component_present(payload: Any) -> bool:
    if payload is None:
        return False
    if isinstance(payload, StrategyInventoryComponent):
        return True
    row = _mapping(payload)
    return all(
        _optional_text(row.get(field))
        for field in ("strategy_id", "portfolio_id", "base_currency")
    )


def _expected_projection_scope_ids(
    *,
    strategy_contracts: tuple[StrategyContractEntry, ...],
    account_scopes: tuple[AccountScopeConfig, ...],
    controller_scopes: tuple[ControllerScopeConfig, ...] = (),
    overrides: tuple[str, ...],
    providers: frozenset[str] | None = None,
) -> list[str]:
    if overrides:
        return sorted(
            {
                scope_id
                for scope_id in overrides
                if scope_id
            },
        )
    provider_filter = providers or PROJECTION_PROVIDERS
    configured_projection_scope_ids = {
        scope.scope_id
        for scope in account_scopes
        if scope.provider.strip().lower() in provider_filter
    }
    referenced_scope_ids = {
        scope_id
        for contract in strategy_contracts
        for scope_id in (
            contract.reference_account_scope_id,
            contract.hedge_account_scope_id,
        )
        if scope_id
    }
    referenced_scope_ids.update(
        contract.execution_account_scope_id
        for contract in strategy_contracts
        if contract.execution_account_scope_id in configured_projection_scope_ids
    )
    referenced_scope_ids.difference_update(
        scope.writer_account_scope_id
        for scope in controller_scopes
    )
    return sorted(referenced_scope_ids)


def _signal_max_age_ms(payload: Mapping[str, Any]) -> int:
    params = _mapping(payload.get("params"))
    return _safe_int(params.get("max_age_ms")) or DEFAULT_SIGNAL_MAX_AGE_MS


def _normalize_feed_state(value: Any) -> str | None:
    text = (_optional_text(value) or "").lower()
    return text if text in {"ok", "degraded", "down", "unknown"} else None


def _normalize_quote_state(value: Any) -> str | None:
    text = (_optional_text(value) or "").lower()
    return text if text in {"fresh", "old", "missing"} else None


def _safe_decimal(value: Any) -> Decimal | None:
    if value is None or isinstance(value, bool):
        return None
    try:
        return Decimal(str(value))
    except Exception:
        return None


def _signal_quote_snapshot_leg(payload: Mapping[str, Any], *, role: str) -> Mapping[str, Any]:
    equities_arb = _mapping(payload.get("equities_arb"))
    quote_snapshot = _mapping(equities_arb.get("quote_snapshot"))
    if not quote_snapshot:
        maker_v4 = _mapping(payload.get("maker_v4"))
        quote_snapshot = _mapping(maker_v4.get("quote_snapshot"))
    leg_key = {
        "maker": "maker_leg",
        "reference": "ref_leg",
    }.get(role)
    return _mapping(quote_snapshot.get(leg_key))


def _signal_leg_health(
    *,
    explicit_leg: Mapping[str, Any],
    fallback_leg: Mapping[str, Any],
    leg_role: str,
    max_age_ms: int,
) -> tuple[str | None, str | None]:
    explicit_feed_state = _normalize_feed_state(explicit_leg.get("feed_state"))
    explicit_quote_state = _normalize_quote_state(explicit_leg.get("quote_state"))
    if explicit_feed_state is not None or explicit_quote_state is not None:
        return explicit_feed_state, explicit_quote_state

    candidate_leg = explicit_leg if explicit_leg else fallback_leg
    has_transport_hint = (
        candidate_leg.get("ts_ms") is not None
        or candidate_leg.get("bid") is not None
        or candidate_leg.get("ask") is not None
        or _normalize_feed_state(candidate_leg.get("feed_state")) is not None
        or _normalize_quote_state(candidate_leg.get("quote_state")) is not None
    )
    if has_transport_hint:
        health = evaluate_quote_health(
            leg_role=leg_role,
            bid=_safe_decimal(candidate_leg.get("bid")),
            ask=_safe_decimal(candidate_leg.get("ask")),
            quote_age_ms=_safe_int(candidate_leg.get("age_ms")),
            max_quote_age_ms=max_age_ms,
            transport_connected=True if has_transport_hint else None,
            subscription_healthy=True if has_transport_hint else None,
        )
        return health.feed_state, health.quote_state

    age_ms = _safe_int(fallback_leg.get("age_ms"))
    if age_ms is None or age_ms >= max_age_ms:
        return None, "old"
    return None, "fresh"


def _quote_snapshot_leg_recovers_stale_marker(
    *,
    explicit_leg: Mapping[str, Any],
    feed_state: str | None,
    quote_state: str | None,
) -> bool:
    if not explicit_leg:
        return False
    if quote_state != "fresh":
        return False
    return feed_state in {None, "ok"}


def _is_us_equities_regular_session(now_ms_value: int) -> bool:
    if ZoneInfo is None:
        return True
    local_dt = datetime.fromtimestamp(now_ms_value / 1000, tz=timezone.utc).astimezone(
        ZoneInfo(US_EQUITIES_REGULAR_TZ),
    )
    if local_dt.weekday() >= 5:
        return False
    local_time = local_dt.timetz().replace(tzinfo=None)
    return US_EQUITIES_REGULAR_START <= local_time < US_EQUITIES_REGULAR_END


def _candidate_leg_ids(*, exchange: str, instrument_id: str) -> tuple[str, ...]:
    exchange_text = _optional_text(exchange) or ""
    instrument_text = _optional_text(instrument_id) or ""
    if not exchange_text or not instrument_text:
        return ()
    candidates: list[str] = []
    for candidate in (
        f"{exchange_text.lower()}:{instrument_text}",
        f"{exchange_text.lower()}:{instrument_text.upper()}",
        contract_id_for_leg(
            exchange=exchange_text,
            symbol=instrument_text,
            instrument_id=instrument_text,
        ),
    ):
        if candidate and candidate not in candidates:
            candidates.append(candidate)
    return tuple(candidates)


def _resolve_leg(
    *,
    legs: Mapping[str, Any],
    exchange: str,
    instrument_id: str,
) -> tuple[str | None, Mapping[str, Any]]:
    candidate_leg_ids = _candidate_leg_ids(exchange=exchange, instrument_id=instrument_id)
    for leg_id in candidate_leg_ids:
        if leg_id in legs:
            return leg_id, _mapping(legs.get(leg_id))
    return (candidate_leg_ids[0] if candidate_leg_ids else None), {}


def _resolve_signal_leg(
    *,
    payload: Mapping[str, Any],
    legs: Mapping[str, Any],
    role: str,
    fallback_exchange: str,
    fallback_instrument_id: str | None,
) -> tuple[str | None, Mapping[str, Any]]:
    preferred_leg_id = _optional_text(_signal_role_map(payload).get(role))
    if preferred_leg_id is not None:
        preferred_leg = _mapping(legs.get(preferred_leg_id))
        if preferred_leg:
            return preferred_leg_id, preferred_leg

    fallback_instrument_text = _optional_text(fallback_instrument_id)
    if fallback_instrument_text is not None:
        resolved_leg_id, resolved_leg = _resolve_leg(
            legs=legs,
            exchange=fallback_exchange,
            instrument_id=fallback_instrument_text,
        )
        if resolved_leg:
            return resolved_leg_id, resolved_leg
        if preferred_leg_id is None:
            return resolved_leg_id, resolved_leg

    return preferred_leg_id, _mapping(legs.get(preferred_leg_id))


def _build_projection_health_snapshot(
    *,
    expected_projection_scope_ids: list[str],
    configured_projection_scope_ids: set[str],
    projection_payloads_by_scope_id: Mapping[str, Mapping[str, Any] | None],
    projection_max_age_ms: int,
    now_ms_value: int,
) -> ProjectionHealthSnapshot:
    missing_config_scope_ids = sorted(
        scope_id
        for scope_id in expected_projection_scope_ids
        if scope_id not in configured_projection_scope_ids
    )
    missing_scope_ids: list[str] = []
    empty_scope_ids: list[str] = []
    stale_scope_ids: list[str] = []
    for scope_id in expected_projection_scope_ids:
        payload = projection_payloads_by_scope_id.get(scope_id)
        if not isinstance(payload, Mapping):
            missing_scope_ids.append(scope_id)
            continue
        rows = payload.get("rows")
        if not isinstance(rows, list) or not rows:
            empty_scope_ids.append(scope_id)
        server_ts_ms = _safe_int(payload.get("server_ts_ms"))
        if server_ts_ms is None or (now_ms_value - server_ts_ms > projection_max_age_ms):
            stale_scope_ids.append(scope_id)
    return ProjectionHealthSnapshot(
        check=ReadinessCheck(
            name="profile_account_projections",
            ok=(
                not missing_config_scope_ids
                and not missing_scope_ids
                and not empty_scope_ids
                and not stale_scope_ids
            ),
            summary=(
                f"expected={len(expected_projection_scope_ids)} "
                f"config_missing={len(missing_config_scope_ids)} "
                f"missing={len(missing_scope_ids)} empty={len(empty_scope_ids)} "
                f"stale={len(stale_scope_ids)}"
            ),
            details={
                "expected_scope_ids": expected_projection_scope_ids,
                "missing_config_scope_ids": missing_config_scope_ids,
                "missing_scope_ids": missing_scope_ids,
                "empty_scope_ids": empty_scope_ids,
                "stale_scope_ids": stale_scope_ids,
                "projection_max_age_ms": projection_max_age_ms,
            },
        ),
        missing_config_scope_ids=missing_config_scope_ids,
        missing_scope_ids=missing_scope_ids,
        empty_scope_ids=empty_scope_ids,
        stale_scope_ids=stale_scope_ids,
    )


def _build_signal_health_snapshot(
    *,
    strategy_contracts: tuple[StrategyContractEntry, ...],
    required_strategy_ids: tuple[str, ...],
    signals_payload: Mapping[str, Any] | None,
    max_stale_signal_legs: int,
    max_unhealthy_strategies: int,
    now_ms_value: int,
    ignore_reference_freshness_outside_regular_session: bool,
) -> SignalHealthSnapshot:
    signal_rows = signals_payload.get("strategies") if isinstance(signals_payload, Mapping) else None
    signal_map = {
        strategy_id: row
        for row in signal_rows or []
        if isinstance(row, Mapping)
        and (strategy_id := _optional_text(row.get("id")))
    }
    contracts_by_strategy_id = {
        contract.strategy_id: contract
        for contract in strategy_contracts
    }
    stale_signal_legs: set[str] = set()
    over_age_signal_legs: set[str] = set()
    missing_signal_legs: set[str] = set()
    feed_down_signal_legs: set[str] = set()
    feed_degraded_signal_legs: set[str] = set()
    unhealthy_strategy_ids: list[str] = []
    missing_signal_strategy_ids: list[str] = []
    reference_unhealthy_strategy_ids: list[str] = []
    regular_session_active = _is_us_equities_regular_session(now_ms_value)
    reference_freshness_enforced = (
        not ignore_reference_freshness_outside_regular_session or regular_session_active
    )

    for strategy_id in required_strategy_ids:
        signal_row = signal_map.get(strategy_id)
        if signal_row is None:
            missing_signal_strategy_ids.append(strategy_id)
            unhealthy_strategy_ids.append(strategy_id)
            reference_unhealthy_strategy_ids.append(strategy_id)
            continue

        stale_legs = _signal_stale_legs(signal_row)
        state_name = _signal_state_name(signal_row)
        state_stale = _signal_state_stale(signal_row)
        contract = contracts_by_strategy_id.get(strategy_id)
        max_age_ms = _signal_max_age_ms(signal_row)
        legs = _mapping(signal_row.get("legs"))
        maker_exchange = (
            _optional_text(contract.maker_venue)
            if contract is not None
            else None
        ) or "hyperliquid"
        maker_leg_id, maker_leg = _resolve_signal_leg(
            payload=signal_row,
            legs=legs,
            role="maker_leg",
            fallback_exchange=maker_exchange,
            fallback_instrument_id=contract.maker_instrument_id if contract is not None else None,
        )
        reference_leg_id, reference_leg = _resolve_signal_leg(
            payload=signal_row,
            legs=legs,
            role="ref_leg",
            fallback_exchange="ibkr",
            fallback_instrument_id=contract.reference_instrument_id if contract is not None else None,
        )
        maker_snapshot_leg = _signal_quote_snapshot_leg(signal_row, role="maker")
        reference_snapshot_leg = _signal_quote_snapshot_leg(signal_row, role="reference")
        maker_feed_state, maker_quote_state = _signal_leg_health(
            explicit_leg=maker_snapshot_leg,
            fallback_leg=maker_leg,
            leg_role="maker",
            max_age_ms=max_age_ms,
        )
        reference_feed_state, reference_quote_state = _signal_leg_health(
            explicit_leg=reference_snapshot_leg,
            fallback_leg=reference_leg,
            leg_role="reference",
            max_age_ms=max_age_ms,
        )
        candidate_stale_legs = [
            (maker_leg_id, maker_feed_state, maker_quote_state),
            (
                reference_leg_id if reference_freshness_enforced else None,
                reference_feed_state,
                reference_quote_state,
            ),
        ]
        relevant_stale_legs = sorted(
            leg_id
            for leg_id, feed_state, quote_state in candidate_stale_legs
            if leg_id is not None
            and leg_id in stale_legs
            and not _quote_snapshot_leg_recovers_stale_marker(
                explicit_leg=(
                    maker_snapshot_leg
                    if leg_id == maker_leg_id
                    else reference_snapshot_leg
                ),
                feed_state=feed_state,
                quote_state=quote_state,
            )
        )
        stale_signal_legs.update(relevant_stale_legs)
        old_legs_for_strategy: list[str] = []
        missing_legs_for_strategy: list[str] = []
        feed_down_legs_for_strategy: list[str] = []
        feed_degraded_legs_for_strategy: list[str] = []
        for leg_role, resolved_leg_id, feed_state, quote_state, exchange, instrument_id in (
            (
                "maker",
                maker_leg_id,
                maker_feed_state,
                maker_quote_state,
                maker_exchange,
                contract.maker_instrument_id if contract is not None else None,
            ),
            (
                "reference",
                reference_leg_id,
                reference_feed_state,
                reference_quote_state,
                "ibkr",
                contract.reference_instrument_id if contract is not None else None,
            ),
        ):
            leg_id = resolved_leg_id
            instrument_text = _optional_text(instrument_id)
            if leg_id is None and instrument_text is not None:
                leg_id = contract_id_for_leg(
                    exchange=exchange,
                    symbol=instrument_text,
                    instrument_id=instrument_text,
                )
            if leg_id is None:
                continue
            if feed_state == "down":
                feed_down_legs_for_strategy.append(leg_id)
            elif feed_state in {"degraded", "unknown"}:
                feed_degraded_legs_for_strategy.append(leg_id)
            if leg_role == "reference" and not reference_freshness_enforced:
                continue
            if quote_state == "old":
                old_legs_for_strategy.append(leg_id)
            elif quote_state == "missing":
                missing_legs_for_strategy.append(leg_id)
        over_age_signal_legs.update(old_legs_for_strategy)
        missing_signal_legs.update(missing_legs_for_strategy)
        feed_down_signal_legs.update(feed_down_legs_for_strategy)
        feed_degraded_signal_legs.update(feed_degraded_legs_for_strategy)
        unhealthy_leg_ids = (
            set(relevant_stale_legs)
            | set(old_legs_for_strategy)
            | set(missing_legs_for_strategy)
            | set(feed_down_legs_for_strategy)
            | set(feed_degraded_legs_for_strategy)
        )
        unhealthy = (
            state_stale
            or bool(unhealthy_leg_ids)
            or _signal_state_denotes_feed_unhealthy_block(state_name)
        )
        if unhealthy:
            unhealthy_strategy_ids.append(strategy_id)

        reference_unhealthy = (
            state_name.startswith("blocked_reference")
            or (
                reference_leg_id is not None
                and (
                    reference_leg_id in relevant_stale_legs
                    or reference_leg_id in feed_down_legs_for_strategy
                    or reference_leg_id in feed_degraded_legs_for_strategy
                    or (
                        reference_freshness_enforced
                        and (
                            reference_leg_id in old_legs_for_strategy
                            or reference_leg_id in missing_legs_for_strategy
                        )
                    )
                )
            )
        )
        if reference_leg_id is None and reference_freshness_enforced:
            reference_unhealthy = True
        if reference_unhealthy:
            reference_unhealthy_strategy_ids.append(strategy_id)

    stale_signal_legs_list = sorted(stale_signal_legs)
    over_age_signal_legs_list = sorted(over_age_signal_legs)
    missing_signal_legs_list = sorted(missing_signal_legs)
    feed_down_signal_legs_list = sorted(feed_down_signal_legs)
    feed_degraded_signal_legs_list = sorted(feed_degraded_signal_legs)
    failing_signal_legs = sorted(
        stale_signal_legs
        .union(over_age_signal_legs)
        .union(missing_signal_legs)
        .union(feed_down_signal_legs)
        .union(feed_degraded_signal_legs),
    )
    healthy_strategy_count = max(0, len(required_strategy_ids) - len(unhealthy_strategy_ids))
    return SignalHealthSnapshot(
        check=ReadinessCheck(
            name="signals",
            ok=(
                len(failing_signal_legs) <= max_stale_signal_legs
                and len(unhealthy_strategy_ids) <= max_unhealthy_strategies
                and not missing_signal_strategy_ids
            ),
            summary=(
                f"required={len(required_strategy_ids)} healthy={healthy_strategy_count} "
                f"stale_legs={len(failing_signal_legs)} unhealthy={len(unhealthy_strategy_ids)}"
            ),
            details={
                "required_strategy_ids": list(required_strategy_ids),
                "missing_strategy_ids": missing_signal_strategy_ids,
                "stale_signal_legs": stale_signal_legs_list,
                "over_age_signal_legs": over_age_signal_legs_list,
                "old_signal_legs": over_age_signal_legs_list,
                "missing_signal_legs": missing_signal_legs_list,
                "feed_down_signal_legs": feed_down_signal_legs_list,
                "feed_degraded_signal_legs": feed_degraded_signal_legs_list,
                "stale_signal_leg_count": len(failing_signal_legs),
                "unhealthy_strategy_ids": unhealthy_strategy_ids,
                "healthy_strategy_count": healthy_strategy_count,
                "max_stale_signal_legs": max_stale_signal_legs,
                "max_unhealthy_strategies": max_unhealthy_strategies,
                "regular_session_active": regular_session_active,
                "reference_freshness_enforced": reference_freshness_enforced,
            },
        ),
        healthy_strategy_count=healthy_strategy_count,
        reference_unhealthy_strategy_ids=sorted(set(reference_unhealthy_strategy_ids)),
    )


def _signal_state_denotes_feed_unhealthy_block(state_name: str) -> bool:
    normalized = str(state_name or "").strip().lower()
    if not normalized:
        return False
    return normalized.startswith(("blocked_reference", "blocked_maker")) or normalized in {
        "blocked_stale_quote",
        "blocked_missing_ref_quote",
    }


def evaluate_equities_readiness(
    *,
    profile_id: str,
    portfolio_id: str,
    strategy_contracts: tuple[StrategyContractEntry, ...],
    account_scopes: tuple[AccountScopeConfig, ...],
    controller_scopes: tuple[ControllerScopeConfig, ...] = (),
    required_strategy_ids: tuple[str, ...],
    balances_payload: Mapping[str, Any] | None,
    signals_payload: Mapping[str, Any] | None,
    projection_payloads_by_scope_id: Mapping[str, Mapping[str, Any] | None],
    component_payloads_by_strategy_id: Mapping[str, Any],
    publisher_status_payload: Mapping[str, Any] | None = None,
    now_ms_value: int,
    require_ibkr_reference_publisher: bool = False,
    ibkr_reference_publisher_service_id: str = "ibkr_reference_publisher",
    ibkr_reference_publisher_account_scope_id: str | None = None,
    thresholds: EquitiesReadinessThresholds | None = None,
) -> EquitiesReadinessResult:
    active_thresholds = thresholds or EquitiesReadinessThresholds()
    configured_projection_scope_ids = {
        scope.scope_id
        for scope in account_scopes
        if scope.provider.strip().lower() in PROJECTION_PROVIDERS
    }
    configured_ibkr_projection_scope_ids = {
        scope.scope_id
        for scope in account_scopes
        if scope.provider.strip().lower() == "ibkr"
    }
    expected_projection_scope_ids = _expected_projection_scope_ids(
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        controller_scopes=controller_scopes,
        overrides=active_thresholds.expected_projection_scope_ids,
    )
    expected_ibkr_projection_scope_ids = _expected_projection_scope_ids(
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        controller_scopes=controller_scopes,
        overrides=tuple(
            scope_id
            for scope_id in active_thresholds.expected_projection_scope_ids
            if scope_id in configured_ibkr_projection_scope_ids
        ),
        providers=frozenset({"ibkr"}),
    )
    required_ids = tuple(required_strategy_ids)

    balance_data = dict(balances_payload or {})
    balance_source = _optional_text(balance_data.get("source")) or ""
    balance_missing_required = _sorted_unique_texts(balance_data.get("missing_required"))
    balance_degraded = bool(balance_data.get("degraded", False))
    balance_source_ok = (
        active_thresholds.required_balance_source is None
        or balance_source == active_thresholds.required_balance_source
    )
    balances_check = ReadinessCheck(
        name="balances",
        ok=balance_source_ok and not balance_degraded and not balance_missing_required,
        summary=(
            f"source={balance_source or 'missing'} degraded={balance_degraded} "
            f"missing_required={len(balance_missing_required)}"
        ),
        details={
            "source": balance_source,
            "required_source": active_thresholds.required_balance_source,
            "degraded": balance_degraded,
            "missing_required": balance_missing_required,
        },
    )

    missing_component_strategy_ids = sorted(
        contract.strategy_id
        for contract in strategy_contracts
        if not _component_present(component_payloads_by_strategy_id.get(contract.strategy_id))
    )
    component_check = ReadinessCheck(
        name="component_keys",
        ok=not missing_component_strategy_ids,
        summary=(
            f"expected={len(strategy_contracts)} "
            f"missing={len(missing_component_strategy_ids)}"
        ),
        details={
            "expected_strategy_ids": [contract.strategy_id for contract in strategy_contracts],
            "missing_strategy_ids": missing_component_strategy_ids,
            "portfolio_id": portfolio_id,
        },
    )

    projection_snapshot = _build_projection_health_snapshot(
        expected_projection_scope_ids=expected_projection_scope_ids,
        configured_projection_scope_ids=configured_projection_scope_ids,
        projection_payloads_by_scope_id=projection_payloads_by_scope_id,
        projection_max_age_ms=active_thresholds.projection_max_age_ms,
        now_ms_value=now_ms_value,
    )
    projection_check = projection_snapshot.check
    ibkr_projection_snapshot = _build_projection_health_snapshot(
        expected_projection_scope_ids=expected_ibkr_projection_scope_ids,
        configured_projection_scope_ids=configured_ibkr_projection_scope_ids,
        projection_payloads_by_scope_id=projection_payloads_by_scope_id,
        projection_max_age_ms=active_thresholds.projection_max_age_ms,
        now_ms_value=now_ms_value,
    )

    signal_snapshot = _build_signal_health_snapshot(
        strategy_contracts=strategy_contracts,
        required_strategy_ids=required_ids,
        signals_payload=signals_payload,
        max_stale_signal_legs=active_thresholds.max_stale_signal_legs,
        max_unhealthy_strategies=active_thresholds.max_unhealthy_strategies,
        now_ms_value=now_ms_value,
        ignore_reference_freshness_outside_regular_session=(
            active_thresholds.ignore_reference_freshness_outside_regular_session
        ),
    )
    signals_check = signal_snapshot.check

    publisher_checks: dict[str, ReadinessCheck] = {}
    if require_ibkr_reference_publisher:
        expected_publisher_scope_id = (
            _optional_text(ibkr_reference_publisher_account_scope_id)
            or _expected_reference_account_scope_id(strategy_contracts)
        )
        publisher_data = _mapping(publisher_status_payload)
        observed_service_id = _optional_text(publisher_data.get("service_id"))
        observed_account_scope_id = _optional_text(publisher_data.get("account_scope_id"))
        state = (_optional_text(publisher_data.get("state")) or "").lower()
        connected = bool(publisher_data.get("connected"))
        instrument_status = _mapping(publisher_data.get("instrument_status"))
        unhealthy_instrument_ids = sorted(
            instrument_id
            for instrument_id, status_payload in instrument_status.items()
            if (_optional_text(_mapping(status_payload).get("state")) or "unknown").lower()
            != "healthy"
        )
        stale_after_ms = _safe_int(publisher_data.get("stale_after_ms"))
        status_ts_ms = _safe_int(publisher_data.get("ts_ms")) or _safe_int(
            publisher_data.get("last_success_ts_ms"),
        )
        stale = (
            status_ts_ms is None
            or stale_after_ms is None
            or stale_after_ms <= 0
            or (now_ms_value - status_ts_ms) > stale_after_ms
        )
        scope_matches = (
            expected_publisher_scope_id is None
            or observed_account_scope_id == expected_publisher_scope_id
        )
        service_matches = (
            observed_service_id == ibkr_reference_publisher_service_id
            if observed_service_id is not None
            else False
        )
        state_ok = state == "publishing"
        publisher_check = ReadinessCheck(
            name="ibkr_reference_publisher",
            ok=(
                bool(publisher_data)
                and connected
                and state_ok
                and service_matches
                and scope_matches
                and not stale
                and not unhealthy_instrument_ids
            ),
            summary=(
                f"service={ibkr_reference_publisher_service_id} "
                f"state={state or 'missing'} connected={connected} "
                f"stale={stale} unhealthy={len(unhealthy_instrument_ids)}"
            ),
            details={
                "missing": not bool(publisher_data),
                "service_id": ibkr_reference_publisher_service_id,
                "account_scope_id": expected_publisher_scope_id,
                "observed_service_id": observed_service_id,
                "observed_account_scope_id": observed_account_scope_id,
                "state": state or None,
                "connected": connected,
                "stale": stale,
                "stale_after_ms": stale_after_ms,
                "status_ts_ms": status_ts_ms,
                "unhealthy_instrument_ids": unhealthy_instrument_ids,
            },
        )
        publisher_checks[publisher_check.name] = publisher_check

    ibkr_auth_check = ReadinessCheck(
        name="ibkr_auth",
        ok=(
            ibkr_projection_snapshot.check.ok
            and not signal_snapshot.reference_unhealthy_strategy_ids
        ),
        summary=(
            f"projection_scopes={len(expected_ibkr_projection_scope_ids)} "
            f"reference_unhealthy={len(signal_snapshot.reference_unhealthy_strategy_ids)}"
        ),
        details={
            "expected_scope_ids": expected_ibkr_projection_scope_ids,
            "missing_config_scope_ids": ibkr_projection_snapshot.missing_config_scope_ids,
            "missing_scope_ids": ibkr_projection_snapshot.missing_scope_ids,
            "empty_scope_ids": ibkr_projection_snapshot.empty_scope_ids,
            "stale_scope_ids": ibkr_projection_snapshot.stale_scope_ids,
            "unhealthy_strategy_ids": signal_snapshot.reference_unhealthy_strategy_ids,
            "regular_session_active": signals_check.details["regular_session_active"],
            "reference_freshness_enforced": signals_check.details["reference_freshness_enforced"],
        },
    )

    checks = {
        balances_check.name: balances_check,
        projection_check.name: projection_check,
        component_check.name: component_check,
        signals_check.name: signals_check,
        **publisher_checks,
        ibkr_auth_check.name: ibkr_auth_check,
    }
    overall_ok = all(check.ok for check in checks.values())
    return EquitiesReadinessResult(
        ok=overall_ok,
        checks=checks,
        summary={
            "profile_id": profile_id,
            "portfolio_id": portfolio_id,
            "required_strategy_ids": list(required_ids),
            "expected_projection_scope_ids": expected_projection_scope_ids,
            "healthy_strategy_count": signal_snapshot.healthy_strategy_count,
            "stale_signal_leg_count": signals_check.details["stale_signal_leg_count"],
        },
    )


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=EQUITIES_DESCRIPTOR.env_prefix)


def _fetch_api_payload(
    *,
    base_url: str,
    path: str,
    query: Mapping[str, str],
    timeout_secs: float = DEFAULT_REQUEST_TIMEOUT_SECS,
) -> dict[str, Any]:
    url = f"{base_url.rstrip('/')}{path}?{urlencode(query)}"
    with urlopen(url, timeout=timeout_secs) as response:  # noqa: S310 - host-local operator check
        payload = json.loads(response.read().decode("utf-8"))
    if not isinstance(payload, Mapping):
        raise ValueError(f"Expected JSON object from {url}")
    data = payload.get("data")
    if not isinstance(data, Mapping):
        raise ValueError(f"Expected `data` object from {url}")
    return dict(data)


def _collect_projection_payloads(
    *,
    redis_client: Any,
    profile_id: str,
    scope_ids: list[str],
    namespace: str,
    schema_version: str,
) -> dict[str, dict[str, Any] | None]:
    if not scope_ids:
        return {}
    pipeline = redis_client.pipeline(transaction=False)
    for scope_id in scope_ids:
        pipeline.get(
            FluxRedisKeys.profile_account_projection(
                profile_id=profile_id,
                account_scope_id=scope_id,
                namespace=namespace,
                schema_version=schema_version,
            ),
        )
    raw_payloads = pipeline.execute()
    return {
        scope_id: decode_profile_account_snapshot(raw)
        for scope_id, raw in zip(scope_ids, raw_payloads, strict=True)
    }


def _collect_component_payloads(
    *,
    redis_client: Any,
    strategy_contracts: tuple[StrategyContractEntry, ...],
    portfolio_id: str,
    namespace: str,
    schema_version: str,
) -> dict[str, StrategyInventoryComponent | None]:
    if not strategy_contracts:
        return {}
    pipeline = redis_client.pipeline(transaction=False)
    for contract in strategy_contracts:
        pipeline.get(
            FluxRedisKeys.portfolio_inventory_component(
                strategy_id=contract.strategy_id,
                portfolio_id=portfolio_id,
                base_currency=contract.portfolio_asset_id,
                namespace=namespace,
                schema_version=schema_version,
            ),
        )
    raw_components = pipeline.execute()
    return {
        contract.strategy_id: decode_component(raw)
        for contract, raw in zip(strategy_contracts, raw_components, strict=True)
    }


def _collect_publisher_status_payload(
    *,
    redis_client: Any,
    profile_id: str,
    account_scope_id: str | None,
    service_id: str,
    namespace: str,
    schema_version: str,
) -> dict[str, Any] | None:
    if account_scope_id is None:
        return None
    raw_payload = redis_client.get(
        FluxRedisKeys.profile_market_data_status(
            profile_id=profile_id,
            account_scope_id=account_scope_id,
            service_id=service_id,
            namespace=namespace,
            schema_version=schema_version,
        ),
    )
    return _decode_json_mapping(raw_payload)


def resolve_api_base_url(config: Mapping[str, Any], *, explicit_base_url: str | None = None) -> str:
    explicit = _optional_text(explicit_base_url)
    if explicit is not None:
        return explicit.rstrip("/")
    env_base_url = _optional_text(os.getenv("EQUITIES_API_BACKEND_URL"))
    if env_base_url is not None:
        return env_base_url.rstrip("/")
    api_cfg = shared_table(dict(config), "api")
    host = _optional_text(api_cfg.get("host")) or "127.0.0.1"
    if host in UNSPECIFIED_BIND_HOSTS:
        host = "127.0.0.1"
    port = int(api_cfg.get("port", 5022))
    return f"http://{host}:{port}"


def format_readiness_result(result: EquitiesReadinessResult) -> str:
    lines = [
        f"[equities-readiness] {'OK' if result.ok else 'FAIL'} profile={result.summary['profile_id']}",
    ]
    for check in result.checks.values():
        lines.append(
            f"[equities-readiness] {'OK' if check.ok else 'FAIL'} "
            f"{check.name}: {check.summary}",
        )
    return "\n".join(lines)


def _parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Read-only live readiness gate for the equities stack.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--api-base-url", default=None)
    parser.add_argument("--max-stale-signal-legs", type=int, default=0)
    parser.add_argument("--max-unhealthy-strategies", type=int, default=0)
    parser.add_argument("--projection-max-age-ms", type=int, default=DEFAULT_PROJECTION_MAX_AGE_MS)
    parser.add_argument("--required-balance-source", default=DEFAULT_REQUIRED_BALANCE_SOURCE)
    parser.add_argument(
        "--ignore-reference-freshness-outside-regular-session",
        action="store_true",
        help=(
            "Ignore IBKR reference-leg freshness outside the regular US equities session "
            "(09:30-16:00 America/New_York)."
        ),
    )
    parser.add_argument(
        "--expected-projection-scope-id",
        action="append",
        dest="expected_projection_scope_ids",
        default=None,
        help="Override the set of shared projection scopes that must be present.",
    )
    parser.add_argument("--json", action="store_true")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = _parse_args(argv)
    try:
        config = _load_config(args.config)
        flux_cfg = shared_table(config, "flux")
        api_cfg = shared_table(config, "api")
        portfolio_cfg = shared_table(config, "portfolio")
        redis_cfg = shared_table(config, "redis")
        publisher_cfg = shared_table(config, "ibkr_reference_publisher")

        strategy_ids = parse_strategy_ids(api_cfg, descriptor=EQUITIES_DESCRIPTOR)
        required_strategy_ids = tuple(
            parse_required_strategy_ids(
                api_cfg,
                descriptor=EQUITIES_DESCRIPTOR,
                fallback=strategy_ids,
            ),
        )
        strategy_id_set = set(strategy_ids)
        strategy_contracts = tuple(
            contract
            for contract in decode_strategy_contracts(config.get("strategy_contracts") or [])
            if contract.strategy_id in strategy_id_set
        )
        account_scopes = decode_account_scopes(config.get("account_scopes") or [])
        controller_scopes = decode_controller_scopes(config.get("controller_scopes") or [])
        thresholds = EquitiesReadinessThresholds(
            max_stale_signal_legs=args.max_stale_signal_legs,
            max_unhealthy_strategies=args.max_unhealthy_strategies,
            projection_max_age_ms=args.projection_max_age_ms,
            required_balance_source=_optional_text(args.required_balance_source),
            ignore_reference_freshness_outside_regular_session=(
                args.ignore_reference_freshness_outside_regular_session
            ),
            expected_projection_scope_ids=tuple(
                _optional_text(scope_id) or ""
                for scope_id in (args.expected_projection_scope_ids or [])
                if _optional_text(scope_id) is not None
            ),
        )

        api_base_url = resolve_api_base_url(config, explicit_base_url=args.api_base_url)
        namespace = _optional_text(flux_cfg.get("namespace")) or "flux"
        schema_version = _optional_text(flux_cfg.get("schema_version")) or "v1"
        portfolio_id = (
            _optional_text(portfolio_cfg.get("portfolio_id"))
            or EQUITIES_DESCRIPTOR.default_portfolio_id
        )
        publisher_service_id = (
            _optional_text(publisher_cfg.get("service_id"))
            or "ibkr_reference_publisher"
        )
        publisher_account_scope_id = (
            _optional_text(publisher_cfg.get("account_scope_id"))
            or _expected_reference_account_scope_id(strategy_contracts)
        )

        redis_client = build_redis_client(redis_cfg)
        expected_scope_ids = _expected_projection_scope_ids(
            strategy_contracts=strategy_contracts,
            account_scopes=account_scopes,
            controller_scopes=controller_scopes,
            overrides=thresholds.expected_projection_scope_ids,
        )
        projection_payloads = _collect_projection_payloads(
            redis_client=redis_client,
            profile_id=EQUITIES_DESCRIPTOR.profile,
            scope_ids=expected_scope_ids,
            namespace=namespace,
            schema_version=schema_version,
        )
        component_payloads = _collect_component_payloads(
            redis_client=redis_client,
            strategy_contracts=strategy_contracts,
            portfolio_id=portfolio_id,
            namespace=namespace,
            schema_version=schema_version,
        )
        publisher_status_payload = _collect_publisher_status_payload(
            redis_client=redis_client,
            profile_id=EQUITIES_DESCRIPTOR.profile,
            account_scope_id=publisher_account_scope_id,
            service_id=publisher_service_id,
            namespace=namespace,
            schema_version=schema_version,
        )
        balances_payload = _fetch_api_payload(
            base_url=api_base_url,
            path="/api/v1/balances",
            query={"profile": EQUITIES_DESCRIPTOR.profile},
        )
        signals_payload = _fetch_api_payload(
            base_url=api_base_url,
            path="/api/v1/signals",
            query={"profile": EQUITIES_DESCRIPTOR.profile},
        )
        result = evaluate_equities_readiness(
            profile_id=EQUITIES_DESCRIPTOR.profile,
            portfolio_id=portfolio_id,
            strategy_contracts=strategy_contracts,
            account_scopes=account_scopes,
            controller_scopes=controller_scopes,
            required_strategy_ids=required_strategy_ids,
            balances_payload=balances_payload,
            signals_payload=signals_payload,
            projection_payloads_by_scope_id=projection_payloads,
            component_payloads_by_strategy_id=component_payloads,
            publisher_status_payload=publisher_status_payload,
            now_ms_value=int(time.time() * 1000),
            require_ibkr_reference_publisher=True,
            ibkr_reference_publisher_service_id=publisher_service_id,
            ibkr_reference_publisher_account_scope_id=publisher_account_scope_id,
            thresholds=thresholds,
        )
    except Exception as exc:
        print(f"[equities-readiness] FAIL {type(exc).__name__}: {exc}", file=sys.stderr)
        return 1

    if args.json:
        print(json.dumps(result.as_dict(), indent=2, sort_keys=True))
    else:
        print(format_readiness_result(result))
    return 0 if result.ok else 1


__all__ = (
    "EquitiesReadinessResult",
    "EquitiesReadinessThresholds",
    "ReadinessCheck",
    "evaluate_equities_readiness",
    "format_readiness_result",
    "main",
    "resolve_api_base_url",
)


if __name__ == "__main__":
    raise SystemExit(main())
