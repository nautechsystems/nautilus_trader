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
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import LimitIfTouchedOrder
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class ExecTesterConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``ExecTester`` instances.
    """

    instrument_id: InstrumentId
    order_qty: Decimal
    order_display_qty: Decimal | None = None
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
    open_position_on_start_qty: Decimal | None = None
    open_position_time_in_force: TimeInForce = TimeInForce.GTC
    enable_buys: bool = True
    enable_sells: bool = True
    enable_stop_buys: bool = False
    enable_stop_sells: bool = False
    tob_offset_ticks: PositiveInt = 500  # Definitely out of the market
    stop_order_type: OrderType = OrderType.STOP_MARKET
    stop_offset_ticks: PositiveInt = 100
    stop_limit_offset_ticks: PositiveInt | None = None
    stop_trigger_type: TriggerType | str | None = None
    enable_brackets: bool = False
    bracket_entry_order_type: OrderType = OrderType.LIMIT
    bracket_offset_ticks: PositiveInt = 500
    modify_orders_to_maintain_tob_offset: bool = False
    modify_stop_orders_to_maintain_offset: bool = False
    cancel_replace_orders_to_maintain_tob_offset: bool = False
    cancel_replace_stop_orders_to_maintain_offset: bool = False
    use_post_only: bool = False
    use_quote_quantity: bool = False
    emulation_trigger: TriggerType | str | None = None
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
        self.buy_stop_order: Order | None = None
        self.sell_stop_order: Order | None = None

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

            own_book = self.cache.own_order_book(book.instrument_id)
            if own_book:
                self.log.info(
                    f"\n{own_book.instrument_id}\n{own_book.pprint(num_levels)}",
                    LogColor.MAGENTA,
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

        if self.config.enable_stop_buys:
            self.maintain_stop_buy_orders(self.instrument, best_bid, best_ask)

        if self.config.enable_stop_sells:
            self.maintain_stop_sell_orders(self.instrument, best_bid, best_ask)

    def maintain_buy_orders(
        self,
        instrument: Instrument,
        best_bid: Price,
        best_ask: Price,
    ) -> None:
        price = instrument.make_price(best_bid - self.price_offset)

        if self.config.enable_brackets:
            if not self.buy_order or not self.is_order_active(self.buy_order):
                self.submit_bracket_order(OrderSide.BUY, price)
            return

        if not self.buy_order or not self.is_order_active(self.buy_order):
            if self.config.use_post_only and self.config.test_reject_post_only:
                price = instrument.make_price(best_ask + self.price_offset)

            self.submit_limit_order(OrderSide.BUY, price)
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
                self.submit_limit_order(OrderSide.BUY, price)

    def maintain_sell_orders(
        self,
        instrument: Instrument,
        best_bid: Price,
        best_ask: Price,
    ) -> None:
        price = instrument.make_price(best_ask + self.price_offset)

        if self.config.enable_brackets:
            if not self.sell_order or not self.is_order_active(self.sell_order):
                self.submit_bracket_order(OrderSide.SELL, price)
            return

        if not self.sell_order or not self.is_order_active(self.sell_order):
            if self.config.use_post_only and self.config.test_reject_post_only:
                price = instrument.make_price(best_bid - self.price_offset)

            self.submit_limit_order(OrderSide.SELL, price)
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
                self.submit_limit_order(OrderSide.SELL, price)

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

    def submit_limit_order(self, order_side: OrderSide, price: Price) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if self.config.dry_run:
            self.log.warning(f"Dry run, skipping create {order_side} order")
            return

        if order_side == OrderSide.BUY and not self.config.enable_buys:
            self.log.warning("BUY orders not enabled, skipping")
            return
        elif order_side == OrderSide.SELL and not self.config.enable_sells:
            self.log.warning("SELL orders not enabled, skipping")
            return

        if self.config.enable_brackets:
            self.submit_bracket_order(order_side, price)
            return

        if self.config.order_expire_time_delta_mins is not None:
            time_in_force = TimeInForce.GTD
            expire_time = self.clock.utc_now() + pd.Timedelta(
                minutes=self.config.order_expire_time_delta_mins,
            )
        else:
            time_in_force = TimeInForce.GTC
            expire_time = None

        if self.config.order_display_qty is not None:
            # Zero display_qty represents a "hidden" order, otherwise "iceberg"
            display_qty = self.instrument.make_qty(self.config.order_display_qty)
        else:
            display_qty = None

        emulation_trigger = (
            TriggerType[self.config.emulation_trigger]
            if isinstance(self.config.emulation_trigger, str)
            else (
                self.config.emulation_trigger
                if self.config.emulation_trigger
                else TriggerType.NO_TRIGGER
            )
        )

        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=order_side,
            quantity=self.instrument.make_qty(self.config.order_qty),
            price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            post_only=self.config.use_post_only,
            quote_quantity=self.config.use_quote_quantity,
            display_qty=display_qty,
            emulation_trigger=emulation_trigger,
        )

        if order_side == OrderSide.BUY:
            self.buy_order = order
        else:
            self.sell_order = order

        self.submit_order(
            order,
            client_id=self.client_id,
            params=self.config.order_params,
        )

    def submit_bracket_order(
        self,
        order_side: OrderSide,
        price: Price,
    ) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if self.config.dry_run:
            self.log.warning(f"Dry run, skipping create {order_side} bracket order")
            return

        if order_side == OrderSide.BUY and not self.config.enable_buys:
            self.log.warning("BUY orders not enabled, skipping")
            return
        elif order_side == OrderSide.SELL and not self.config.enable_sells:
            self.log.warning("SELL orders not enabled, skipping")
            return

        if self.config.bracket_entry_order_type != OrderType.LIMIT:
            self.log.error("Only LIMIT entry bracket orders are currently supported")
            return

        if self.config.order_expire_time_delta_mins is not None:
            time_in_force = TimeInForce.GTD
            expire_time = self.clock.utc_now() + pd.Timedelta(
                minutes=self.config.order_expire_time_delta_mins,
            )
        else:
            time_in_force = TimeInForce.GTC
            expire_time = None

        emulation_trigger = (
            TriggerType[self.config.emulation_trigger]
            if isinstance(self.config.emulation_trigger, str)
            else (
                self.config.emulation_trigger
                if self.config.emulation_trigger
                else TriggerType.NO_TRIGGER
            )
        )

        trigger_type = (
            TriggerType[self.config.stop_trigger_type]
            if isinstance(self.config.stop_trigger_type, str)
            else (
                self.config.stop_trigger_type
                if self.config.stop_trigger_type
                else TriggerType.DEFAULT
            )
        )

        target_offset = self.instrument.price_increment * self.config.bracket_offset_ticks
        stop_offset = self.instrument.price_increment * self.config.bracket_offset_ticks
        entry_value = Decimal(str(price))

        if order_side == OrderSide.BUY:
            tp_price = self.instrument.make_price(entry_value + target_offset)
            sl_trigger_price = self.instrument.make_price(entry_value - stop_offset)
        else:
            tp_price = self.instrument.make_price(entry_value - target_offset)
            sl_trigger_price = self.instrument.make_price(entry_value + stop_offset)

        order_list = self.order_factory.bracket(
            instrument_id=self.config.instrument_id,
            order_side=order_side,
            quantity=self.instrument.make_qty(self.config.order_qty),
            quote_quantity=self.config.use_quote_quantity,
            emulation_trigger=emulation_trigger,
            entry_order_type=self.config.bracket_entry_order_type,
            entry_price=price,
            time_in_force=time_in_force,
            expire_time=expire_time,
            entry_post_only=self.config.use_post_only,
            tp_price=tp_price,
            tp_time_in_force=time_in_force,
            tp_post_only=self.config.use_post_only,
            sl_trigger_price=sl_trigger_price,
            sl_trigger_type=trigger_type,
            sl_time_in_force=time_in_force,
        )

        entry_order = order_list.first
        if order_side == OrderSide.BUY:
            self.buy_order = entry_order
            self.buy_stop_order = None
        else:
            self.sell_order = entry_order
            self.sell_stop_order = None

        self.submit_order_list(
            order_list,
            client_id=self.client_id,
            params=self.config.order_params,
        )

    def submit_stop_order(  # noqa: C901
        self,
        order_side: OrderSide,
        trigger_price: Price,
        limit_price: Price | None = None,
    ) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if self.config.dry_run:
            self.log.warning(f"Dry run, skipping create {order_side} stop order")
            return

        if order_side == OrderSide.BUY and not self.config.enable_stop_buys:
            self.log.warning("BUY stop orders not enabled, skipping")
            return
        elif order_side == OrderSide.SELL and not self.config.enable_stop_sells:
            self.log.warning("SELL stop orders not enabled, skipping")
            return

        if self.config.order_expire_time_delta_mins is not None:
            time_in_force = TimeInForce.GTD
            expire_time = self.clock.utc_now() + pd.Timedelta(
                minutes=self.config.order_expire_time_delta_mins,
            )
        else:
            time_in_force = TimeInForce.GTC
            expire_time = None

        trigger_type = (
            TriggerType[self.config.stop_trigger_type]
            if isinstance(self.config.stop_trigger_type, str)
            else (
                self.config.stop_trigger_type
                if self.config.stop_trigger_type
                else TriggerType.DEFAULT
            )
        )
        emulation_trigger = (
            TriggerType[self.config.emulation_trigger]
            if isinstance(self.config.emulation_trigger, str)
            else (
                self.config.emulation_trigger
                if self.config.emulation_trigger
                else TriggerType.NO_TRIGGER
            )
        )

        if self.config.stop_order_type == OrderType.STOP_MARKET:
            order = self.order_factory.stop_market(
                instrument_id=self.config.instrument_id,
                order_side=order_side,
                quantity=self.instrument.make_qty(self.config.order_qty),
                trigger_price=trigger_price,
                trigger_type=trigger_type,
                time_in_force=time_in_force,
                expire_time=expire_time,
                quote_quantity=self.config.use_quote_quantity,
                emulation_trigger=emulation_trigger,
            )
        elif self.config.stop_order_type == OrderType.STOP_LIMIT:
            if limit_price is None:
                self.log.error("STOP_LIMIT order requires limit_price")
                return

            if self.config.order_display_qty is not None:
                # Zero display_qty represents a "hidden" order, otherwise "iceberg"
                display_qty = self.instrument.make_qty(self.config.order_display_qty)
            else:
                display_qty = None

            order = self.order_factory.stop_limit(
                instrument_id=self.config.instrument_id,
                order_side=order_side,
                quantity=self.instrument.make_qty(self.config.order_qty),
                price=limit_price,
                trigger_price=trigger_price,
                trigger_type=trigger_type,
                time_in_force=time_in_force,
                expire_time=expire_time,
                post_only=False,
                quote_quantity=self.config.use_quote_quantity,
                display_qty=display_qty,
                emulation_trigger=emulation_trigger,
            )
        elif self.config.stop_order_type == OrderType.MARKET_IF_TOUCHED:
            order = self.order_factory.market_if_touched(
                instrument_id=self.config.instrument_id,
                order_side=order_side,
                quantity=self.instrument.make_qty(self.config.order_qty),
                trigger_price=trigger_price,
                trigger_type=trigger_type,
                time_in_force=time_in_force,
                expire_time=expire_time,
                quote_quantity=self.config.use_quote_quantity,
                emulation_trigger=emulation_trigger,
            )
        elif self.config.stop_order_type == OrderType.LIMIT_IF_TOUCHED:
            if limit_price is None:
                self.log.error("LIMIT_IF_TOUCHED order requires limit_price")
                return

            if self.config.order_display_qty is not None:
                # Zero display_qty represents a "hidden" order, otherwise "iceberg"
                display_qty = self.instrument.make_qty(self.config.order_display_qty)
            else:
                display_qty = None

            order = self.order_factory.limit_if_touched(
                instrument_id=self.config.instrument_id,
                order_side=order_side,
                quantity=self.instrument.make_qty(self.config.order_qty),
                price=limit_price,
                trigger_price=trigger_price,
                trigger_type=trigger_type,
                time_in_force=time_in_force,
                expire_time=expire_time,
                post_only=False,
                quote_quantity=self.config.use_quote_quantity,
                display_qty=display_qty,
                emulation_trigger=emulation_trigger,
            )
        else:
            self.log.error(f"Unknown stop order type: {self.config.stop_order_type}")
            return

        if order_side == OrderSide.BUY:
            self.buy_stop_order = order
        else:
            self.sell_stop_order = order

        self.submit_order(
            order,
            client_id=self.client_id,
            params=self.config.order_params,
        )

    def maintain_stop_buy_orders(
        self,
        instrument: Instrument,
        best_bid: Price,
        best_ask: Price,
    ) -> None:
        stop_offset = instrument.price_increment * self.config.stop_offset_ticks

        # Determine trigger price based on order type
        if self.config.stop_order_type in (OrderType.LIMIT_IF_TOUCHED, OrderType.MARKET_IF_TOUCHED):
            # IF_TOUCHED buy: place BELOW market (buy on dip)
            trigger_price = instrument.make_price(best_bid - stop_offset)
        else:
            # STOP buy orders are placed ABOVE the market (stop loss on short)
            trigger_price = instrument.make_price(best_ask + stop_offset)

        limit_price = None
        if self.config.stop_order_type in (OrderType.STOP_LIMIT, OrderType.LIMIT_IF_TOUCHED):
            if self.config.stop_limit_offset_ticks:
                limit_offset = instrument.price_increment * self.config.stop_limit_offset_ticks
                # For IF_TOUCHED buy, limit should be below trigger (better price)
                # For STOP buy, limit should be above trigger (worse price acceptable)
                if self.config.stop_order_type == OrderType.LIMIT_IF_TOUCHED:
                    limit_price = instrument.make_price(trigger_price - limit_offset)
                else:
                    limit_price = instrument.make_price(trigger_price + limit_offset)
            else:
                # Default: use trigger price as limit price
                limit_price = trigger_price

        if not self.buy_stop_order or not self.is_order_active(self.buy_stop_order):
            self.submit_stop_order(OrderSide.BUY, trigger_price, limit_price)
        elif (
            self.buy_stop_order
            and self.buy_stop_order.venue_order_id
            and not self.buy_stop_order.is_pending_update
            and not self.buy_stop_order.is_pending_cancel
        ):
            # Check if we need to adjust the stop order
            current_trigger = self.get_order_trigger_price(self.buy_stop_order)
            if current_trigger and current_trigger != trigger_price:
                if self.config.modify_stop_orders_to_maintain_offset:
                    # Modification not supported for all stop order types
                    self.log.warning("Stop order modification not yet implemented")
                elif self.config.cancel_replace_stop_orders_to_maintain_offset:
                    self.cancel_order(self.buy_stop_order)
                    self.submit_stop_order(OrderSide.BUY, trigger_price, limit_price)

    def maintain_stop_sell_orders(
        self,
        instrument: Instrument,
        best_bid: Price,
        best_ask: Price,
    ) -> None:
        stop_offset = instrument.price_increment * self.config.stop_offset_ticks

        # Determine trigger price based on order type
        if self.config.stop_order_type in (OrderType.LIMIT_IF_TOUCHED, OrderType.MARKET_IF_TOUCHED):
            # IF_TOUCHED sell: place ABOVE market (sell on rally)
            trigger_price = instrument.make_price(best_ask + stop_offset)
        else:
            # STOP sell orders are placed BELOW the market (stop loss on long)
            trigger_price = instrument.make_price(best_bid - stop_offset)

        limit_price = None
        if self.config.stop_order_type in (OrderType.STOP_LIMIT, OrderType.LIMIT_IF_TOUCHED):
            if self.config.stop_limit_offset_ticks:
                limit_offset = instrument.price_increment * self.config.stop_limit_offset_ticks
                # For IF_TOUCHED sell, limit should be above trigger (better price)
                # For STOP sell, limit should be below trigger (worse price acceptable)
                if self.config.stop_order_type == OrderType.LIMIT_IF_TOUCHED:
                    limit_price = instrument.make_price(trigger_price + limit_offset)
                else:
                    limit_price = instrument.make_price(trigger_price - limit_offset)
            else:
                # Default: use trigger price as limit price
                limit_price = trigger_price

        if not self.sell_stop_order or not self.is_order_active(self.sell_stop_order):
            self.submit_stop_order(OrderSide.SELL, trigger_price, limit_price)
        elif (
            self.sell_stop_order
            and self.sell_stop_order.venue_order_id
            and not self.sell_stop_order.is_pending_update
            and not self.sell_stop_order.is_pending_cancel
        ):
            # Check if we need to adjust the stop order
            current_trigger = self.get_order_trigger_price(self.sell_stop_order)
            if current_trigger and current_trigger != trigger_price:
                if self.config.modify_stop_orders_to_maintain_offset:
                    # Modification not supported for all stop order types
                    self.log.warning("Stop order modification not yet implemented")
                elif self.config.cancel_replace_stop_orders_to_maintain_offset:
                    self.cancel_order(self.sell_stop_order)
                    self.submit_stop_order(OrderSide.SELL, trigger_price, limit_price)

    def get_order_trigger_price(self, order: Order) -> Price | None:
        """
        Get the trigger price for stop/conditional orders.
        """
        if isinstance(
            order,
            StopMarketOrder | StopLimitOrder | MarketIfTouchedOrder | LimitIfTouchedOrder,
        ):
            return order.trigger_price
        return None

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
