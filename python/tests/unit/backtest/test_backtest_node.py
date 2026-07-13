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

from decimal import Decimal

import pytest

from nautilus_trader.backtest import BacktestDataConfig
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.backtest import BacktestRunConfig
from nautilus_trader.backtest import BacktestVenueConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OmsType
from nautilus_trader.model import Quantity
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import EmaCrossConfig
from tests.providers import TestInstrumentProvider
from tests.stubs import TestDataProviderPyo3


def test_node_construction():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    assert node is not None


def test_node_exposes_builtin_strategy_registration():
    assert hasattr(BacktestNode, "add_builtin_strategy")


def test_node_empty_configs_raises():
    with pytest.raises(RuntimeError, match="At least one run config"):
        BacktestNode([])


def test_node_venue_mismatch_raises():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("BTC/USDT.BINANCE"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    with pytest.raises(RuntimeError, match="No venue config found for venue"):
        BacktestNode([config])


def test_node_repr():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    assert "BacktestNode" in repr(node)


def test_node_dispose():
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USD"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path="/data/catalog",
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
    )
    config = BacktestRunConfig(venues=[venue], data=[data])
    node = BacktestNode([config])
    node.dispose()


def test_node_streaming_matches_oneshot_from_local_catalog(tmp_path):
    instrument = TestInstrumentProvider.ethusdt_binance()
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    quotes = _whipsaw_quotes(instrument, count=30)
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks(quotes)

    oneshot = _run_ema_cross_node(str(catalog_path), instrument, chunk_size=None)
    streaming = _run_ema_cross_node(str(catalog_path), instrument, chunk_size=7)

    assert oneshot.iterations == streaming.iterations == len(quotes)
    assert oneshot.total_events == streaming.total_events
    assert oneshot.total_orders == streaming.total_orders
    assert oneshot.total_positions == streaming.total_positions
    assert oneshot.total_orders >= 4
    assert oneshot.total_positions >= 2
    assert oneshot.summary["orders.open"] == streaming.summary["orders.open"] == "0"
    assert oneshot.summary["orders.closed"] == streaming.summary["orders.closed"]
    assert oneshot.summary["positions.open"] == streaming.summary["positions.open"] == "0"
    assert oneshot.summary["positions.closed"] == streaming.summary["positions.closed"]


def _run_ema_cross_node(catalog_path, instrument, chunk_size):
    venue = BacktestVenueConfig(
        name="BINANCE",
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=["1_000_000 USDT"],
    )
    data = BacktestDataConfig(
        data_type="QuoteTick",
        catalog_path=catalog_path,
        instrument_id=instrument.id,
    )
    config = BacktestRunConfig(
        venues=[venue],
        data=[data],
        engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
        chunk_size=chunk_size,
    )
    node = BacktestNode([config])
    node.build()
    node.add_builtin_strategy(
        config.id,
        "EmaCross",
        EmaCrossConfig(
            instrument_id=instrument.id,
            trade_size=Quantity.from_str("0.10000"),
            fast_period=3,
            slow_period=6,
        ),
    )
    result = node.run()[0]
    node.dispose()
    return result


def _whipsaw_quotes(instrument, count):
    base_ns = 1_600_000_200_000_000_000
    quotes = []

    for i in range(count):
        mid = Decimal("2000.00") + (Decimal((i % 10) - 5) * Decimal(2))
        quotes.append(
            TestDataProviderPyo3.quote_tick(
                instrument_id=instrument.id,
                bid_price=mid - Decimal("0.05"),
                ask_price=mid + Decimal("0.05"),
                bid_size="10.00000",
                ask_size="10.00000",
                ts_event=base_ns + (i * 1_000_000_000),
                ts_init=base_ns + (i * 1_000_000_000),
            ),
        )

    return quotes
