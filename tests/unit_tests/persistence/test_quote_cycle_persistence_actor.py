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

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


TOPIC_EVENT = "flux.makerv3.event"


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


def _make_actor(tmp_path):
    from nautilus_trader.flux.persistence.quote_cycles.actor import QuoteCyclePersistenceActor
    from nautilus_trader.flux.persistence.quote_cycles.config import QuoteCyclePersistenceActorConfig

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
    db_path = str(tmp_path / "quote_cycles.sqlite")

    config = QuoteCyclePersistenceActorConfig(
        component_id="QUOTE-CYCLE-DB",
        db_path=db_path,
        topic=TOPIC_EVENT,
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

    actor = QuoteCyclePersistenceActor(
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


def test_quote_cycle_actor_persists_skipped_cycles_without_heavy_decision_context(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path)

    actor.start()
    msgbus.publish(
        topic=TOPIC_EVENT,
        msg=json.dumps(
            {
                "event": "quote_cycle",
                "strategy_id": "MAKERV3-001",
                "instrument_id": "ETHUSDT.BINANCE",
                "run_id": "run-telemetry-001",
                "quote_cycle_id": "run-telemetry-001:1",
                "quote_cycle_seq": 1,
                "quote_cycle_event": "skipped",
                "reason_code": "skip_requote_throttled",
                "trigger_source": "maker_bbo_update",
                "trigger_instrument_id": "ETHUSDT.BINANCE",
                "trigger_md_ts_event_ns": 1_001,
                "trigger_md_ts_init_ns": 1_002,
                "ts_cycle_start_ns": 1_010,
                "ts_cycle_end_ns": 1_011,
            },
        ),
    )
    actor.flush()
    actor.stop()

    row = _fetch_one(
        db_path,
        """
        SELECT
          run_id,
          quote_cycle_id,
          quote_cycle_seq,
          quote_cycle_event,
          reason_code,
          trigger_source,
          trigger_md_ts_event_ns,
          trigger_md_ts_init_ns,
          ts_cycle_start_ns,
          ts_cycle_end_ns,
          decision_context_json
        FROM quote_cycle
        WHERE quote_cycle_id = ?
        """,
        ("run-telemetry-001:1",),
    )
    assert row is not None
    assert row["run_id"] == "run-telemetry-001"
    assert row["quote_cycle_id"] == "run-telemetry-001:1"
    assert row["quote_cycle_seq"] == 1
    assert row["quote_cycle_event"] == "skipped"
    assert row["reason_code"] == "skip_requote_throttled"
    assert row["trigger_source"] == "maker_bbo_update"
    assert row["trigger_md_ts_event_ns"] == 1_001
    assert row["trigger_md_ts_init_ns"] == 1_002
    assert row["ts_cycle_start_ns"] == 1_010
    assert row["ts_cycle_end_ns"] == 1_011
    assert row["decision_context_json"] == "null"


def test_quote_cycle_actor_persists_heavy_completed_cycles_with_decision_context_json(
    tmp_path,
) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path)

    actor.start()
    msgbus.publish(
        topic=TOPIC_EVENT,
        msg=json.dumps(
            {
                "event": "quote_cycle",
                "strategy_id": "MAKERV3-001",
                "instrument_id": "ETHUSDT.BINANCE",
                "run_id": "run-telemetry-001",
                "quote_cycle_id": "run-telemetry-001:2",
                "quote_cycle_seq": 2,
                "quote_cycle_event": "completed",
                "reason_code": "completed_rebalanced",
                "trigger_source": "maker_bbo_update",
                "trigger_instrument_id": "ETHUSDT.BINANCE",
                "trigger_md_ts_event_ns": 2_001,
                "trigger_md_ts_init_ns": 2_002,
                "ts_cycle_start_ns": 2_010,
                "ts_cycle_end_ns": 2_099,
                "cancel_count": 2,
                "place_count": 4,
                "bid_levels": 3,
                "ask_levels": 3,
                "decision_context_json": {
                    "pricing_debug": {
                        "effective_edge_bps": "4.1",
                        "inventory_skew_bps": "1.2",
                    },
                    "runtime_params": {
                        "max_orders_per_side": 3,
                        "quote_width_bps": "6.0",
                    },
                    "maker_quote_status": {
                        "maker_book_ready": True,
                        "reference_md_ready": True,
                    },
                    "per_level_outcomes": [
                        {"side": "BUY", "level_index": 0, "outcome": "placed"},
                        {"side": "SELL", "level_index": 2, "outcome": "skipped_existing_match"},
                    ],
                },
            },
        ),
    )
    actor.flush()
    actor.stop()

    row = _fetch_one(
        db_path,
        """
        SELECT
          quote_cycle_event,
          reason_code,
          cancel_count,
          place_count,
          bid_levels,
          ask_levels,
          decision_context_json
        FROM quote_cycle
        WHERE quote_cycle_id = ?
        """,
        ("run-telemetry-001:2",),
    )
    assert row is not None
    assert row["quote_cycle_event"] == "completed"
    assert row["reason_code"] == "completed_rebalanced"
    assert row["cancel_count"] == 2
    assert row["place_count"] == 4
    assert row["bid_levels"] == 3
    assert row["ask_levels"] == 3
    assert json.loads(row["decision_context_json"]) == {
        "pricing_debug": {
            "effective_edge_bps": "4.1",
            "inventory_skew_bps": "1.2",
        },
        "runtime_params": {
            "max_orders_per_side": 3,
            "quote_width_bps": "6.0",
        },
        "maker_quote_status": {
            "maker_book_ready": True,
            "reference_md_ready": True,
        },
        "per_level_outcomes": [
            {"side": "BUY", "level_index": 0, "outcome": "placed"},
            {"side": "SELL", "level_index": 2, "outcome": "skipped_existing_match"},
        ],
    }


def test_quote_cycle_schema_has_core_columns_for_latency_and_debug_context(tmp_path) -> None:
    from nautilus_trader.flux.persistence.quote_cycles.sqlite import connect
    from nautilus_trader.flux.persistence.quote_cycles.sqlite import ensure_schema

    db_path = tmp_path / "quote_cycles.sqlite"
    conn = connect(str(db_path))
    ensure_schema(conn)

    columns = {
        row[1]
        for row in conn.execute("PRAGMA table_info(quote_cycle)").fetchall()
    }

    assert "strategy_id" in columns
    assert "instrument_id" in columns
    assert "run_id" in columns
    assert "quote_cycle_id" in columns
    assert "quote_cycle_seq" in columns
    assert "quote_cycle_event" in columns
    assert "reason_code" in columns
    assert "trigger_source" in columns
    assert "trigger_instrument_id" in columns
    assert "trigger_md_ts_event_ns" in columns
    assert "trigger_md_ts_init_ns" in columns
    assert "ts_cycle_start_ns" in columns
    assert "ts_cycle_end_ns" in columns
    assert "cancel_count" in columns
    assert "place_count" in columns
    assert "bid_levels" in columns
    assert "ask_levels" in columns
    assert "decision_context_json" in columns

    conn.close()
