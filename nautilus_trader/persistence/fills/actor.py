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

from nautilus_trader.persistence._action_intent import ActionIntentCache
from nautilus_trader.persistence._action_intent import ActionIntentRecord
from nautilus_trader.persistence._action_intent import current_ts_ns
from nautilus_trader.persistence._action_intent import iter_json_payload_mappings
from nautilus_trader.persistence._action_intent import PLACE_INTENT_TYPE
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.persistence._async_sqlite import _AsyncSQLitePersistenceActor
from nautilus_trader.persistence._async_sqlite import writer_startup_timeout_seconds
from nautilus_trader.persistence._execution_timing import ExecutionTimingCache
from nautilus_trader.persistence._execution_timing import ExecutionTimingRecord
from nautilus_trader.persistence._execution_timing import iter_execution_timing_records
from nautilus_trader.persistence.fills.config import ExecutionFillPersistenceActorConfig
from nautilus_trader.persistence.fills.sqlite import ExecutionFillRow
from nautilus_trader.persistence.fills.sqlite import connect
from nautilus_trader.persistence.fills.sqlite import ensure_schema
from nautilus_trader.persistence.fills.sqlite import fill_to_row
from nautilus_trader.persistence.fills.sqlite import insert_fills


def _writer_startup_timeout_seconds(config: ExecutionFillPersistenceActorConfig) -> float:
    return writer_startup_timeout_seconds(config)


@dataclass(frozen=True, slots=True)
class _ExecutionFillEnvelope:
    event: OrderFilled
    client_order_id: str
    info: dict[str, Any]
    ts_ingest_ns: int
    action_intent: ActionIntentRecord | None
    execution_timing: ExecutionTimingRecord | None


class ExecutionFillPersistenceActor(_AsyncSQLitePersistenceActor[_ExecutionFillEnvelope, ExecutionFillRow]):
    """
    Persist `OrderFilled` events from `events.fills.*` into SQLite.

    The message-bus hot path snapshots mutable fill fields and enqueues them,
    while JSON encoding and DB I/O are handled off the hot path via batched
    flushes.
    """

    def __init__(
        self,
        config: ExecutionFillPersistenceActorConfig,
        *,
        connect_fn: Callable[[str], sqlite3.Connection] = connect,
        ensure_schema_fn: Callable[[sqlite3.Connection], None] = ensure_schema,
        insert_fills_fn: Callable[[sqlite3.Connection, list[ExecutionFillRow]], tuple[int, int]] = insert_fills,
        run_writer_thread: bool = True,
    ) -> None:
        super().__init__(
            config,
            connect_fn=connect_fn,
            ensure_schema_fn=ensure_schema_fn,
            insert_rows_fn=insert_fills_fn,
            run_writer_thread=run_writer_thread,
            thread_name_suffix="fills",
            writer_name="Execution fill",
            queue_item_name="fill",
        )
        self.info_encode_errors = 0
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
        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_fill_message)
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
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_fill_message)
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

    def _on_fill_message(self, msg: object) -> None:
        if isinstance(msg, OrderFilled):
            self.on_order_filled(msg)

    def on_order_filled(self, event: OrderFilled) -> None:
        now_ns = current_ts_ns(self.clock)
        self._action_intent_cache.prune(now_ns=now_ns)
        self._execution_timing_cache.prune(now_ns=now_ns)
        self._enqueue_payload(
            _ExecutionFillEnvelope(
                event=event,
                client_order_id=event.client_order_id.value,
                info=copy.deepcopy(event.info),
                ts_ingest_ns=now_ns,
                action_intent=self._action_intent_cache.get(
                    client_order_id=event.client_order_id.value,
                    intent_type=PLACE_INTENT_TYPE,
                    strategy_id=event.strategy_id.value,
                    now_ns=now_ns,
                ),
                execution_timing=self._execution_timing_cache.get(
                    client_order_id=event.client_order_id.value,
                    action_type=PLACE_INTENT_TYPE,
                    strategy_id=event.strategy_id.value,
                    now_ns=now_ns,
                ),
            ),
        )

    def _build_row(self, payload: _ExecutionFillEnvelope) -> ExecutionFillRow:
        return fill_to_row(
            payload.event,
            info_override=payload.info,
            client_order_id_override=payload.client_order_id,
            action_intent=payload.action_intent,
            execution_timing=payload.execution_timing,
            ts_ingest_ns=payload.ts_ingest_ns,
            on_info_encode_error=self._on_info_encode_error,
        )

    def _on_info_encode_error(self) -> None:
        self.info_encode_errors += 1

    def _on_action_intent_message(self, msg: object) -> None:
        now_ns = current_ts_ns(self.clock)
        for payload in iter_json_payload_mappings(msg):
            action_intent = ActionIntentRecord.from_payload(payload)
            if action_intent is not None:
                self._action_intent_cache.add(action_intent, now_ns=now_ns)

    def _on_execution_timing_message(self, msg: object) -> None:
        now_ns = current_ts_ns(self.clock)
        for record in iter_execution_timing_records(msg):
            self._execution_timing_cache.add(record, now_ns=now_ns)
