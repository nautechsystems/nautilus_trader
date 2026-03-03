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

import queue
import sqlite3
import threading
import time
from collections import deque
from collections.abc import Callable

from nautilus_trader.common.actor import Actor
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.persistence.fills.config import ExecutionFillPersistenceActorConfig
from nautilus_trader.persistence.fills.sqlite import ExecutionFillRow
from nautilus_trader.persistence.fills.sqlite import connect
from nautilus_trader.persistence.fills.sqlite import ensure_schema
from nautilus_trader.persistence.fills.sqlite import fill_to_row
from nautilus_trader.persistence.fills.sqlite import insert_fills


class ExecutionFillPersistenceActor(Actor):
    """
    Persist `OrderFilled` events from `events.fills.*` into SQLite.

    The message-bus hot path is enqueue-only (`put_nowait`), while DB I/O is
    handled off the hot path via batched flushes.
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
        super().__init__(config)

        self._connect_fn = connect_fn
        self._ensure_schema_fn = ensure_schema_fn
        self._insert_fills_fn = insert_fills_fn
        self._run_writer_thread = run_writer_thread

        self._conn: sqlite3.Connection | None = None
        self._queue: queue.Queue[ExecutionFillRow] = queue.Queue(maxsize=config.max_queue_size)
        self._pending_rows: deque[ExecutionFillRow] = deque()
        self._stop_event = threading.Event()
        self._flush_event = threading.Event()
        self._writer_thread: threading.Thread | None = None
        self._writer_error: RuntimeError | None = None

        self.enqueued = 0
        self.persisted = 0
        self.deduped = 0
        self.dropped = 0
        self.db_write_errors = 0
        self.info_encode_errors = 0

    def on_start(self) -> None:
        self._queue = queue.Queue(maxsize=self.config.max_queue_size)
        self._pending_rows.clear()
        self._stop_event.clear()
        self._flush_event.clear()
        self._writer_error = None

        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_fill_message)

        if self._run_writer_thread:
            # Create schema with a short-lived connection in the actor thread.
            conn = self._connect_fn(self.config.db_path)
            self._ensure_schema_fn(conn)
            conn.close()

            self._writer_thread = threading.Thread(
                target=self._writer_loop,
                name=f"{self.id}-fills-writer",
                daemon=True,
            )
            self._writer_thread.start()
        else:
            self._conn = self._connect_fn(self.config.db_path)
            self._ensure_schema_fn(self._conn)

    def on_stop(self) -> None:
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_fill_message)

        self._stop_event.set()
        self._flush_event.set()

        if self._writer_thread is not None:
            self._writer_thread.join(timeout=5.0)
            if self._writer_thread.is_alive():
                self._writer_error = RuntimeError(
                    "Execution fill writer thread did not stop cleanly",
                )
                raise self._writer_error
            self._writer_thread = None

        if not self._run_writer_thread:
            # Final best-effort flush for synchronous mode.
            try:
                self.flush()
            except RuntimeError:
                # Keep stop path best-effort and non-fatal.
                pass

            if self._conn is not None:
                self._conn.close()
                self._conn = None

        self._pending_rows.clear()

    def _on_fill_message(self, msg: object) -> None:
        if isinstance(msg, OrderFilled):
            self.on_order_filled(msg)

    def on_order_filled(self, event: OrderFilled) -> None:
        self._raise_if_writer_failed()

        # Keep cross-thread payloads primitive (tuple row), no waits / no DB I/O.
        row = fill_to_row(event, on_info_encode_error=self._on_info_encode_error)

        try:
            self._queue.put_nowait(row)
            self.enqueued += 1
        except queue.Full as exc:
            self._on_queue_full(exc)
            return

        if self._run_writer_thread:
            self._flush_event.set()

    def flush(self) -> None:
        """
        Flush buffered rows to the DB.

        In threaded mode this requests an immediate writer flush and waits briefly.
        In synchronous mode this flushes inline (used by unit tests).
        """
        if self._conn is None:
            return

        if self._run_writer_thread and self._writer_thread is not None:
            self._flush_event.set()
            timeout = max(0.050, (self.config.flush_interval_ms / 1000.0) * 4.0)
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if self._writer_error is not None:
                    break
                if self._queue.empty():
                    break
                time.sleep(0.001)
            self._raise_if_writer_failed()
            return

        while self._flush_once():
            pass

        self._raise_if_writer_failed()

    def _writer_loop(self) -> None:
        try:
            conn = self._connect_fn(self.config.db_path)
            self._conn = conn
        except Exception as exc:
            self._writer_error = RuntimeError("Execution fill writer failed to start")
            self.log.error(f"{self._writer_error}: {exc!r}")
            return

        while True:
            processed = self._flush_once()

            if self._writer_error is not None:
                break

            if self._stop_event.is_set() and self._queue.empty() and not self._pending_rows:
                break

            if processed:
                continue

            if self._stop_event.is_set() and (not self._queue.empty() or self._pending_rows):
                continue

            self._flush_event.wait(timeout=self.config.flush_interval_ms / 1000.0)
            self._flush_event.clear()

        try:
            conn.close()
        finally:
            self._conn = None

    def _flush_once(self) -> bool:
        if self._conn is None:
            return False

        batch = self._next_batch()
        if not batch:
            return False

        try:
            inserted, deduped = self._insert_fills_fn(self._conn, batch)
        except Exception as exc:
            return self._on_write_error(batch, exc)

        self.persisted += inserted
        self.deduped += deduped
        return True

    def _next_batch(self) -> list[ExecutionFillRow]:
        batch: list[ExecutionFillRow] = []

        if self._pending_rows:
            take = min(self.config.max_batch_size, len(self._pending_rows))
            for _ in range(take):
                batch.append(self._pending_rows.popleft())
            return batch

        try:
            batch.append(self._queue.get_nowait())
        except queue.Empty:
            return batch

        budget_deadline: float | None = None
        if self.config.flush_time_budget_ms is not None:
            budget_deadline = time.monotonic() + (self.config.flush_time_budget_ms / 1000.0)

        while len(batch) < self.config.max_batch_size:
            if budget_deadline is not None and time.monotonic() >= budget_deadline:
                break

            try:
                batch.append(self._queue.get_nowait())
            except queue.Empty:
                break

        return batch

    def _on_write_error(self, batch: list[ExecutionFillRow], exc: Exception) -> bool:
        self.db_write_errors += 1

        if self.config.on_error == "log_and_drop":
            self.dropped += len(batch)
            self.log.error(f"Execution fill DB write failed, dropping {len(batch)} rows: {exc!r}")
            return True

        if self.config.on_error == "buffer_until_full_then_fail":
            if self._stop_event.is_set():
                # During shutdown, avoid hanging forever on a failing DB.
                self.dropped += len(batch)
            else:
                self._pending_rows.extendleft(reversed(batch))
            self.log.error(f"Execution fill DB write failed, retaining batch for retry: {exc!r}")
            return False

        self._writer_error = RuntimeError("Execution fill persistence write failed")
        self.log.error(f"{self._writer_error}: {exc!r}")
        return False

    def _on_queue_full(self, exc: queue.Full) -> None:
        if self.config.on_error == "log_and_drop":
            self.dropped += 1
            self.log.error("Execution fill persistence queue is full, dropping fill")
            return

        self._writer_error = RuntimeError("Execution fill persistence queue is full")
        raise self._writer_error from exc

    def _raise_if_writer_failed(self) -> None:
        if self._writer_error is not None:
            raise self._writer_error

    def _on_info_encode_error(self) -> None:
        self.info_encode_errors += 1
