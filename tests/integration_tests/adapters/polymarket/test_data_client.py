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

from decimal import Decimal
from typing import Any
from unittest.mock import MagicMock

from nautilus_trader.adapters.polymarket.config import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket.data import PolymarketDataClient
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookLevel
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class _RecordingPolymarketDataClient(PolymarketDataClient):
    def __init__(self, *args: Any, **kwargs: Any) -> None:
        super().__init__(*args, **kwargs)
        self.emitted: list[Any] = []

    def _handle_data(self, data: Any) -> None:
        self.emitted.append(data)


def _make_binary_option(price_inc: str) -> BinaryOption:
    instrument_id = InstrumentId.from_str(
        "0xABCDEF.POLYMARKET",
    )
    price_increment = Price.from_str(price_inc)
    size_increment = Quantity.from_str("0.01")
    return BinaryOption(
        instrument_id=instrument_id,
        raw_symbol=instrument_id.symbol,
        outcome="YES",
        description="Test Polymarket Instrument",
        asset_class=AssetClass.ALTERNATIVE,
        currency=USDC,
        price_precision=price_increment.precision,
        price_increment=price_increment,
        size_precision=size_increment.precision,
        size_increment=size_increment,
        activation_ns=0,
        expiration_ns=0,
        max_quantity=None,
        min_quantity=Quantity.from_int(1),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
        ts_event=0,
        ts_init=0,
    )


def _build_snapshot(prices: tuple[str, str, str, str]) -> PolymarketBookSnapshot:
    bid_low, bid_high, ask_low, ask_high = prices
    return PolymarketBookSnapshot(
        market="0xMARKET",
        asset_id="0xASSET",
        bids=[
            PolymarketBookLevel(price=bid_low, size="15"),
            PolymarketBookLevel(price=bid_high, size="10"),
        ],
        asks=[
            PolymarketBookLevel(price=ask_high, size="12"),
            PolymarketBookLevel(price=ask_low, size="8"),
        ],
        timestamp="1700000000000",
    )


def test_tick_size_change_rebuilds_local_book_precision(event_loop) -> None:
    # Arrange
    loop = event_loop
    clock = LiveClock()
    msgbus = MessageBus(trader_id=TraderId("TEST-001"), clock=clock)
    cache = Cache()
    provider = MagicMock(spec=PolymarketInstrumentProvider)
    http_client = MagicMock()

    config = PolymarketDataClientConfig()
    client = _RecordingPolymarketDataClient(
        loop=loop,
        http_client=http_client,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        instrument_provider=provider,
        config=config,
        name="TEST-POLYMARKET",
    )

    instrument_old = _make_binary_option("0.01")
    client._cache.add_instrument(instrument_old)
    client._add_subscription_quote_ticks(instrument_old.id)

    snapshot_old = _build_snapshot(("0.90", "0.94", "0.96", "0.99"))
    deltas_old = snapshot_old.parse_to_snapshot(instrument=instrument_old, ts_init=0)
    book_old = OrderBook(instrument_old.id, book_type=BookType.L2_MBP)
    book_old.apply_deltas(deltas_old)
    client._local_books[instrument_old.id] = book_old

    quote_old = snapshot_old.parse_to_quote(
        instrument=instrument_old,
        ts_init=0,
        drop_quotes_missing_side=False,
    )
    assert quote_old is not None
    client._last_quotes[instrument_old.id] = quote_old

    change = PolymarketTickSizeChange(
        market="0xMARKET",
        asset_id="0xASSET",
        new_tick_size="0.001",
        old_tick_size="0.01",
        timestamp="1700000001000",
    )

    # Act
    client._handle_instrument_update(instrument=instrument_old, ws_message=change)

    # Assert
    instrument_id = instrument_old.id
    provider.add.assert_called_once()

    cached_instrument = client._cache.instrument(instrument_id)
    assert cached_instrument is not None
    assert cached_instrument.price_precision == 3

    rebuilt_book = client._local_books[instrument_id]
    bid_price = rebuilt_book.best_bid_price()
    ask_price = rebuilt_book.best_ask_price()
    assert bid_price is not None and ask_price is not None
    assert bid_price.precision == ask_price.precision == 3

    assert any(
        isinstance(item, QuoteTick)
        and item.instrument_id == instrument_id
        and item.bid_price.precision == item.ask_price.precision == 3
        for item in client.emitted
    )
