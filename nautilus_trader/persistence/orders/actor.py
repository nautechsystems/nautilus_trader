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

import copy
import sqlite3
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

import msgspec

from nautilus_trader.persistence._action_intent import ActionIntentCache
from nautilus_trader.persistence._action_intent import ActionIntentRecord
from nautilus_trader.persistence._action_intent import current_ts_ns
from nautilus_trader.persistence._action_intent import DECISION_CONTEXT_JSON_DEFAULT_LITERAL
from nautilus_trader.persistence._action_intent import intent_type_for_order_event
from nautilus_trader.persistence._action_intent import intent_types_to_evict_for_order_event
from nautilus_trader.persistence._action_intent import iter_json_payload_mappings
from nautilus_trader.persistence._operator_quantity import OperatorQuantitySnapshot
from nautilus_trader.persistence._operator_quantity import snapshot_operator_quantity
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.events import OrderEvent
from nautilus_trader.persistence._async_sqlite import _AsyncSQLitePersistenceActor
from nautilus_trader.persistence._async_sqlite import writer_startup_timeout_seconds
from nautilus_trader.persistence._execution_timing import ExecutionTimingCache
from nautilus_trader.persistence._execution_timing import ExecutionTimingRecord
from nautilus_trader.persistence._execution_timing import iter_execution_timing_records
from nautilus_trader.persistence.orders.config import OrderActionPersistenceActorConfig
from nautilus_trader.persistence.orders.sqlite import OrderActionRow
from nautilus_trader.persistence.orders.sqlite import connect
from nautilus_trader.persistence.orders.sqlite import ensure_schema
from nautilus_trader.persistence.orders.sqlite import insert_many

_ACTION_MAP: dict[str, tuple[str, str]] = {
    "OrderInitialized": ("PLACE", "INITIALIZED"),
    "OrderSubmitted": ("PLACE", "SUBMITTED"),
    "OrderAccepted": ("PLACE", "ACCEPTED"),
    "OrderRejected": ("PLACE", "REJECTED"),
    "OrderPendingCancel": ("CANCEL", "REQUESTED"),
    "OrderCanceled": ("CANCEL", "COMPLETED"),
    "OrderCancelRejected": ("CANCEL", "REJECTED"),
}


def _writer_startup_timeout_seconds(config: OrderActionPersistenceActorConfig) -> float:
    return writer_startup_timeout_seconds(config)


def _current_ts_ingest_ns(clock: object | None) -> int:
    return current_ts_ns(clock)


def _extract_order_px(options: object) -> str | None:
    if not isinstance(options, dict):
        return None

    for key in ("price", "trigger_price", "activation_price"):
        value = options.get(key)
        if value is not None:
            return str(value)

    return None


def _optional_text(value: object) -> str | None:
    if value is None:
        return None
    return str(value)


def _encode_payload_json(
    payload: dict[str, Any],
    on_payload_encode_error: Callable[[], None] | None = None,
) -> str:
    try:
        return msgspec.json.encode(payload, enc_hook=msgspec_encoding_hook).decode("utf-8")
    except Exception:
        if on_payload_encode_error is not None:
            on_payload_encode_error()
        return "{}"


def _snapshot_payload_data(data: dict[str, Any]) -> dict[str, Any]:
    snapped = dict(data)
    for key, value in tuple(snapped.items()):
        if isinstance(value, (dict, list)):
            snapped[key] = copy.deepcopy(value)
    return snapped


def _parse_intent_tags(tags: object) -> tuple[str | None, str | None, int | None, str]:
    action_id: str | None = None
    action_reason: str | None = None
    ts_decision_ns: int | None = None
    decision_context_json = DECISION_CONTEXT_JSON_DEFAULT_LITERAL

    if not isinstance(tags, list):
        return action_id, action_reason, ts_decision_ns, decision_context_json

    for tag in tags:
        if not isinstance(tag, str):
            continue
        key, has_sep, value = tag.partition("=")
        if not has_sep:
            continue
        value = value.strip()
        if key == "nautilus.intent.action_id":
            action_id = value or None
        elif key == "nautilus.intent.reason":
            action_reason = value or None
        elif key == "nautilus.intent.ts_decision_ns":
            try:
                ts_decision_ns = int(value)
            except ValueError:
                ts_decision_ns = None
        elif key == "nautilus.intent.signal":
            if not value:
                continue
            try:
                decoded = msgspec.json.decode(value.encode("utf-8"))
            except Exception:
                decision_context_json = msgspec.json.encode(value).decode("utf-8")
            else:
                decision_context_json = msgspec.json.encode(decoded).decode("utf-8")

    return action_id, action_reason, ts_decision_ns, decision_context_json


def order_event_to_row(
    event: OrderEvent | dict[str, Any],
    *,
    event_type: str | None = None,
    ts_ingest: int,
    action_intent: ActionIntentRecord | None = None,
    execution_timing: ExecutionTimingRecord | None = None,
    order_qty_base: str | None = None,
    order_qty_venue: str | None = None,
    qty_conversion_status: str | None = None,
    qty_conversion_source: str | None = None,
    on_payload_encode_error: Callable[[], None] | None = None,
) -> OrderActionRow | None:
    """
    Convert supported order lifecycle events to a primitive order action row.
    """
    if event_type is None and not isinstance(event, dict):
        event_type = type(event).__name__
    action_fields = _ACTION_MAP.get(event_type)
    if action_fields is None:
        return None

    if isinstance(event, dict):
        data = event
    else:
        data = _snapshot_payload_data(event.to_dict(event))
    action_type, action_state = action_fields

    action_id: str | None = None
    action_reason: str | None = None
    run_id: str | None = None
    quote_cycle_id: str | None = None
    reason_code: str | None = None
    level_index: int | None = None
    target_px: str | None = None
    cancel_px: str | None = None
    match_tol: str | None = None
    ts_market_data_event_ns: int | None = None
    ts_market_data_recv_ns: int | None = None
    ts_decision_ns: int | None = None
    ts_submit_local_ns: int | None = None
    ts_command_init_ns: int | None = None
    ts_risk_recv_ns: int | None = None
    ts_risk_forward_ns: int | None = None
    ts_exec_recv_ns: int | None = None
    ts_exec_forward_ns: int | None = None
    ts_client_submit_ns: int | None = None
    ts_adapter_submit_start_ns: int | None = None
    ts_cancel_request_local_ns: int | None = None
    decision_context_json = DECISION_CONTEXT_JSON_DEFAULT_LITERAL
    order_side: str | None = None
    order_type: str | None = None
    time_in_force: str | None = None
    post_only: int | None = None
    reduce_only: int | None = None
    order_qty: str | None = None
    order_px: str | None = None
    rejection_reason: str | None = None
    normalized_order_qty_base = order_qty_base
    normalized_order_qty_venue = order_qty_venue
    normalized_qty_conversion_status = qty_conversion_status
    normalized_qty_conversion_source = qty_conversion_source

    if event_type == "OrderInitialized":
        action_id, action_reason, ts_decision_ns, decision_context_json = _parse_intent_tags(data.get("tags"))
        order_side = data.get("order_side")
        order_type = data.get("order_type")
        time_in_force = data.get("time_in_force")
        post_only_raw = data.get("post_only")
        reduce_only_raw = data.get("reduce_only")
        post_only = int(bool(post_only_raw)) if post_only_raw is not None else None
        reduce_only = int(bool(reduce_only_raw)) if reduce_only_raw is not None else None
        normalized_order_qty_base = normalized_order_qty_base or _optional_text(data.get("order_qty_base"))
        normalized_order_qty_venue = normalized_order_qty_venue or _optional_text(data.get("order_qty_venue"))
        normalized_qty_conversion_status = normalized_qty_conversion_status or _optional_text(
            data.get("qty_conversion_status"),
        )
        normalized_qty_conversion_source = normalized_qty_conversion_source or _optional_text(
            data.get("qty_conversion_source"),
        )
        order_qty = normalized_order_qty_venue or _optional_text(data.get("quantity"))
        order_px = _extract_order_px(data.get("options"))

    if event_type in ("OrderRejected", "OrderCancelRejected"):
        reason = data.get("reason")
        rejection_reason = str(reason) if reason is not None else None

    if action_intent is not None:
        run_id = action_intent.run_id
        quote_cycle_id = action_intent.quote_cycle_id
        reason_code = action_intent.reason_code
        level_index = action_intent.level_index
        target_px = action_intent.target_px
        cancel_px = action_intent.cancel_px
        match_tol = action_intent.match_tol
        ts_market_data_event_ns = action_intent.ts_market_data_event_ns
        ts_market_data_recv_ns = action_intent.ts_market_data_recv_ns
        ts_decision_ns = action_intent.ts_decision_ns if action_intent.ts_decision_ns is not None else ts_decision_ns
        ts_submit_local_ns = action_intent.ts_submit_local_ns
        ts_cancel_request_local_ns = action_intent.ts_cancel_request_local_ns
        if action_intent.decision_context_json != DECISION_CONTEXT_JSON_DEFAULT_LITERAL:
            decision_context_json = action_intent.decision_context_json

    if execution_timing is not None:
        ts_command_init_ns = execution_timing.ts_command_init_ns
        ts_risk_recv_ns = execution_timing.ts_risk_recv_ns
        ts_risk_forward_ns = execution_timing.ts_risk_forward_ns
        ts_exec_recv_ns = execution_timing.ts_exec_recv_ns
        ts_exec_forward_ns = execution_timing.ts_exec_forward_ns
        ts_client_submit_ns = execution_timing.ts_client_submit_ns
        ts_adapter_submit_start_ns = execution_timing.ts_adapter_submit_start_ns

    return OrderActionRow(
        trader_id=data["trader_id"],
        event_id=data["event_id"],
        strategy_id=data["strategy_id"],
        instrument_id=data["instrument_id"],
        client_order_id=data["client_order_id"],
        account_id=data.get("account_id"),
        venue_order_id=data.get("venue_order_id"),
        position_id=data.get("position_id"),
        action_type=action_type,
        action_state=action_state,
        event_type=event_type,
        action_id=action_id,
        action_reason=action_reason,
        run_id=run_id,
        quote_cycle_id=quote_cycle_id,
        reason_code=reason_code,
        level_index=level_index,
        target_px=target_px,
        cancel_px=cancel_px,
        match_tol=match_tol,
        ts_market_data_event_ns=ts_market_data_event_ns,
        ts_market_data_recv_ns=ts_market_data_recv_ns,
        ts_decision_ns=ts_decision_ns,
        ts_submit_local_ns=ts_submit_local_ns,
        ts_command_init_ns=ts_command_init_ns,
        ts_risk_recv_ns=ts_risk_recv_ns,
        ts_risk_forward_ns=ts_risk_forward_ns,
        ts_exec_recv_ns=ts_exec_recv_ns,
        ts_exec_forward_ns=ts_exec_forward_ns,
        ts_client_submit_ns=ts_client_submit_ns,
        ts_adapter_submit_start_ns=ts_adapter_submit_start_ns,
        ts_cancel_request_local_ns=ts_cancel_request_local_ns,
        decision_context_json=decision_context_json,
        order_side=order_side,
        order_type=order_type,
        time_in_force=time_in_force,
        post_only=post_only,
        reduce_only=reduce_only,
        order_qty=order_qty,
        order_px=order_px,
        rejection_reason=rejection_reason,
        ts_event=int(data["ts_event"]),
        ts_init=int(data["ts_init"]),
        ts_ingest=ts_ingest,
        reconciliation=int(bool(data.get("reconciliation", False))),
        payload_json=_encode_payload_json(data, on_payload_encode_error=on_payload_encode_error),
        order_qty_base=normalized_order_qty_base,
        order_qty_venue=normalized_order_qty_venue,
        qty_conversion_status=normalized_qty_conversion_status,
        qty_conversion_source=normalized_qty_conversion_source,
    )


@dataclass(frozen=True, slots=True)
class _OrderActionEnvelope:
    data: dict[str, Any]
    event_type: str
    ts_ingest: int
    action_intent: ActionIntentRecord | None
    execution_timing: ExecutionTimingRecord | None
    quantity: OperatorQuantitySnapshot | None


class OrderActionPersistenceActor(_AsyncSQLitePersistenceActor[_OrderActionEnvelope, OrderActionRow]):
    """
    Persist selected `OrderEvent` instances from `events.order.*` into SQLite.

    The message-bus hot path snapshots the event payload and enqueues it, while
    tag parsing, JSON encoding, and DB I/O are handled off the hot path via
    batched flushes.
    """

    def __init__(
        self,
        config: OrderActionPersistenceActorConfig,
        *,
        connect_fn: Callable[[str], sqlite3.Connection] = connect,
        ensure_schema_fn: Callable[[sqlite3.Connection], None] = ensure_schema,
        insert_many_fn: Callable[[sqlite3.Connection, list[OrderActionRow]], tuple[int, int]] = insert_many,
        run_writer_thread: bool = True,
    ) -> None:
        self._event_types = frozenset(config.event_types)
        super().__init__(
            config,
            connect_fn=connect_fn,
            ensure_schema_fn=ensure_schema_fn,
            insert_rows_fn=insert_many_fn,
            run_writer_thread=run_writer_thread,
            thread_name_suffix="orders",
            writer_name="Order action",
            queue_item_name="event",
        )
        self.filtered = 0
        self.payload_encode_errors = 0
        self._action_intent_cache = ActionIntentCache(
            max_entries=config.action_intent_max_entries,
            ttl_ns=config.action_intent_ttl_ms * 1_000_000,
        )
        self._execution_timing_cache = ExecutionTimingCache(
            max_entries=config.execution_timing_max_entries,
            ttl_ns=config.execution_timing_ttl_ms * 1_000_000,
        )

    def on_start(self) -> None:
        self._action_intent_cache.clear()
        self._execution_timing_cache.clear()
        super().on_start()
        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_order_message)
        if self.config.action_intent_topic is not None:
            self.msgbus.subscribe(
                topic=self.config.action_intent_topic,
                handler=self._on_action_intent_message,
            )
        if self.config.execution_timing_topic is not None:
            self.msgbus.subscribe(
                topic=self.config.execution_timing_topic,
                handler=self._on_execution_timing_message,
            )

    def on_stop(self) -> None:
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_order_message)
            if self.config.action_intent_topic is not None:
                self.msgbus.unsubscribe(
                    topic=self.config.action_intent_topic,
                    handler=self._on_action_intent_message,
                )
            if self.config.execution_timing_topic is not None:
                self.msgbus.unsubscribe(
                    topic=self.config.execution_timing_topic,
                    handler=self._on_execution_timing_message,
                )
        self._action_intent_cache.clear()
        self._execution_timing_cache.clear()
        super().on_stop()

    def _on_order_message(self, msg: object) -> None:
        if isinstance(msg, OrderEvent):
            self.on_order_event(msg)

    def on_order_event(self, event: OrderEvent) -> None:
        event_type = type(event).__name__
        if event_type not in self._event_types:
            self.filtered += 1
            return

        payload = _snapshot_payload_data(event.to_dict(event))
        now_ns = _current_ts_ingest_ns(self.clock)
        self._action_intent_cache.prune(now_ns=now_ns)
        self._execution_timing_cache.prune(now_ns=now_ns)
        action_intent = None
        execution_timing = None
        quantity = None
        intent_type = intent_type_for_order_event(event_type)
        if intent_type is not None:
            action_intent = self._action_intent_cache.get(
                client_order_id=str(event.client_order_id),
                intent_type=intent_type,
                strategy_id=str(event.strategy_id),
                now_ns=now_ns,
            )
            execution_timing = self._execution_timing_cache.get(
                client_order_id=str(event.client_order_id),
                action_type=intent_type,
                strategy_id=str(event.strategy_id),
                now_ns=now_ns,
            )
        if event_type == "OrderInitialized":
            instrument = self.cache.instrument(event.instrument_id)
            quantity = snapshot_operator_quantity(
                instrument,
                payload.get("quantity"),
                last_px=_extract_order_px(payload.get("options")),
                missing_metadata_source="persistence:instrument cache miss",
            )
        self._enqueue_payload(
            _OrderActionEnvelope(
                data=payload,
                event_type=event_type,
                ts_ingest=now_ns,
                action_intent=action_intent,
                execution_timing=execution_timing,
                quantity=quantity,
            ),
        )
        evict_types = intent_types_to_evict_for_order_event(event_type)
        if evict_types:
            self._action_intent_cache.evict_types(
                client_order_id=str(event.client_order_id),
                strategy_id=str(event.strategy_id),
                intent_types=evict_types,
            )
            self._execution_timing_cache.evict_types(
                client_order_id=str(event.client_order_id),
                strategy_id=str(event.strategy_id),
                action_types=evict_types,
            )

    def _build_row(self, payload: _OrderActionEnvelope) -> OrderActionRow | None:
        return order_event_to_row(
            payload.data,
            event_type=payload.event_type,
            ts_ingest=payload.ts_ingest,
            action_intent=payload.action_intent,
            execution_timing=payload.execution_timing,
            order_qty_base=payload.quantity.qty_base if payload.quantity is not None else None,
            order_qty_venue=payload.quantity.qty_venue if payload.quantity is not None else None,
            qty_conversion_status=(
                payload.quantity.qty_conversion_status if payload.quantity is not None else None
            ),
            qty_conversion_source=(
                payload.quantity.qty_conversion_source if payload.quantity is not None else None
            ),
            on_payload_encode_error=self._on_payload_encode_error,
        )

    def _on_payload_encode_error(self) -> None:
        self.payload_encode_errors += 1

    def _on_action_intent_message(self, msg: object) -> None:
        now_ns = _current_ts_ingest_ns(self.clock)
        for payload in iter_json_payload_mappings(msg):
            action_intent = ActionIntentRecord.from_payload(payload)
            if action_intent is not None:
                self._action_intent_cache.add(action_intent, now_ns=now_ns)

    def _on_execution_timing_message(self, msg: object) -> None:
        now_ns = _current_ts_ingest_ns(self.clock)
        for record in iter_execution_timing_records(msg):
            self._execution_timing_cache.add(record, now_ns=now_ns)
