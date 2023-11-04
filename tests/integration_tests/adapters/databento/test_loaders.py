# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests import TEST_DATA_DIR


def test_loader_with_xnasitch_definition() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/definition.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Equity)
    assert isinstance(data[1], Equity)
    instrument = data[0]
    assert instrument.id == InstrumentId.from_str("MSFT.XNAS")
    assert instrument.raw_symbol == Symbol("MSFT")
    assert instrument.asset_class == AssetClass.EQUITY
    assert instrument.asset_type == AssetType.SPOT
    assert instrument.quote_currency == USD
    assert not instrument.is_inverse
    assert instrument.price_precision == 2
    assert instrument.price_increment == Price.from_str("0.01")
    assert instrument.size_precision == 0
    assert instrument.size_increment == 1
    assert instrument.multiplier == 1
    assert instrument.lot_size == 100
    assert instrument.ts_event == 1633331241618018154
    assert instrument.ts_init == 1633331241618029519


def test_loader_with_xnasitch_mbo() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/mbo.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], OrderBookDelta)
    assert isinstance(data[1], OrderBookDelta)
    delta = data[0]
    assert delta.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert delta.action == BookAction.DELETE
    assert delta.order.side == OrderSide.BUY
    assert delta.order.price == Price.from_str("3722.75")
    assert delta.order.size == Quantity.from_int(1)
    assert delta.order.order_id == 647784973705
    assert delta.flags == 128
    assert delta.sequence == 1170352
    assert delta.ts_event == 1609160400000429831
    assert delta.ts_init == 1609160400000704060


def test_loader_with_mbp_1() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/mbp-1.dbn.zst"

    # Act
    data = loader.from_dbn(path)

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
    assert quote.ts_event == 1609160400006001487
    assert quote.ts_init == 1609160400006136329


def test_loader_with_mbp_10() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/mbp-10.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], QuoteTick)
    assert isinstance(data[1], QuoteTick)
    quote = data[0]
    assert quote.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert quote.bid_price == Price.from_str("3720.25")
    assert quote.ask_price == Price.from_str("3720.50")
    assert quote.bid_size == Quantity.from_int(24)
    assert quote.ask_size == Quantity.from_int(10)
    assert quote.ts_event == 1609160400000429831
    assert quote.ts_init == 1609160400000704060


def test_loader_with_tbbo() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/tbbo.dbn.zst"

    # Act
    data = loader.from_dbn(path)

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
    assert quote.ts_event == 1609160400098821953
    assert quote.ts_init == 1609160400099150057
    trade = data[1]
    assert trade.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert trade.price == Price.from_str("3720.25")
    assert trade.size == Quantity.from_int(5)
    assert trade.aggressor_side == AggressorSide.BUYER
    assert trade.trade_id == TradeId("1170380")
    assert trade.ts_event == 1609160400098821953
    assert trade.ts_init == 1609160400099150057


def test_loader_with_trades() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/trades.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], TradeTick)
    assert isinstance(data[1], TradeTick)
    trade = data[0]
    assert trade.instrument_id == InstrumentId.from_str("ESH1.GLBX")
    assert trade.price == Price.from_str("3720.25")
    assert trade.size == Quantity.from_int(5)
    assert trade.aggressor_side == AggressorSide.BUYER
    assert trade.trade_id == TradeId("1170380")
    assert trade.ts_event == 1609160400098821953
    assert trade.ts_init == 1609160400099150057


def test_loader_with_ohlcv_1s() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/ohlcv-1s.dbn.zst"

    # Act
    data = loader.from_dbn(path)

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
    assert bar.volume == Price.from_str("0")
    assert bar.ts_event == 1609160400000000000
    assert bar.ts_init == 1609160400000000000


def test_loader_with_ohlcv_1m() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/ohlcv-1m.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert bar.bar_type == BarType.from_str("ESH1.GLBX-1-MINUTE-LAST-EXTERNAL")
    assert bar.open == Price.from_str("3720.25")
    assert bar.ts_event == 1609160400000000000
    assert bar.ts_init == 1609160400000000000


def test_loader_with_ohlcv_1h() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/ohlcv-1h.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Bar)
    assert isinstance(data[1], Bar)
    bar = data[0]
    assert bar.bar_type == BarType.from_str("ESH1.GLBX-1-HOUR-LAST-EXTERNAL")
    assert bar.open == Price.from_str("3720.25")  # Bug??
    assert bar.ts_event == 1609160400000000000
    assert bar.ts_init == 1609160400000000000


def test_loader_with_ohlcv_1d() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/ohlcv-1d.dbn.zst"

    # Act
    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 0  # ??
