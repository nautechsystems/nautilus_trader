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

import time
from collections import OrderedDict
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

import msgspec

from nautilus_trader.common.config import msgspec_encoding_hook


DECISION_CONTEXT_JSON_DEFAULT_LITERAL = "null"

PLACE_INTENT_TYPE = "PLACE"
CANCEL_INTENT_TYPE = "CANCEL"

PLACE_EVENT_TYPES = frozenset(
    (
        "OrderInitialized",
        "OrderSubmitted",
        "OrderAccepted",
        "OrderRejected",
    ),
)
CANCEL_EVENT_TYPES = frozenset(
    (
        "OrderPendingCancel",
        "OrderCanceled",
        "OrderCancelRejected",
    ),
)


def current_ts_ns(clock: object | None) -> int:
    if clock is None:
        return time.time_ns()
    return int(clock.timestamp_ns())  # type: ignore[no-any-return]


def encode_json_literal(
    value: Any,
    *,
    fallback: str = DECISION_CONTEXT_JSON_DEFAULT_LITERAL,
) -> str:
    if value is None:
        return fallback
    try:
        return msgspec.json.encode(value, enc_hook=msgspec_encoding_hook).decode("utf-8")
    except Exception:
        return fallback


def iter_json_payload_mappings(msg: object) -> list[dict[str, Any]]:
    if isinstance(msg, Mapping):
        return [dict(msg)]

    payload_attr = getattr(msg, "payload", None)
    if isinstance(payload_attr, str):
        raw_payload: object = payload_attr
    else:
        raw_payload = msg

    if isinstance(raw_payload, str):
        try:
            decoded = msgspec.json.decode(raw_payload.encode("utf-8"))
        except Exception:
            return []
    else:
        decoded = raw_payload

    if isinstance(decoded, Mapping):
        return [dict(decoded)]

    if isinstance(decoded, list):
        rows: list[dict[str, Any]] = []
        for item in decoded:
            if isinstance(item, Mapping):
                rows.append(dict(item))
        return rows

    return []


@dataclass(frozen=True, slots=True)
class ActionIntentRecord:
    strategy_id: str
    client_order_id: str
    intent_type: str
    run_id: str | None
    quote_cycle_id: str | None
    reason_code: str | None
    level_index: int | None
    target_px: str | None
    cancel_px: str | None
    match_tol: str | None
    ts_market_data_event_ns: int | None
    ts_market_data_recv_ns: int | None
    ts_decision_ns: int | None
    ts_submit_local_ns: int | None
    ts_cancel_request_local_ns: int | None
    decision_context_json: str

    @classmethod
    def from_payload(cls, payload: Mapping[str, Any]) -> ActionIntentRecord | None:
        strategy_id = str(payload.get("strategy_id") or "").strip()
        client_order_id = str(payload.get("client_order_id") or "").strip()
        intent_type = str(payload.get("intent_type") or "").strip().upper()
        if not strategy_id or not client_order_id or intent_type not in {PLACE_INTENT_TYPE, CANCEL_INTENT_TYPE}:
            return None

        return cls(
            strategy_id=strategy_id,
            client_order_id=client_order_id,
            intent_type=intent_type,
            run_id=_optional_text(payload.get("run_id")),
            quote_cycle_id=_optional_text(payload.get("quote_cycle_id")),
            reason_code=_optional_text(payload.get("reason_code")),
            level_index=_optional_int(payload.get("level_index")),
            target_px=_optional_text(payload.get("target_px")),
            cancel_px=_optional_text(payload.get("cancel_px")),
            match_tol=_optional_text(payload.get("match_tol")),
            ts_market_data_event_ns=_optional_int(payload.get("ts_market_data_event_ns")),
            ts_market_data_recv_ns=_optional_int(payload.get("ts_market_data_recv_ns")),
            ts_decision_ns=_optional_int(payload.get("ts_decision_ns")),
            ts_submit_local_ns=_optional_int(payload.get("ts_submit_local_ns")),
            ts_cancel_request_local_ns=_optional_int(payload.get("ts_cancel_request_local_ns")),
            decision_context_json=encode_json_literal(payload.get("decision_context_json")),
        )


def intent_type_for_order_event(event_type: str) -> str | None:
    if event_type in PLACE_EVENT_TYPES:
        return PLACE_INTENT_TYPE
    if event_type in CANCEL_EVENT_TYPES:
        return CANCEL_INTENT_TYPE
    return None


def should_evict_intent_for_order_event(event_type: str) -> bool:
    return bool(intent_types_to_evict_for_order_event(event_type))


def intent_types_to_evict_for_order_event(event_type: str) -> tuple[str, ...]:
    if event_type == "OrderRejected":
        return (PLACE_INTENT_TYPE,)
    if event_type == "OrderCanceled":
        return (PLACE_INTENT_TYPE, CANCEL_INTENT_TYPE)
    if event_type == "OrderCancelRejected":
        return (CANCEL_INTENT_TYPE,)
    return ()


class ActionIntentCache:
    def __init__(
        self,
        *,
        max_entries: int = 50_000,
        ttl_ns: int = 24 * 60 * 60 * 1_000_000_000,
    ) -> None:
        self._max_entries = max(1, int(max_entries))
        self._ttl_ns = max(1, int(ttl_ns))
        self._entries: OrderedDict[tuple[str, str, str], tuple[ActionIntentRecord, int]] = OrderedDict()

    def add(self, intent: ActionIntentRecord, *, now_ns: int) -> None:
        self.prune(now_ns=now_ns)
        key = (intent.strategy_id, intent.client_order_id, intent.intent_type)
        self._entries.pop(key, None)
        self._entries[key] = (intent, now_ns + self._ttl_ns)
        while len(self._entries) > self._max_entries:
            self._entries.popitem(last=False)

    def get(
        self,
        *,
        client_order_id: str,
        intent_type: str,
        strategy_id: str | None = None,
        now_ns: int,
    ) -> ActionIntentRecord | None:
        if strategy_id is not None:
            key = (strategy_id, client_order_id, intent_type)
            cached = self._entries.get(key)
            if cached is None:
                return None
            intent, expires_at_ns = cached
            if expires_at_ns <= now_ns:
                self._entries.pop(key, None)
                return None
            return intent

        for key, (intent, expires_at_ns) in reversed(self._entries.items()):
            if intent.client_order_id != client_order_id or intent.intent_type != intent_type:
                continue
            if expires_at_ns <= now_ns:
                self._entries.pop(key, None)
                continue
            return intent
        return None

    def evict(self, *, client_order_id: str, strategy_id: str | None = None) -> None:
        self.evict_types(
            client_order_id=client_order_id,
            strategy_id=strategy_id,
            intent_types=(PLACE_INTENT_TYPE, CANCEL_INTENT_TYPE),
        )

    def evict_types(
        self,
        *,
        client_order_id: str,
        intent_types: tuple[str, ...],
        strategy_id: str | None = None,
    ) -> None:
        if strategy_id is not None:
            for intent_type in intent_types:
                self._entries.pop((strategy_id, client_order_id, intent_type), None)
            return

        for key in tuple(self._entries):
            if key[1] == client_order_id and key[2] in intent_types:
                self._entries.pop(key, None)

    def prune(self, *, now_ns: int) -> None:
        while self._entries:
            _, (_, expires_at_ns) = next(iter(self._entries.items()))
            if expires_at_ns > now_ns:
                break
            self._entries.popitem(last=False)

    def clear(self) -> None:
        self._entries.clear()


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _optional_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None
