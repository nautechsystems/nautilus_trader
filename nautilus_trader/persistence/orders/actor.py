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
from typing import Any

import msgspec

from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.events import OrderEvent
from nautilus_trader.persistence.orders.config import OrderActionPersistenceActorConfig
from nautilus_trader.persistence.orders.schema import SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL
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


def _parse_intent_tags(tags: object) -> tuple[str | None, str | None, int | None, str]:
    action_id: str | None = None
    action_reason: str | None = None
    ts_decision_ns: int | None = None
    signal_snapshot_json = SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL

    if not isinstance(tags, list):
        return action_id, action_reason, ts_decision_ns, signal_snapshot_json

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
                signal_snapshot_json = msgspec.json.encode(value).decode("utf-8")
            else:
                signal_snapshot_json = msgspec.json.encode(decoded).decode("utf-8")

    return action_id, action_reason, ts_decision_ns, signal_snapshot_json


def order_event_to_row(
    event: OrderEvent,
    *,
    event_type: str | None = None,
    ts_ingest: int,
    on_payload_encode_error: Callable[[], None] | None = None,
) -> OrderActionRow | None:
    """
    Convert supported order lifecycle events to a primitive order action row.
    """
    if event_type is None:
        event_type = type(event).__name__
    action_fields = _ACTION_MAP.get(event_type)
    if action_fields is None:
        return None

    data = event.to_dict(event)
    action_type, action_state = action_fields

    action_id: str | None = None
    action_reason: str | None = None
    ts_decision_ns: int | None = None
    signal_snapshot_json = SIGNAL_SNAPSHOT_JSON_DEFAULT_LITERAL
    order_side: str | None = None
    order_type: str | None = None
    time_in_force: str | None = None
    post_only: int | None = None
    reduce_only: int | None = None
    order_qty: str | None = None
    order_px: str | None = None
    rejection_reason: str | None = None

    if event_type == "OrderInitialized":
        action_id, action_reason, ts_decision_ns, signal_snapshot_json = _parse_intent_tags(data.get("tags"))
        order_side = data.get("order_side")
        order_type = data.get("order_type")
        time_in_force = data.get("time_in_force")
        post_only_raw = data.get("post_only")
        reduce_only_raw = data.get("reduce_only")
        post_only = int(bool(post_only_raw)) if post_only_raw is not None else None
        reduce_only = int(bool(reduce_only_raw)) if reduce_only_raw is not None else None
        order_qty = data.get("quantity")
        options = data.get("options")
        if isinstance(options, dict):
            price = options.get("price")
            if price is not None:
                order_px = str(price)

    if event_type in ("OrderRejected", "OrderCancelRejected"):
        reason = data.get("reason")
        rejection_reason = str(reason) if reason is not None else None

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
        ts_decision_ns=ts_decision_ns,
        signal_snapshot_json=signal_snapshot_json,
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
    )


class OrderActionPersistenceActor(Actor):
    """
    Persist selected `OrderEvent` instances from `events.order.*` into SQLite.

    The message-bus hot path is enqueue-only (`put_nowait`): the handler only
    performs CPU-bound normalization to a primitive row tuple and never performs
    DB I/O, blocking DB waits, or queue waits. DB I/O is handled off the hot
    path via batched flushes.
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
        super().__init__(config)

        self._connect_fn = connect_fn
        self._ensure_schema_fn = ensure_schema_fn
        self._insert_many_fn = insert_many_fn
        self._run_writer_thread = run_writer_thread
        self._event_types = frozenset(config.event_types)

        self._conn: sqlite3.Connection | None = None
        self._queue: queue.Queue[OrderActionRow] = queue.Queue(maxsize=config.max_queue_size)
        self._pending_rows: deque[OrderActionRow] = deque()
        self._stop_event = threading.Event()
        self._flush_event = threading.Event()
        self._writer_started = threading.Event()
        self._writer_thread: threading.Thread | None = None
        self._writer_cleanup_thread: threading.Thread | None = None
        self._writer_cleanup_done = threading.Event()
        self._writer_cleanup_done.set()
        self._writer_error: RuntimeError | None = None

        self.enqueued = 0
        self.filtered = 0
        self.persisted = 0
        self.deduped = 0
        self.dropped = 0
        self.db_write_errors = 0
        self.payload_encode_errors = 0

    def on_start(self) -> None:
        if not self._writer_cleanup_done.is_set():
            if self._writer_cleanup_thread is not None and not self._writer_cleanup_thread.is_alive():
                self._writer_cleanup_thread = None
                self._writer_cleanup_done.set()
            else:
                raise RuntimeError("Order action writer cleanup in progress from previous stop timeout")

        if self._writer_thread is not None:
            if self._writer_thread.is_alive():
                raise RuntimeError("Order action writer thread is still running from previous lifecycle")
            self._writer_thread = None
        if self._writer_cleanup_thread is not None and not self._writer_cleanup_thread.is_alive():
            self._writer_cleanup_thread = None
        if self._conn is not None:
            self._conn.close()
            self._conn = None

        self._queue = queue.Queue(maxsize=self.config.max_queue_size)
        self._pending_rows.clear()
        self._stop_event.clear()
        self._flush_event.clear()
        self._writer_started.clear()
        self._writer_error = None

        try:
            if self._run_writer_thread:
                # Create schema with a short-lived connection in the actor thread.
                conn = self._connect_fn(self.config.db_path)
                self._ensure_schema_fn(conn)
                conn.close()

                self._writer_thread = threading.Thread(
                    target=self._writer_loop,
                    name=f"{self.id}-orders-writer",
                    daemon=True,
                )
                self._writer_thread.start()

                startup_timeout = max(1.0, (self.config.flush_interval_ms / 1000.0) * 4.0)
                if not self._writer_started.wait(timeout=startup_timeout):
                    self._writer_error = RuntimeError("Order action writer thread startup timed out")
                    raise self._writer_error

                self._raise_if_writer_failed()
            else:
                self._conn = self._connect_fn(self.config.db_path)
                self._ensure_schema_fn(self._conn)

            # Subscribe only after persistence backend is ready.
            self.msgbus.subscribe(topic=self.config.topic, handler=self._on_order_message)
        except Exception:
            self._stop_event.set()
            self._flush_event.set()
            if self._writer_thread is not None:
                self._writer_thread.join(timeout=self.config.stop_timeout_ms / 1000.0)
                if self._writer_thread.is_alive():
                    self._writer_error = RuntimeError(
                        "Order action writer thread did not stop during startup cleanup",
                    )
                    self._schedule_writer_ref_cleanup(self._writer_thread)
                    self.log.error(str(self._writer_error))
                else:
                    self._writer_thread = None
            if self._conn is not None:
                self._conn.close()
                self._conn = None
            self._pending_rows.clear()
            raise

    def on_stop(self) -> None:
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_order_message)

        self._stop_event.set()
        self._flush_event.set()

        if self._writer_thread is not None:
            self._writer_thread.join(timeout=self.config.stop_timeout_ms / 1000.0)
            if self._writer_thread.is_alive():
                msg = "Order action writer thread did not stop cleanly"
                self._writer_error = RuntimeError(msg)
                self._schedule_writer_ref_cleanup(self._writer_thread)
                if self.config.strict_stop:
                    raise self._writer_error
                self.log.error(msg)
                return
            self._writer_thread = None
            if self.config.strict_stop:
                self._raise_if_writer_failed()
                if self._queue.unfinished_tasks != 0:
                    raise RuntimeError(
                        "Order action writer stopped with unfinished persisted tasks",
                    )

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

    def _on_order_message(self, msg: object) -> None:
        if isinstance(msg, OrderEvent):
            self.on_order_event(msg)

    def on_order_event(self, event: OrderEvent) -> None:
        self._raise_if_writer_failed()

        event_type = type(event).__name__
        if event_type not in self._event_types:
            self.filtered += 1
            return

        # Keep cross-thread payloads primitive (tuple row) and handler-only CPU work.
        # Intentional tradeoff: normalize/serialize here once so the queued payload
        # is thread-safe and writer-thread DB work stays purely I/O.
        # No queue waits and no DB I/O on this hot path.
        ts_ingest = 0 if self.clock is None else self.clock.timestamp_ns()
        row = order_event_to_row(
            event,
            event_type=event_type,
            ts_ingest=ts_ingest,
            on_payload_encode_error=self._on_payload_encode_error,
        )
        if row is None:
            self.filtered += 1
            return

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

        In threaded mode this requests an immediate writer flush and waits for
        queue task accounting to drain (`unfinished_tasks == 0`) for the
        currently queued work. This assumes ingestion is quiesced for
        deterministic completion.
        In synchronous mode this flushes inline (used by unit tests).
        """
        self._raise_if_writer_failed()

        if self._run_writer_thread and self._writer_thread is not None:
            if not self._writer_started.wait(timeout=max(1.0, (self.config.flush_interval_ms / 1000.0) * 4.0)):
                raise RuntimeError("Order action writer thread is not ready")

            self._raise_if_writer_failed()
            self._flush_event.set()
            timeout = self.config.flush_timeout_ms / 1000.0
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if self._writer_error is not None:
                    break
                if self._queue.unfinished_tasks == 0:
                    break
                time.sleep(0.001)
            self._raise_if_writer_failed()
            if self._queue.unfinished_tasks != 0:
                raise RuntimeError("Order action flush timed out before persistence barrier")
            return

        if self._conn is None:
            return

        while self._flush_once():
            pass

        self._raise_if_writer_failed()

    def _writer_loop(self) -> None:
        conn: sqlite3.Connection | None = None
        try:
            conn = self._connect_fn(self.config.db_path)
            self._conn = conn
        except Exception as exc:
            self._writer_error = RuntimeError("Order action writer failed to start")
            self.log.error(f"{self._writer_error}: {exc!r}")
            self._writer_started.set()
            return
        else:
            self._writer_started.set()

        try:
            while True:
                processed = self._flush_once()

                if self._writer_error is not None:
                    break

                if self._stop_event.is_set() and self._queue.empty() and not self._pending_rows:
                    break

                if processed:
                    continue

                if self._stop_event.is_set() and (not self._queue.empty() or self._pending_rows):
                    # Avoid tight loop when stopping with backlog and no immediate progress.
                    self._flush_event.wait(timeout=0.01)
                    self._flush_event.clear()
                    continue

                self._flush_event.wait(timeout=self.config.flush_interval_ms / 1000.0)
                self._flush_event.clear()
        except Exception as exc:
            self._writer_error = RuntimeError("Order action writer loop crashed")
            self.log.error(f"{self._writer_error}: {exc!r}")
        finally:
            try:
                if conn is not None:
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
            inserted, deduped = self._insert_many_fn(self._conn, batch)
        except Exception as exc:
            return self._on_write_error(batch, exc)

        self.persisted += inserted
        self.deduped += deduped
        self._mark_batch_done(len(batch))
        return True

    def _next_batch(self) -> list[OrderActionRow]:
        batch: list[OrderActionRow] = []

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

    def _on_write_error(self, batch: list[OrderActionRow], exc: Exception) -> bool:
        self.db_write_errors += 1

        if self.config.on_error == "log_and_drop":
            self.dropped += len(batch)
            self._mark_batch_done(len(batch))
            self.log.error(f"Order action DB write failed, dropping {len(batch)} rows: {exc!r}")
            return True

        if self.config.on_error == "buffer_until_full_then_fail":
            if self._stop_event.is_set():
                # During shutdown, avoid hanging forever on a failing DB.
                self.dropped += len(batch)
                self._mark_batch_done(len(batch))
                self.log.error(
                    f"Order action DB write failed during shutdown, dropping {len(batch)} rows: {exc!r}",
                )
                return True
            else:
                self._pending_rows.extendleft(reversed(batch))
            self.log.error(
                f"Order action DB write failed, retaining {len(batch)} rows for retry: {exc!r}",
            )
            return False

        self._writer_error = RuntimeError("Order action persistence write failed")
        self.log.error(f"{self._writer_error}: {exc!r}")
        return False

    def _mark_batch_done(self, count: int) -> None:
        for _ in range(count):
            self._queue.task_done()

    def _on_queue_full(self, exc: queue.Full) -> None:
        if self.config.on_error == "log_and_drop":
            self.dropped += 1
            self.log.error("Order action persistence queue is full, dropping event")
            return

        self._writer_error = RuntimeError("Order action persistence queue is full")
        raise self._writer_error from exc

    def _on_payload_encode_error(self) -> None:
        self.payload_encode_errors += 1

    def _schedule_writer_ref_cleanup(self, writer_thread: threading.Thread) -> None:
        if self._writer_cleanup_thread is not None and self._writer_cleanup_thread.is_alive():
            return

        # Detach actor-owned refs immediately on timeout so lifecycle state is
        # consistent even while the writer finishes in the background.
        if self._writer_thread is writer_thread:
            self._writer_thread = None
        self._conn = None
        self._writer_cleanup_done.clear()

        def _cleanup() -> None:
            try:
                writer_thread.join()
            finally:
                self._writer_cleanup_done.set()
                self._writer_cleanup_thread = None

        cleanup_thread = threading.Thread(
            target=_cleanup,
            name=f"{self.id}-orders-writer-cleanup",
            daemon=True,
        )
        self._writer_cleanup_thread = cleanup_thread
        cleanup_thread.start()

    def _raise_if_writer_failed(self) -> None:
        if self._writer_error is not None:
            raise self._writer_error
