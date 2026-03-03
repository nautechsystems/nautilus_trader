#!/usr/bin/env python3
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
Smoke test proving Bybit + Binance bookbuilding in Nautilus.

Subscribes to L2 deltas and trades for both venues, maintains local books,
and logs top-of-book updates as deltas stream in.
"""

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


class MultiVenueBookSmokeConfig(StrategyConfig, frozen=True):
    instrument_ids: tuple[InstrumentId, ...]


class MultiVenueBookSmoke(Strategy):
    def __init__(self, config: MultiVenueBookSmokeConfig) -> None:
        super().__init__(config)
        self._books: dict[InstrumentId, OrderBook] = {}
        self._last_bbo: dict[InstrumentId, tuple[str, str, str, str]] = {}

    def on_start(self) -> None:
        for instrument_id in self.config.instrument_ids:
            instrument = self.cache.instrument(instrument_id)
            if instrument is None:
                self.log.error(f"Could not find instrument for {instrument_id}")
                self.stop()
                return

            self._books[instrument_id] = OrderBook(
                instrument_id=instrument.id,
                book_type=BookType.L2_MBP,
            )
            self.subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=BookType.L2_MBP,
            )
            self.subscribe_trade_ticks(instrument_id=instrument_id)
            self.log.info(f"Subscribed to deltas + trades for {instrument_id}")

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        book = self._books.get(deltas.instrument_id)
        if book is None:
            return

        book.apply_deltas(deltas)

        bid_price = book.best_bid_price()
        ask_price = book.best_ask_price()
        bid_size = book.best_bid_size()
        ask_size = book.best_ask_size()

        if bid_price is None or ask_price is None or bid_size is None or ask_size is None:
            return

        bbo = (str(bid_price), str(bid_size), str(ask_price), str(ask_size))
        if bbo == self._last_bbo.get(deltas.instrument_id):
            return

        self._last_bbo[deltas.instrument_id] = bbo
        self.log.info(
            f"BBO {deltas.instrument_id} | bid={bbo[1]} @ {bbo[0]} | ask={bbo[3]} @ {bbo[2]}",
        )

    def on_trade_tick(self, tick: TradeTick) -> None:
        self.log.info(f"TRADE {tick.instrument_id} | px={tick.price} qty={tick.size}")


BYBIT_INSTRUMENT_ID = InstrumentId.from_str(f"BTCUSDT-LINEAR.{BYBIT}")
BINANCE_INSTRUMENT_ID = InstrumentId.from_str(f"BTCUSDT.{BINANCE}")

config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    data_clients={
        BYBIT: BybitDataClientConfig(
            api_key=None,
            api_secret=None,
            product_types=(BybitProductType.LINEAR,),
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset({BYBIT_INSTRUMENT_ID}),
            ),
        ),
        BINANCE: BinanceDataClientConfig(
            api_key=None,
            api_secret=None,
            account_type=BinanceAccountType.SPOT,
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset({BINANCE_INSTRUMENT_ID}),
            ),
        ),
    },
    timeout_connection=20.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

node = TradingNode(config=config_node)

strategy = MultiVenueBookSmoke(
    config=MultiVenueBookSmokeConfig(
        instrument_ids=(
            BYBIT_INSTRUMENT_ID,
            BINANCE_INSTRUMENT_ID,
        ),
    ),
)

node.trader.add_strategy(strategy)
node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.build()

if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
