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

from collections.abc import Callable
from collections.abc import Mapping
from collections.abc import Sequence
from contextlib import suppress
from copy import deepcopy
from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from threading import RLock
from threading import Thread
from typing import Any
from typing import Protocol
from typing import cast
from uuid import uuid4

from flask import Flask
from flask import request
from flask_socketio import SocketIO
from flask_socketio import join_room
from flask_socketio import leave_room

from nautilus_trader.flux.api.payloads import coerce_ts_ms
from nautilus_trader.flux.api.payloads import decode_text
from nautilus_trader.flux.api.payloads import now_ms
from nautilus_trader.flux.api.payloads import safe_int


TOKENMM_PROFILE_ALIASES: frozenset[str] = frozenset({"tokenm", "tokenmm"})
SOCKETIO_DEFAULT_PATH = "/socket.io"
SOCKETIO_DEFAULT_POLL_INTERVAL_S = 0.75
SOCKETIO_TRADE_POLL_LIMIT = 200
SOCKETIO_TRADE_SCAN_LIMIT = 2_000
SOCKETIO_ALERTS_PREVIEW_LIMIT = 25


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
    text = decode_text(profile).strip().lower()
    if text in TOKENMM_PROFILE_ALIASES:
        return "tokenmm"
    return text


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
        poll_interval_s: float = SOCKETIO_DEFAULT_POLL_INTERVAL_S,
    ) -> None:
        self._socketio = socketio
        self._store = store
        self._metadata_resolver = metadata_resolver
        self._strategy_resolver = strategy_resolver
        self._poll_interval_s = max(0.25, float(poll_interval_s))
        self._lock = RLock()
        self._running = False
        self._thread: Thread | None = None
        self._seq_by_profile: dict[str, int] = {}
        self._signal_by_profile: dict[str, dict[str, Any]] = {}
        self._trade_cursor_by_profile: dict[str, int] = {}
        self._alerts_by_profile: dict[str, tuple[int, int | None, str]] = {}
        self._trade_poll_limit = SOCKETIO_TRADE_POLL_LIMIT
        self._trade_scan_limit = SOCKETIO_TRADE_SCAN_LIMIT
        self._alerts_preview_limit = SOCKETIO_ALERTS_PREVIEW_LIMIT

    def start(self) -> None:
        with self._lock:
            if self._running:
                return
            self._running = True
            self._thread = Thread(target=self._run_loop, name=f"flux-socket-emitter-{uuid4().hex[:8]}", daemon=True)
            self._thread.start()

    def stop(self) -> None:
        with self._lock:
            self._running = False
            thread = self._thread
            self._thread = None
        if thread is not None and thread.is_alive():
            thread.join(timeout=self._poll_interval_s * 2.0)

    def emit_once(self, *, profile: str | None = None) -> None:
        if profile is None:
            profiles = self._active_profiles()
        else:
            normalized = normalize_profile(profile)
            profiles = [normalized] if normalized else []

        for current_profile in profiles:
            self._emit_profile(current_profile)

    def _run_loop(self) -> None:
        while True:
            with self._lock:
                running = self._running
            if not running:
                return
            # Keep emitter resilient; REST remains authoritative.
            with suppress(Exception):
                self.emit_once()
            self._socketio.sleep(self._poll_interval_s)

    def _active_profiles(self) -> list[str]:
        manager_rooms = self._socketio.server.manager.rooms.get("/", {})
        out: list[str] = []
        for room_name, members in manager_rooms.items():
            if not isinstance(room_name, str) or not room_name.startswith("profile:"):
                continue
            if members is None:
                continue
            if len(members) <= 0:
                continue
            profile = normalize_profile(room_name.split("profile:", maxsplit=1)[1])
            if profile:
                out.append(profile)
        return sorted(set(out))

    def _next_seq(self, profile: str) -> int:
        with self._lock:
            seq = self._seq_by_profile.get(profile, 0) + 1
            self._seq_by_profile[profile] = seq
            return seq

    def _emit_profile(self, profile: str) -> None:
        strategy_id = self._strategy_resolver(profile)
        if not strategy_id:
            return
        metadata = self._metadata_resolver(strategy_id)
        signal_payload = build_stable_signal_view(self._store.load_signals_payload(strategy_id, metadata))

        trade_cursor = self._trade_cursor_by_profile.get(profile, 0)
        trades_rows = self._store.load_trades_rows(
            strategy_id,
            limit=self._trade_poll_limit,
            since_ms=None,
            since_seq=trade_cursor,
            scan_limit=self._trade_scan_limit,
        )
        alerts_rows = self._store.load_alerts_rows(strategy_id, limit=self._alerts_preview_limit)
        alerts_total = self._store.alerts_stream_len(strategy_id)

        previous_signal = self._signal_by_profile.get(profile)
        previous_alerts_signature = self._alerts_by_profile.get(profile)

        signal_patch = build_signal_delta_patch(previous_signal, signal_payload)
        if signal_patch:
            signal_event = {
                "profile": profile,
                "strategy_id": strategy_id,
                "seq": self._next_seq(profile),
                "server_ts_ms": now_ms(),
                "patch": signal_patch,
            }
            self._socketio.emit("signal_delta", signal_event, to=profile_room(profile))

        latest_trade_seq = trade_cursor
        for row in trades_rows:
            if not isinstance(row, Mapping):
                continue
            row_seq = safe_int(row.get("seq"))
            if row_seq is None:
                continue
            latest_trade_seq = max(latest_trade_seq, row_seq)
            row_id = decode_text(row.get("row_id")).strip()
            if not row_id:
                continue
            version = safe_int(row.get("version")) or 1
            operation = decode_text(row.get("op")).strip().lower() or "upsert"
            if operation == "delete":
                payload = build_trade_update_payload(
                    profile=profile,
                    strategy_id=strategy_id,
                    seq=self._next_seq(profile),
                    op="delete",
                    row_id=row_id,
                    version=version,
                    trade=None,
                    server_ts_ms=now_ms(),
                )
                self._socketio.emit("trade_update", payload, to=profile_room(profile))
                continue

            normalized = _normalize_trade_row(row, row_id=row_id, version=version)
            payload = build_trade_update_payload(
                profile=profile,
                strategy_id=strategy_id,
                seq=self._next_seq(profile),
                op="upsert",
                row_id=row_id,
                version=version,
                trade=normalized,
                server_ts_ms=now_ms(),
            )
            self._socketio.emit("trade_update", payload, to=profile_room(profile))

        alerts_signature = _alerts_signature(alerts_rows, total_count=alerts_total)
        strategy_changed = previous_signal != signal_payload
        alerts_changed = previous_alerts_signature != alerts_signature
        if strategy_changed or alerts_changed:
            market_payload = {
                "profile": profile,
                "seq": self._next_seq(profile),
                "server_ts_ms": now_ms(),
                "server_time": datetime.now(tz=UTC).isoformat(timespec="milliseconds").replace("+00:00", "Z"),
                "strategies": {"changed": [strategy_id] if strategy_changed else []},
                "alerts": {
                    "count": alerts_signature[0],
                    "latest_ts_ms": alerts_signature[1],
                },
            }
            self._socketio.emit("market_update", market_payload, to=profile_room(profile))

        self._signal_by_profile[profile] = _copy_mapping(signal_payload)
        self._trade_cursor_by_profile[profile] = latest_trade_seq
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
        poll_interval_s=poll_interval_s,
    )

    sid_profiles: dict[str, str] = {}
    sid_lock = RLock()

    @socketio.on("connect")
    def _on_connect(auth: Any = None) -> bool:
        _ = auth
        profile = normalize_profile(request.args.get("profile"))
        if not profile:
            return True
        if strategy_resolver(profile) is None:
            return True
        with sid_lock:
            sid_profiles[request.sid] = profile
        join_room(profile_room(profile))
        emitter.start()
        return True

    @socketio.on("disconnect")
    def _on_disconnect() -> None:
        with sid_lock:
            profile = sid_profiles.pop(request.sid, "")
        if profile:
            leave_room(profile_room(profile))

    @socketio.on("set_profile")
    def _on_set_profile(payload: Any) -> dict[str, Any]:
        next_profile = ""
        if isinstance(payload, Mapping):
            next_profile = normalize_profile(payload.get("profile"))
        else:
            next_profile = normalize_profile(payload)

        previous_profile = ""
        with sid_lock:
            previous_profile = sid_profiles.pop(request.sid, "")
            if previous_profile:
                leave_room(profile_room(previous_profile))
        if not next_profile:
            return {
                "ok": True,
                "profile": "",
                "room": None,
            }

        if strategy_resolver(next_profile) is None:
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
]
