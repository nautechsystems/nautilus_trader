from __future__ import annotations

import inspect
import json
import logging
import sys
from collections.abc import Callable
from collections.abc import Mapping
from collections.abc import Sequence
from copy import deepcopy
from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from threading import Event
from threading import RLock
from threading import Thread
from time import monotonic
from typing import Any
from typing import Protocol
from typing import cast
from uuid import uuid4

from flask import Flask
from flask import request
from flask_socketio import SocketIO
from flask_socketio import join_room
from flask_socketio import leave_room

from flux.api._payloads_balances import build_balance_risk_groups
from flux.api.payloads import coerce_ts_ms
from flux.api.payloads import decode_text
from flux.api.payloads import filter_balance_rows_for_contract_scope
from flux.api.payloads import merge_portfolio_balances_rows
from flux.api.payloads import now_ms
from flux.api.payloads import safe_int
from flux.runners.shared.strategy_set import normalize_profile as normalize_strategy_set_profile
from flux.runners.shared.strategy_set import supported_profile_ids as supported_strategy_set_profiles

if __name__ == "flux.api.socketio":
    sys.modules.setdefault("nautilus_trader.flux.api.socketio", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.api.socketio":
    sys.modules.setdefault("flux.api.socketio", sys.modules[__name__])


SOCKETIO_DEFAULT_PATH = "/socket.io"
SOCKETIO_DEFAULT_POLL_INTERVAL_S = 0.75
SOCKETIO_TRADE_POLL_LIMIT = 200
SOCKETIO_TRADE_SCAN_LIMIT = 2_000
SOCKETIO_ALERTS_PREVIEW_LIMIT = 25
SOCKETIO_FAILURE_BACKOFF_CAP_S = 30.0
SOCKETIO_FAILURE_STREAK_CAP = 6
SOCKETIO_TOKENMM_BALANCES_STALE_AFTER_MS = 30_000
REALTIME_STANDARD_CONTRACT_VERSION = 2
REALTIME_STANDARD_EVENT = "realtime_event"
REALTIME_SUPPORTED_SURFACES = ("signal", "trades", "alerts", "balances")
REALTIME_STANDARD_SNAPSHOT_REVISION = 1
REALTIME_HEARTBEAT_JITTER_TOLERANCE_MS = 250
REALTIME_MISSED_HEARTBEATS_BEFORE_STALE = 2

_LOG = logging.getLogger(__name__)


class FluxSocketStoreProtocol(Protocol):
    def load_signals_payload(
        self,
        strategy_id: str,
        metadata: Any,
        *,
        running: bool | None = None,
    ) -> dict[str, Any]: ...

    def load_running_states(self, strategy_ids: Sequence[str]) -> dict[str, bool | None]: ...

    def load_trades_rows(
        self,
        strategy_id: str,
        *,
        limit: int,
        since_ms: int | None,
        since_seq: int | None = None,
        scan_limit: int | None = None,
        base_first_qty: bool = False,
    ) -> list[dict[str, Any]]: ...

    def load_alerts_rows(self, strategy_id: str, *, limit: int) -> list[dict[str, Any]]: ...

    def alerts_stream_len(self, strategy_id: str) -> int | None: ...

    def load_market_rows_for_strategies(
        self,
        strategy_ids: Sequence[str],
    ) -> dict[str, dict[str, Any]]: ...

    def load_balances_rows(self, strategy_id: str) -> list[dict[str, Any]]: ...

    def load_balances_rows_with_presence(self, strategy_id: str) -> tuple[list[dict[str, Any]], bool]: ...

    def load_portfolio_snapshot(self, portfolio_id: str) -> dict[str, Any] | None: ...


def normalize_profile(profile: Any) -> str:
    """
    Normalize inbound profile IDs for room and payload scoping.
    """
    return normalize_strategy_set_profile(decode_text(profile))


def supported_profile_ids() -> tuple[str, ...]:
    return supported_strategy_set_profiles()


def profile_room(profile: Any) -> str:
    """
    Return the room model for the normalized profile.
    """
    return f"profile:{normalize_profile(profile)}"


def normalize_surface(surface: Any) -> str:
    return decode_text(surface).strip().lower()


def _dedupe_strategy_ids(strategy_ids: Sequence[str]) -> tuple[str, ...]:
    out: list[str] = []
    seen: set[str] = set()
    for strategy_id in strategy_ids:
        text = decode_text(strategy_id).strip()
        if not text or text in seen:
            continue
        seen.add(text)
        out.append(text)
    return tuple(out)


def build_standard_surface_query_key(
    *,
    surface: str,
    profile: str,
    strategy_ids: Sequence[str],
) -> str:
    joined_ids = ",".join(_dedupe_strategy_ids(strategy_ids)) or "-"
    return f"{normalize_surface(surface)}|profile={normalize_profile(profile) or '-'}|strategy_ids={joined_ids}"


def build_standard_stream_id(
    *,
    surface: str,
    profile: str,
    strategy_ids: Sequence[str],
) -> str:
    joined_ids = ",".join(_dedupe_strategy_ids(strategy_ids)) or "-"
    return f"{normalize_surface(surface)}:{normalize_profile(profile) or '-'}:{joined_ids}"


def build_standard_capabilities(
    *,
    surface: str,
    poll_interval_s: float,
) -> dict[str, Any]:
    heartbeat_interval_ms = max(250, int(float(poll_interval_s) * 1000.0))
    stale_after_ms = heartbeat_interval_ms * REALTIME_MISSED_HEARTBEATS_BEFORE_STALE
    _ = normalize_surface(surface)
    return {
        "recovery_mode": "invalidate_only",
        "replay_supported": False,
        "replay_retention_sec": 0,
        "liveness_policy": f"heartbeat_or_data_within_{stale_after_ms}ms",
        "heartbeat_interval_ms": heartbeat_interval_ms,
        "heartbeat_jitter_tolerance_ms": REALTIME_HEARTBEAT_JITTER_TOLERANCE_MS,
        "missed_heartbeats_before_stale": REALTIME_MISSED_HEARTBEATS_BEFORE_STALE,
        "transport_mode": "polling_only",
    }


def build_standard_snapshot_metadata(
    *,
    surface: str,
    profile: str,
    strategy_ids: Sequence[str],
    last_seq: int,
    poll_interval_s: float,
) -> dict[str, Any]:
    normalized_surface = normalize_surface(surface)
    normalized_profile = normalize_profile(profile)
    return {
        "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
        "surface": normalized_surface,
        "profile": normalized_profile,
        "surface_query_key": build_standard_surface_query_key(
            surface=normalized_surface,
            profile=normalized_profile,
            strategy_ids=strategy_ids,
        ),
        "stream_id": build_standard_stream_id(
            surface=normalized_surface,
            profile=normalized_profile,
            strategy_ids=strategy_ids,
        ),
        "snapshot_revision": REALTIME_STANDARD_SNAPSHOT_REVISION,
        "last_seq": max(0, int(last_seq)),
        "capabilities": build_standard_capabilities(
            surface=normalized_surface,
            poll_interval_s=poll_interval_s,
        ),
    }


def default_realtime_rollout() -> dict[str, Any]:
    return {
        "supported_contract_versions": {REALTIME_STANDARD_CONTRACT_VERSION},
        "hard_kill_switch": False,
        "surface_enabled": {
            surface: True for surface in REALTIME_SUPPORTED_SURFACES
        },
        "surface_canary_profiles": {
            surface: None for surface in REALTIME_SUPPORTED_SURFACES
        },
    }


def _normalize_rollout_versions(raw_value: Any) -> set[int]:
    if isinstance(raw_value, (set, frozenset)):
        values = list(raw_value)
    elif isinstance(raw_value, Sequence) and not isinstance(raw_value, str | bytes):
        values = list(raw_value)
    elif raw_value is None:
        values = [REALTIME_STANDARD_CONTRACT_VERSION]
    else:
        values = [raw_value]
    versions: set[int] = set()
    for value in values:
        parsed = safe_int(value)
        if parsed is not None:
            versions.add(int(parsed))
    return versions or {REALTIME_STANDARD_CONTRACT_VERSION}


def _rollout_surface_enabled(rollout: Mapping[str, Any], surface: str) -> bool:
    surface_enabled = rollout.get("surface_enabled")
    if isinstance(surface_enabled, Mapping):
        value = surface_enabled.get(normalize_surface(surface))
        if isinstance(value, bool):
            return value
    return True


def _rollout_canary_profiles(
    rollout: Mapping[str, Any],
    surface: str,
) -> set[str] | None:
    surface_canary = rollout.get("surface_canary_profiles")
    if not isinstance(surface_canary, Mapping):
        return None
    raw_value = surface_canary.get(normalize_surface(surface))
    if raw_value is None:
        return None
    if isinstance(raw_value, str | bytes):
        values = [raw_value]
    elif isinstance(raw_value, (set, frozenset)):
        values = list(raw_value)
    elif isinstance(raw_value, Sequence):
        values = list(raw_value)
    else:
        return set()
    profiles = {normalize_profile(value) for value in values if normalize_profile(value)}
    return profiles


def standard_subscribe_rejection_reason(
    *,
    contract_version: int,
    surface: str,
    profile: str,
    rollout: Mapping[str, Any],
) -> str | None:
    normalized_surface = normalize_surface(surface)
    if safe_int(rollout.get("hard_kill_switch")) == 1:
        return "backend_kill_switch"
    if contract_version not in _normalize_rollout_versions(rollout.get("supported_contract_versions")):
        return "unsupported_contract_version"
    if normalized_surface not in REALTIME_SUPPORTED_SURFACES:
        return "unsupported_surface"
    if not _rollout_surface_enabled(rollout, normalized_surface):
        return "capability_unavailable"
    canary_profiles = _rollout_canary_profiles(rollout, normalized_surface)
    if canary_profiles is not None and normalize_profile(profile) not in canary_profiles:
        return "canary_denied"
    return None


def standard_active_withdrawal_reason(
    *,
    surface: str,
    profile: str,
    rollout: Mapping[str, Any],
) -> str | None:
    normalized_surface = normalize_surface(surface)
    if safe_int(rollout.get("hard_kill_switch")) == 1:
        return "backend_kill_switch"
    if not _rollout_surface_enabled(rollout, normalized_surface):
        return "capability_withdrawn"
    canary_profiles = _rollout_canary_profiles(rollout, normalized_surface)
    if canary_profiles is not None and normalize_profile(profile) not in canary_profiles:
        return "capability_withdrawn"
    return None


def _copy_mapping(value: Mapping[str, Any]) -> dict[str, Any]:
    return {str(key): deepcopy(item) for key, item in value.items()}


def _normalize_legs(value: Any) -> dict[str, Any]:
    if not isinstance(value, Mapping):
        return {}
    return {decode_text(key): deepcopy(item) for key, item in value.items()}


def _normalize_trade_row(row: Mapping[str, Any], *, row_id: str, version: int) -> dict[str, Any]:
    out = _copy_mapping(cast(Mapping[str, Any], row))
    out["row_id"] = row_id
    out["version"] = version
    return out


def _metadata_is_tokenmm(metadata: Any) -> bool:
    groups = decode_text(getattr(metadata, "strategy_groups", "")).strip().lower()
    if not groups:
        return False
    return "tokenmm" in {part.strip() for part in groups.split(",") if part.strip()}


def build_stable_signal_view(signal_payload: Mapping[str, Any]) -> dict[str, Any]:
    """
    Remove volatile signal fields so steady-state polls do not emit noisy deltas.
    """
    stable = _copy_mapping(signal_payload)

    stable_legs = _normalize_legs(stable.get("legs"))
    for contract_id, leg_row in list(stable_legs.items()):
        if not isinstance(leg_row, Mapping):
            continue
        normalized_leg = _copy_mapping(cast(Mapping[str, Any], leg_row))
        normalized_leg.pop("age_ms", None)
        stable_legs[contract_id] = normalized_leg
    if "legs" in stable:
        stable["legs"] = stable_legs

    debug_payload = stable.get("debug")
    if isinstance(debug_payload, Mapping):
        normalized_debug = _copy_mapping(cast(Mapping[str, Any], debug_payload))
        md_health = normalized_debug.get("md_health")
        if isinstance(md_health, Mapping):
            normalized_md_health = _copy_mapping(cast(Mapping[str, Any], md_health))
            normalized_md_health.pop("strategy_state_age_ms", None)
            normalized_md_health.pop("signal_state_age_ms", None)
            normalized_debug["md_health"] = normalized_md_health
        stable["debug"] = normalized_debug

    return stable


def build_signal_delta_patch(
    previous: Mapping[str, Any] | None,
    current: Mapping[str, Any],
) -> dict[str, Any]:
    """
    Build a patch where missing keys mean no-change and explicit null means delete.
    """
    prev = _copy_mapping(previous) if isinstance(previous, Mapping) else {}
    curr = _copy_mapping(current)
    patch: dict[str, Any] = {}

    prev_legs = _normalize_legs(prev.get("legs"))
    curr_legs = _normalize_legs(curr.get("legs"))

    for key in sorted(set(prev) | set(curr)):
        if key in {"id", "meta", "legs"}:
            continue
        if key not in curr:
            patch[key] = None
            continue
        if key not in prev or prev[key] != curr[key]:
            patch[key] = deepcopy(curr[key])

    legs_patch: dict[str, Any] = {}
    for contract_id in sorted(set(prev_legs) | set(curr_legs)):
        if contract_id not in curr_legs:
            legs_patch[contract_id] = None
            continue
        if contract_id not in prev_legs or prev_legs[contract_id] != curr_legs[contract_id]:
            leg_row = curr_legs[contract_id]
            if isinstance(leg_row, Mapping):
                leg_payload = _copy_mapping(cast(Mapping[str, Any], leg_row))
                leg_payload.setdefault("contract_id", contract_id)
                legs_patch[contract_id] = leg_payload
            else:
                legs_patch[contract_id] = deepcopy(leg_row)
    if legs_patch:
        patch["legs"] = legs_patch

    return patch


def apply_signal_delta_patch(
    previous: Mapping[str, Any],
    patch: Mapping[str, Any],
) -> dict[str, Any]:
    """
    Apply a signal patch following delta semantics.
    """
    out = _copy_mapping(previous)
    for key, value in patch.items():
        key_text = decode_text(key)
        if key_text == "legs":
            existing = _normalize_legs(out.get("legs"))
            if isinstance(value, Mapping):
                for contract_id, leg_value in value.items():
                    contract_text = decode_text(contract_id)
                    if leg_value is None:
                        existing.pop(contract_text, None)
                        continue
                    if isinstance(leg_value, Mapping):
                        leg_payload = _copy_mapping(cast(Mapping[str, Any], leg_value))
                    else:
                        leg_payload = deepcopy(leg_value)
                    existing[contract_text] = leg_payload
            out["legs"] = existing
            continue

        if value is None:
            out.pop(key_text, None)
        else:
            out[key_text] = deepcopy(value)

    return out


def build_trade_update_payload(
    *,
    profile: str,
    strategy_id: str,
    seq: int,
    op: str,
    row_id: str,
    version: int,
    trade: Mapping[str, Any] | None,
    server_ts_ms: int | None = None,
) -> dict[str, Any]:
    timestamp_ms = now_ms() if server_ts_ms is None else int(server_ts_ms)
    payload: dict[str, Any] = {
        "profile": profile,
        "strategy_id": strategy_id,
        "seq": int(seq),
        "server_ts_ms": timestamp_ms,
        "op": op,
        "row_id": row_id,
        "version": int(version),
        "trade": _copy_mapping(trade) if isinstance(trade, Mapping) else None,
    }
    return payload


def _alerts_signature(
    alerts_rows: Sequence[Mapping[str, Any]],
    *,
    total_count: int | None,
) -> tuple[int, int | None, str]:
    count = int(total_count) if total_count is not None else len(alerts_rows)
    latest_ts: int | None = None
    latest_row_id = ""
    for row in alerts_rows:
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        if ts_ms is None or ts_ms < 0:
            continue
        row_id = decode_text(row.get("row_id")).strip()
        if latest_ts is None or ts_ms > latest_ts:
            latest_ts = ts_ms
            latest_row_id = row_id
    return count, latest_ts, latest_row_id


def _balances_signature(
    balances_rows: Sequence[Mapping[str, Any]],
    *,
    portfolio_snapshot: Mapping[str, Any] | None = None,
    extra_payload: Mapping[str, Any] | None = None,
    market_rows: Mapping[str, Mapping[str, Any]] | None = None,
) -> tuple[int, int | None, str]:
    def _stable_fingerprint(value: Any) -> str:
        return json.dumps(value, sort_keys=True, separators=(",", ":"), default=str)

    count = len(balances_rows)
    latest_ts: int | None = None
    fingerprint_parts: list[str] = []
    for row in balances_rows:
        row_id = decode_text(
            row.get("row_id")
            or row.get("id")
            or row.get("coin")
            or row.get("asset"),
        ).strip()
        ts_ms = coerce_ts_ms(
            row.get("last_ts")
            or row.get("ts_ms")
            or row.get("ts")
            or row.get("timestamp"),
        )
        if ts_ms is not None and (latest_ts is None or ts_ms > latest_ts):
            latest_ts = ts_ms
        fingerprint_parts.append(
            ":".join(
                (
                    row_id,
                    str(ts_ms or ""),
                    str(row.get("qty_raw") or row.get("quantity") or row.get("total") or ""),
                    str(row.get("mv_raw") or row.get("mv_usd") or ""),
                    str(row.get("mark_raw") or row.get("mark") or ""),
                ),
            ),
        )
    fingerprint_parts.sort()
    if isinstance(portfolio_snapshot, Mapping):
        snapshot_ts = coerce_ts_ms(portfolio_snapshot.get("server_ts_ms"))
        if snapshot_ts is not None and (latest_ts is None or snapshot_ts > latest_ts):
            latest_ts = snapshot_ts
        fingerprint_parts.append(
            f"portfolio:{_stable_fingerprint({
                'portfolio_id': portfolio_snapshot.get('portfolio_id'),
                'base_currency': portfolio_snapshot.get('base_currency'),
                'inventory': portfolio_snapshot.get('inventory'),
                'inventory_by_asset': portfolio_snapshot.get('inventory_by_asset'),
                'components': portfolio_snapshot.get('components'),
                'balances': portfolio_snapshot.get('balances'),
                'accounts': portfolio_snapshot.get('accounts'),
            })}",
        )
    if isinstance(market_rows, Mapping) and market_rows:
        fingerprint_parts.append(f"market:{_stable_fingerprint(dict(market_rows))}")
    if isinstance(extra_payload, Mapping) and extra_payload:
        fingerprint_parts.append(f"extra:{_stable_fingerprint(extra_payload)}")
    return count, latest_ts, "|".join(fingerprint_parts)


def _timestamp_is_fresh(
    ts_ms: Any,
    *,
    now_ms_value: int,
    stale_after_ms: int,
) -> bool:
    parsed = safe_int(ts_ms)
    return parsed is not None and (now_ms_value - parsed) <= stale_after_ms


def _portfolio_snapshot_inventory_stale_after_ms(
    portfolio_snapshot: Mapping[str, Any],
) -> int | None:
    raw_inventory_by_asset = portfolio_snapshot.get("inventory_by_asset")
    if not isinstance(raw_inventory_by_asset, Mapping):
        return None
    stale_after_ms = SOCKETIO_TOKENMM_BALANCES_STALE_AFTER_MS
    for payload in raw_inventory_by_asset.values():
        if not isinstance(payload, Mapping):
            continue
        stale_after_ms = max(
            stale_after_ms,
            safe_int(payload.get("stale_after_ms")) or SOCKETIO_TOKENMM_BALANCES_STALE_AFTER_MS,
        )
    return stale_after_ms


def _canonical_balances_signature(
    *,
    profile: str,
    balances_rows_by_strategy: Mapping[str, Sequence[Mapping[str, Any]]],
    balance_snapshot_presence: Mapping[str, bool],
    portfolio_snapshot: Mapping[str, Any] | None,
    contracts: Sequence[Any],
    required_strategy_ids: Sequence[str],
    market_rows: Mapping[str, Mapping[str, Any]] | None,
) -> tuple[int, int | None, str]:
    normalized_profile = normalize_profile(profile)
    now_ms_value = now_ms()
    if isinstance(portfolio_snapshot, Mapping):
        if normalized_profile == "equities":
            stale_after_ms = _portfolio_snapshot_inventory_stale_after_ms(portfolio_snapshot)
            if stale_after_ms is not None and _timestamp_is_fresh(
                portfolio_snapshot.get("server_ts_ms"),
                now_ms_value=now_ms_value,
                stale_after_ms=stale_after_ms,
            ):
                return _balances_signature(
                    [],
                    portfolio_snapshot=portfolio_snapshot,
                    market_rows=market_rows,
                )
        elif normalized_profile == "tokenmm":
            inventory = portfolio_snapshot.get("inventory")
            inventory_payload = dict(inventory) if isinstance(inventory, Mapping) else {}
            stale_after_ms = (
                safe_int(inventory_payload.get("stale_after_ms"))
                or SOCKETIO_TOKENMM_BALANCES_STALE_AFTER_MS
            )
            if (
                _timestamp_is_fresh(
                    portfolio_snapshot.get("server_ts_ms"),
                    now_ms_value=now_ms_value,
                    stale_after_ms=stale_after_ms,
                )
                and _timestamp_is_fresh(
                    inventory_payload.get("ts_ms"),
                    now_ms_value=now_ms_value,
                    stale_after_ms=stale_after_ms,
                )
            ):
                return _balances_signature(
                    [],
                    portfolio_snapshot=portfolio_snapshot,
                    market_rows=market_rows,
                )

    required_strategy_id_set = {decode_text(value).strip() for value in required_strategy_ids}
    components: list[dict[str, Any]] = []
    for strategy_id, strategy_rows in balances_rows_by_strategy.items():
        latest_ts_ms: int | None = None
        for row in strategy_rows:
            parsed = safe_int(row.get("ts_ms"))
            if parsed is None:
                parsed = coerce_ts_ms(row.get("ts") or row.get("timestamp"))
            if parsed is None:
                continue
            if latest_ts_ms is None or parsed > latest_ts_ms:
                latest_ts_ms = parsed
        snapshot_present = bool(balance_snapshot_presence.get(strategy_id, False))
        age_ms = (now_ms_value - latest_ts_ms) if latest_ts_ms is not None else None
        empty_snapshot_present = normalized_profile == "equities" and snapshot_present and not strategy_rows
        stale = (
            not snapshot_present
            or (latest_ts_ms is None and not empty_snapshot_present)
            or (age_ms is not None and age_ms > SOCKETIO_TOKENMM_BALANCES_STALE_AFTER_MS)
        )
        missing = (not snapshot_present) or (not strategy_rows and not empty_snapshot_present)
        components.append(
            {
                "strategy_id": strategy_id,
                "snapshot_present": snapshot_present,
                "rows": len(strategy_rows),
                "latest_ts_ms": latest_ts_ms,
                "age_ms": age_ms,
                "stale": stale,
                "required": strategy_id in required_strategy_id_set,
                "missing": missing,
            },
        )

    merged_rows = merge_portfolio_balances_rows(
        rows_by_strategy=balances_rows_by_strategy,
        portfolio_id=normalized_profile,
        preserve_product_scope_cash=True,
    )
    filtered_rows = filter_balance_rows_for_contract_scope(
        merged_rows,
        contracts=contracts,
        preserve_shared_account_rows=(normalized_profile == "equities"),
    )
    if filtered_rows:
        merged_rows = filtered_rows
    canonical_rows, risk_groups = build_balance_risk_groups(merged_rows)
    missing_required = sorted(
        component["strategy_id"]
        for component in components
        if component["required"] and component["missing"]
    )
    degraded = bool(missing_required) or any(component["stale"] for component in components)
    return _balances_signature(
        canonical_rows,
        extra_payload={
            "components": components,
            "risk_groups": risk_groups,
            "degraded": degraded,
            "missing_required": missing_required,
        },
    )


@dataclass(frozen=True)
class FluxStandardSubscription:
    sid: str
    surface: str
    profile: str
    surface_query_key: str
    stream_id: str
    snapshot_revision: int
    capabilities: dict[str, Any]


class FluxSocketEmitter:
    """
    Polling emitter for TokenMM room updates.
    """

    def __init__(
        self,
        *,
        socketio: SocketIO,
        store: FluxSocketStoreProtocol,
        metadata_resolver: Callable[[str], Any],
        strategy_resolver: Callable[[str], str | None],
        strategy_ids_resolver: Callable[[str], Sequence[str]] | None = None,
        required_strategy_ids_resolver: Callable[..., Sequence[str]] | None = None,
        realtime_rollout_resolver: Callable[[], Mapping[str, Any]] | None = None,
        poll_interval_s: float = SOCKETIO_DEFAULT_POLL_INTERVAL_S,
    ) -> None:
        self._socketio = socketio
        self._store = store
        self._metadata_resolver = metadata_resolver
        self._strategy_resolver = strategy_resolver
        self._strategy_ids_resolver = strategy_ids_resolver
        self._required_strategy_ids_resolver = required_strategy_ids_resolver
        self._realtime_rollout_resolver = realtime_rollout_resolver
        self._poll_interval_s = max(0.25, float(poll_interval_s))
        self._lock = RLock()
        self._running = False
        self._thread: Thread | None = None
        self._wake_event = Event()
        self._profile_refcounts: dict[str, int] = {}
        self._legacy_profile_refcounts: dict[str, int] = {}
        self._seq_by_profile: dict[str, int] = {}
        self._standard_seq_by_profile_surface: dict[tuple[str, str], int] = {}
        self._signal_by_profile: dict[str, dict[str, dict[str, Any]]] = {}
        self._trade_cursor_by_profile: dict[str, dict[str, int]] = {}
        self._alerts_by_profile: dict[str, tuple[int, int | None, str]] = {}
        self._balances_by_profile: dict[str, tuple[int, int | None, str]] = {}
        self._failure_streak_by_profile: dict[str, int] = {}
        self._backoff_until_by_profile: dict[str, float] = {}
        self._standard_subscriptions_by_sid: dict[str, dict[str, FluxStandardSubscription]] = {}
        self.metrics: dict[str, Any] = {
            "active_standard_subscribers": {},
            "standard_subscribe_counts": {},
            "standard_recovery_required_counts": {},
            "standard_event_counts": {},
            "legacy_event_counts": {},
        }
        self._tokenmm_clean_trade_stream_signatures: dict[str, tuple[int, str]] = {}
        self._trade_poll_limit = SOCKETIO_TRADE_POLL_LIMIT
        self._trade_scan_limit = SOCKETIO_TRADE_SCAN_LIMIT
        self._alerts_preview_limit = SOCKETIO_ALERTS_PREVIEW_LIMIT

    @property
    def poll_interval_s(self) -> float:
        return self._poll_interval_s

    def current_seq(self, profile: Any) -> int:
        normalized = normalize_profile(profile)
        with self._lock:
            return int(self._seq_by_profile.get(normalized, 0))

    def current_standard_seq(self, profile: Any, surface: Any) -> int:
        normalized_profile = normalize_profile(profile)
        normalized_surface = normalize_surface(surface)
        with self._lock:
            return int(
                self._standard_seq_by_profile_surface.get((normalized_profile, normalized_surface), 0),
            )

    def describe_standard_stream(self, *, surface: Any, profile: Any) -> dict[str, Any] | None:
        normalized_surface = normalize_surface(surface)
        normalized_profile = normalize_profile(profile)
        strategy_ids = self._resolve_profile_strategy_ids(normalized_profile)
        if (
            normalized_surface not in REALTIME_SUPPORTED_SURFACES
            or not normalized_profile
            or not strategy_ids
        ):
            return None
        return build_standard_snapshot_metadata(
            surface=normalized_surface,
            profile=normalized_profile,
            strategy_ids=strategy_ids,
            last_seq=self.current_standard_seq(normalized_profile, normalized_surface),
            poll_interval_s=self._poll_interval_s,
        )

    def resolve_standard_subscription_descriptor(
        self,
        *,
        contract_version: int,
        surface: Any,
        profile: Any,
    ) -> tuple[dict[str, Any] | None, str | None]:
        normalized_surface = normalize_surface(surface)
        normalized_profile = normalize_profile(profile)
        rejection_reason = standard_subscribe_rejection_reason(
            contract_version=int(contract_version),
            surface=normalized_surface,
            profile=normalized_profile,
            rollout=self._rollout_state(),
        )
        descriptor = self.describe_standard_stream(
            surface=normalized_surface,
            profile=normalized_profile,
        )
        if rejection_reason is not None or descriptor is None:
            return None, rejection_reason or "unsupported_profile"
        return descriptor, None

    def _rollout_state(self) -> Mapping[str, Any]:
        if self._realtime_rollout_resolver is None:
            return default_realtime_rollout()
        raw_value = self._realtime_rollout_resolver()
        if isinstance(raw_value, Mapping):
            return raw_value
        return default_realtime_rollout()

    def _record_metric(self, metric_name: str, key: str) -> None:
        bucket = self.metrics.get(metric_name)
        if not isinstance(bucket, dict):
            bucket = {}
            self.metrics[metric_name] = bucket
        bucket[key] = int(bucket.get(key, 0)) + 1

    def _refresh_active_standard_metrics(self) -> None:
        counts: dict[str, int] = {}
        with self._lock:
            subscriptions = {
                sid: dict(surface_map)
                for sid, surface_map in self._standard_subscriptions_by_sid.items()
            }
        for surface_map in subscriptions.values():
            for subscription in surface_map.values():
                key = f"{subscription.surface}:v{REALTIME_STANDARD_CONTRACT_VERSION}"
                counts[key] = counts.get(key, 0) + 1
        self.metrics["active_standard_subscribers"] = counts

    def start(self) -> None:
        with self._lock:
            if self._running:
                self._wake_event.set()
                return
            self._running = True
            self._thread = Thread(
                target=self._run_loop,
                name=f"flux-socket-emitter-{uuid4().hex[:8]}",
                daemon=True,
            )
            self._thread.start()
        self._wake_event.set()

    def stop(self) -> None:
        with self._lock:
            self._running = False
            thread = self._thread
            self._thread = None
            self._wake_event.set()
        if thread is not None and thread.is_alive():
            thread.join(timeout=self._poll_interval_s * 2.0)

    def emit_once(self, *, profile: str | None = None) -> None:
        if profile is None:
            profiles = self._active_profiles()
        else:
            normalized = normalize_profile(profile)
            profiles = [normalized] if normalized else []

        for current_profile in profiles:
            self._emit_profile_safely(current_profile)

    def _run_loop(self) -> None:
        while True:
            with self._lock:
                if not self._running:
                    return
                profiles = sorted(
                    profile for profile, count in self._profile_refcounts.items() if count > 0
                )
                if not profiles:
                    self._wake_event.clear()
            if not profiles:
                self._wake_event.wait(timeout=self._poll_interval_s * 4.0)
                continue

            for profile in profiles:
                self._emit_profile_safely(profile)

            self._socketio.sleep(self._poll_interval_s)

    def acquire_profile(self, profile: Any) -> None:
        normalized = normalize_profile(profile)
        if not normalized:
            return
        with self._lock:
            self._profile_refcounts[normalized] = self._profile_refcounts.get(normalized, 0) + 1
            self._wake_event.set()

    def acquire_legacy_profile(self, profile: Any) -> None:
        normalized = normalize_profile(profile)
        if not normalized:
            return
        with self._lock:
            self._legacy_profile_refcounts[normalized] = (
                self._legacy_profile_refcounts.get(normalized, 0) + 1
            )
        self.acquire_profile(normalized)

    def release_profile(self, profile: Any) -> None:
        normalized = normalize_profile(profile)
        if not normalized:
            return
        with self._lock:
            next_count = self._profile_refcounts.get(normalized, 0) - 1
            if next_count <= 0:
                self._profile_refcounts.pop(normalized, None)
                self._cleanup_profile_state_locked(normalized)
            else:
                self._profile_refcounts[normalized] = next_count
            self._wake_event.set()

    def release_legacy_profile(self, profile: Any) -> None:
        normalized = normalize_profile(profile)
        if not normalized:
            return
        with self._lock:
            next_count = self._legacy_profile_refcounts.get(normalized, 0) - 1
            if next_count <= 0:
                self._legacy_profile_refcounts.pop(normalized, None)
            else:
                self._legacy_profile_refcounts[normalized] = next_count
        self.release_profile(normalized)

    def has_legacy_profile_subscribers(self, profile: Any) -> bool:
        normalized = normalize_profile(profile)
        with self._lock:
            return self._legacy_profile_refcounts.get(normalized, 0) > 0

    def _active_profiles(self) -> list[str]:
        with self._lock:
            return sorted(
                profile for profile, count in self._profile_refcounts.items() if count > 0
            )

    def _cleanup_profile_state_locked(self, profile: str) -> None:
        self._seq_by_profile.pop(profile, None)
        for key in list(self._standard_seq_by_profile_surface):
            if key[0] == profile:
                self._standard_seq_by_profile_surface.pop(key, None)
        self._legacy_profile_refcounts.pop(profile, None)
        self._signal_by_profile.pop(profile, None)
        self._trade_cursor_by_profile.pop(profile, None)
        self._alerts_by_profile.pop(profile, None)
        self._balances_by_profile.pop(profile, None)
        self._failure_streak_by_profile.pop(profile, None)
        self._backoff_until_by_profile.pop(profile, None)

    def _clear_profile_failure(self, profile: str) -> None:
        with self._lock:
            self._failure_streak_by_profile.pop(profile, None)
            self._backoff_until_by_profile.pop(profile, None)

    def _record_profile_failure(self, profile: str) -> tuple[int, float]:
        with self._lock:
            streak = min(
                self._failure_streak_by_profile.get(profile, 0) + 1,
                SOCKETIO_FAILURE_STREAK_CAP,
            )
            backoff_s = min(
                self._poll_interval_s * (2 ** max(0, streak - 1)),
                SOCKETIO_FAILURE_BACKOFF_CAP_S,
            )
            self._failure_streak_by_profile[profile] = streak
            self._backoff_until_by_profile[profile] = monotonic() + backoff_s
            return streak, backoff_s

    def _is_profile_backing_off(self, profile: str, *, now_s: float) -> bool:
        with self._lock:
            return self._backoff_until_by_profile.get(profile, 0.0) > now_s

    def _next_seq(self, profile: str) -> int:
        with self._lock:
            seq = self._seq_by_profile.get(profile, 0) + 1
            self._seq_by_profile[profile] = seq
            return seq

    def _next_standard_seq(self, profile: str, surface: str) -> int:
        normalized_profile = normalize_profile(profile)
        normalized_surface = normalize_surface(surface)
        with self._lock:
            key = (normalized_profile, normalized_surface)
            seq = self._standard_seq_by_profile_surface.get(key, 0) + 1
            self._standard_seq_by_profile_surface[key] = seq
            return seq

    def _tokenmm_trade_stream_requires_reset(self, strategy_id: str, metadata: Any) -> bool:
        if not _metadata_is_tokenmm(metadata):
            return False
        stream_reset_resolver = getattr(self._store, "tokenmm_trade_stream_requires_reset", None)
        signature_resolver = getattr(self._store, "tokenmm_trade_stream_signature", None)
        signature: tuple[int, str] | None = None
        if callable(signature_resolver):
            raw_signature = signature_resolver(strategy_id)
            if isinstance(raw_signature, Sequence) and not isinstance(raw_signature, str | bytes) and len(raw_signature) == 2:
                signature = (
                    safe_int(raw_signature[0]) or 0,
                    decode_text(raw_signature[1]).strip(),
                )
        if signature is not None and self._tokenmm_clean_trade_stream_signatures.get(strategy_id) == signature:
            return False
        if not callable(stream_reset_resolver):
            return False
        requires_reset = bool(stream_reset_resolver(strategy_id))
        if requires_reset:
            self._tokenmm_clean_trade_stream_signatures.pop(strategy_id, None)
            return True
        if signature is not None:
            self._tokenmm_clean_trade_stream_signatures[strategy_id] = signature
        return False

    def _resolve_profile_strategy_ids(self, profile: str) -> list[str]:
        ids: list[str] = []
        if self._strategy_ids_resolver is not None:
            raw_values = self._strategy_ids_resolver(profile)
            if isinstance(raw_values, Sequence) and not isinstance(raw_values, str | bytes):
                candidates = list(raw_values)
            elif raw_values is None:
                candidates = []
            else:
                candidates = [raw_values]
            seen: set[str] = set()
            for value in candidates:
                strategy_id = decode_text(value).strip()
                if not strategy_id or strategy_id in seen:
                    continue
                seen.add(strategy_id)
                ids.append(strategy_id)
        if ids:
            return ids
        resolved_strategy_id = self._strategy_resolver(profile)
        return [resolved_strategy_id] if resolved_strategy_id else []

    def _resolve_required_strategy_ids(self, profile: str, *, fallback: Sequence[str]) -> list[str]:
        ids: list[str] = []
        if self._required_strategy_ids_resolver is not None:
            try:
                raw_values = self._required_strategy_ids_resolver(profile, fallback=fallback)
            except TypeError:
                raw_values = self._required_strategy_ids_resolver(profile)
            if isinstance(raw_values, Sequence) and not isinstance(raw_values, str | bytes):
                candidates = list(raw_values)
            elif raw_values is None:
                candidates = []
            else:
                candidates = [raw_values]
            seen: set[str] = set()
            for value in candidates:
                strategy_id = decode_text(value).strip()
                if not strategy_id or strategy_id in seen:
                    continue
                seen.add(strategy_id)
                ids.append(strategy_id)
        return ids or list(fallback)

    def _load_canonical_balances_signature(
        self,
        profile: str,
        *,
        strategy_ids: Sequence[str],
    ) -> Any:
        required_strategy_ids = self._resolve_required_strategy_ids(
            profile,
            fallback=strategy_ids,
        )
        market_rows = self._store.load_market_rows_for_strategies(strategy_ids)
        balances_rows_by_strategy: dict[str, list[dict[str, Any]]] = {}
        balance_snapshot_presence: dict[str, bool] = {}
        portfolio_snapshot = self._store.load_portfolio_snapshot(profile)
        for current_strategy_id in strategy_ids:
            strategy_balance_rows, snapshot_present = self._store.load_balances_rows_with_presence(
                current_strategy_id
            )
            balance_snapshot_presence[current_strategy_id] = snapshot_present
            normalized_balance_rows: list[dict[str, Any]] = []
            for row in strategy_balance_rows:
                if not isinstance(row, Mapping):
                    continue
                normalized_row = dict(row)
                normalized_row.setdefault("strategy_id", current_strategy_id)
                normalized_balance_rows.append(normalized_row)
            balances_rows_by_strategy[current_strategy_id] = normalized_balance_rows
        balances_signature = _canonical_balances_signature(
            profile=profile,
            balances_rows_by_strategy=balances_rows_by_strategy,
            balance_snapshot_presence=balance_snapshot_presence,
            portfolio_snapshot=portfolio_snapshot,
            contracts=getattr(self._store, "_contracts", ()),
            required_strategy_ids=required_strategy_ids,
            market_rows=market_rows,
        )
        return balances_signature

    def unsubscribe_standard(
        self,
        sid: str,
        *,
        surface: str | None = None,
    ) -> None:
        normalized_surface = normalize_surface(surface) if surface is not None else None
        released_profiles: list[str] = []
        with self._lock:
            surface_map = self._standard_subscriptions_by_sid.get(sid)
            if not surface_map:
                return
            if normalized_surface is None:
                released_profiles = [subscription.profile for subscription in surface_map.values()]
                self._standard_subscriptions_by_sid.pop(sid, None)
            else:
                subscription = surface_map.pop(normalized_surface, None)
                if subscription is not None:
                    released_profiles.append(subscription.profile)
                if not surface_map:
                    self._standard_subscriptions_by_sid.pop(sid, None)
        for profile in released_profiles:
            self.release_profile(profile)
        self._refresh_active_standard_metrics()

    def subscribe_standard(
        self,
        sid: str,
        *,
        contract_version: int,
        surface: Any,
        profile: Any,
        surface_query_key: Any,
        stream_id: Any,
        snapshot_revision: Any,
        resume_from_seq: Any,
    ) -> dict[str, Any]:
        normalized_surface = normalize_surface(surface)
        normalized_profile = normalize_profile(profile)
        descriptor, rejection_reason = self.resolve_standard_subscription_descriptor(
            contract_version=int(contract_version),
            surface=normalized_surface,
            profile=normalized_profile,
        )

        if descriptor is None:
            reason = rejection_reason or "unsupported_profile"
            self._record_metric("standard_subscribe_counts", reason)
            return {
                "accepted": False,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "surface": normalized_surface,
                "profile": normalized_profile,
                "reason": reason,
            }

        request_surface_query_key = decode_text(surface_query_key).strip()
        request_stream_id = decode_text(stream_id).strip()
        request_snapshot_revision = safe_int(snapshot_revision)
        if (
            not request_surface_query_key
            or not request_stream_id
            or request_snapshot_revision is None
        ):
            self._record_metric("standard_subscribe_counts", "missing_snapshot_lineage")
            return {
                "accepted": False,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "surface": normalized_surface,
                "profile": normalized_profile,
                "reason": "missing_snapshot_lineage",
            }
        if request_stream_id and request_stream_id != descriptor["stream_id"]:
            self._record_metric("standard_subscribe_counts", "stream_rollover")
            return {
                "accepted": False,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "surface": normalized_surface,
                "profile": normalized_profile,
                "reason": "stream_rollover",
            }
        if (
            request_snapshot_revision is not None
            and request_snapshot_revision != descriptor["snapshot_revision"]
        ):
            self._record_metric("standard_subscribe_counts", "snapshot_revision_mismatch")
            return {
                "accepted": False,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "surface": normalized_surface,
                "profile": normalized_profile,
                "reason": "snapshot_revision_mismatch",
            }
        if request_surface_query_key and request_surface_query_key != descriptor["surface_query_key"]:
            self._record_metric("standard_subscribe_counts", "surface_query_key_mismatch")
            return {
                "accepted": False,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "surface": normalized_surface,
                "profile": normalized_profile,
                "reason": "surface_query_key_mismatch",
            }

        self.unsubscribe_standard(sid, surface=normalized_surface)
        self.acquire_profile(normalized_profile)
        profile_acquired = True
        subscription_registered = False
        try:
            self._prime_profile_state(normalized_profile)
            refreshed, refreshed_reason = self.resolve_standard_subscription_descriptor(
                contract_version=int(contract_version),
                surface=normalized_surface,
                profile=normalized_profile,
            )
            if refreshed is None:
                reason = refreshed_reason or "unsupported_profile"
                self.release_profile(normalized_profile)
                profile_acquired = False
                self._record_metric("standard_subscribe_counts", reason)
                return {
                    "accepted": False,
                    "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                    "surface": normalized_surface,
                    "profile": normalized_profile,
                    "reason": reason,
                }

            subscription = FluxStandardSubscription(
                sid=sid,
                surface=normalized_surface,
                profile=normalized_profile,
                surface_query_key=refreshed["surface_query_key"],
                stream_id=refreshed["stream_id"],
                snapshot_revision=int(refreshed["snapshot_revision"]),
                capabilities=dict(refreshed["capabilities"]),
            )
            with self._lock:
                surface_map = self._standard_subscriptions_by_sid.setdefault(sid, {})
                surface_map[normalized_surface] = subscription
            subscription_registered = True
            profile_acquired = False
            if normalized_surface == "balances":
                strategy_ids = self._resolve_profile_strategy_ids(normalized_profile)
                if strategy_ids:
                    balances_signature = self._load_canonical_balances_signature(
                        normalized_profile,
                        strategy_ids=strategy_ids,
                    )
                    with self._lock:
                        if (
                            self._profile_refcounts.get(normalized_profile, 0) > 0
                            and self._balances_by_profile.get(normalized_profile) is None
                        ):
                            self._balances_by_profile[normalized_profile] = balances_signature
            self._refresh_active_standard_metrics()
            self._record_metric("standard_subscribe_counts", "accepted")
            accepted_start_seq = int(refreshed["last_seq"])
            return {
                "accepted": True,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "surface": normalized_surface,
                "profile": normalized_profile,
                "surface_query_key": refreshed["surface_query_key"],
                "stream_id": refreshed["stream_id"],
                "snapshot_revision": int(refreshed["snapshot_revision"]),
                "accepted_start_seq": accepted_start_seq,
                "last_seq": int(refreshed["last_seq"]),
                "capabilities": dict(refreshed["capabilities"]),
                "requested_resume_from_seq": safe_int(resume_from_seq) or 0,
            }
        except Exception:
            if subscription_registered:
                self.unsubscribe_standard(sid, surface=normalized_surface)
            elif profile_acquired:
                self.release_profile(normalized_profile)
            raise

    def _standard_subscriptions_for_profile(self, profile: str) -> list[FluxStandardSubscription]:
        normalized_profile = normalize_profile(profile)
        with self._lock:
            return [
                subscription
                for surface_map in self._standard_subscriptions_by_sid.values()
                for subscription in surface_map.values()
                if subscription.profile == normalized_profile
            ]

    def _emit_standard_event(
        self,
        subscription: FluxStandardSubscription,
        *,
        kind: str,
        seq: int | None,
        reason: str | None = None,
        payload: Mapping[str, Any] | None = None,
    ) -> None:
        event_seq = (
            self.current_standard_seq(subscription.profile, subscription.surface)
            if seq is None
            else int(seq)
        )
        event: dict[str, Any] = {
            "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
            "surface": subscription.surface,
            "stream_id": subscription.stream_id,
            "profile": subscription.profile,
            "kind": kind,
            "seq": max(0, int(event_seq)),
            "snapshot_revision": int(subscription.snapshot_revision),
            "server_ts_ms": now_ms(),
        }
        if reason:
            event["reason"] = reason
        if payload is not None:
            event["payload"] = deepcopy(payload)
        self._socketio.emit(REALTIME_STANDARD_EVENT, event, to=subscription.sid)
        self._record_metric("standard_event_counts", f"{subscription.surface}:{kind}")
        if kind == "recovery_required" and reason:
            self._record_metric("standard_recovery_required_counts", reason)

    def _prime_profile_state(self, profile: str) -> None:
        strategy_ids = self._resolve_profile_strategy_ids(profile)
        if not strategy_ids:
            return
        self._emit_profile(
            profile,
            strategy_id=strategy_ids[0],
            strategy_ids=strategy_ids,
            room=profile_room(profile),
            emit_legacy=False,
            emit_standard=False,
        )

    def _emit_profile_safely(self, profile: str) -> None:
        if self._is_profile_backing_off(profile, now_s=monotonic()):
            return

        strategy_ids = self._resolve_profile_strategy_ids(profile)
        if not strategy_ids:
            return

        strategy_id = strategy_ids[0]
        room = profile_room(profile)
        try:
            self._emit_profile(
                profile,
                strategy_id=strategy_id,
                strategy_ids=strategy_ids,
                room=room,
            )
        except Exception as e:
            streak, backoff_s = self._record_profile_failure(profile)
            _LOG.exception(
                "Flux socket emitter profile tick failed profile=%s strategy_id=%s streak=%s backoff_s=%.3f error=%s",
                profile,
                strategy_id,
                streak,
                backoff_s,
                type(e).__name__,
            )
        else:
            self._clear_profile_failure(profile)

    def _emit_profile(  # noqa: C901
        self,
        profile: str,
        *,
        strategy_id: str,
        strategy_ids: Sequence[str],
        room: str,
        emit_legacy: bool = True,
        emit_standard: bool = True,
    ) -> None:
        signal_payloads_by_strategy: dict[str, dict[str, Any]] = {}
        load_running_states = getattr(self._store, "load_running_states", None)
        running_states = (
            load_running_states(strategy_ids)
            if callable(load_running_states)
            else {strategy_id: None for strategy_id in strategy_ids}
        )
        load_signals_parameters = inspect.signature(self._store.load_signals_payload).parameters
        supports_running = "running" in load_signals_parameters
        for current_strategy_id in strategy_ids:
            metadata = self._metadata_resolver(current_strategy_id)
            if supports_running:
                signal_payload = self._store.load_signals_payload(
                    current_strategy_id,
                    metadata,
                    running=running_states.get(current_strategy_id),
                )
            else:
                signal_payload = self._store.load_signals_payload(current_strategy_id, metadata)
            signal_payloads_by_strategy[current_strategy_id] = build_stable_signal_view(signal_payload)

        with self._lock:
            trade_cursors = dict(self._trade_cursor_by_profile.get(profile, {}))
            previous_signals = self._signal_by_profile.get(profile, {})
            previous_alerts_signature = self._alerts_by_profile.get(profile)
            previous_balances_signature = self._balances_by_profile.get(profile)
            legacy_profile_active = self._legacy_profile_refcounts.get(profile, 0) > 0

        next_trade_cursors = dict(trade_cursors)
        for current_strategy_id in strategy_ids:
            next_trade_cursors[current_strategy_id] = int(next_trade_cursors.get(current_strategy_id, 0))

        load_trades_parameters = inspect.signature(self._store.load_trades_rows).parameters
        supports_base_first_qty = "base_first_qty" in load_trades_parameters
        scanned_trade_entries: list[tuple[int, dict[str, Any]]] = []
        trade_gap = False
        for current_strategy_id in strategy_ids:
            current_metadata = self._metadata_resolver(current_strategy_id)
            if self._tokenmm_trade_stream_requires_reset(current_strategy_id, current_metadata):
                _LOG.warning(
                    "Flux socket emitter detected TokenMM legacy trade rows without normalized qty fields profile=%s strategy_id=%s",
                    profile,
                    current_strategy_id,
                )
                trade_gap = True
                continue
            load_trades_kwargs: dict[str, Any] = {
                "limit": self._trade_scan_limit,
                "since_ms": None,
                "since_seq": None,
                "scan_limit": self._trade_scan_limit,
            }
            if supports_base_first_qty:
                load_trades_kwargs["base_first_qty"] = _metadata_is_tokenmm(current_metadata)
            strategy_rows = self._store.load_trades_rows(
                current_strategy_id,
                **load_trades_kwargs,
            )
            current_cursor = int(next_trade_cursors.get(current_strategy_id, 0))
            seq_values = [safe_int(row.get("seq")) for row in strategy_rows if isinstance(row, Mapping)]
            parsed_seqs = [seq for seq in seq_values if seq is not None]
            if parsed_seqs:
                min_seq = min(parsed_seqs)
                max_seq = max(parsed_seqs)
                if current_cursor > 0 and current_cursor < (min_seq - 1):
                    _LOG.warning(
                        "Flux socket emitter detected trade replay boundary profile=%s strategy_id=%s cursor_seq=%s window_min_seq=%s window_max_seq=%s scan_limit=%s",
                        profile,
                        current_strategy_id,
                        current_cursor,
                        min_seq,
                        max_seq,
                        self._trade_scan_limit,
                    )
                    trade_gap = True
                    next_trade_cursors[current_strategy_id] = max(0, min_seq - 1)
                    continue
                if current_cursor > max_seq:
                    _LOG.warning(
                        "Flux socket emitter detected trade cursor overrun profile=%s strategy_id=%s cursor_seq=%s window_max_seq=%s scan_limit=%s",
                        profile,
                        current_strategy_id,
                        current_cursor,
                        max_seq,
                        self._trade_scan_limit,
                    )
                    trade_gap = True
                    next_trade_cursors[current_strategy_id] = int(max_seq)
                    continue
            for row in strategy_rows:
                if not isinstance(row, Mapping):
                    continue
                normalized_row = dict(row)
                normalized_row.setdefault("strategy_id", current_strategy_id)
                row_seq = safe_int(normalized_row.get("seq"))
                if row_seq is None or row_seq <= current_cursor:
                    continue
                scanned_trade_entries.append((row_seq, normalized_row))
        alerts_rows: list[dict[str, Any]] = []
        alerts_total = 0
        for current_strategy_id in strategy_ids:
            strategy_alert_rows = self._store.load_alerts_rows(
                current_strategy_id,
                limit=self._alerts_preview_limit,
            )
            for row in strategy_alert_rows:
                normalized_row = dict(row)
                normalized_row.setdefault("strategy_id", current_strategy_id)
                alerts_rows.append(normalized_row)

            strategy_alert_total = self._store.alerts_stream_len(current_strategy_id)
            if strategy_alert_total is None:
                alerts_total += len(strategy_alert_rows)
            else:
                alerts_total += int(strategy_alert_total)
        alerts_rows.sort(
            key=lambda row: coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
            or 0,
            reverse=True,
        )
        alerts_rows = alerts_rows[: self._alerts_preview_limit]

        trades_rows: list[tuple[int, dict[str, Any]]] = []
        if not trade_gap and scanned_trade_entries:
            trades_rows.extend(scanned_trade_entries)
            trades_rows.sort(
                key=lambda item: (
                    coerce_ts_ms(
                        item[1].get("ts_ms") or item[1].get("ts") or item[1].get("timestamp")
                    )
                    or 0,
                    safe_int(item[1].get("seq")) or 0,
                    decode_text(item[1].get("strategy_id")).strip(),
                    decode_text(item[1].get("row_id")).strip(),
                ),
            )
            trades_rows = trades_rows[: self._trade_poll_limit]
        elif trade_gap and emit_legacy:
            # Force a per-profile Socket.IO seq gap so clients trigger REST resync per contract.
            _ = self._next_seq(profile)

        signal_changed_ids: list[str] = []
        signal_changes: list[dict[str, Any]] = []
        for current_strategy_id in strategy_ids:
            signal_payload = signal_payloads_by_strategy[current_strategy_id]
            previous_signal = previous_signals.get(current_strategy_id)
            signal_patch = build_signal_delta_patch(previous_signal, signal_payload)
            if signal_patch:
                signal_changes.append(
                    {
                        "strategy_id": current_strategy_id,
                        "patch": signal_patch,
                    },
                )
                if emit_legacy:
                    signal_event = {
                        "profile": profile,
                        "strategy_id": current_strategy_id,
                        "seq": self._next_seq(profile),
                        "server_ts_ms": now_ms(),
                        "patch": signal_patch,
                    }
                    self._record_metric("legacy_event_counts", "signal_delta")
                    self._socketio.emit("signal_delta", signal_event, to=room)
            if previous_signal != signal_payload:
                signal_changed_ids.append(current_strategy_id)

        for previous_strategy_id in previous_signals:
            if previous_strategy_id in signal_payloads_by_strategy:
                continue
            signal_changed_ids.append(previous_strategy_id)

        emitted_max_seq: dict[str, int] = {
            current_strategy_id: int(next_trade_cursors.get(current_strategy_id, 0))
            for current_strategy_id in strategy_ids
        }
        trade_changes: list[dict[str, Any]] = []
        for cursor, row in trades_rows:
            if not isinstance(row, Mapping):
                continue
            row_id = decode_text(row.get("row_id")).strip()
            if not row_id:
                continue
            row_strategy_id = decode_text(row.get("strategy_id")).strip() or strategy_id
            emitted_max_seq[row_strategy_id] = max(emitted_max_seq.get(row_strategy_id, 0), int(cursor))
            version = safe_int(row.get("version")) or 1
            operation = decode_text(row.get("op")).strip().lower() or "upsert"
            if operation == "delete":
                trade_changes.append(
                    {
                        "strategy_id": row_strategy_id,
                        "op": "delete",
                        "row_id": row_id,
                        "version": version,
                        "trade": None,
                    },
                )
                if emit_legacy:
                    payload = build_trade_update_payload(
                        profile=profile,
                        strategy_id=row_strategy_id,
                        seq=int(cursor),
                        op="delete",
                        row_id=row_id,
                        version=version,
                        trade=None,
                        server_ts_ms=now_ms(),
                    )
                    self._record_metric("legacy_event_counts", "trade_update")
                    self._socketio.emit("trade_update", payload, to=room)
                continue

            normalized = _normalize_trade_row(row, row_id=row_id, version=version)
            trade_changes.append(
                {
                    "strategy_id": row_strategy_id,
                    "op": "upsert",
                    "row_id": row_id,
                    "version": version,
                    "trade": _copy_mapping(normalized),
                },
            )
            if emit_legacy:
                payload = build_trade_update_payload(
                    profile=profile,
                    strategy_id=row_strategy_id,
                    seq=self._next_seq(profile),
                    op="upsert",
                    row_id=row_id,
                    version=version,
                    trade=normalized,
                    server_ts_ms=now_ms(),
                )
                self._record_metric("legacy_event_counts", "trade_update")
                self._socketio.emit("trade_update", payload, to=room)
        if not trade_gap:
            for current_strategy_id, seq in emitted_max_seq.items():
                next_trade_cursors[current_strategy_id] = max(
                    int(next_trade_cursors.get(current_strategy_id, 0)),
                    int(seq),
                )

        alerts_signature = _alerts_signature(alerts_rows, total_count=alerts_total)
        balances_signature = previous_balances_signature
        strategy_changed = bool(signal_changed_ids)
        alerts_changed = previous_alerts_signature != alerts_signature
        balances_changed = False
        if legacy_profile_active:
            balances_signature = self._load_canonical_balances_signature(
                profile,
                strategy_ids=strategy_ids,
            )
            balances_changed = (
                previous_balances_signature is not None
                and previous_balances_signature != balances_signature
            )
        if emit_legacy and (trade_gap or strategy_changed or alerts_changed or balances_changed):
            market_payload = {
                "profile": profile,
                "seq": self._next_seq(profile),
                "server_ts_ms": now_ms(),
                "server_time": datetime.now(tz=UTC)
                .isoformat(timespec="milliseconds")
                .replace("+00:00", "Z"),
                "strategies": {"changed": signal_changed_ids},
                "alerts": {
                    "count": alerts_signature[0],
                    "latest_ts_ms": alerts_signature[1],
                },
            }
            if trade_gap:
                market_payload["recovery"] = {"required": True, "reason": "trade_gap"}
            self._record_metric("legacy_event_counts", "market_update")
            self._socketio.emit("market_update", market_payload, to=room)

        if emit_standard:
            subscriptions = self._standard_subscriptions_for_profile(profile)
            if subscriptions:
                rollout = self._rollout_state()
                grouped_subscriptions: dict[str, list[FluxStandardSubscription]] = {}
                for subscription in subscriptions:
                    grouped_subscriptions.setdefault(subscription.surface, []).append(subscription)

                active_subscriptions: dict[str, list[FluxStandardSubscription]] = {}
                pending_unsubscribes: list[tuple[str, str]] = []
                for surface_name, surface_subscriptions in grouped_subscriptions.items():
                    withdrawal_reason = standard_active_withdrawal_reason(
                        surface=surface_name,
                        profile=profile,
                        rollout=rollout,
                    )
                    if withdrawal_reason is None:
                        active_subscriptions[surface_name] = surface_subscriptions
                        continue
                    withdrawal_seq = self._next_standard_seq(profile, surface_name)
                    for subscription in surface_subscriptions:
                        self._emit_standard_event(
                            subscription,
                            kind="recovery_required",
                            seq=withdrawal_seq,
                            reason=withdrawal_reason,
                            payload={},
                        )
                        pending_unsubscribes.append((subscription.sid, subscription.surface))

                signal_subscriptions = active_subscriptions.get("signal", [])
                if signal_subscriptions:
                    if signal_changes or alerts_changed:
                        signal_seq = self._next_standard_seq(profile, "signal")
                        payload: dict[str, Any] = {
                            "strategies": {"changed": list(signal_changed_ids)},
                        }
                        if signal_changes:
                            payload["signals"] = signal_changes
                        if alerts_changed:
                            payload["alerts"] = {
                                "count": alerts_signature[0],
                                "latest_ts_ms": alerts_signature[1],
                            }
                        for subscription in signal_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="delta_batch",
                                seq=signal_seq,
                                payload=payload,
                            )
                    else:
                        for subscription in signal_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="heartbeat",
                                seq=self.current_standard_seq(profile, "signal"),
                                payload={},
                            )

                alerts_subscriptions = active_subscriptions.get("alerts", [])
                if alerts_subscriptions:
                    if alerts_changed:
                        alerts_seq = self._next_standard_seq(profile, "alerts")
                        payload = {
                            "alerts": {
                                "count": alerts_signature[0],
                                "latest_ts_ms": alerts_signature[1],
                            },
                        }
                        for subscription in alerts_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="invalidate",
                                seq=alerts_seq,
                                payload=payload,
                            )
                    else:
                        for subscription in alerts_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="heartbeat",
                                seq=self.current_standard_seq(profile, "alerts"),
                                payload={},
                            )

                balances_subscriptions = active_subscriptions.get("balances", [])
                if balances_subscriptions:
                    balances_signature = self._load_canonical_balances_signature(
                        profile,
                        strategy_ids=strategy_ids,
                    )
                    balances_changed = (
                        previous_balances_signature is not None
                        and previous_balances_signature != balances_signature
                    )
                    if balances_changed:
                        balances_seq = self._next_standard_seq(profile, "balances")
                        for subscription in balances_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="invalidate",
                                seq=balances_seq,
                                payload={},
                            )
                    else:
                        for subscription in balances_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="heartbeat",
                                seq=self.current_standard_seq(profile, "balances"),
                                payload={},
                            )
                else:
                    balances_signature = None

                trades_subscriptions = active_subscriptions.get("trades", [])
                if trades_subscriptions:
                    if trade_gap:
                        trade_gap_seq = self._next_standard_seq(profile, "trades")
                        for subscription in trades_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="recovery_required",
                                seq=trade_gap_seq,
                                reason="trade_gap",
                                payload={},
                            )
                            pending_unsubscribes.append((subscription.sid, subscription.surface))
                    elif trade_changes:
                        trades_seq = self._next_standard_seq(profile, "trades")
                        payload = {"trades": trade_changes}
                        for subscription in trades_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="delta_batch",
                                seq=trades_seq,
                                payload=payload,
                            )
                    else:
                        for subscription in trades_subscriptions:
                            self._emit_standard_event(
                                subscription,
                                kind="heartbeat",
                                seq=self.current_standard_seq(profile, "trades"),
                                payload={},
                            )

                for sid, surface_name in pending_unsubscribes:
                    self.unsubscribe_standard(sid, surface=surface_name)

        with self._lock:
            if self._profile_refcounts.get(profile, 0) <= 0:
                self._cleanup_profile_state_locked(profile)
                return
            self._signal_by_profile[profile] = {
                key: _copy_mapping(value) for key, value in signal_payloads_by_strategy.items()
            }
            self._trade_cursor_by_profile[profile] = {
                current_strategy_id: int(next_trade_cursors.get(current_strategy_id, 0))
                for current_strategy_id in strategy_ids
            }
            self._alerts_by_profile[profile] = alerts_signature
            if balances_signature is None:
                self._balances_by_profile.pop(profile, None)
            else:
                self._balances_by_profile[profile] = balances_signature


@dataclass(frozen=True)
class FluxSocketServer:
    socketio: SocketIO
    emitter: FluxSocketEmitter


def _socketio_path(path: str) -> str:
    path_value = decode_text(path).strip()
    if not path_value:
        return "socket.io"
    return path_value.lstrip("/")


def create_flux_socket_server(  # noqa: C901
    app: Flask,
    *,
    store: FluxSocketStoreProtocol,
    metadata_resolver: Callable[[str], Any],
    strategy_resolver: Callable[[str], str | None],
    strategy_ids_resolver: Callable[[str], Sequence[str]] | None = None,
    required_strategy_ids_resolver: Callable[..., Sequence[str]] | None = None,
    path: str = SOCKETIO_DEFAULT_PATH,
    poll_interval_s: float = SOCKETIO_DEFAULT_POLL_INTERVAL_S,
) -> FluxSocketServer:
    """
    Attach Socket.IO handlers and tokenmm emitter to a Flask app.
    """
    socketio = SocketIO(
        app,
        async_mode="threading",
        path=_socketio_path(path),
        logger=False,
        engineio_logger=False,
        allow_upgrades=False,  # Keep long-polling as the default transport.
    )

    def _realtime_rollout_state() -> Mapping[str, Any]:
        raw_value = app.extensions.get("flux_realtime_rollout")
        if isinstance(raw_value, Mapping):
            return raw_value
        return default_realtime_rollout()

    emitter = FluxSocketEmitter(
        socketio=socketio,
        store=store,
        metadata_resolver=metadata_resolver,
        strategy_resolver=strategy_resolver,
        strategy_ids_resolver=strategy_ids_resolver,
        required_strategy_ids_resolver=required_strategy_ids_resolver,
        realtime_rollout_resolver=_realtime_rollout_state,
        poll_interval_s=poll_interval_s,
    )

    def _profile_supported(profile: str) -> bool:
        if strategy_ids_resolver is not None:
            raw_values = strategy_ids_resolver(profile)
            if isinstance(raw_values, Sequence) and not isinstance(raw_values, str | bytes):
                for value in raw_values:
                    if decode_text(value).strip():
                        return True
            elif raw_values is not None and decode_text(raw_values).strip():
                return True
        return strategy_resolver(profile) is not None

    sid_profiles: dict[str, str] = {}
    sid_lock = RLock()

    @socketio.on("connect")
    def _on_connect(auth: Any = None) -> bool:
        _ = auth
        profile = normalize_profile(request.args.get("profile"))
        if not profile:
            return True
        if not _profile_supported(profile):
            return True
        with sid_lock:
            sid_profiles[request.sid] = profile
        join_room(profile_room(profile))
        emitter.acquire_legacy_profile(profile)
        emitter.start()
        return True

    @socketio.on("disconnect")
    def _on_disconnect() -> None:
        emitter.unsubscribe_standard(request.sid)
        with sid_lock:
            profile = sid_profiles.pop(request.sid, "")
        if profile:
            leave_room(profile_room(profile))
            emitter.release_legacy_profile(profile)

    @socketio.on("subscribe")
    def _on_subscribe(payload: Any) -> dict[str, Any]:
        if not isinstance(payload, Mapping):
            return {
                "accepted": False,
                "contract_version": REALTIME_STANDARD_CONTRACT_VERSION,
                "reason": "invalid_request",
            }

        contract_version = safe_int(payload.get("contract_version")) or 0
        ack = emitter.subscribe_standard(
            request.sid,
            contract_version=int(contract_version),
            surface=payload.get("surface"),
            profile=payload.get("profile"),
            surface_query_key=payload.get("surface_query_key"),
            stream_id=payload.get("stream_id"),
            snapshot_revision=payload.get("snapshot_revision"),
            resume_from_seq=payload.get("resume_from_seq"),
        )
        if ack.get("accepted") is True:
            emitter.start()
        return ack

    @socketio.on("unsubscribe")
    def _on_unsubscribe(payload: Any = None) -> dict[str, Any]:
        surface = ""
        if isinstance(payload, Mapping):
            surface = normalize_surface(payload.get("surface"))
        elif payload is not None:
            surface = normalize_surface(payload)
        emitter.unsubscribe_standard(request.sid, surface=surface or None)
        return {
            "ok": True,
            "surface": surface or None,
        }

    @socketio.on("set_profile")
    def _on_set_profile(payload: Any) -> dict[str, Any]:
        next_profile = ""
        if isinstance(payload, Mapping):
            next_profile = normalize_profile(payload.get("profile"))
        else:
            next_profile = normalize_profile(payload)

        previous_profile = ""
        with sid_lock:
            previous_profile = sid_profiles.get(request.sid, "")
        if previous_profile and previous_profile == next_profile:
            return {
                "ok": True,
                "profile": next_profile,
                "room": profile_room(next_profile) if next_profile else None,
            }
        emitter.unsubscribe_standard(request.sid)
        with sid_lock:
            previous_profile = sid_profiles.pop(request.sid, "")
        if previous_profile:
            leave_room(profile_room(previous_profile))
            emitter.release_legacy_profile(previous_profile)
        if not next_profile:
            return {
                "ok": True,
                "profile": "",
                "room": None,
            }

        if not _profile_supported(next_profile):
            return {
                "ok": False,
                "profile": "",
                "room": None,
                "error": {
                    "code": "unsupported_profile",
                    "requested_profile": next_profile,
                },
            }

        with sid_lock:
            sid_profiles[request.sid] = next_profile
        join_room(profile_room(next_profile))
        emitter.acquire_legacy_profile(next_profile)

        if next_profile:
            emitter.start()

        return {
            "ok": True,
            "profile": next_profile,
            "room": profile_room(next_profile) if next_profile else None,
        }

    app.extensions["flux_socketio"] = socketio
    app.extensions["flux_socketio_server"] = socketio.server
    app.extensions["flux_socket_emitter"] = emitter
    app.extensions["flux_socketio_state"] = sid_profiles
    app.extensions["flux_realtime_metrics"] = emitter.metrics

    server = FluxSocketServer(socketio=socketio, emitter=emitter)
    app.extensions["flux_socket_server"] = server
    return server


__all__ = [
    "REALTIME_STANDARD_CONTRACT_VERSION",
    "REALTIME_STANDARD_EVENT",
    "SOCKETIO_ALERTS_PREVIEW_LIMIT",
    "SOCKETIO_DEFAULT_PATH",
    "SOCKETIO_DEFAULT_POLL_INTERVAL_S",
    "SOCKETIO_TRADE_POLL_LIMIT",
    "SOCKETIO_TRADE_SCAN_LIMIT",
    "FluxSocketEmitter",
    "FluxSocketServer",
    "apply_signal_delta_patch",
    "build_standard_capabilities",
    "build_standard_snapshot_metadata",
    "build_standard_stream_id",
    "build_standard_surface_query_key",
    "build_signal_delta_patch",
    "build_stable_signal_view",
    "build_trade_update_payload",
    "create_flux_socket_server",
    "default_realtime_rollout",
    "normalize_profile",
    "normalize_surface",
    "profile_room",
    "standard_active_withdrawal_reason",
    "standard_subscribe_rejection_reason",
    "supported_profile_ids",
]
