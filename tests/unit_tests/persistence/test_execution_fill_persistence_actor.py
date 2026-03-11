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

import json
import sqlite3
import threading
import time

import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.persistence.fills.actor import ExecutionFillPersistenceActor
from nautilus_trader.persistence.fills.actor import _writer_startup_timeout_seconds
from nautilus_trader.persistence.fills.config import ExecutionFillPersistenceActorConfig
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


ACTION_INTENT_TOPIC = "flux.makerv3.order_intent"
EXECUTION_TIMING_TOPIC = "events.execution.timing"


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
    on_error: str = "buffer_until_full_then_fail",
    max_queue_size: int = 10_000,
    run_writer_thread: bool = False,
    flush_timeout_ms: int = 5_000,
    stop_timeout_ms: int = 5_000,
    strict_stop: bool = False,
    propagate_errors_to_bus: bool = False,
    insert_fills_fn=None,
    action_intent_topic: str | None = None,
    execution_timing_topic: str | None = None,
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

    config_kwargs = {
        "component_id": "FILL-DB",
        "db_path": db_path,
        "topic": "events.fills.*",
        "flush_interval_ms": 10,
        "max_batch_size": 1000,
        "flush_time_budget_ms": 10,
        "flush_timeout_ms": flush_timeout_ms,
        "max_queue_size": max_queue_size,
        "on_error": on_error,
        "stop_timeout_ms": stop_timeout_ms,
        "strict_stop": strict_stop,
        "propagate_errors_to_bus": propagate_errors_to_bus,
    }
    if action_intent_topic is not None:
        config_kwargs["action_intent_topic"] = action_intent_topic
    if execution_timing_topic is not None:
        config_kwargs["execution_timing_topic"] = execution_timing_topic
    config = ExecutionFillPersistenceActorConfig(**config_kwargs)

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


def test_actor_threaded_publish_returns_before_fill_row_transform_runs(
    tmp_path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    from nautilus_trader.persistence.fills import actor as fills_actor
    from nautilus_trader.persistence.fills.sqlite import fill_to_row as _real_fill_to_row

    transform_started = threading.Event()
    release_transform = threading.Event()
    publish_done = threading.Event()

    def _fill_to_row_blocking(event, **kwargs):
        transform_started.set()
        if not release_transform.wait(timeout=1.0):
            raise RuntimeError("test transform gate timeout")
        return _real_fill_to_row(event, **kwargs)

    monkeypatch.setattr(fills_actor, "fill_to_row", _fill_to_row_blocking)

    actor, msgbus, db_path = _make_actor(tmp_path, run_writer_thread=True)
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=150)

    actor.start()

    thread = threading.Thread(
        target=lambda: (msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill), publish_done.set()),
    )
    thread.start()

    assert publish_done.wait(timeout=0.1)
    assert transform_started.wait(timeout=1.0)

    release_transform.set()
    actor.flush()
    actor.stop()
    thread.join(timeout=1.0)

    assert not thread.is_alive()
    assert _row_count(db_path) == 1


def test_actor_snapshots_fill_info_before_background_transform(tmp_path) -> None:
    actor, _, db_path = _make_actor(tmp_path, run_writer_thread=False)
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=160)
    fill.info["persisted"] = "before"

    actor.start()
    actor.on_order_filled(fill)

    fill.info["persisted"] = "after"

    actor.flush()
    actor.stop()

    row = _fetch_one(
        db_path,
        "SELECT info_json FROM execution_fill WHERE event_id = ?",
        (fill.id.value,),
    )
    assert row is not None
    assert '"before"' in row["info_json"]
    assert '"after"' not in row["info_json"]


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
        except Exception as e:  # pragma: no cover - test assertion captures this
            flush_errors.append(e)
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


def test_actor_overflow_policy_tiny_queue_disables_persistence_after_first_overflow_without_raising(
    tmp_path,
) -> None:
    actor, _, db_path = _make_actor(
        tmp_path,
        on_error="buffer_until_full_then_fail",
        max_queue_size=1,
        run_writer_thread=False,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=1))
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=2))
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=3))

    actor.flush()
    actor.stop()

    assert actor.dropped == 2
    assert actor.persistence_disabled
    assert actor._writer_error is None
    assert _row_count(db_path) == 1


def test_actor_overflow_policy_tiny_queue_fails_immediately_when_propagating_errors(tmp_path) -> None:
    actor, _, _ = _make_actor(
        tmp_path,
        on_error="buffer_until_full_then_fail",
        max_queue_size=1,
        run_writer_thread=False,
        propagate_errors_to_bus=True,
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
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=4))

    assert actor.db_write_errors >= 1
    assert actor.dropped == 1
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


def test_actor_threaded_db_down_fail_fast_disables_persistence_without_raising_when_not_propagating(
    tmp_path,
) -> None:
    def _insert_fail(_conn, _rows):
        raise RuntimeError("db down")

    actor, _, _ = _make_actor(
        tmp_path,
        on_error="fail_fast",
        run_writer_thread=True,
        insert_fills_fn=_insert_fail,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=1))

    deadline = time.monotonic() + 1.0
    while time.monotonic() < deadline and actor.db_write_errors == 0:
        time.sleep(0.01)

    assert actor.db_write_errors >= 1

    actor.on_order_filled(_make_fill(instrument=instrument, ts_event=2))
    actor.stop()

    assert actor.persistence_disabled
    assert actor.dropped == 2
    assert actor._writer_error is not None


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


def test_actor_pending_rows_respect_retry_backoff_before_retry_deadline(tmp_path) -> None:
    from nautilus_trader.persistence.fills.sqlite import fill_to_row

    actor, _, _ = _make_actor(tmp_path, run_writer_thread=False)
    instrument = TestInstrumentProvider.btcusdt_binance()

    actor.start()
    actor._pending_rows.append(fill_to_row(_make_fill(instrument=instrument, ts_event=700)))
    actor._next_retry_after = time.monotonic() + 60.0

    assert actor._next_batch() == []
    assert len(actor._pending_rows) == 1

    actor._pending_rows.clear()
    actor.stop()


def test_actor_non_strict_stop_timeout_sets_error_and_releases_refs_after_writer_finishes(
    tmp_path,
) -> None:
    from nautilus_trader.persistence.fills.sqlite import insert_fills as _real_insert_fills

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_blocking(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=5.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_fills(conn, rows)

    actor, msgbus, _ = _make_actor(
        tmp_path,
        run_writer_thread=True,
        stop_timeout_ms=10,
        strict_stop=False,
        insert_fills_fn=_insert_blocking,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=104)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
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


def test_actor_strict_stop_timeout_allows_replacement_actor_restart_after_cleanup(tmp_path) -> None:
    from nautilus_trader.persistence.fills.sqlite import insert_fills as _real_insert_fills

    write_started = threading.Event()
    release_write = threading.Event()

    def _insert_blocking(conn, rows):
        write_started.set()
        if not release_write.wait(timeout=5.0):
            raise RuntimeError("test write gate timeout")
        return _real_insert_fills(conn, rows)

    actor, msgbus, db_path = _make_actor(
        tmp_path,
        run_writer_thread=True,
        stop_timeout_ms=10,
        strict_stop=True,
        insert_fills_fn=_insert_blocking,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill1 = _make_fill(instrument=instrument, ts_event=205)
    fill2 = _make_fill(instrument=instrument, ts_event=206)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill1)
    assert write_started.wait(timeout=1.0)

    with pytest.raises(RuntimeError, match="did not stop cleanly"):
        actor.stop()

    release_write.set()
    assert actor._writer_cleanup_done.wait(timeout=1.0)

    replacement_actor, replacement_msgbus, replacement_db_path = _make_actor(
        tmp_path,
        run_writer_thread=True,
    )
    assert replacement_db_path == db_path

    replacement_actor.start()
    replacement_msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill2)
    replacement_actor.flush()
    replacement_actor.stop()

    assert _row_count(db_path) == 2


def test_actor_startup_timeout_uses_flush_timeout_when_it_is_largest_budget() -> None:
    config = ExecutionFillPersistenceActorConfig(
        component_id="FILL-DB",
        db_path="fills.sqlite",
        flush_interval_ms=10,
        flush_timeout_ms=2_000,
        stop_timeout_ms=500,
    )

    timeout = _writer_startup_timeout_seconds(config)

    assert timeout == 2.0


def test_actor_enriches_fill_from_place_action_intent_and_sets_ts_ingest_ns(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(
        tmp_path,
        run_writer_thread=False,
        action_intent_topic=ACTION_INTENT_TOPIC,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=7_000)

    actor.start()
    assert actor.clock is not None
    actor.clock.advance_time(9_999_000_000)
    msgbus.publish(
        topic=ACTION_INTENT_TOPIC,
        msg={
            "strategy_id": fill.strategy_id.value,
            "client_order_id": fill.client_order_id.value,
            "intent_type": "PLACE",
            "run_id": "run-telemetry-001",
            "quote_cycle_id": "run-telemetry-001:27",
            "reason_code": "place_missing_level",
            "level_index": 3,
            "target_px": "100.55",
            "cancel_px": None,
            "match_tol": "0.05",
            "ts_market_data_event_ns": 4_111,
            "ts_market_data_recv_ns": 4_222,
            "ts_decision_ns": 4_333,
            "ts_submit_local_ns": 4_444,
            "decision_context_json": {
                "effective_edge_bps": "4.1",
                "maker_role": "lead",
            },
        },
    )
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    actor.flush()
    actor.stop()

    row = _fetch_one(
        db_path,
        """
        SELECT
          run_id,
          quote_cycle_id,
          reason_code,
          level_index,
          target_px,
          cancel_px,
          match_tol,
          ts_market_data_event_ns,
          ts_market_data_recv_ns,
          ts_decision_ns,
          ts_submit_local_ns,
          ts_ingest_ns,
          info_json
        FROM execution_fill
        WHERE event_id = ?
        """,
        (fill.id.value,),
    )
    assert row is not None
    assert row["run_id"] == "run-telemetry-001"
    assert row["quote_cycle_id"] == "run-telemetry-001:27"
    assert row["reason_code"] == "place_missing_level"
    assert row["level_index"] == 3
    assert row["target_px"] == "100.55"
    assert row["cancel_px"] is None
    assert row["match_tol"] == "0.05"
    assert row["ts_market_data_event_ns"] == 4_111
    assert row["ts_market_data_recv_ns"] == 4_222
    assert row["ts_decision_ns"] == 4_333
    assert row["ts_submit_local_ns"] == 4_444
    assert row["ts_ingest_ns"] == 9_999_000_000
    assert json.loads(row["info_json"]) == {}


def test_actor_enriches_fill_from_execution_timing_topic(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(
        tmp_path,
        run_writer_thread=False,
        execution_timing_topic=EXECUTION_TIMING_TOPIC,
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, ts_event=7_100)

    actor.start()
    msgbus.publish(
        topic=EXECUTION_TIMING_TOPIC,
        msg={
            "strategy_id": fill.strategy_id.value,
            "client_order_id": fill.client_order_id.value,
            "action_type": "PLACE",
            "ts_command_init_ns": 5_100,
            "ts_risk_recv_ns": 5_200,
            "ts_risk_forward_ns": 5_300,
            "ts_exec_recv_ns": 5_400,
            "ts_exec_forward_ns": 5_500,
            "ts_client_submit_ns": 5_600,
            "ts_adapter_submit_start_ns": 5_700,
        },
    )
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    actor.flush()
    actor.stop()

    row = _fetch_one(
        db_path,
        """
        SELECT
          ts_command_init_ns,
          ts_risk_recv_ns,
          ts_risk_forward_ns,
          ts_exec_recv_ns,
          ts_exec_forward_ns,
          ts_client_submit_ns,
          ts_adapter_submit_start_ns
        FROM execution_fill
        WHERE event_id = ?
        """,
        (fill.id.value,),
    )
    assert row is not None
    assert row["ts_command_init_ns"] == 5_100
    assert row["ts_risk_recv_ns"] == 5_200
    assert row["ts_risk_forward_ns"] == 5_300
    assert row["ts_exec_recv_ns"] == 5_400
    assert row["ts_exec_forward_ns"] == 5_500
    assert row["ts_client_submit_ns"] == 5_600
    assert row["ts_adapter_submit_start_ns"] == 5_700
