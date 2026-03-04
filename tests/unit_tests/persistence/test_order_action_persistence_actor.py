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
from nautilus_trader.persistence.orders.actor import OrderActionPersistenceActor
from nautilus_trader.persistence.orders.actor import _current_ts_ingest_ns
from nautilus_trader.persistence.orders.actor import _writer_startup_timeout_seconds
from nautilus_trader.persistence.orders.actor import order_event_to_row
from nautilus_trader.persistence.orders.config import OrderActionPersistenceActorConfig
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _row_count(db_path: str) -> int:
    conn = sqlite3.connect(db_path)
    try:
        return conn.execute("SELECT COUNT(*) FROM order_action").fetchone()[0]
    finally:
        conn.close()


def _rows(db_path: str) -> list[sqlite3.Row]:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        return conn.execute(
            """
            SELECT event_type, action_type, action_state, ts_event, ts_init, ts_ingest
            FROM order_action
            ORDER BY ts_event
            """,
        ).fetchall()
    finally:
        conn.close()


def _fetch_one(
    db_path: str,
    sql: str,
    params: tuple[object, ...] = (),
) -> sqlite3.Row | None:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        return conn.execute(sql, params).fetchone()
    finally:
        conn.close()


def _make_actor(
    tmp_path,
    *,
    event_types: tuple[str, ...] | None = None,
    run_writer_thread: bool = False,
    max_batch_size: int = 1000,
    flush_timeout_ms: int = 5_000,
    stop_timeout_ms: int = 5_000,
    strict_stop: bool = False,
    on_error: str = "buffer_until_full_then_fail",
    insert_many_fn=None,
    connect_fn=None,
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
    db_path = str(tmp_path / "orders.sqlite")

    config_kwargs = {
        "component_id": "ORDER-ACTION-DB",
        "db_path": db_path,
        "topic": "events.order.*",
        "flush_interval_ms": 10,
        "max_batch_size": max_batch_size,
        "flush_time_budget_ms": 10,
        "flush_timeout_ms": flush_timeout_ms,
        "max_queue_size": 10_000,
        "on_error": on_error,
        "stop_timeout_ms": stop_timeout_ms,
        "strict_stop": strict_stop,
    }
    if event_types is not None:
        config_kwargs["event_types"] = event_types
    config = OrderActionPersistenceActorConfig(**config_kwargs)

    actor_kwargs = {"run_writer_thread": run_writer_thread}
    if insert_many_fn is not None:
        actor_kwargs["insert_many_fn"] = insert_many_fn
    if connect_fn is not None:
        actor_kwargs["connect_fn"] = connect_fn

    actor = OrderActionPersistenceActor(config=config, **actor_kwargs)
    actor.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return actor, msgbus, db_path, clock


def test_actor_filters_to_configured_order_event_types_and_sets_ts_ingest(tmp_path) -> None:
    actor, msgbus, db_path, clock = _make_actor(
        tmp_path,
        event_types=("OrderAccepted",),
        run_writer_thread=False,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted = TestEventStubs.order_accepted(order=order, ts_event=101)
    rejected = TestEventStubs.order_rejected(order=order, ts_event=102)

    actor.start()
    clock.advance_time(123_456_789)
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted)
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=rejected)
    actor.flush()
    actor.stop()

    rows = _rows(db_path)
    assert len(rows) == 1
    row = rows[0]
    assert row["event_type"] == "OrderAccepted"
    assert row["action_type"] == "PLACE"
    assert row["action_state"] == "ACCEPTED"
    assert row["ts_event"] == 101
    assert row["ts_init"] == 0
    assert row["ts_ingest"] == 123_456_789


def test_current_ts_ingest_ns_uses_system_clock_when_clock_is_missing() -> None:
    ts_ingest = _current_ts_ingest_ns(None)
    assert ts_ingest > 0


class _FakeOrderEvent:
    def __init__(self, data: dict[str, object]) -> None:
        self._data = data

    def to_dict(self, _event: object) -> dict[str, object]:
        return self._data


def test_order_event_to_row_uses_trigger_price_when_price_not_present() -> None:
    fake = _FakeOrderEvent(
        {
            "trader_id": "TRADER-001",
            "event_id": "event-1",
            "strategy_id": "STRAT-001",
            "instrument_id": "ETHUSDT.BINANCE",
            "client_order_id": "client-1",
            "account_id": None,
            "venue_order_id": None,
            "position_id": None,
            "order_side": "BUY",
            "order_type": "STOP_MARKET",
            "time_in_force": "GTC",
            "post_only": False,
            "reduce_only": False,
            "quantity": "1.0",
            "options": {"trigger_price": "105.25"},
            "tags": [],
            "ts_event": 100,
            "ts_init": 90,
            "reconciliation": False,
        },
    )

    row = order_event_to_row(
        fake,  # type: ignore[arg-type]
        event_type="OrderInitialized",
        ts_ingest=123,
    )

    assert row is not None
    assert row.order_px == "105.25"


def test_actor_threaded_flush_is_db_commit_barrier(tmp_path) -> None:
    from nautilus_trader.persistence.orders.sqlite import insert_many as _real_insert_many

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_slow(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=1.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_many(conn, rows)

    actor, msgbus, db_path, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        insert_many_fn=_insert_slow,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted = TestEventStubs.order_accepted(order=order, ts_event=103)

    actor.start()
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted)
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


def test_actor_non_strict_stop_timeout_sets_error_and_releases_refs_after_writer_finishes(
    tmp_path,
) -> None:
    from nautilus_trader.persistence.orders.sqlite import insert_many as _real_insert_many

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_blocking(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=5.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_many(conn, rows)

    actor, msgbus, _, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        stop_timeout_ms=10,
        strict_stop=False,
        insert_many_fn=_insert_blocking,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted = TestEventStubs.order_accepted(order=order, ts_event=104)

    actor.start()
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted)
    assert write_started.wait(timeout=1.0)

    actor.stop()
    assert actor._writer_error is not None
    assert "did not stop cleanly" in str(actor._writer_error)
    assert actor._writer_thread is None
    assert not actor._writer_cleanup_done.is_set()
    assert actor._conn is not None

    release_write.set()
    assert actor._writer_cleanup_done.wait(timeout=1.0)
    assert actor._conn is None


def test_actor_strict_stop_timeout_cleans_refs_and_signals_cleanup_in_progress(tmp_path) -> None:
    from nautilus_trader.persistence.orders.sqlite import insert_many as _real_insert_many

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_blocking(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=5.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_many(conn, rows)

    actor, msgbus, _, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        stop_timeout_ms=10,
        strict_stop=True,
        insert_many_fn=_insert_blocking,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted = TestEventStubs.order_accepted(order=order, ts_event=105)

    actor.start()
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted)
    assert write_started.wait(timeout=1.0)

    with pytest.raises(RuntimeError, match="did not stop cleanly"):
        actor.stop()

    assert actor._writer_error is not None
    assert actor._writer_thread is None
    assert not actor._writer_cleanup_done.is_set()
    assert actor._conn is not None

    release_write.set()
    assert actor._writer_cleanup_done.wait(timeout=1.0)
    assert actor._conn is None


def test_actor_strict_stop_timeout_allows_replacement_actor_restart_after_cleanup(tmp_path) -> None:
    from nautilus_trader.persistence.orders.sqlite import insert_many as _real_insert_many

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_blocking(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=5.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_many(conn, rows)

    actor, msgbus, db_path, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        stop_timeout_ms=10,
        strict_stop=True,
        insert_many_fn=_insert_blocking,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted1 = TestEventStubs.order_accepted(order=order, ts_event=205)
    accepted2 = TestEventStubs.order_accepted(order=order, ts_event=206)

    actor.start()
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted1)
    assert write_started.wait(timeout=1.0)

    with pytest.raises(RuntimeError, match="did not stop cleanly"):
        actor.stop()

    release_write.set()
    assert actor._writer_cleanup_done.wait(timeout=1.0)

    replacement_actor, replacement_msgbus, replacement_db_path, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
    )
    assert replacement_db_path == db_path

    replacement_actor.start()
    replacement_msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted2)
    replacement_actor.flush()
    replacement_actor.stop()

    assert _row_count(db_path) == 2


def test_writer_startup_timeout_uses_flush_timeout_when_it_is_largest_budget() -> None:
    config = OrderActionPersistenceActorConfig(
        component_id="ORDER-ACTION-DB",
        db_path="orders.sqlite",
        flush_interval_ms=10,
        flush_timeout_ms=2_000,
        stop_timeout_ms=500,
    )

    timeout = _writer_startup_timeout_seconds(config)

    assert timeout == 2.0


def test_actor_strict_stop_timeout_with_backlog_larger_than_batch_drains_and_completes_cleanup(
    tmp_path,
) -> None:
    from nautilus_trader.persistence.orders.sqlite import insert_many as _real_insert_many

    write_started = threading.Event()
    release_write = threading.Event()
    insert_calls = {"count": 0}

    def _insert_block_first(conn, rows):
        insert_calls["count"] += 1
        if insert_calls["count"] == 1:
            write_started.set()
            if not release_write.wait(timeout=5.0):
                raise RuntimeError("test write gate timeout")
        return _real_insert_many(conn, rows)

    actor, msgbus, db_path, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        max_batch_size=2,
        stop_timeout_ms=10,
        strict_stop=True,
        insert_many_fn=_insert_block_first,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)

    actor.start()
    msgbus.publish(
        topic=f"events.order.{order.strategy_id.value}",
        msg=TestEventStubs.order_accepted(order=order, ts_event=300),
    )
    assert write_started.wait(timeout=1.0)

    # Build backlog while first write remains blocked.
    for i in range(1, 6):
        msgbus.publish(
            topic=f"events.order.{order.strategy_id.value}",
            msg=TestEventStubs.order_accepted(order=order, ts_event=300 + i),
        )

    with pytest.raises(RuntimeError, match="did not stop cleanly"):
        actor.stop()

    assert not actor._writer_cleanup_done.is_set()

    release_write.set()
    assert actor._writer_cleanup_done.wait(timeout=2.0)
    assert _row_count(db_path) == 6

    # Gating should unblock once cleanup completes.
    actor.start()
    actor.stop()


def test_actor_db_down_log_and_drop_drops_rows(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, msgbus, db_path, _ = _make_actor(
        tmp_path,
        on_error="log_and_drop",
        run_writer_thread=False,
        insert_many_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted = TestEventStubs.order_accepted(order=order, ts_event=106)

    actor.start()
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted)
    actor.flush()
    actor.stop()

    assert actor.dropped == 1
    assert actor.db_write_errors == 1
    assert _row_count(db_path) == 0
    assert actor._queue.unfinished_tasks == 0


def test_actor_db_down_fail_fast_raises_and_sets_writer_error(tmp_path) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, msgbus, db_path, _ = _make_actor(
        tmp_path,
        on_error="fail_fast",
        run_writer_thread=False,
        insert_many_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.make_accepted_order(instrument=instrument)
    accepted = TestEventStubs.order_accepted(order=order, ts_event=107)

    actor.start()
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=accepted)
    with pytest.raises(RuntimeError, match="write failed"):
        actor.flush()

    assert actor.db_write_errors == 1
    assert actor._writer_error is not None
    assert _row_count(db_path) == 0

    actor.stop()


def test_actor_startup_failure_cleans_resources(tmp_path) -> None:
    def _connect_fail(_path: str):
        raise RuntimeError("connect fail")

    actor, _, _, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        connect_fn=_connect_fail,
    )

    with pytest.raises(RuntimeError, match="connect fail"):
        actor.start()

    assert actor._conn is None
    assert actor._writer_thread is None
    assert actor._writer_cleanup_thread is None
    assert actor._writer_cleanup_done.is_set()
    assert len(actor._pending_rows) == 0


def test_actor_parses_order_initialized_tags_with_invalid_decision_and_signal_fallback(tmp_path) -> None:
    actor, msgbus, db_path, clock = _make_actor(
        tmp_path,
        event_types=("OrderInitialized",),
        run_writer_thread=False,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    order = TestExecStubs.limit_order(
        instrument=instrument,
        tags=[
            "nautilus.intent.action_id=act-1",
            "nautilus.intent.reason=quote:reprice",
            "nautilus.intent.ts_decision_ns=invalid-int",
            "nautilus.intent.signal={bad-json",
        ],
    )
    initialized = order.init_event

    actor.start()
    clock.advance_time(987_654_321)
    msgbus.publish(topic=f"events.order.{order.strategy_id.value}", msg=initialized)
    actor.flush()
    actor.stop()

    row = _fetch_one(
        db_path,
        """
        SELECT
          event_type,
          action_id,
          action_reason,
          ts_decision_ns,
          signal_snapshot_json,
          ts_ingest
        FROM order_action
        WHERE event_type = 'OrderInitialized'
        """,
    )
    assert row is not None
    assert row["event_type"] == "OrderInitialized"
    assert row["action_id"] == "act-1"
    assert row["action_reason"] == "quote:reprice"
    assert row["ts_decision_ns"] is None
    assert row["signal_snapshot_json"] == "\"{bad-json\""
    assert row["ts_ingest"] == 987_654_321
