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
"""
Test the tutorial strategy's spread filter through native book replay.
"""

from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import Venue
from tests.providers import TestInstrumentProvider


@pytest.mark.parametrize(
    ("ask", "expected_orders"),
    [("2000.10", 2), ("1999.90", 0), ("1999.80", 0), (None, 0)],
    ids=["positive", "locked", "crossed", "one-sided"],
)
def test_imbalance_requires_positive_spread(
    monkeypatch: pytest.MonkeyPatch,
    ask: str | None,
    expected_orders: int,
) -> None:
    """
    Reject nonpositive and absent spreads while allowing a signal and its exit.
    """
    monkeypatch.syspath_prepend(str(Path(__file__).resolve().parents[4] / "docs/tutorials"))
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine = BacktestEngine(
        BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
            data_engine=DataEngineConfig(emit_quotes_from_book=True),
        ),
    )
    try:
        engine.add_venue(
            venue=Venue("BINANCE"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            starting_balances=[Money.from_str("100000 USDT")],
            book_type=BookType.L2_MBP,
            fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
        )
        engine.add_instrument(instrument)
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="orderbook_imbalance:OrderBookImbalance",
                config_path="orderbook_imbalance:OrderBookImbalanceConfig",
                config={
                    "instrument_id": str(instrument.id),
                    "max_trade_size": "0.01000",
                    "trigger_min_size": 1.0,
                },
            ),
        )
        levels = [(OrderSide.BUY, "1999.90", "10.00000")]
        if ask is not None:
            levels.append((OrderSide.SELL, ask, "0.10000"))
        ts = 1_600_000_000_000_000_000
        deltas = [
            OrderBookDelta(
                instrument.id,
                BookAction.ADD,
                BookOrder(side, Price.from_str(price), Quantity.from_str(size), i),
                RecordFlag.F_LAST.value if i == len(levels) else 0,
                i,
                ts,
                ts,
            )
            for i, (side, price, size) in enumerate(levels, start=1)
        ]
        engine.add_data([OrderBookDeltas(instrument.id, deltas)])

        engine.run()

        orders = engine.cache.orders()
        assert len(orders) == expected_orders
        assert engine.cache.positions_open(instrument_id=instrument.id) == []

        if expected_orders:
            assert {str(order.filled_qty) for order in orders} == {"0.01000"}
            assert {order.side for order in orders} == {OrderSide.BUY, OrderSide.SELL}
    finally:
        engine.dispose()
