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

from nautilus_trader.model.events import OrderFilled
from nautilus_trader.persistence._async_sqlite import _AsyncSQLitePersistenceActor
from nautilus_trader.persistence._async_sqlite import writer_startup_timeout_seconds
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

    def on_start(self) -> None:
        super().on_start()
        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_fill_message)

    def on_stop(self) -> None:
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_fill_message)
        super().on_stop()

    def _on_fill_message(self, msg: object) -> None:
        if isinstance(msg, OrderFilled):
            self.on_order_filled(msg)

    def on_order_filled(self, event: OrderFilled) -> None:
        self._enqueue_payload(
            _ExecutionFillEnvelope(
                event=event,
                client_order_id=event.client_order_id.value,
                info=copy.deepcopy(event.info),
            ),
        )

    def _build_row(self, payload: _ExecutionFillEnvelope) -> ExecutionFillRow:
        return fill_to_row(
            payload.event,
            info_override=payload.info,
            client_order_id_override=payload.client_order_id,
            on_info_encode_error=self._on_info_encode_error,
        )

    def _on_info_encode_error(self) -> None:
        self.info_encode_errors += 1
