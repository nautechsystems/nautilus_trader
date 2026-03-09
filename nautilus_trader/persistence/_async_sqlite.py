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
from typing import Generic
from typing import Protocol
from typing import TypeVar

from nautilus_trader.common.actor import Actor


EnvelopeT = TypeVar("EnvelopeT")
RowT = TypeVar("RowT")


class _AsyncSQLitePersistenceConfig(Protocol):
    db_path: str
    flush_interval_ms: int
    max_batch_size: int
    flush_time_budget_ms: int | None
    flush_timeout_ms: int
    max_queue_size: int
    on_error: str
    stop_timeout_ms: int
    strict_stop: bool
    propagate_errors_to_bus: bool


def writer_startup_timeout_seconds(config: _AsyncSQLitePersistenceConfig) -> float:
    """
    Return a conservative writer-start readiness timeout in seconds.

    Protects actor startup/flush readiness checks against slow connection
    initialization by honoring flush/stop budgets in addition to interval cadence.
    """
    return max(
        1.0,
        config.flush_timeout_ms / 1000.0,
        config.stop_timeout_ms / 1000.0,
        (config.flush_interval_ms / 1000.0) * 4.0,
    )


def retry_backoff_seconds(
    config: _AsyncSQLitePersistenceConfig,
    consecutive_failures: int,
) -> float:
    """
    Return the delay before retrying a retained batch after a DB write failure.
    """
    base = max(0.05, config.flush_interval_ms / 1000.0)
    exponent = max(0, min(consecutive_failures - 1, 5))
    return min(5.0, base * (2**exponent))


class _AsyncSQLitePersistenceActor(Actor, Generic[EnvelopeT, RowT]):
    def __init__(
        self,
        config: _AsyncSQLitePersistenceConfig,
        *,
        connect_fn: Callable[[str], sqlite3.Connection],
        ensure_schema_fn: Callable[[sqlite3.Connection], None],
        insert_rows_fn: Callable[[sqlite3.Connection, list[RowT]], tuple[int, int]],
        run_writer_thread: bool,
        thread_name_suffix: str,
        writer_name: str,
        queue_item_name: str,
    ) -> None:
        super().__init__(config)

        self._connect_fn = connect_fn
        self._ensure_schema_fn = ensure_schema_fn
        self._insert_rows_fn = insert_rows_fn
        self._run_writer_thread = run_writer_thread
        self._thread_name_suffix = thread_name_suffix
        self._writer_name = writer_name
        self._queue_item_name = queue_item_name

        self._conn: sqlite3.Connection | None = None
        self._queue: queue.Queue[EnvelopeT] = queue.Queue(maxsize=config.max_queue_size)
        self._pending_rows: deque[RowT] = deque()
        self._stop_event = threading.Event()
        self._flush_event = threading.Event()
        self._writer_started = threading.Event()
        self._writer_thread: threading.Thread | None = None
        self._writer_cleanup_thread: threading.Thread | None = None
        self._writer_cleanup_done = threading.Event()
        self._writer_cleanup_done.set()
        self._drain_on_stop_timeout_cleanup = False
        self._writer_error: RuntimeError | None = None
        self._ingress_error: RuntimeError | None = None
        self._next_retry_after: float | None = None
        self._consecutive_write_failures = 0
        self._last_log_at: dict[str, float] = {}

        self.enqueued = 0
        self.persisted = 0
        self.deduped = 0
        self.dropped = 0
        self.db_write_errors = 0
        self.transform_errors = 0
        self.queue_high_watermark = 0
        self.persistence_disabled = False

    def on_start(self) -> None:
        if not self._writer_cleanup_done.is_set():
            if self._writer_cleanup_thread is not None and not self._writer_cleanup_thread.is_alive():
                self._writer_cleanup_thread = None
                self._writer_cleanup_done.set()
            else:
                raise RuntimeError(f"{self._writer_name} writer cleanup in progress from previous stop timeout")

        if self._writer_thread is not None:
            if self._writer_thread.is_alive():
                raise RuntimeError(f"{self._writer_name} writer thread is still running from previous lifecycle")
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
        self._drain_on_stop_timeout_cleanup = False
        self._writer_error = None
        self._ingress_error = None
        self._next_retry_after = None
        self._consecutive_write_failures = 0
        self.persistence_disabled = False

        try:
            if self._run_writer_thread:
                conn = self._connect_fn(self.config.db_path)
                self._ensure_schema_fn(conn)
                conn.close()

                self._writer_thread = threading.Thread(
                    target=self._writer_loop,
                    name=f"{self.id}-{self._thread_name_suffix}-writer",
                    daemon=True,
                )
                self._writer_thread.start()

                startup_timeout = writer_startup_timeout_seconds(self.config)
                if not self._writer_started.wait(timeout=startup_timeout):
                    self._writer_error = RuntimeError(f"{self._writer_name} writer thread startup timed out")
                    raise self._writer_error

                self._raise_if_writer_failed(force=True)
            else:
                self._conn = self._connect_fn(self.config.db_path)
                self._ensure_schema_fn(self._conn)
        except Exception:
            self._stop_event.set()
            self._flush_event.set()
            if self._writer_thread is not None:
                self._writer_thread.join(timeout=self.config.stop_timeout_ms / 1000.0)
                if self._writer_thread.is_alive():
                    self._writer_error = RuntimeError(
                        f"{self._writer_name} writer thread did not stop during startup cleanup",
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
        self._stop_event.set()
        self._flush_event.set()

        if self._writer_thread is not None:
            self._writer_thread.join(timeout=self.config.stop_timeout_ms / 1000.0)
            if self._writer_thread.is_alive():
                msg = f"{self._writer_name} writer thread did not stop cleanly"
                self._writer_error = RuntimeError(msg)
                self._drain_on_stop_timeout_cleanup = True
                self._schedule_writer_ref_cleanup(self._writer_thread)
                if self.config.strict_stop:
                    raise self._writer_error
                self.log.error(msg)
                return
            self._writer_thread = None
            if self._writer_error is not None and not self.config.strict_stop:
                self._discard_unprocessed_backlog()
            if self.config.strict_stop:
                self._raise_if_writer_failed(force=True)
                if self._queue.unfinished_tasks != 0:
                    raise RuntimeError(
                        f"{self._writer_name} writer stopped with unfinished persisted tasks",
                    )

        if not self._run_writer_thread:
            try:
                self.flush()
            except RuntimeError:
                pass

            if self._conn is not None:
                self._conn.close()
                self._conn = None

        self._pending_rows.clear()

    def flush(self) -> None:
        """
        Flush buffered rows to the DB.

        In threaded mode this requests an immediate writer flush and waits for
        queue task accounting to drain (`unfinished_tasks == 0`) for the
        currently queued work. This assumes ingestion is quiesced for
        deterministic completion.
        In synchronous mode this flushes inline (used by unit tests).
        """
        self._raise_if_writer_failed(force=True)

        if self._run_writer_thread and self._writer_thread is not None:
            if not self._writer_started.wait(timeout=writer_startup_timeout_seconds(self.config)):
                raise RuntimeError(f"{self._writer_name} writer thread is not ready")

            self._raise_if_writer_failed(force=True)
            self._flush_event.set()
            timeout = self.config.flush_timeout_ms / 1000.0
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if self._writer_error is not None:
                    break
                if self._queue.unfinished_tasks == 0:
                    break
                time.sleep(0.001)
            self._raise_if_writer_failed(force=True)
            if self._queue.unfinished_tasks != 0:
                raise RuntimeError(f"{self._writer_name} flush timed out before persistence barrier")
            return

        if self._conn is None:
            return

        while self._flush_once():
            pass

        self._raise_if_writer_failed(force=True)

    def _enqueue_payload(self, payload: EnvelopeT) -> None:
        if self._ingress_error is not None:
            if self.config.propagate_errors_to_bus:
                raise self._ingress_error

            self.persistence_disabled = True
            self.dropped += 1
            self._log_throttled(
                "disabled",
                f"{self._writer_name} persistence is disabled, dropping {self._queue_item_name}",
            )
            return

        if self._writer_error is not None:
            if self.config.propagate_errors_to_bus:
                raise self._writer_error

            self.persistence_disabled = True
            self.dropped += 1
            self._log_throttled(
                "disabled",
                f"{self._writer_name} persistence is disabled, dropping {self._queue_item_name}",
            )
            return

        try:
            self._queue.put_nowait(payload)
            self.enqueued += 1
            self.queue_high_watermark = max(self.queue_high_watermark, self._queue.qsize())
        except queue.Full as exc:
            self._on_queue_full(exc)
            return

        if self._run_writer_thread:
            self._flush_event.set()

    def _writer_loop(self) -> None:
        conn: sqlite3.Connection | None = None
        try:
            conn = self._connect_fn(self.config.db_path)
            self._conn = conn
        except Exception as exc:
            self._writer_error = RuntimeError(f"{self._writer_name} writer failed to start")
            self.log.error(f"{self._writer_error}: {exc!r}")
            self._writer_started.set()
            return
        else:
            self._writer_started.set()

        try:
            while True:
                processed = self._flush_once()

                if self._writer_error is not None and not (
                    self._drain_on_stop_timeout_cleanup and self._stop_event.is_set()
                ):
                    break

                if self._stop_event.is_set() and self._queue.empty() and not self._pending_rows:
                    break

                if processed:
                    continue

                if self._stop_event.is_set() and (not self._queue.empty() or self._pending_rows):
                    self._flush_event.wait(timeout=0.01)
                    self._flush_event.clear()
                    continue

                self._flush_event.wait(timeout=self.config.flush_interval_ms / 1000.0)
                self._flush_event.clear()
        except Exception as exc:
            self._writer_error = RuntimeError(f"{self._writer_name} writer loop crashed")
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
            inserted, deduped = self._insert_rows_fn(self._conn, batch)
        except Exception as exc:
            return self._on_write_error(batch, exc)

        self._next_retry_after = None
        self._consecutive_write_failures = 0
        self.persisted += inserted
        self.deduped += deduped
        self._mark_batch_done(len(batch))
        return True

    def _next_batch(self) -> list[RowT]:
        batch: list[RowT] = []

        if (
            self._pending_rows
            and not self._stop_event.is_set()
            and self._next_retry_after is not None
            and time.monotonic() < self._next_retry_after
        ):
            return batch

        if self._pending_rows:
            take = min(self.config.max_batch_size, len(self._pending_rows))
            for _ in range(take):
                batch.append(self._pending_rows.popleft())
            return batch

        budget_deadline: float | None = None
        while len(batch) < self.config.max_batch_size:
            if budget_deadline is not None and time.monotonic() >= budget_deadline:
                break

            try:
                payload = self._queue.get_nowait()
            except queue.Empty:
                break

            if budget_deadline is None and self.config.flush_time_budget_ms is not None:
                budget_deadline = time.monotonic() + (self.config.flush_time_budget_ms / 1000.0)

            row = self._payload_to_row(payload)
            if row is not None:
                batch.append(row)

            if self._writer_error is not None:
                break

        return batch

    def _payload_to_row(self, payload: EnvelopeT) -> RowT | None:
        try:
            row = self._build_row(payload)
        except Exception as exc:
            self._on_transform_error(exc)
            self._mark_batch_done(1)
            return None

        if row is None:
            self._mark_batch_done(1)
        return row

    def _build_row(self, payload: EnvelopeT) -> RowT | None:  # pragma: no cover - abstract hook
        raise NotImplementedError

    def _on_transform_error(self, exc: Exception) -> None:
        self.transform_errors += 1

        if self.config.on_error == "fail_fast":
            self._writer_error = RuntimeError(f"{self._writer_name} payload transform failed")
            self.persistence_disabled = not self.config.propagate_errors_to_bus
            self._log_throttled("transform_error", f"{self._writer_error}: {exc!r}")
            return

        self.dropped += 1
        self._log_throttled(
            "transform_error",
            f"{self._writer_name} payload transform failed, dropping {self._queue_item_name}: {exc!r}",
        )

    def _on_write_error(self, batch: list[RowT], exc: Exception) -> bool:
        self.db_write_errors += 1

        if self.config.on_error == "log_and_drop":
            self.dropped += len(batch)
            self._mark_batch_done(len(batch))
            self._log_throttled(
                "write_error",
                f"{self._writer_name} DB write failed, dropping {len(batch)} rows: {exc!r}",
            )
            return True

        if self.config.on_error == "buffer_until_full_then_fail":
            if self._stop_event.is_set():
                self.dropped += len(batch)
                self._mark_batch_done(len(batch))
                self._log_throttled(
                    "write_error",
                    f"{self._writer_name} DB write failed during shutdown, dropping {len(batch)} rows: {exc!r}",
                )
                return True

            self._pending_rows.extendleft(reversed(batch))
            self._consecutive_write_failures += 1
            retry_delay = retry_backoff_seconds(self.config, self._consecutive_write_failures)
            self._next_retry_after = time.monotonic() + retry_delay
            self._log_throttled(
                "write_error",
                f"{self._writer_name} DB write failed, retaining {len(batch)} rows for retry "
                f"in {retry_delay:.2f}s: {exc!r}",
            )
            return False

        self._writer_error = RuntimeError(f"{self._writer_name} persistence write failed")
        self.persistence_disabled = not self.config.propagate_errors_to_bus
        self._log_throttled("write_error", f"{self._writer_error}: {exc!r}")
        if not self.config.propagate_errors_to_bus:
            self.dropped += len(batch)
            self._mark_batch_done(len(batch))
        return False

    def _mark_batch_done(self, count: int) -> None:
        for _ in range(count):
            self._queue.task_done()

    def _on_queue_full(self, exc: queue.Full) -> None:
        if self.config.on_error == "log_and_drop":
            self.dropped += 1
            self._log_throttled(
                "queue_full",
                f"{self._writer_name} persistence queue is full, dropping {self._queue_item_name}",
            )
            return

        self._ingress_error = RuntimeError(f"{self._writer_name} persistence queue is full")
        if self.config.propagate_errors_to_bus:
            raise self._ingress_error from exc

        self.persistence_disabled = True
        self.dropped += 1
        self._log_throttled(
            "queue_full",
            f"{self._ingress_error}; disabling persistence and dropping {self._queue_item_name}",
        )

    def _log_throttled(self, key: str, msg: str) -> None:
        now = time.monotonic()
        last = self._last_log_at.get(key, 0.0)
        if now - last >= 1.0:
            self.log.error(msg)
            self._last_log_at[key] = now

    def _discard_unprocessed_backlog(self) -> None:
        dropped_now = 0

        while True:
            try:
                self._queue.get_nowait()
            except queue.Empty:
                break
            else:
                self._queue.task_done()
                dropped_now += 1

        if self._pending_rows:
            dropped_now += len(self._pending_rows)
            self._pending_rows.clear()

        self.dropped += dropped_now

    def _schedule_writer_ref_cleanup(self, writer_thread: threading.Thread) -> None:
        if self._writer_cleanup_thread is not None and self._writer_cleanup_thread.is_alive():
            return

        if self._writer_thread is writer_thread:
            self._writer_thread = None
        self._writer_cleanup_done.clear()

        def _cleanup() -> None:
            try:
                writer_thread.join()
            finally:
                self._drain_on_stop_timeout_cleanup = False
                self._writer_cleanup_done.set()
                self._writer_cleanup_thread = None

        cleanup_thread = threading.Thread(
            target=_cleanup,
            name=f"{self.id}-{self._thread_name_suffix}-writer-cleanup",
            daemon=True,
        )
        self._writer_cleanup_thread = cleanup_thread
        cleanup_thread.start()

    def _raise_if_writer_failed(self, *, force: bool = False) -> None:
        if self._writer_error is not None and (force or self.config.propagate_errors_to_bus):
            raise self._writer_error
