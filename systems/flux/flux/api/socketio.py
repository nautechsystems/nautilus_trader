from __future__ import annotations

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

from flux.api.payloads import coerce_ts_ms
from flux.api.payloads import decode_text
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

_LOG = logging.getLogger(__name__)


class FluxSocketStoreProtocol(Protocol):
    def load_signals_payload(self, strategy_id: str, metadata: Any) -> dict[str, Any]: ...

    def load_trades_rows(
        self,
        strategy_id: str,
        *,
        limit: int,
        since_ms: int | None,
        since_seq: int | None = None,
        scan_limit: int | None = None,
    ) -> list[dict[str, Any]]: ...

    def load_alerts_rows(self, strategy_id: str, *, limit: int) -> list[dict[str, Any]]: ...

    def alerts_stream_len(self, strategy_id: str) -> int | None: ...


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
        poll_interval_s: float = SOCKETIO_DEFAULT_POLL_INTERVAL_S,
    ) -> None:
        self._socketio = socketio
        self._store = store
        self._metadata_resolver = metadata_resolver
        self._strategy_resolver = strategy_resolver
        self._strategy_ids_resolver = strategy_ids_resolver
        self._poll_interval_s = max(0.25, float(poll_interval_s))
        self._lock = RLock()
        self._running = False
        self._thread: Thread | None = None
        self._wake_event = Event()
        self._profile_refcounts: dict[str, int] = {}
        self._seq_by_profile: dict[str, int] = {}
        self._signal_by_profile: dict[str, dict[str, dict[str, Any]]] = {}
        self._trade_cursor_by_profile: dict[str, dict[str, int]] = {}
        self._alerts_by_profile: dict[str, tuple[int, int | None, str]] = {}
        self._failure_streak_by_profile: dict[str, int] = {}
        self._backoff_until_by_profile: dict[str, float] = {}
        self._trade_poll_limit = SOCKETIO_TRADE_POLL_LIMIT
        self._trade_scan_limit = SOCKETIO_TRADE_SCAN_LIMIT
        self._alerts_preview_limit = SOCKETIO_ALERTS_PREVIEW_LIMIT

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

    def _active_profiles(self) -> list[str]:
        with self._lock:
            return sorted(
                profile for profile, count in self._profile_refcounts.items() if count > 0
            )

    def _cleanup_profile_state_locked(self, profile: str) -> None:
        self._seq_by_profile.pop(profile, None)
        self._signal_by_profile.pop(profile, None)
        self._trade_cursor_by_profile.pop(profile, None)
        self._alerts_by_profile.pop(profile, None)
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
    ) -> None:
        signal_payloads_by_strategy: dict[str, dict[str, Any]] = {}
        for current_strategy_id in strategy_ids:
            metadata = self._metadata_resolver(current_strategy_id)
            signal_payloads_by_strategy[current_strategy_id] = build_stable_signal_view(
                self._store.load_signals_payload(current_strategy_id, metadata),
            )

        with self._lock:
            trade_cursors = dict(self._trade_cursor_by_profile.get(profile, {}))
            previous_signals = self._signal_by_profile.get(profile, {})
            previous_alerts_signature = self._alerts_by_profile.get(profile)

        next_trade_cursors = dict(trade_cursors)
        for current_strategy_id in strategy_ids:
            next_trade_cursors[current_strategy_id] = int(next_trade_cursors.get(current_strategy_id, 0))

        scanned_trade_entries: list[tuple[int, dict[str, Any]]] = []
        trade_gap = False
        for current_strategy_id in strategy_ids:
            strategy_rows = self._store.load_trades_rows(
                current_strategy_id,
                limit=self._trade_scan_limit,
                since_ms=None,
                since_seq=None,
                scan_limit=self._trade_scan_limit,
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
        elif trade_gap:
            # Force a per-profile Socket.IO seq gap so clients trigger REST resync per contract.
            _ = self._next_seq(profile)

        signal_changed_ids: list[str] = []
        for current_strategy_id in strategy_ids:
            signal_payload = signal_payloads_by_strategy[current_strategy_id]
            previous_signal = previous_signals.get(current_strategy_id)
            signal_patch = build_signal_delta_patch(previous_signal, signal_payload)
            if signal_patch:
                signal_event = {
                    "profile": profile,
                    "strategy_id": current_strategy_id,
                    "seq": self._next_seq(profile),
                    "server_ts_ms": now_ms(),
                    "patch": signal_patch,
                }
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
                self._socketio.emit("trade_update", payload, to=room)
                continue

            normalized = _normalize_trade_row(row, row_id=row_id, version=version)
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
            self._socketio.emit("trade_update", payload, to=room)
        if not trade_gap:
            for current_strategy_id, seq in emitted_max_seq.items():
                next_trade_cursors[current_strategy_id] = max(
                    int(next_trade_cursors.get(current_strategy_id, 0)),
                    int(seq),
                )

        alerts_signature = _alerts_signature(alerts_rows, total_count=alerts_total)
        strategy_changed = bool(signal_changed_ids)
        alerts_changed = previous_alerts_signature != alerts_signature
        if trade_gap or strategy_changed or alerts_changed:
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
            self._socketio.emit("market_update", market_payload, to=room)

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
    emitter = FluxSocketEmitter(
        socketio=socketio,
        store=store,
        metadata_resolver=metadata_resolver,
        strategy_resolver=strategy_resolver,
        strategy_ids_resolver=strategy_ids_resolver,
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
        emitter.acquire_profile(profile)
        emitter.start()
        return True

    @socketio.on("disconnect")
    def _on_disconnect() -> None:
        with sid_lock:
            profile = sid_profiles.pop(request.sid, "")
        if profile:
            leave_room(profile_room(profile))
            emitter.release_profile(profile)

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
        with sid_lock:
            previous_profile = sid_profiles.pop(request.sid, "")
        if previous_profile:
            leave_room(profile_room(previous_profile))
            emitter.release_profile(previous_profile)
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
        emitter.acquire_profile(next_profile)

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

    server = FluxSocketServer(socketio=socketio, emitter=emitter)
    app.extensions["flux_socket_server"] = server
    return server


__all__ = [
    "SOCKETIO_ALERTS_PREVIEW_LIMIT",
    "SOCKETIO_DEFAULT_PATH",
    "SOCKETIO_DEFAULT_POLL_INTERVAL_S",
    "SOCKETIO_TRADE_POLL_LIMIT",
    "SOCKETIO_TRADE_SCAN_LIMIT",
    "FluxSocketEmitter",
    "FluxSocketServer",
    "apply_signal_delta_patch",
    "build_signal_delta_patch",
    "build_stable_signal_view",
    "build_trade_update_payload",
    "create_flux_socket_server",
    "normalize_profile",
    "profile_room",
    "supported_profile_ids",
]
