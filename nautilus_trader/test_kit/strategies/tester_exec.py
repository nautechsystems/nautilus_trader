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

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import MarkPriceUpdate
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class ExecTesterConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``ExecTester`` instances.
    """

    instrument_id: InstrumentId
    order_qty: Decimal
    order_expire_time_delta_mins: PositiveInt | None = None
    order_params: dict[str, Any] | None = None
    client_id: ClientId | None = None
    subscribe_quotes: bool = True
    subscribe_trades: bool = True
    subscribe_book: bool = False
    book_type: BookType = BookType.L2_MBP
    book_depth: PositiveInt | None = None
    book_interval_ms: PositiveInt = 1000
    book_levels_to_print: PositiveInt = 10
    enable_buys: bool = True
    enable_sells: bool = True
    open_position_on_start_qty: Decimal | None = None
    open_position_time_in_force: TimeInForce = TimeInForce.GTC
    tob_offset_ticks: PositiveInt = 500  # Definitely out of the market
    modify_orders_to_maintain_tob_offset: bool = False
    cancel_replace_orders_to_maintain_tob_offset: bool = False
    use_post_only: bool = True
    use_quote_quantity: bool = False
    emulation_trigger: str = "NO_TRIGGER"
    cancel_orders_on_stop: bool = True
    close_positions_on_stop: bool = True
    close_positions_time_in_force: TimeInForce | None = None
    reduce_only_on_stop: bool = True
    use_individual_cancels_on_stop: bool = False
    use_batch_cancel_on_stop: bool = False
    dry_run: bool = False
    log_data: bool = True
    test_reject_post_only: bool = False
    can_unsubscribe: bool = True


class ExecTester(Strategy):
    """
    A strategy for testing execution functionality for integration adapters.

    Cancels all orders and closes all positions on stop by default.

    Parameters
    ----------
    config : ExecTesterConfig
        The configuration for the instance.

    """

    def __init__(self, config: ExecTesterConfig) -> None:
        super().__init__(config)

        self.instrument: Instrument | None = None  # Initialized in on_start
        self.client_id = config.client_id

        # Order management
        self.buy_order: LimitOrder | None = None
        self.sell_order: LimitOrder | None = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        self.price_offset = self.get_price_offset(self.instrument)

        # Subscribe to live data
        if self.config.subscribe_quotes:
            self.subscribe_quote_ticks(self.config.instrument_id, client_id=self.client_id)

        if self.config.subscribe_trades:
            self.subscribe_trade_ticks(self.config.instrument_id, client_id=self.client_id)

        if self.config.subscribe_book:
            self.subscribe_order_book_at_interval(
                self.config.instrument_id,
                book_type=self.config.book_type,
                depth=self.config.book_depth or 0,
                interval_ms=self.config.book_interval_ms,
                client_id=self.client_id,
            )

        if self.config.open_position_on_start_qty:
            self.open_position(self.config.open_position_on_start_qty)

    def on_order_book(self, book: OrderBook) -> None:
        """
        Actions to be performed when the strategy is running and receives a book.
        """
        if self.config.log_data:
            num_levels = self.config.book_levels_to_print
            self.log.info(
                f"\n{book.instrument_id}\n{book.pprint(num_levels)}",
                LogColor.CYAN,
            )

        best_bid = book.best_bid_price()
        best_ask = book.best_ask_price()
        if best_bid is None or best_ask is None:
            return  # Wait for market

        self.maintain_orders(best_bid, best_ask)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when the strategy is running and receives book deltas.
        """
        if self.config.log_data:
            self.log.info(repr(deltas), LogColor.CYAN)

    def on_quote_tick(self, quote: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote.
        """
        if self.config.log_data:
            self.log.info(repr(quote), LogColor.CYAN)

        self.maintain_orders(quote.bid_price, quote.ask_price)

    def on_trade_tick(self, trade: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade.
        """
        if self.config.log_data:
            self.log.info(repr(trade), LogColor.CYAN)

    def on_mark_price(self, update: MarkPriceUpdate) -> None:
        """
        Actions to be performed when the strategy is running and receives a mark price
        update.
        """
        if self.config.log_data:
            self.log.info(repr(update), LogColor.CYAN)

    def on_index_price(self, update: MarkPriceUpdate) -> None:
        """
        Actions to be performed when the strategy is running and receives an index price
        update.
        """
        if self.config.log_data:
            self.log.info(repr(update), LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when the strategy is running and receives a bar.
        """
        if self.config.log_data:
            self.log.info(repr(bar), LogColor.CYAN)

    def maintain_orders(self, best_bid: Price, best_ask: Price) -> None:
        if self.instrument is None or self.config.dry_run:
            return

        if self.config.enable_buys:
            self.maintain_buy_orders(self.instrument, best_bid, best_ask)

        if self.config.enable_sells:
            self.maintain_sell_orders(self.instrument, best_bid, best_ask)

    def maintain_buy_orders(
        self,
        instrument: Instrument,
        best_bid: Price,
        best_ask: Price,
    ) -> None:
        price = instrument.make_price(best_bid - self.price_offset)

        if not self.buy_order or not self.is_order_active(self.buy_order):
            if self.config.use_post_only and self.config.test_reject_post_only:
                price = instrument.make_price(best_ask + self.price_offset)

            self.submit_buy_limit_order(price)
        elif (
            self.buy_order
            and self.buy_order.venue_order_id
            and not self.buy_order.is_pending_update
            and not self.buy_order.is_pending_cancel
            and self.buy_order.price < price
        ):
            if self.config.modify_orders_to_maintain_tob_offset:
                self.modify_order(self.buy_order, price=price)
            elif self.config.cancel_replace_orders_to_maintain_tob_offset:
                self.cancel_order(self.buy_order)
                self.submit_buy_limit_order(price)

    def maintain_sell_orders(
        self,
        instrument: Instrument,
        best_bid: Price,
        best_ask: Price,
    ) -> None:
        price = instrument.make_price(best_ask + self.price_offset)

        if not self.sell_order or not self.is_order_active(self.sell_order):
            if self.config.use_post_only and self.config.test_reject_post_only:
                price = instrument.make_price(best_bid - self.price_offset)

            self.submit_sell_limit_order(price)
        elif (
            self.sell_order
            and self.sell_order.venue_order_id
            and not self.sell_order.is_pending_update
            and not self.sell_order.is_pending_cancel
            and self.sell_order.price > price
        ):
            if self.config.modify_orders_to_maintain_tob_offset:
                self.modify_order(self.sell_order, price=price)
            elif self.config.cancel_replace_orders_to_maintain_tob_offset:
                self.cancel_order(self.sell_order)
                self.submit_sell_limit_order(price)

    def open_position(self, net_qty: Decimal) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if net_qty == Decimal(0):
            self.log.warning(f"Open position with {net_qty}, skipping")
            return

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY if net_qty > 0 else OrderSide.SELL,
            quantity=self.instrument.make_qty(self.config.order_qty),
            time_in_force=self.config.open_position_time_in_force,
            quote_quantity=self.config.use_quote_quantity,
        )

        self.submit_order(
            order,
            client_id=self.client_id,
            params=self.config.order_params,
        )

    def get_price_offset(self, instrument: Instrument) -> Decimal:
        return instrument.price_increment * self.config.tob_offset_ticks

    def is_order_active(self, order: Order) -> bool:
        return order.is_active_local or order.is_inflight or order.is_open

    def submit_buy_limit_order(self, price: Price) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if self.config.dry_run:
            self.log.warning("Dry run, skipping create BUY order")
            return

        if not self.config.enable_buys:
            self.log.warning("BUY orders not enabled, skipping")
            return

        if self.config.order_expire_time_delta_mins is not None:
            time_in_force = TimeInForce.GTD
            expire_time = self.clock.utc_now() + pd.Timedelta(
                minutes=self.config.order_expire_time_delta_mins,
            )
        else:
            time_in_force = TimeInForce.GTC
            expire_time = None

        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.config.order_qty),
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            post_only=self.config.use_post_only,
            quote_quantity=self.config.use_quote_quantity,
            emulation_trigger=TriggerType[self.config.emulation_trigger],
        )

        self.buy_order = order
        self.submit_order(
            order,
            client_id=self.client_id,
            params=self.config.order_params,
        )

    def submit_sell_limit_order(self, price: Price) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if self.config.dry_run:
            self.log.warning("Dry run, skipping create SELL order")
            return

        if not self.config.enable_sells:
            self.log.warning("SELL orders not enabled, skipping")
            return

        if self.config.order_expire_time_delta_mins is not None:
            time_in_force = TimeInForce.GTD
            expire_time = self.clock.utc_now() + pd.Timedelta(
                minutes=self.config.order_expire_time_delta_mins,
            )
        else:
            time_in_force = TimeInForce.GTC
            expire_time = None

        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.config.order_qty),
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            post_only=self.config.use_post_only,
            quote_quantity=self.config.use_quote_quantity,
            emulation_trigger=TriggerType[self.config.emulation_trigger],
        )

        self.sell_order = order
        self.submit_order(
            order,
            client_id=self.client_id,
            params=self.config.order_params,
        )

    def on_stop(self) -> None:  # noqa: C901 (too complex)
        """
        Actions to be performed when the strategy is stopped.
        """
        if self.config.dry_run:
            self.log.warning("Dry run mode, skipping cancel all orders and close all positions")
            return

        if self.config.cancel_orders_on_stop:
            if self.config.use_individual_cancels_on_stop:
                for order in self.cache.orders_open(
                    instrument_id=self.config.instrument_id,
                    strategy_id=self.id,
                ):
                    self.cancel_order(order)
            elif self.config.use_batch_cancel_on_stop:
                open_orders = self.cache.orders_open(
                    instrument_id=self.config.instrument_id,
                    strategy_id=self.id,
                )
                if open_orders:
                    self.cancel_orders(open_orders, client_id=self.client_id)
            else:
                self.cancel_all_orders(self.config.instrument_id, client_id=self.client_id)

        if self.config.close_positions_on_stop:
            self.close_all_positions(
                instrument_id=self.config.instrument_id,
                client_id=self.client_id,
                time_in_force=self.config.close_positions_time_in_force or TimeInForce.GTC,
                reduce_only=self.config.reduce_only_on_stop,
            )

        # Unsubscribe from data (if supported)
        if self.config.can_unsubscribe:
            if self.config.subscribe_quotes:
                self.unsubscribe_quote_ticks(self.config.instrument_id, client_id=self.client_id)

            if self.config.subscribe_trades:
                self.unsubscribe_trade_ticks(self.config.instrument_id, client_id=self.client_id)

            if self.config.subscribe_book:
                self.unsubscribe_order_book_at_interval(
                    self.config.instrument_id,
                    client_id=self.client_id,
                )
