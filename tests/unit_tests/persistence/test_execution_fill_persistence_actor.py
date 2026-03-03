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
        max_queue_size=max_queue_size,
        on_error=on_error,
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

