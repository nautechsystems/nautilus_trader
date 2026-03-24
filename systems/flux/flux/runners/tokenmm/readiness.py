from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from flux.api.payloads import decode_text
from flux.api.payloads import safe_int
from flux.common.keys import FluxRedisKeys
from flux.strategies.makerv3.constants import TOPIC_STATE


DEFAULT_STATE_STREAM_MAX_AGE_MS = 30_000
BLOCKED_RECONCILIATION = "blocked_reconciliation"


@dataclass(frozen=True, slots=True)
class TokenMMReadinessThresholds:
    state_stream_max_age_ms: int = DEFAULT_STATE_STREAM_MAX_AGE_MS

    def __post_init__(self) -> None:
        if self.state_stream_max_age_ms < 0:
            raise ValueError("`state_stream_max_age_ms` must be >= 0")


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
class TokenMMReadinessResult:
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


def _mapping(value: Any) -> Mapping[str, Any]:
    return value if isinstance(value, Mapping) else {}


def _sorted_texts(values: list[str]) -> list[str]:
    return sorted({value for value in values if value})


def _signal_rows_by_strategy_id(signals_payload: Mapping[str, Any]) -> dict[str, Mapping[str, Any]]:
    strategies = signals_payload.get("strategies")
    if not isinstance(strategies, list):
        return {}
    result: dict[str, Mapping[str, Any]] = {}
    for item in strategies:
        if not isinstance(item, Mapping):
            continue
        strategy_id = decode_text(item.get("id")).strip()
        if strategy_id:
            result[strategy_id] = item
    return result


def _signal_md_health(payload: Mapping[str, Any]) -> Mapping[str, Any]:
    debug = _mapping(payload.get("debug"))
    return _mapping(debug.get("md_health"))


def _signal_state_stale(payload: Mapping[str, Any]) -> bool:
    return bool(_signal_md_health(payload).get("state_stale"))


def _signal_state_age_ms(payload: Mapping[str, Any]) -> int | None:
    return safe_int(_signal_md_health(payload).get("signal_state_age_ms"))


def _signal_state_name(payload: Mapping[str, Any]) -> str:
    state = _mapping(payload.get("state"))
    return decode_text(state.get("state")).strip().lower()


def _state_stream_ts_ms(entry_id: str | None) -> int | None:
    text = (entry_id or "").strip()
    if not text:
        return None
    raw_ts_ms, _, _ = text.partition("-")
    return safe_int(raw_ts_ms)


def load_state_streams_by_strategy_id(
    *,
    redis_client: Any,
    strategy_ids: tuple[str, ...],
    namespace: str,
    schema_version: str,
    environment: str,
    now_ms_value: int,
) -> dict[str, dict[str, Any]]:
    state_streams: dict[str, dict[str, Any]] = {}
    for strategy_id in strategy_ids:
        stream_key = FluxRedisKeys(
            strategy_id=strategy_id,
            namespace=namespace,
            schema_version=schema_version,
        ).inbound_stream(environment, TOPIC_STATE)
        rows = redis_client.xrevrange(stream_key, count=1)
        entry_id = None
        if rows:
            raw_entry = rows[0]
            if isinstance(raw_entry, tuple) and raw_entry:
                entry_id = decode_text(raw_entry[0]).strip()
        ts_ms = _state_stream_ts_ms(entry_id)
        state_streams[strategy_id] = {
            "key": stream_key,
            "entry_id": entry_id,
            "ts_ms": ts_ms,
            "age_ms": max(0, now_ms_value - ts_ms) if ts_ms is not None else None,
            "present": ts_ms is not None,
        }
    return state_streams


def evaluate_tokenmm_readiness(
    *,
    required_strategy_ids: tuple[str, ...],
    signals_payload: Mapping[str, Any],
    state_streams_by_strategy_id: Mapping[str, Mapping[str, Any]],
    now_ms_value: int,
    thresholds: TokenMMReadinessThresholds = TokenMMReadinessThresholds(),
) -> TokenMMReadinessResult:
    signal_rows = _signal_rows_by_strategy_id(signals_payload)
    missing_signal_strategy_ids: list[str] = []
    stale_signal_strategy_ids: list[str] = []
    blocked_reconciliation_strategy_ids: list[str] = []
    missing_state_stream_strategy_ids: list[str] = []
    stale_state_stream_strategy_ids: list[str] = []
    signal_state_age_ms_by_strategy_id: dict[str, int] = {}
    state_stream_age_ms_by_strategy_id: dict[str, int] = {}

    for strategy_id in required_strategy_ids:
        signal_row = signal_rows.get(strategy_id)
        if signal_row is None:
            missing_signal_strategy_ids.append(strategy_id)
        else:
            signal_state_age_ms = _signal_state_age_ms(signal_row)
            if signal_state_age_ms is not None:
                signal_state_age_ms_by_strategy_id[strategy_id] = signal_state_age_ms
            if _signal_state_stale(signal_row):
                stale_signal_strategy_ids.append(strategy_id)
            if _signal_state_name(signal_row) == BLOCKED_RECONCILIATION:
                blocked_reconciliation_strategy_ids.append(strategy_id)

        state_stream = _mapping(state_streams_by_strategy_id.get(strategy_id))
        state_stream_age_ms = safe_int(state_stream.get("age_ms"))
        if state_stream_age_ms is not None:
            state_stream_age_ms_by_strategy_id[strategy_id] = state_stream_age_ms
        if not bool(state_stream.get("present")):
            missing_state_stream_strategy_ids.append(strategy_id)
            continue
        if state_stream_age_ms is None or state_stream_age_ms >= thresholds.state_stream_max_age_ms:
            stale_state_stream_strategy_ids.append(strategy_id)

    missing_signal_strategy_ids = _sorted_texts(missing_signal_strategy_ids)
    stale_signal_strategy_ids = _sorted_texts(stale_signal_strategy_ids)
    blocked_reconciliation_strategy_ids = _sorted_texts(blocked_reconciliation_strategy_ids)
    missing_state_stream_strategy_ids = _sorted_texts(missing_state_stream_strategy_ids)
    stale_state_stream_strategy_ids = _sorted_texts(stale_state_stream_strategy_ids)

    signals_ok = (
        not missing_signal_strategy_ids
        and not stale_signal_strategy_ids
        and not blocked_reconciliation_strategy_ids
    )
    state_streams_ok = (
        not missing_state_stream_strategy_ids and not stale_state_stream_strategy_ids
    )
    signal_failures = _sorted_texts(
        missing_signal_strategy_ids
        + stale_signal_strategy_ids
        + blocked_reconciliation_strategy_ids,
    )
    state_stream_failures = _sorted_texts(
        missing_state_stream_strategy_ids + stale_state_stream_strategy_ids,
    )
    unready_strategy_ids = _sorted_texts(signal_failures + state_stream_failures)

    checks = {
        "signals": ReadinessCheck(
            name="signals",
            ok=signals_ok,
            summary=(
                "All required TokenMM strategies expose fresh signals."
                if signals_ok
                else f"{len(signal_failures)} strategy signals are missing, stale, or blocked."
            ),
            details={
                "now_ms": now_ms_value,
                "required_strategy_ids": list(required_strategy_ids),
                "missing_signal_strategy_ids": missing_signal_strategy_ids,
                "stale_signal_strategy_ids": stale_signal_strategy_ids,
                "blocked_reconciliation_strategy_ids": blocked_reconciliation_strategy_ids,
                "signal_state_age_ms_by_strategy_id": signal_state_age_ms_by_strategy_id,
            },
        ),
        "state_stream_freshness": ReadinessCheck(
            name="state_stream_freshness",
            ok=state_streams_ok,
            summary=(
                "All required TokenMM state streams are fresh."
                if state_streams_ok
                else f"{len(state_stream_failures)} strategy state streams are missing or stale."
            ),
            details={
                "now_ms": now_ms_value,
                "required_strategy_ids": list(required_strategy_ids),
                "missing_state_stream_strategy_ids": missing_state_stream_strategy_ids,
                "stale_state_stream_strategy_ids": stale_state_stream_strategy_ids,
                "state_stream_age_ms_by_strategy_id": state_stream_age_ms_by_strategy_id,
                "state_stream_max_age_ms": thresholds.state_stream_max_age_ms,
            },
        ),
    }
    failed_checks = [
        name
        for name, check in checks.items()
        if not check.ok
    ]
    ready_strategy_count = max(0, len(required_strategy_ids) - len(unready_strategy_ids))

    return TokenMMReadinessResult(
        ok=not failed_checks,
        checks=checks,
        summary={
            "now_ms": now_ms_value,
            "required_strategy_ids": list(required_strategy_ids),
            "required_strategy_count": len(required_strategy_ids),
            "ready_strategy_count": ready_strategy_count,
            "missing_signal_strategy_ids": missing_signal_strategy_ids,
            "stale_signal_strategy_ids": stale_signal_strategy_ids,
            "blocked_reconciliation_strategy_ids": blocked_reconciliation_strategy_ids,
            "missing_state_stream_strategy_ids": missing_state_stream_strategy_ids,
            "stale_state_stream_strategy_ids": stale_state_stream_strategy_ids,
            "failed_checks": failed_checks,
            "state_stream_max_age_ms": thresholds.state_stream_max_age_ms,
        },
    )
