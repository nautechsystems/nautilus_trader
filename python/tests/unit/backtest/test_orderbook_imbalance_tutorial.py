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
Test the canonical Rust imbalance strategy through Python book replay.
"""

from collections.abc import Iterator
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import Money
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import StrategyId
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import OrderBookImbalanceConfig
from tests.providers import TestInstrumentProvider


@pytest.fixture
def imbalance_engine() -> Iterator[BacktestEngine]:
    """
    Provide a simulated venue with zero fees for native strategy replay.
    """
    engine = BacktestEngine(
        BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
            data_engine=DataEngineConfig(emit_quotes_from_book=True),
        ),
    )
    engine.add_venue(
        venue=Venue("BINANCE"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money.from_str("100000 USDT")],
        book_type=BookType.L2_MBP,
        fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
    )
    try:
        yield engine
    finally:
        engine.dispose()


@pytest.mark.parametrize("dry_run", [False, True])
@pytest.mark.parametrize(
    ("ask", "expected_orders"),
    [("2000.10", 2), ("1999.90", 0), ("1999.80", 0), (None, 0)],
    ids=["positive", "locked", "crossed", "one-sided"],
)
def test_imbalance_requires_positive_spread(
    imbalance_engine: BacktestEngine,
    dry_run: bool,
    ask: str | None,
    expected_orders: int,
) -> None:
    """
    Reject nonpositive and absent spreads while allowing a signal and its exit.
    """
    engine = imbalance_engine
    instrument = TestInstrumentProvider.ethusdt_binance()
    engine.add_instrument(instrument)
    engine.add_builtin_strategy(
        "OrderBookImbalance",
        OrderBookImbalanceConfig(
            instrument_id=instrument.id,
            max_trade_size=Quantity.from_str("0.01000"),
            trigger_min_size=Decimal(1),
            dry_run=dry_run,
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
            BookOrder(
                side,
                Price.from_str(price),
                Quantity.from_decimal_dp(Decimal(size), instrument.size_precision),
                i,
            ),
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
    assert len(orders) == (0 if dry_run else expected_orders)
    assert engine.cache.positions_open(instrument_id=instrument.id) == []

    if expected_orders and not dry_run:
        assert {str(order.filled_qty) for order in orders} == {"0.01000"}
        assert {order.side for order in orders} == {OrderSide.BUY, OrderSide.SELL}


def test_imbalance_config_preserves_native_fields() -> None:
    """
    Preserve every configurable signal control across the Python binding.
    """
    instrument_id = TestInstrumentProvider.ethusdt_binance().id
    config = OrderBookImbalanceConfig(
        instrument_id=instrument_id,
        max_trade_size=Quantity.from_str("3.14259"),
        trigger_min_size=Decimal("15.25"),
        trigger_imbalance_ratio=Decimal("0.125"),
        min_seconds_between_triggers=2.5,
        book_type=BookType.L3_MBO,
        dry_run=True,
        log_interval=7,
        strategy_id=StrategyId("IMBALANCE-123"),
        order_id_tag="456",
    )
    assert config.instrument_id == instrument_id
    assert config.max_trade_size == Quantity.from_str("3.14259")
    assert config.trigger_min_size == Decimal("15.25")
    assert config.trigger_imbalance_ratio == Decimal("0.125")
    assert config.min_seconds_between_triggers == 2.5
    assert config.book_type == BookType.L3_MBO
    assert config.dry_run is True
    assert config.log_interval == 7
    assert config.strategy_id == StrategyId("IMBALANCE-123")
    assert config.order_id_tag == "456"


@pytest.mark.parametrize(
    ("field", "value", "message"),
    [
        ("max_trade_size", Quantity.from_str("0"), "max_trade_size must be positive"),
        ("trigger_min_size", Decimal(-1), "trigger_min_size must be non-negative"),
        ("trigger_imbalance_ratio", Decimal(0), "trigger_imbalance_ratio must be in"),
        ("trigger_imbalance_ratio", Decimal(1), "trigger_imbalance_ratio must be in"),
        ("min_seconds_between_triggers", -1.0, "min_seconds_between_triggers must be finite"),
        (
            "min_seconds_between_triggers",
            float("nan"),
            "min_seconds_between_triggers must be finite",
        ),
        (
            "min_seconds_between_triggers",
            float("inf"),
            "min_seconds_between_triggers must be finite",
        ),
    ],
)
def test_imbalance_config_rejects_invalid_fields(field: str, value: object, message: str) -> None:
    """
    Reject invalid Python signal controls at construction.
    """
    fields = {
        "instrument_id": TestInstrumentProvider.ethusdt_binance().id,
        "max_trade_size": Quantity.from_str("1.00000"),
        field: value,
    }
    with pytest.raises(ValueError, match=message):
        OrderBookImbalanceConfig(**fields)


def test_imbalance_instances_follow_current_books_independently(
    imbalance_engine: BacktestEngine,
) -> None:
    """
    Keep per-instrument throttles independent and ignore a balanced replacement book.
    """
    engine = imbalance_engine
    instruments = [
        TestInstrumentProvider.ethusdt_binance(),
        TestInstrumentProvider.btcusdt_binance(),
    ]
    data = []
    ts = 1_600_000_000_000_000_000

    for index, instrument in enumerate(instruments):
        engine.add_instrument(instrument)
        tag = str(index)
        engine.add_builtin_strategy(
            "OrderBookImbalance",
            OrderBookImbalanceConfig(
                instrument_id=instrument.id,
                max_trade_size=Quantity.from_str("0.01000"),
                trigger_min_size=Decimal(1),
                strategy_id=StrategyId(f"IMBALANCE-{tag}"),
                order_id_tag=tag,
            ),
        )

        for step, ask_size in enumerate(["0.100000", "10.000000", "0.100000"]):
            now = ts + step * 1_000_000_000
            levels = [
                (OrderSide.BUY, "1999.90", "10.000000"),
                (OrderSide.SELL, "2000.10", ask_size),
            ]
            deltas = [
                OrderBookDelta(
                    instrument.id,
                    BookAction.ADD if step == 0 else BookAction.UPDATE,
                    BookOrder(
                        side,
                        Price.from_str(price),
                        Quantity.from_decimal_dp(Decimal(size), instrument.size_precision),
                        i,
                    ),
                    RecordFlag.F_LAST.value if i == len(levels) else 0,
                    step * 2 + i,
                    now,
                    now,
                )
                for i, (side, price, size) in enumerate(levels, start=1)
            ]
            data.append(OrderBookDeltas(instrument.id, deltas))
    data.sort(key=lambda batch: batch.ts_init)
    engine.add_data(data)

    engine.run()

    for index, instrument in enumerate(instruments):
        orders = engine.cache.orders(strategy_id=StrategyId(f"IMBALANCE-{index}"))
        buys = [order for order in orders if order.side == OrderSide.BUY]
        sells = [order for order in orders if order.side == OrderSide.SELL]
        assert len(buys) == 2
        assert len(sells) == 1
        assert {order.instrument_id for order in orders} == {instrument.id}
        assert [order.ts_init for order in sorted(buys, key=lambda order: order.ts_init)] == [
            ts,
            ts + 2_000_000_000,
        ]
        assert [order.quantity.as_decimal() for order in buys] == [Decimal("0.01")] * 2
        assert sells[0].quantity.as_decimal() == Decimal("0.02")
    assert engine.cache.positions_open() == []


def test_imbalance_registers_on_backtest_node(tmp_path: Path) -> None:
    """
    Register the canonical strategy through the node path used by tutorials.
    """
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog = ParquetDataCatalog(str(tmp_path))
    catalog.write_instruments([instrument])
    ts = 1_600_000_000_000_000_000
    levels = [(OrderSide.BUY, "1999.90", "10.00000"), (OrderSide.SELL, "2000.10", "0.10000")]
    deltas = [
        OrderBookDelta(
            instrument.id,
            BookAction.ADD,
            BookOrder(side, Price.from_str(price), Quantity.from_str(size), i),
            RecordFlag.F_LAST.value if i == 2 else 0,
            i,
            ts,
            ts,
        )
        for i, (side, price, size) in enumerate(levels, start=1)
    ]
    catalog.write_order_book_deltas(deltas)
    run = BacktestRunConfig(
        engine=BacktestEngineConfig(
            bypass_logging=True,
            run_analysis=False,
            data_engine=DataEngineConfig(emit_quotes_from_book=True),
        ),
        venues=[
            BacktestVenueConfig(
                name="BINANCE",
                oms_type=OmsType.NETTING,
                account_type=AccountType.MARGIN,
                book_type=BookType.L2_MBP,
                starting_balances=["100000 USDT"],
                fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
            ),
        ],
        data=[
            BacktestDataConfig(
                data_type=NautilusDataType.OrderBookDelta,
                catalog=DataCatalogConfig(path=str(tmp_path)),
                instrument_id=instrument.id,
            ),
        ],
        dispose_on_completion=False,
    )
    node = BacktestNode([run])
    try:
        node.build()
        node.add_builtin_strategy(
            run.id,
            "OrderBookImbalance",
            OrderBookImbalanceConfig(
                instrument_id=instrument.id,
                trigger_min_size=Decimal(1),
                max_trade_size=Quantity.from_str("0.01000"),
                strategy_id=StrategyId("IMBALANCE-789"),
                order_id_tag="789",
            ),
        )
        node.run()
        cache = node.get_engine_cache(run.id)
        assert cache.strategy_ids() == [StrategyId("IMBALANCE-789")]
        assert len(cache.orders()) == 2
        assert cache.positions_open() == []
    finally:
        node.dispose()


def test_imbalance_registration_rejects_invalid_order_id_tag(
    imbalance_engine: BacktestEngine,
) -> None:
    """
    Return a Python error for an invalid base config instead of panicking.
    """
    config = OrderBookImbalanceConfig(
        instrument_id=TestInstrumentProvider.ethusdt_binance().id,
        max_trade_size=Quantity.from_str("0.01000"),
        order_id_tag="runner-1",
    )
    with pytest.raises(
        ValueError,
        match="`order_id_tag` cannot contain the '-' strategy ID separator, was 'runner-1'",
    ):
        imbalance_engine.add_builtin_strategy("OrderBookImbalance", config)
