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

import sqlite3
import threading

import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.persistence.fills.actor import ExecutionFillPersistenceActor
from nautilus_trader.persistence.fills.config import ExecutionFillPersistenceActorConfig
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _make_fill(instrument, trade_id=None, ts_event: int = 123):
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    return TestEventStubs.order_filled(
        order=order,
        instrument=instrument,
        trade_id=trade_id,
        ts_event=ts_event,
    )


def _row_count(db_path: str) -> int:
    conn = sqlite3.connect(db_path)
    try:
        return conn.execute("SELECT COUNT(*) FROM execution_fill").fetchone()[0]
    finally:
        conn.close()


def _row_count_for_trade_id(db_path: str, trade_id: str) -> int:
    conn = sqlite3.connect(db_path)
    try:
        return conn.execute(
            "SELECT COUNT(*) FROM execution_fill WHERE trade_id = ?",
            (trade_id,),
        ).fetchone()[0]
    finally:
        conn.close()


def _make_actor(
    tmp_path,
    *,
    on_error: str = "buffer_until_full_then_fail",
    max_queue_size: int = 10_000,
    run_writer_thread: bool = False,
    flush_timeout_ms: int = 5_000,
    stop_timeout_ms: int = 5_000,
    strict_stop: bool = False,
    insert_fills_fn=None,
):
    clock = TestClock()
    msgbus = MessageBus(
        trader_id=TestIdStubs.trader_id(),
        clock=clock,
    )
    cache = TestComponentStubs.cache()
    portfolio = Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    db_path = str(tmp_path / "fills.sqlite")

    config = ExecutionFillPersistenceActorConfig(
        component_id="FILL-DB",
        db_path=db_path,
        topic="events.fills.*",
        flush_interval_ms=10,
        max_batch_size=1000,
        flush_time_budget_ms=10,
        flush_timeout_ms=flush_timeout_ms,
        max_queue_size=max_queue_size,
        on_error=on_error,
        stop_timeout_ms=stop_timeout_ms,
        strict_stop=strict_stop,
    )

    actor_kwargs = {"run_writer_thread": run_writer_thread}
    if insert_fills_fn is not None:
        actor_kwargs["insert_fills_fn"] = insert_fills_fn

    actor = ExecutionFillPersistenceActor(config=config, **actor_kwargs)
    actor.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    return actor, msgbus, db_path


def test_actor_subscribes_to_events_fills_only_and_matches_dotted_instrument_ids(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path)
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument)

    actor.start()

    msgbus.publish(topic=f"events.order.{fill.strategy_id.value}", msg=fill)
    actor.flush()
    assert _row_count(db_path) == 0

    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    actor.flush()
    actor.stop()

    assert _row_count(db_path) == 1


def test_actor_threaded_writer_mode_persists_without_thread_affinity_errors(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, run_writer_thread=True)
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    actor.flush()
    actor.stop()

    assert _row_count(db_path) == 1


def test_actor_threaded_flush_is_db_commit_barrier(tmp_path) -> None:
    from nautilus_trader.persistence.fills.sqlite import insert_fills as _real_insert_fills

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_slow(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=1.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_fills(conn, rows)

    actor, msgbus, db_path = _make_actor(
        tmp_path,
        run_writer_thread=True,
        insert_fills_fn=_insert_slow,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    assert write_started.wait(timeout=1.0)

    flush_done = threading.Event()
    flush_errors: list[Exception] = []

    def _flush():
        try:
            actor.flush()
        except Exception as exc:  # pragma: no cover - test assertion captures this
            flush_errors.append(exc)
        finally:
            flush_done.set()

    thread = threading.Thread(target=_flush)
    thread.start()
    assert not flush_done.wait(timeout=0.05)
    release_write.set()
    assert flush_done.wait(timeout=1.0)
    thread.join(timeout=1.0)
    assert not thread.is_alive()
    assert flush_errors == []
    assert _row_count(db_path) == 1
    actor.stop()


def test_actor_start_failure_does_not_leave_subscription(tmp_path) -> None:
    connect_calls = {"count": 0}

    def _connect_once_then_fail(path: str):
        connect_calls["count"] += 1
        if connect_calls["count"] == 1:
            return sqlite3.connect(path)
        raise RuntimeError("writer connect failed")

    actor, msgbus, _ = _make_actor(tmp_path, run_writer_thread=True)
    actor._connect_fn = _connect_once_then_fail

    with pytest.raises(RuntimeError):
        actor.start()

    assert msgbus.subscriptions(actor.config.topic) == []


def test_actor_shutdown_drop_path_marks_queue_tasks_done(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, _, _ = _make_actor(
        tmp_path,
        on_error="buffer_until_full_then_fail",
        run_writer_thread=False,
        insert_fills_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument))
    actor._stop_event.set()
    actor.flush()
    actor.stop()

    assert actor.dropped == 1
    assert actor._queue.unfinished_tasks == 0


def test_actor_threaded_flush_timeout_is_configurable(tmp_path) -> None:
    from nautilus_trader.persistence.fills.sqlite import insert_fills as _real_insert_fills

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_slow(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=1.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_fills(conn, rows)

    actor, msgbus, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        flush_timeout_ms=10,
        insert_fills_fn=_insert_slow,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    assert write_started.wait(timeout=1.0)
    with pytest.raises(RuntimeError, match="flush timed out"):
        actor.flush()
    release_write.set()
    actor.stop()


def test_actor_strict_stop_raises_for_writer_errors(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, _, _ = _make_actor(
        tmp_path,
        on_error="fail_fast",
        run_writer_thread=True,
        strict_stop=True,
        insert_fills_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument))
    with pytest.raises(RuntimeError, match="write failed"):
        actor.flush()
    with pytest.raises(RuntimeError, match="write failed"):
        actor.stop()


def test_actor_enforces_idempotency_and_allows_trade_id_collision(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path)
    instrument = TestInstrumentProvider.btcusdt_binance()

    fill1 = _make_fill(instrument=instrument, ts_event=100)
    fill2 = _make_fill(instrument=instrument, trade_id=fill1.trade_id, ts_event=101)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill1)
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill1)  # Duplicate event_id
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill2)  # Distinct event_id, same trade_id
    actor.flush()
    actor.stop()

    assert _row_count(db_path) == 2
    assert _row_count_for_trade_id(db_path, fill1.trade_id.value) == 2


def test_actor_overflow_policy_tiny_queue_fails_immediately(tmp_path) -> None:
    actor, _, _ = _make_actor(
        tmp_path,
        on_error="buffer_until_full_then_fail",
        max_queue_size=1,
        run_writer_thread=False,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=1))

    with pytest.raises(RuntimeError, match="queue is full"):
        actor.on_order_filled(_make_fill(instrument=instrument, ts_event=2))

    actor.stop()


def test_actor_db_down_fail_fast_raises(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, _, _ = _make_actor(
        tmp_path,
        on_error="fail_fast",
        run_writer_thread=False,
        insert_fills_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument))

    with pytest.raises(RuntimeError, match="write failed"):
        actor.flush()

    actor.stop()


def test_actor_db_down_buffer_until_full_then_fail(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, _, _ = _make_actor(
        tmp_path,
        on_error="buffer_until_full_then_fail",
        max_queue_size=2,
        run_writer_thread=False,
        insert_fills_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=1))
    actor.flush()  # Write failure retained for retry

    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=2))
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=3))
    with pytest.raises(RuntimeError, match="queue is full"):
        actor.on_order_filled(_make_fill(instrument=instrument, ts_event=4))

    assert actor.db_write_errors >= 1
    actor.stop()


def test_actor_db_down_log_and_drop_drops_rows(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, _, db_path = _make_actor(
        tmp_path,
        on_error="log_and_drop",
        run_writer_thread=False,
        insert_fills_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument))
    actor.flush()
    actor.stop()

    assert actor.dropped == 1
    assert _row_count(db_path) == 0


def test_actor_queue_full_log_and_drop_drops_without_raising(tmp_path) -> None:
    actor, _, db_path = _make_actor(
        tmp_path,
        on_error="log_and_drop",
        max_queue_size=1,
        run_writer_thread=False,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=1))
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=2))  # Drops on full queue
    actor.flush()
    actor.stop()

    assert actor.dropped == 1
    assert _row_count(db_path) == 1
