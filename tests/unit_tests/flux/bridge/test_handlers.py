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

from nautilus_trader.flux.bridge.handlers import default_topic_handlers
from nautilus_trader.flux.bridge.handlers.alerts import transform_alert
from nautilus_trader.flux.bridge.handlers.balances import transform_balances
from nautilus_trader.flux.bridge.handlers.events import transform_event
from nautilus_trader.flux.bridge.handlers.fv import transform_fv
from nautilus_trader.flux.bridge.handlers.market_bbo import transform_market_bbo
from nautilus_trader.flux.bridge.handlers.state import transform_state
from nautilus_trader.flux.bridge.handlers.trades import transform_trade
from nautilus_trader.flux.bridge.handlers.types import CorrelationContext
from nautilus_trader.flux.bridge.handlers.types import ReplaceHashJSONOp
from nautilus_trader.flux.bridge.handlers.types import SetJSONOp
from nautilus_trader.flux.bridge.handlers.types import StreamJSONOp


def _context(topic: str) -> CorrelationContext:
    return CorrelationContext(
        strategy_id="maker_v3_01",
        topic=topic,
        entry_id="1700000001000-0",
        ts_ms=1700000001000,
    )


def test_default_topic_handlers_uses_modular_topic_mapping() -> None:
    handlers = default_topic_handlers()

    assert sorted(handlers.keys()) == [
        "alert",
        "balances",
        "event",
        "fv",
        "market_bbo",
        "state",
        "trade",
    ]


def test_transform_state_adds_correlation_context_and_ts_ms() -> None:
    ops = transform_state({"mode": "quote", "timestamp": "1700000000"}, _context("state"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, SetJSONOp)
    assert op.key == "flux:v1:state:maker_v3_01"
    assert op.value["strategy_id"] == "maker_v3_01"
    assert op.value["topic"] == "state"
    assert op.value["entry_id"] == "1700000001000-0"
    assert op.value["ts_ms"] == 1700000000000


def test_transform_event_writes_bounded_stream_rows() -> None:
    ops = transform_event({"event": "quote_refresh"}, _context("event"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, StreamJSONOp)
    assert op.key == "flux:v1:events:maker_v3_01"
    assert op.maxlen == 5_000
    assert op.row["strategy_id"] == "maker_v3_01"
    assert op.row["topic"] == "event"
    assert op.row["entry_id"] == "1700000001000-0"
    assert isinstance(op.row["ts_ms"], int)


def test_transform_trade_writes_bounded_stream_rows_with_normalized_ts_ms() -> None:
    ops = transform_trade({"trade_id": "t-1", "timestamp": "1700000000"}, _context("trade"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, StreamJSONOp)
    assert op.key == "flux:v1:trades:stream:maker_v3_01"
    assert op.maxlen == 20_000
    assert op.row["strategy_id"] == "maker_v3_01"
    assert op.row["topic"] == "trade"
    assert op.row["entry_id"] == "1700000001000-0"
    assert op.row["ts_ms"] == 1700000000000


def test_transform_alert_writes_bounded_stream_rows() -> None:
    ops = transform_alert({"severity": "warn"}, _context("alert"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, StreamJSONOp)
    assert op.key == "flux:v1:alerts:maker_v3_01"
    assert op.maxlen == 2_000
    assert op.row["strategy_id"] == "maker_v3_01"
    assert op.row["topic"] == "alert"
    assert op.row["entry_id"] == "1700000001000-0"
    assert isinstance(op.row["ts_ms"], int)


def test_transform_fv_writes_bounded_stream_rows() -> None:
    ops = transform_fv({"rows": [{"fv": "1.025", "ts": "1700000000"}]}, _context("fv"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, StreamJSONOp)
    assert op.key == "flux:v1:fv:stream:maker_v3_01"
    assert op.maxlen == 10_000
    assert op.row["strategy_id"] == "maker_v3_01"
    assert op.row["topic"] == "fv"
    assert op.row["entry_id"] == "1700000001000-0"
    assert op.row["ts_ms"] == 1700000000000


def test_transform_market_bbo_writes_strategy_scoped_snapshot() -> None:
    payload = {
        "exchange": "BYBIT",
        "symbol": "PLUMEUSDT",
        "bid": "1.0",
        "ask": "1.1",
        "timestamp": "1700000000",
    }

    ops = transform_market_bbo(payload, _context("market_bbo"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, SetJSONOp)
    assert op.key == "flux:v1:market:last:maker_v3_01:bybit:PLUME_USDT"
    assert op.ttl_seconds == 120
    assert op.value["strategy_id"] == "maker_v3_01"
    assert op.value["topic"] == "market_bbo"
    assert op.value["entry_id"] == "1700000001000-0"
    assert op.value["ts_ms"] == 1700000000000


def test_transform_market_bbo_supports_extended_quote_suffixes() -> None:
    payload = {
        "exchange": "BYBIT",
        "symbol": "PLUMEPUSD",
        "bid": "1.0",
        "ask": "1.1",
        "timestamp": "1700000000",
    }

    ops = transform_market_bbo(payload, _context("market_bbo"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, SetJSONOp)
    assert op.key == "flux:v1:market:last:maker_v3_01:bybit:PLUME_PUSD"


def test_transform_balances_writes_snapshot_and_rows_hash() -> None:
    payload = {
        "accounts": [
            {
                "exchange": "BYBIT",
                "asset": "USDT",
                "account_id": "spot",
                "total": "1000",
            },
        ],
    }

    ops = transform_balances(payload, _context("balances"))

    assert len(ops) == 2
    snapshot_op = ops[0]
    rows_op = ops[1]
    assert isinstance(snapshot_op, SetJSONOp)
    assert isinstance(rows_op, ReplaceHashJSONOp)
    assert snapshot_op.key == "flux:v1:balances:snapshot:maker_v3_01"
    assert rows_op.key == "flux:v1:balances:rows:maker_v3_01"
    assert "bybit:USDT:spot" in rows_op.mapping
    row = rows_op.mapping["bybit:USDT:spot"]
    assert row["strategy_id"] == "maker_v3_01"
    assert row["topic"] == "balances"
    assert row["entry_id"] == "1700000001000-0"
    assert isinstance(row["ts_ms"], int)


def test_transform_event_supports_ts_event_fallback() -> None:
    ops = transform_event({"event": "quote_refresh", "ts_event": "1700000001"}, _context("event"))

    assert len(ops) == 1
    op = ops[0]
    assert isinstance(op, StreamJSONOp)
    assert op.row["ts_ms"] == 1700000001000
