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

import pytest

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_binance_snapshot5
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_binance_snapshot25
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_bitmex_trades
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_deribit_book_l2
from nautilus_trader.test_kit.providers import ensure_data_exists_tardis_huobi_quotes


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 3],
        [2, None],
        [2, 3],
    ],
)
def test_tardis_load_deltas(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_deribit_book_l2()
    instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")  # Override instrument in data
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
        instrument_id=instrument_id,
    )

    # Act
    deltas = loader.load_deltas(filepath, limit=10_000)

    # Assert
    assert len(deltas) == 10_000
    assert deltas[0].instrument_id == instrument_id
    assert deltas[0].action == BookAction.ADD
    assert deltas[0].order.side == OrderSide.SELL
    assert deltas[0].order.price == Price.from_str("6421.5")
    assert deltas[0].order.size == Quantity.from_str("18640")
    assert deltas[0].flags == 0
    assert deltas[0].sequence == 0
    assert deltas[0].ts_event == 1585699200245000000
    assert deltas[0].ts_init == 1585699200355684000


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 3],
        [2, None],
        [2, 3],
    ],
)
def test_tardis_load_depth10_from_snapshot5(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_binance_snapshot5()
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    deltas = loader.load_depth10(filepath, levels=5, limit=10_000)

    # Assert
    assert len(deltas) == 10_000
    assert deltas[0].instrument_id == InstrumentId.from_str("BTCUSDT.BINANCE")
    assert len(deltas[0].bids) == 10
    assert deltas[0].bids[0].price == Price.from_str("11657.07")
    assert deltas[0].bids[0].size == Quantity.from_str("10.896")
    assert deltas[0].bids[0].side == OrderSide.BUY
    assert deltas[0].bids[0].order_id == 0
    assert len(deltas[0].asks) == 10
    assert deltas[0].asks[0].price == Price.from_str("11657.08")
    assert deltas[0].asks[0].size == Quantity.from_str("1.714")
    assert deltas[0].asks[0].side == OrderSide.SELL
    assert deltas[0].asks[0].order_id == 0
    assert deltas[0].bid_counts[0] == 1
    assert deltas[0].ask_counts[0] == 1
    assert deltas[0].flags == 128
    assert deltas[0].ts_event == 1598918403696000000
    assert deltas[0].ts_init == 1598918403810979000
    assert deltas[0].sequence == 0


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 3],
        [2, None],
        [2, 3],
    ],
)
def test_tardis_load_depth10_from_snapshot25(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_binance_snapshot25()
    instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BINANCE")  # Override instrument in data
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
        instrument_id=instrument_id,
    )

    # Act
    deltas = loader.load_depth10(filepath, levels=25, limit=10_000)

    # Assert
    assert len(deltas) == 10_000
    assert deltas[0].instrument_id == InstrumentId.from_str("BTCUSDT-PERP.BINANCE")
    assert len(deltas[0].bids) == 10
    assert deltas[0].bids[0].price == Price.from_str("11657.07")
    assert deltas[0].bids[0].size == Quantity.from_str("10.896")
    assert deltas[0].bids[0].side == OrderSide.BUY
    assert deltas[0].bids[0].order_id == 0
    assert len(deltas[0].asks) == 10
    assert deltas[0].asks[0].price == Price.from_str("11657.08")
    assert deltas[0].asks[0].size == Quantity.from_str("1.714")
    assert deltas[0].asks[0].side == OrderSide.SELL
    assert deltas[0].asks[0].order_id == 0
    assert deltas[0].bid_counts[0] == 1
    assert deltas[0].ask_counts[0] == 1
    assert deltas[0].flags == 128
    assert deltas[0].ts_event == 1598918403696000000
    assert deltas[0].ts_init == 1598918403810979000
    assert deltas[0].sequence == 0


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 0],
        [1, None],
        [1, 0],
    ],
)
def test_tardis_load_quotes(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_huobi_quotes()
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    trades = loader.load_quotes(filepath, limit=10_000)

    # Assert
    assert len(trades) == 10_000
    assert trades[0].instrument_id == InstrumentId.from_str("BTC-USD.HUOBI_DELIVERY")
    assert trades[0].bid_price == Price.from_str("8629.2")
    assert trades[0].ask_price == Price.from_str("8629.3")
    assert trades[0].bid_size == Quantity.from_str("806")
    assert trades[0].ask_size == Quantity.from_str("5494")
    assert trades[0].ts_event == 1588291201099000000
    assert trades[0].ts_init == 1588291201234268000


@pytest.mark.parametrize(
    ("price_precision", "size_precision"),
    [
        [None, None],
        [None, 0],
        [1, None],
        [1, 0],
    ],
)
def test_tardis_load_trades(
    price_precision: int | None,
    size_precision: int | None,
):
    # Arrange
    filepath = ensure_data_exists_tardis_bitmex_trades()
    loader = TardisCSVDataLoader(
        price_precision=price_precision,
        size_precision=size_precision,
    )

    # Act
    trades = loader.load_trades(filepath, limit=10_000)

    # Assert
    assert len(trades) == 10_000
    assert trades[0].instrument_id == InstrumentId.from_str("XBTUSD.BITMEX")
    assert trades[0].price == Price.from_str("8531.5")
    assert trades[0].size == Quantity.from_str("2152")
    assert trades[0].aggressor_side == AggressorSide.SELLER
    assert trades[0].trade_id == TradeId("ccc3c1fa-212c-e8b0-1706-9b9c4f3d5ecf")
    assert trades[0].ts_event == 1583020803145000000
    assert trades[0].ts_init == 1583020803307160000
