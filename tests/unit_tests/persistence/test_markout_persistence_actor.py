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

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


ACTION_INTENT_TOPIC = "flux.makerv3.order_intent"
FV_TOPIC = "flux.makerv3.fv"
STRATEGY_ID = "MAKERV3-001"


def _fetch_rows(
    db_path: str,
    sql: str,
    params: tuple[object, ...] = (),
) -> list[sqlite3.Row]:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        return conn.execute(sql, params).fetchall()
    finally:
        conn.close()


def _make_actor(
    tmp_path,
    *,
    horizons_s: tuple[int, ...] = (30, 60, 120),
    max_pending_ms: int = 180_000,
    benchmark_name: str = "fv_market_mid",
    benchmark_field: str = "fv",
):
    from nautilus_trader.flux.persistence.markouts.actor import ExecutionMarkoutPersistenceActor
    from nautilus_trader.flux.persistence.markouts.config import (
        ExecutionMarkoutPersistenceActorConfig,
    )

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
    db_path = str(tmp_path / "markouts.sqlite")

    config = ExecutionMarkoutPersistenceActorConfig(
        component_id="MARKOUT-DB",
        db_path=db_path,
        topic="events.fills.*",
        fv_topic=FV_TOPIC,
        action_intent_topic=ACTION_INTENT_TOPIC,
        horizons_s=horizons_s,
        benchmark_name=benchmark_name,
        benchmark_field=benchmark_field,
        max_pending_ms=max_pending_ms,
        flush_interval_ms=10,
        max_batch_size=1000,
        flush_time_budget_ms=10,
        flush_timeout_ms=5_000,
        max_queue_size=10_000,
        on_error="buffer_until_full_then_fail",
        stop_timeout_ms=5_000,
        strict_stop=False,
        propagate_errors_to_bus=False,
    )

    actor = ExecutionMarkoutPersistenceActor(
        config=config,
        run_writer_thread=False,
    )
    actor.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return actor, msgbus, db_path


def _make_fill(
    instrument,
    *,
    side: str,
    last_px: str,
    last_qty: str = "1",
    ts_event: int,
    strategy_id: str = STRATEGY_ID,
    client_order_id: str = "O-19700101-000000-001-001-1",
    trade_id: str = "E-001",
):
    order = TestExecStubs.make_accepted_order(
        instrument=instrument,
        strategy_id=StrategyId(strategy_id),
        client_order_id=ClientOrderId(client_order_id),
        order_side=OrderSide.BUY if side == "BUY" else OrderSide.SELL,
        quantity=instrument.make_qty(float(last_qty)),
    )
    return TestEventStubs.order_filled(
        order=order,
        instrument=instrument,
        strategy_id=StrategyId(strategy_id),
        client_order_id=ClientOrderId(client_order_id),
        trade_id=TradeId(trade_id),
        last_qty=instrument.make_qty(float(last_qty)),
        last_px=instrument.make_price(float(last_px)),
        side=OrderSide.BUY if side == "BUY" else OrderSide.SELL,
        ts_event=ts_event,
    )


def test_markout_actor_persists_resolved_rows_for_each_horizon_and_enriches_from_action_intent(
    tmp_path,
) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30, 60, 120))
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        client_order_id="O-001",
        trade_id="E-001",
    )

    actor.start()
    msgbus.publish(
        topic=ACTION_INTENT_TOPIC,
        msg={
            "strategy_id": STRATEGY_ID,
            "client_order_id": fill.client_order_id.value,
            "intent_type": "PLACE",
            "run_id": "run-telemetry-001",
            "quote_cycle_id": "run-telemetry-001:27",
            "reason_code": "place_missing_level",
            "level_index": 3,
        },
    )
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"101","ts_ms":31000}')
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"102","ts_ms":61000}')
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"103","ts_ms":121000}')
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT
          horizon_s,
          benchmark_px,
          markout_abs,
          resolution_status,
          run_id,
          quote_cycle_id,
          reason_code,
          level_index
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
            row["run_id"],
            row["quote_cycle_id"],
            row["reason_code"],
            row["level_index"],
        )
        for row in rows
    ] == [
        (
            30,
            "101",
            "1",
            "resolved",
            "run-telemetry-001",
            "run-telemetry-001:27",
            "place_missing_level",
            3,
        ),
        (
            60,
            "102",
            "2",
            "resolved",
            "run-telemetry-001",
            "run-telemetry-001:27",
            "place_missing_level",
            3,
        ),
        (
            120,
            "103",
            "3",
            "resolved",
            "run-telemetry-001",
            "run-telemetry-001:27",
            "place_missing_level",
            3,
        ),
    ]


def test_markout_actor_persists_positive_sell_markout_when_fv_falls(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30,))
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="SELL",
        last_px="100",
        ts_event=2_000_000_000,
        client_order_id="O-002",
        trade_id="E-002",
    )

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"99","ts_ms":32000}')
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT horizon_s, benchmark_px, markout_abs, markout_bps, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["markout_bps"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        (30, "99", "1", "100", "resolved"),
    ]


def test_markout_actor_uses_configured_benchmark_field_for_local_market_mid(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(
        tmp_path,
        horizons_s=(30,),
        benchmark_name="local_mkt_mid",
        benchmark_field="maker_mid",
    )
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        client_order_id="O-LOCAL-001",
        trade_id="E-LOCAL-001",
    )

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    msgbus.publish(
        topic=FV_TOPIC,
        msg='{"strategy_id":"MAKERV3-001","fv":"101","maker_mid":"100.5","ts_ms":31000}',
    )
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT benchmark_name, horizon_s, benchmark_px, markout_abs, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["benchmark_name"],
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        ("local_mkt_mid", 30, "100.5", "0.5", "resolved"),
    ]


def test_markout_actor_resolves_zero_second_edge_from_latest_cached_benchmark(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(0, 30))
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        client_order_id="O-ZERO-001",
        trade_id="E-ZERO-001",
    )

    actor.start()
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"100.25","ts_ms":1000}')
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"101","ts_ms":31000}')
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT horizon_s, benchmark_ts_ms, benchmark_px, markout_abs, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["horizon_s"],
            row["benchmark_ts_ms"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        (0, 1000, "100.25", "0.25", "resolved"),
        (30, 31000, "101", "1", "resolved"),
    ]


def test_markout_actor_resolves_fv_for_runtime_strategy_instance_id(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30,))
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        strategy_id="MAKERV3-001-000",
        client_order_id="O-005",
        trade_id="E-005",
    )

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    msgbus.publish(topic=FV_TOPIC, msg='{"strategy_id":"MAKERV3-001","fv":"101","ts_ms":31000}')
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT horizon_s, benchmark_px, markout_abs, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        (30, "101", "1", "resolved"),
    ]


def test_markout_actor_persists_stopped_rows_for_pending_horizons_on_stop(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30, 60))
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        client_order_id="O-STOP-001",
        trade_id="E-STOP-001",
    )

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT horizon_s, benchmark_px, markout_abs, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        (30, None, None, "stopped"),
        (60, None, None, "stopped"),
    ]


def test_markout_actor_expires_rows_when_future_fv_never_arrives(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30,), max_pending_ms=500)
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        client_order_id="O-003",
        trade_id="E-003",
    )

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    assert actor.clock is not None
    actor.clock.advance_time(40_000_000_000)
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        """
        SELECT horizon_s, benchmark_px, markout_abs, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )

    assert [
        (
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        (30, None, None, "expired"),
    ]


def test_markout_actor_uses_periodic_expiry_timer_for_pending_rows(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30,), max_pending_ms=500)
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(
        instrument,
        side="BUY",
        last_px="100",
        ts_event=1_000_000_000,
        client_order_id="O-004",
        trade_id="E-004",
    )

    actor.start()
    assert actor.clock is not None
    timer_name = "execution-markout-expiry:MARKOUT-DB"
    assert timer_name in actor.clock.timer_names

    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    event_handlers = actor.clock.advance_time(40_000_000_000)
    expiry_handlers = [handler for handler in event_handlers if handler.event.name == timer_name]
    assert expiry_handlers

    for handler in expiry_handlers:
        handler.handle()

    actor.flush()
    rows = _fetch_rows(
        db_path,
        """
        SELECT horizon_s, benchmark_px, markout_abs, resolution_status
        FROM execution_markout
        ORDER BY horizon_s
        """,
    )
    actor.stop()

    assert [
        (
            row["horizon_s"],
            row["benchmark_px"],
            row["markout_abs"],
            row["resolution_status"],
        )
        for row in rows
    ] == [
        (30, None, None, "expired"),
    ]
    assert timer_name not in actor.clock.timer_names
