# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pathlib

import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


DATABENTO_TEST_DATA_DIR = TEST_DATA_DIR / "databento"


def test_get_publishers() -> None:
    # Arrange
    loader = DatabentoDataLoader()

    # Act
    result = loader.get_publishers()

    # Assert
    assert len(result) == 104  # From built-in map


def test_loader_definition_glbx_futures() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "definition-glbx-es-fut.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, price_precision=2, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], FuturesContract)
    assert isinstance(data[1], FuturesContract)
    instrument = data[0]
    assert instrument.id == InstrumentId.from_str("ESM3.GLBX")
    assert instrument.raw_symbol == Symbol("ESM3")
    assert instrument.asset_class == AssetClass.INDEX
    assert instrument.instrument_class == InstrumentClass.FUTURE
    assert instrument.quote_currency == USD
    assert not instrument.is_inverse
    assert instrument.underlying == "ES"
    assert instrument.price_precision == 2
    assert instrument.price_increment == Price.from_str("0.25")
    assert instrument.size_precision == 0
    assert instrument.size_increment == 1
    assert instrument.multiplier == 50
    assert instrument.lot_size == 1
    assert instrument.ts_event == 1680451436501583647
    assert instrument.ts_init == 1680451436501583647


def test_loader_definition_xcme_futures() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "definition-glbx-es-fut.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True, use_exchange_as_venue=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], FuturesContract)
    assert isinstance(data[1], FuturesContract)
    instrument = data[0]
    assert instrument.id == InstrumentId.from_str("ESM3.XCME")
    assert instrument.raw_symbol == Symbol("ESM3")
    assert instrument.asset_class == AssetClass.INDEX
    assert instrument.instrument_class == InstrumentClass.FUTURE
    assert instrument.quote_currency == USD
    assert not instrument.is_inverse
    assert instrument.underlying == "ES"
    assert instrument.price_precision == 2
    assert instrument.price_increment == Price.from_str("0.25")
    assert instrument.size_precision == 0
    assert instrument.size_increment == 1
    assert instrument.multiplier == 50
    assert instrument.lot_size == 1
    assert instrument.ts_event == 1680451436501583647
    assert instrument.ts_init == 1680451436501583647


def test_loader_definition_glbx_options() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "definition-glbx-es-opt.dbn.zst"

    # Act
    data = loader.from_dbn_file(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], OptionContract)
    assert isinstance(data[1], OptionContract)
    instrument = data[0]
    assert instrument.id == InstrumentId.from_str("ESM4 C4250.GLBX")
    assert instrument.raw_symbol == Symbol("ESM4 C4250")
    assert instrument.asset_class == AssetClass.COMMODITY  # <-- TODO: This should be EQUITY
    assert instrument.instrument_class == InstrumentClass.OPTION
    assert instrument.quote_currency == USD
    assert not instrument.is_inverse
    assert instrument.underlying == "ESM4"
    assert instrument.option_kind == OptionKind.CALL
    assert instrument.strike_price == Price.from_str("4250.00")
    assert instrument.price_precision == 2
    assert instrument.price_increment == Price.from_str("0.01")
    assert instrument.size_precision == 0
    assert instrument.size_increment == 1
    assert instrument.multiplier == 50
    assert instrument.lot_size == 1
    assert instrument.ts_event == 1690848000000000000
    assert instrument.ts_init == 1690848000000000000


def test_loader_definition_opra_pillar() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "definition-opra.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], OptionContract)
    assert isinstance(data[1], OptionContract)
    instrument = data[0]
    assert instrument.id == InstrumentId.from_str("SPY   240119P00340000.OPRA")  # OSS symbol
    assert instrument.raw_symbol == Symbol("SPY   240119P00340000")
    assert instrument.asset_class == AssetClass.EQUITY
    assert instrument.instrument_class == InstrumentClass.OPTION
    assert instrument.quote_currency == USD
    assert not instrument.is_inverse
    assert instrument.underlying == "SPY"
    assert instrument.option_kind == OptionKind.PUT
    assert instrument.strike_price == Price.from_str("340.00")
    assert instrument.price_precision == 2
    assert instrument.price_increment == Price.from_str("0.01")
    assert instrument.size_precision == 0
    assert instrument.size_increment == 1
    assert instrument.multiplier == 1
    assert instrument.lot_size == 1
    assert instrument.ts_event == 1690885800419158943
    assert instrument.ts_init == 1690885800419158943


def test_loader_xnasitch_definition() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "definition.dbn.zst"

    # Act
    data = loader.from_dbn_file(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Equity)
    assert isinstance(data[1], Equity)
    instrument = data[0]
    assert instrument.id == InstrumentId.from_str("MSFT.XNAS")
    assert instrument.raw_symbol == Symbol("MSFT")
    assert instrument.asset_class == AssetClass.EQUITY
    assert instrument.instrument_class == InstrumentClass.SPOT
    assert instrument.quote_currency == USD
    assert not instrument.is_inverse
    assert instrument.price_precision == 2
    assert instrument.price_increment == Price.from_str("0.01")
    assert instrument.size_precision == 0
    assert instrument.size_increment == 1
    assert instrument.multiplier == 1
    assert instrument.lot_size == 100
    assert instrument.ts_event == 1633331241618029519
    assert instrument.ts_init == 1633331241618029519


def test_loader_mbo() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], OrderBookDelta)
    assert isinstance(data[1], OrderBookDelta)
    delta = data[0]
    assert delta.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert delta.action == BookAction.DELETE
    assert delta.order.side == OrderSide.SELL
    assert delta.order.price == Price.from_str("3722.75")
    assert delta.order.size == Quantity.from_int(1)
    assert delta.order.order_id == 647784973705
    assert delta.flags == 128
    assert delta.sequence == 1170352
    assert delta.ts_event == 1609160400000704060
    assert delta.ts_init == 1609160400000704060


def test_loader_mbo_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.OrderBookDelta)
    assert isinstance(data[1], nautilus_pyo3.OrderBookDelta)
    delta = data[0]
    assert delta.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESH1.GLBX")
    assert delta.action == nautilus_pyo3.BookAction.DELETE
    assert delta.order.side == nautilus_pyo3.OrderSide.SELL
    assert delta.order.price == nautilus_pyo3.Price.from_str("3722.75")
    assert delta.order.size == nautilus_pyo3.Quantity.from_int(1)
    assert delta.order.order_id == 647784973705
    assert delta.flags == 128
    assert delta.sequence == 1170352
    assert delta.ts_event == 1609160400000704060
    assert delta.ts_init == 1609160400000704060


def test_loader_mbp_1() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbp-1.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], QuoteTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert quote.bid_price == Price.from_str("3720.25")
    assert quote.ask_price == Price.from_str("3720.50")
    assert quote.bid_size == Quantity.from_int(24)
    assert quote.ask_size == Quantity.from_int(11)
    assert quote.ts_event == 1609160400006136329
    assert quote.ts_init == 1609160400006136329


def test_loader_mbp_1_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbp-1.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.QuoteTick)
    assert isinstance(data[1], nautilus_pyo3.QuoteTick)
    quote = data[0]
    assert quote.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESH1.GLBX")
    assert quote.bid_price == nautilus_pyo3.Price.from_str("3720.25")
    assert quote.ask_price == nautilus_pyo3.Price.from_str("3720.50")
    assert quote.bid_size == nautilus_pyo3.Quantity.from_int(24)
    assert quote.ask_size == nautilus_pyo3.Quantity.from_int(11)
    assert quote.ts_event == 1609160400006136329
    assert quote.ts_init == 1609160400006136329


def test_loader_bbo_1s() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "bbo-1s.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) > 0
    assert isinstance(data[0], QuoteTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == Price.from_str("5199.50")
    assert quote.ask_price == Price.from_str("5199.75")
    assert quote.bid_size == Quantity.from_int(26)
    assert quote.ask_size == Quantity.from_int(23)
    assert quote.ts_event == 1715248801000000000
    assert quote.ts_init == 1715248801000000000


def test_loader_bbo_1s_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "bbo-1s.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) > 0
    assert isinstance(data[0], nautilus_pyo3.QuoteTick)
    quote = data[0]
    assert quote.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == nautilus_pyo3.Price.from_str("5199.50")
    assert quote.ask_price == nautilus_pyo3.Price.from_str("5199.75")
    assert quote.bid_size == nautilus_pyo3.Quantity.from_int(26)
    assert quote.ask_size == nautilus_pyo3.Quantity.from_int(23)
    assert quote.ts_event == 1715248801000000000
    assert quote.ts_init == 1715248801000000000


def test_loader_bbo_1m() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "bbo-1m.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) > 0
    assert isinstance(data[0], QuoteTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == Price.from_str("5199.50")
    assert quote.ask_price == Price.from_str("5199.75")
    assert quote.bid_size == Quantity.from_int(33)
    assert quote.ask_size == Quantity.from_int(17)
    assert quote.ts_event == 1715248800000000000
    assert quote.ts_init == 1715248800000000000


def test_loader_bbo_1m_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "bbo-1m.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) > 0
    assert isinstance(data[0], nautilus_pyo3.QuoteTick)
    quote = data[0]
    assert quote.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == nautilus_pyo3.Price.from_str("5199.50")
    assert quote.ask_price == nautilus_pyo3.Price.from_str("5199.75")
    assert quote.bid_size == nautilus_pyo3.Quantity.from_int(33)
    assert quote.ask_size == nautilus_pyo3.Quantity.from_int(17)
    assert quote.ts_event == 1715248800000000000
    assert quote.ts_init == 1715248800000000000


def test_loader_mbp_10() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbp-10.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], OrderBookDepth10)
    assert isinstance(data[1], OrderBookDepth10)
    depth = data[0]
    assert depth.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    assert depth.bids[0].price == Price.from_str("3720.25")
    assert depth.bids[0].size == Quantity.from_int(24)
    assert depth.asks[0].price == Price.from_str("3720.50")
    assert depth.asks[0].size == Quantity.from_int(10)
    assert depth.bid_counts == [15, 18, 23, 26, 35, 28, 35, 39, 32, 39]
    assert depth.ask_counts == [8, 24, 25, 17, 19, 33, 40, 38, 35, 26]
    depth = data[1]
    assert depth.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    assert depth.bids[0].price == Price.from_str("3720.25")
    assert depth.bids[0].size == Quantity.from_int(24)
    assert depth.asks[0].price == Price.from_str("3720.50")
    assert depth.asks[0].size == Quantity.from_int(10)
    assert depth.bid_counts == [15, 17, 23, 26, 35, 28, 35, 39, 32, 39]
    assert depth.ask_counts == [8, 24, 25, 17, 19, 33, 40, 38, 35, 26]


def test_loader_mbp_10_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbp-10.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.OrderBookDepth10)
    assert isinstance(data[1], nautilus_pyo3.OrderBookDepth10)
    depth = data[0]
    assert depth.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESH1.GLBX")
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    assert depth.bids[0].price == nautilus_pyo3.Price.from_str("3720.25")
    assert depth.bids[0].size == nautilus_pyo3.Quantity.from_int(24)
    assert depth.asks[0].price == nautilus_pyo3.Price.from_str("3720.50")
    assert depth.asks[0].size == nautilus_pyo3.Quantity.from_int(10)
    assert depth.bid_counts == [15, 18, 23, 26, 35, 28, 35, 39, 32, 39]
    assert depth.ask_counts == [8, 24, 25, 17, 19, 33, 40, 38, 35, 26]
    depth = data[1]
    assert depth.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESH1.GLBX")
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    assert depth.bids[0].price == nautilus_pyo3.Price.from_str("3720.25")
    assert depth.bids[0].size == nautilus_pyo3.Quantity.from_int(24)
    assert depth.asks[0].price == nautilus_pyo3.Price.from_str("3720.50")
    assert depth.asks[0].size == nautilus_pyo3.Quantity.from_int(10)
    assert depth.bid_counts == [15, 17, 23, 26, 35, 28, 35, 39, 32, 39]
    assert depth.ask_counts == [8, 24, 25, 17, 19, 33, 40, 38, 35, 26]


def test_loader_tbbo_quotes() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "tbbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], QuoteTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert quote.bid_price == Price.from_str("3720.25")
    assert quote.ask_price == Price.from_str("3720.50")
    assert quote.bid_size == Quantity.from_int(26)
    assert quote.ask_size == Quantity.from_int(7)
    assert quote.ts_event == 1609160400099150057
    assert quote.ts_init == 1609160400099150057


def test_loader_tbbo_quotes_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "tbbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.QuoteTick)
    quote = data[0]
    assert quote.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESH1.GLBX")
    assert quote.bid_price == nautilus_pyo3.Price.from_str("3720.25")
    assert quote.ask_price == nautilus_pyo3.Price.from_str("3720.50")
    assert quote.bid_size == nautilus_pyo3.Quantity.from_int(26)
    assert quote.ask_size == nautilus_pyo3.Quantity.from_int(7)
    assert quote.ts_event == 1609160400099150057
    assert quote.ts_init == 1609160400099150057


def test_loader_tbbo_quotes_and_trades() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "tbbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(
        path,
        as_legacy_cython=True,
        include_trades=True,
    )

    # Assert
    assert len(data) == 4
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], TradeTick)
    assert isinstance(data[2], QuoteTick)
    assert isinstance(data[3], TradeTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert quote.bid_price == Price.from_str("3720.25")
    assert quote.ask_price == Price.from_str("3720.50")
    assert quote.bid_size == Quantity.from_int(26)
    assert quote.ask_size == Quantity.from_int(7)
    assert quote.ts_event == 1609160400099150057
    assert quote.ts_init == 1609160400099150057
    trade = data[1]
    assert trade.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert trade.price == Price.from_str("3720.25")
    assert trade.size == Quantity.from_int(5)
    assert trade.aggressor_side == AggressorSide.SELLER
    assert trade.trade_id == TradeId("1170380")
    assert trade.ts_event == 1609160400099150057
    assert trade.ts_init == 1609160400099150057


def test_loader_trades() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "trades.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], TradeTick)
    assert isinstance(data[1], TradeTick)
    trade = data[0]
    assert trade.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert trade.price == Price.from_str("3720.25")
    assert trade.size == Quantity.from_int(5)
    assert trade.aggressor_side == AggressorSide.SELLER
    assert trade.trade_id == TradeId("1170380")
    assert trade.ts_event == 1609160400099150057
    assert trade.ts_init == 1609160400099150057


def test_loader_trades_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "trades.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.TradeTick)
    assert isinstance(data[1], nautilus_pyo3.TradeTick)
    trade = data[0]
    assert trade.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESH1.GLBX")
    assert trade.price == nautilus_pyo3.Price.from_str("3720.25")
    assert trade.size == nautilus_pyo3.Quantity.from_int(5)
    assert trade.aggressor_side == nautilus_pyo3.AggressorSide.SELLER
    assert trade.trade_id == nautilus_pyo3.TradeId("1170380")
    assert trade.ts_event == 1609160400099150057
    assert trade.ts_init == 1609160400099150057


@pytest.mark.skip("development_only")
def test_loader_with_trades_large() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst"
    instrument_id = InstrumentId.from_str("TSLA.XNAS")

    # Act
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=True)

    # Assert
    assert len(data) == 6_885_435


@pytest.mark.skip("requires updated test data")
def test_loader_ohlcv_1s() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "ohlcv-1s.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert bar.bar_type == BarType.from_str("ESH1.GLBX-1-SECOND-LAST-EXTERNAL")
    assert bar.open == Price.from_str("3720.25")
    assert bar.high == Price.from_str("3720.50")
    assert bar.low == Price.from_str("3720.25")
    assert bar.close == Price.from_str("3720.50")
    assert bar.volume == Price.from_str("57")
    assert bar.ts_event == 1609160400000000000
    assert bar.ts_init == 1609160401000000000


@pytest.mark.parametrize(
    ("bars_timestamp_on_close", "expected_ts_event", "expected_ts_init"),
    [
        (True, 1715248860000000000, 1715248860000000000),  # Both close time
        (False, 1715248800000000000, 1715248860000000000),  # ts_event=open, ts_init=close
    ],
)
def test_loader_with_ohlcv_1m(
    bars_timestamp_on_close: bool,
    expected_ts_event: int,
    expected_ts_init: int,
) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = (
        DATABENTO_TEST_DATA_DIR
        / "options_catalog"
        / "databento"
        / "futures_ohlcv-1m_2024-05-09T10h00_2024-05-09T10h05.dbn.zst"
    )

    # Act
    data = loader.from_dbn_file(
        path,
        as_legacy_cython=True,
        bars_timestamp_on_close=bars_timestamp_on_close,
    )

    # Assert
    assert len(data) == 5
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert bar.bar_type == BarType.from_str("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL")
    assert bar.open == Price.from_str("5199.75")
    assert bar.ts_event == expected_ts_event
    assert bar.ts_init == expected_ts_init


@pytest.mark.parametrize(
    ("bars_timestamp_on_close", "expected_ts_event", "expected_ts_init"),
    [
        (True, 1715248860000000000, 1715248860000000000),  # Close time (default)
        (False, 1715248800000000000, 1715248860000000000),  # ts_event=open, ts_init=close
    ],
)
def test_loader_with_ohlcv_1m_and_xcme(
    bars_timestamp_on_close: bool,
    expected_ts_event: int,
    expected_ts_init: int,
) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    definition_path = (
        DATABENTO_TEST_DATA_DIR / "options_catalog" / "databento" / "futures_definition.dbn.zst"
    )
    path = (
        DATABENTO_TEST_DATA_DIR
        / "options_catalog"
        / "databento"
        / "futures_ohlcv-1m_2024-05-09T10h00_2024-05-09T10h05.dbn.zst"
    )

    # Act
    # using use_exchange_as_venue=True leads to using the exchange name as the venue for subsequently loaded data
    _ = loader.from_dbn_file(
        definition_path,
        as_legacy_cython=True,
        use_exchange_as_venue=True,
    )
    data = loader.from_dbn_file(
        path,
        as_legacy_cython=True,
        bars_timestamp_on_close=bars_timestamp_on_close,
    )

    # Assert
    assert len(data) == 5
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert bar.bar_type == BarType.from_str("ESM4.XCME-1-MINUTE-LAST-EXTERNAL")
    assert bar.open == Price.from_str("5199.75")
    assert bar.ts_event == expected_ts_event
    assert bar.ts_init == expected_ts_init


@pytest.mark.skip("requires updated test data")
@pytest.mark.parametrize(
    ("bars_timestamp_on_close", "expected_ts_event", "expected_ts_init"),
    [
        (True, 1609160460000000000, 1609160460000000000),  # Close time (default)
        (False, 1609160400000000000, 1609160400000000000),  # Open time
    ],
)
def test_loader_with_ohlcv_1m_pyo3(
    bars_timestamp_on_close: bool,
    expected_ts_event: int,
    expected_ts_init: int,
) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "ohlcv-1m.dbn.zst"

    # Act
    data = loader.from_dbn_file(
        path,
        as_legacy_cython=False,
        bars_timestamp_on_close=bars_timestamp_on_close,
    )

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.Bar)
    assert isinstance(data[1], nautilus_pyo3.Bar)
    bar = data[0]
    assert bar.bar_type == nautilus_pyo3.BarType.from_str("ESH1.GLBX-1-MINUTE-LAST-EXTERNAL")
    assert bar.open == nautilus_pyo3.Price.from_str("3720.25")
    assert bar.ts_event == expected_ts_event
    assert bar.ts_init == expected_ts_init


@pytest.mark.skip("requires updated test data")
def test_loader_with_ohlcv_1h() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "ohlcv-1h.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert bar.bar_type == BarType.from_str("ESH1.GLBX-1-HOUR-LAST-EXTERNAL")
    assert bar.open == Price.from_str("3720.25")
    assert bar.ts_event == 1609160400000000000
    assert bar.ts_init == 1609164000000000000


@pytest.mark.skip("requires updated test data")
def test_loader_with_ohlcv_1d() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "ohlcv-1d.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 0  # No records ??


def test_load_order_book_deltas() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], OrderBookDelta)
    assert isinstance(data[1], OrderBookDelta)


def test_load_order_book_depth10_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbp-10.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.OrderBookDepth10)
    assert isinstance(data[1], nautilus_pyo3.OrderBookDepth10)


def test_load_quote_ticks() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "mbp-1.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], QuoteTick)


def test_load_mixed_ticks() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "tbbo.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], QuoteTick)


def test_load_trade_ticks() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "trades.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], TradeTick)
    assert isinstance(data[1], TradeTick)


@pytest.mark.skip("requires updated test data")
@pytest.mark.parametrize(
    ("filename", "bar_type", "open_price", "ts_event", "ts_init"),
    [
        [
            "ohlcv-1s.dbn.zst",
            "ESH1.GLBX-1-SECOND-LAST-EXTERNAL",
            "3720.25",
            1609160400000000000,
            1609160401000000000,
        ],
        [
            "ohlcv-1m.dbn.zst",
            "ESH1.GLBX-1-MINUTE-LAST-EXTERNAL",
            "3720.25",
            1609160400000000000,
            1609160460000000000,
        ],
        [
            "ohlcv-1h.dbn.zst",
            "ESH1.GLBX-1-HOUR-LAST-EXTERNAL",
            "3720.25",
            1609160400000000000,
            1609164000000000000,
        ],
        # ohlcv-1d has no data?
    ],
)
def test_load_bars(
    filename: str,
    bar_type: str,
    open_price: str,
    ts_event: int,
    ts_init: int,
) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / filename

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert str(bar.bar_type) == bar_type
    assert bar.open == Price.from_str(open_price)
    assert bar.ts_event == ts_event
    assert bar.ts_init == ts_init


def test_load_status() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "status.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=True)

    # Assert
    assert len(data) == 4
    assert isinstance(data[0], InstrumentStatus)
    assert data[0].action == MarketStatusAction.TRADING
    assert data[0].ts_event == 1609110000000000000
    assert data[0].ts_init == 1609113600000000000
    assert data[0].reason == "Scheduled"
    assert data[0].trading_event is None
    assert data[0].is_trading
    assert data[0].is_quoting
    assert data[0].is_short_sell_restricted is None


def test_load_status_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "status.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 4
    assert isinstance(data[0], nautilus_pyo3.InstrumentStatus)
    assert data[0].action == nautilus_pyo3.MarketStatusAction.TRADING
    assert data[0].ts_event == 1609110000000000000
    assert data[0].ts_init == 1609113600000000000
    assert data[0].reason == "Scheduled"
    assert data[0].trading_event is None
    assert data[0].is_trading
    assert data[0].is_quoting
    assert data[0].is_short_sell_restricted is None


def test_load_imbalance() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "imbalance.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 4
    assert isinstance(data[0], nautilus_pyo3.DatabentoImbalance)
    assert isinstance(data[1], nautilus_pyo3.DatabentoImbalance)
    assert isinstance(data[2], nautilus_pyo3.DatabentoImbalance)
    assert isinstance(data[3], nautilus_pyo3.DatabentoImbalance)


def test_load_statistics() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "statistics.dbn.zst"

    # Act
    data = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    assert len(data) == 4
    assert isinstance(data[0], nautilus_pyo3.DatabentoStatistics)
    assert isinstance(data[1], nautilus_pyo3.DatabentoStatistics)
    assert isinstance(data[2], nautilus_pyo3.DatabentoStatistics)
    assert isinstance(data[3], nautilus_pyo3.DatabentoStatistics)


@pytest.mark.skip("development_only")
def test_load_instruments_pyo3_large() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "temp" / "glbx-mdp3-20241020.definition.dbn.zst"

    # Act
    instruments = loader.from_dbn_file(path, as_legacy_cython=False)

    # Assert
    expected_id = nautilus_pyo3.InstrumentId.from_str("CBJ5 P2100.GLBX")
    assert len(instruments) == 601_633
    assert instruments[0].id == expected_id


@pytest.mark.skip("development_only")
def test_load_order_book_deltas_spy_large() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "temp" / "spy-xnas-mbo-20231127.dbn.zst"
    instrument_id = InstrumentId.from_str("SPY.XNAS")

    # Act
    data = loader.from_dbn_file(path, instrument_id, as_legacy_cython=True)

    # Assert
    assert len(data) == 6_197_580  # No trades for now
    assert isinstance(data[0], nautilus_pyo3.OrderBookDelta)
    assert isinstance(data[1], nautilus_pyo3.OrderBookDelta)


@pytest.mark.skip("development_only")
def test_load_status_pyo3_large() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "temp" / "glbx-mdp3-20240718.status.dbn.zst"

    # Act (conversion to Cython objects creates significant overhead)
    instrument_id = InstrumentId.from_str("SPY.XNAS")
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=False)

    # Assert
    assert len(data) == 4_673_675


def test_loader_cmbp_1() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    # Use the test data from the Rust crate
    path = (
        pathlib.Path(__file__).parent.parent.parent.parent.parent
        / "crates"
        / "adapters"
        / "databento"
        / "test_data"
        / "test_data.cmbp-1.dbn.zst"
    )

    # Act
    instrument_id = InstrumentId.from_str("ESM4.GLBX")
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=True)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], QuoteTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == Price.from_str("3720.25")
    assert quote.ask_price == Price.from_str("3720.50")
    assert quote.bid_size == Quantity.from_int(24)
    assert quote.ask_size == Quantity.from_int(11)
    assert quote.ts_event == 1609160400006136329
    assert quote.ts_init == 1609160400006136329


def test_loader_cmbp_1_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    # Use the test data from the Rust crate
    path = (
        pathlib.Path(__file__).parent.parent.parent.parent.parent
        / "crates"
        / "adapters"
        / "databento"
        / "test_data"
        / "test_data.cmbp-1.dbn.zst"
    )

    # Act
    instrument_id = nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.QuoteTick)
    assert isinstance(data[1], nautilus_pyo3.QuoteTick)
    quote = data[0]
    assert quote.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == nautilus_pyo3.Price.from_str("3720.25")
    assert quote.ask_price == nautilus_pyo3.Price.from_str("3720.50")
    assert quote.bid_size == nautilus_pyo3.Quantity.from_int(24)
    assert quote.ask_size == nautilus_pyo3.Quantity.from_int(11)
    assert quote.ts_event == 1609160400006136329
    assert quote.ts_init == 1609160400006136329


def test_loader_cbbo_1s() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    # Use the test data from the Rust crate
    path = (
        pathlib.Path(__file__).parent.parent.parent.parent.parent
        / "crates"
        / "adapters"
        / "databento"
        / "test_data"
        / "test_data.cbbo-1s.dbn.zst"
    )

    # Act
    instrument_id = InstrumentId.from_str("ESM4.GLBX")
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=True)

    # Assert
    assert len(data) == 4  # 2 quotes + 2 trades from CBBO
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], TradeTick)
    assert isinstance(data[2], QuoteTick)
    assert isinstance(data[3], TradeTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == Price.from_str("3720.25")
    assert quote.ask_price == Price.from_str("3720.50")
    assert quote.bid_size == Quantity.from_int(24)
    assert quote.ask_size == Quantity.from_int(11)
    assert quote.ts_event == 1609160400006136329
    assert quote.ts_init == 1609160400006136329


def test_loader_cbbo_1s_pyo3() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    # Use the test data from the Rust crate
    path = (
        pathlib.Path(__file__).parent.parent.parent.parent.parent
        / "crates"
        / "adapters"
        / "databento"
        / "test_data"
        / "test_data.cbbo-1s.dbn.zst"
    )

    # Act
    instrument_id = nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=False)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], nautilus_pyo3.QuoteTick)
    assert isinstance(data[1], nautilus_pyo3.QuoteTick)
    quote = data[0]
    assert quote.instrument_id == nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    assert quote.bid_price == nautilus_pyo3.Price.from_str("3720.25")
    assert quote.ask_price == nautilus_pyo3.Price.from_str("3720.50")
    assert quote.bid_size == nautilus_pyo3.Quantity.from_int(24)
    assert quote.ask_size == nautilus_pyo3.Quantity.from_int(11)
    assert quote.ts_event == 1609160400006136329
    assert quote.ts_init == 1609160400006136329
