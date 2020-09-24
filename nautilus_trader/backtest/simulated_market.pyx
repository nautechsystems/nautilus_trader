# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pytz

from cpython.datetime cimport datetime

from nautilus_trader.backtest.clock cimport TestClock
from nautilus_trader.backtest.config cimport BacktestConfig
from nautilus_trader.backtest.logging cimport TestLogger
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.uuid cimport TestUUIDFactory
from nautilus_trader.common.account cimport Account
from nautilus_trader.common.market cimport ExchangeRateCalculator
from nautilus_trader.common.market cimport CommissionModel
from nautilus_trader.common.market cimport RolloverInterestCalculator
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.market_position cimport MarketPosition
from nautilus_trader.model.c_enums.market_position cimport market_position_to_string
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_type cimport OrderType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.commands cimport AccountInquiry
from nautilus_trader.model.commands cimport CancelOrder
from nautilus_trader.model.commands cimport ModifyOrder
from nautilus_trader.model.commands cimport SubmitBracketOrder
from nautilus_trader.model.commands cimport SubmitOrder
from nautilus_trader.model.events cimport AccountState
from nautilus_trader.model.events cimport OrderAccepted
from nautilus_trader.model.events cimport OrderCancelReject
from nautilus_trader.model.events cimport OrderCancelled
from nautilus_trader.model.events cimport OrderExpired
from nautilus_trader.model.events cimport OrderFillEvent
from nautilus_trader.model.events cimport OrderFilled
from nautilus_trader.model.events cimport OrderModified
from nautilus_trader.model.events cimport OrderRejected
from nautilus_trader.model.events cimport OrderSubmitted
from nautilus_trader.model.events cimport OrderWorking
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ClientPositionId
from nautilus_trader.model.identifiers cimport ExecutionId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Decimal64
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport LimitOrder
from nautilus_trader.model.order cimport MarketOrder
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick

_TZ_US_EAST = pytz.timezone("US/Eastern")


cdef class SimulatedMarket:
    """
    Provides a simulated brokerage.
    """

    def __init__(
            self,
            ExecutionEngine exec_engine not None,
            dict instruments not None: {Symbol, Instrument},
            BacktestConfig config not None,
            CommissionModel commission_model not None,
            FillModel fill_model not None,
            TestClock clock not None,
            TestUUIDFactory uuid_factory not None,
            TestLogger logger not None,
    ):
        """
        Initialize a new instance of the SimulatedBroker class.

        Parameters
        ----------
        exec_engine : ExecutionEngine
            The execution engine for the backtest.
        instruments : Dict[Symbol, Instrument]
            The instruments needed for the backtest.
        config : BacktestConfig
            The backtest configuration.
        commission_model : CommissionModel
            The commission model for the backtest.
        fill_model : FillModel
            The fill model for the backtest.
        clock : TestClock
            The clock for the component.
        uuid_factory : TestUUIDFactory
            The UUID factory for the component.
        logger : TestLogger
            The logger for the component.

        Raises
        ------
        TypeError
            If instruments contains a type other than Instrument.

        """
        Condition.dict_types(instruments, Symbol, Instrument, "instruments")

        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)

        self.exec_engine = exec_engine
        self.instruments = instruments

        self.day_number = 0
        self.rollover_time = None
        self.rollover_applied = False
        self.frozen_account = config.frozen_account
        self.starting_capital = config.starting_capital
        self.account_currency = config.account_currency
        self.account_capital = config.starting_capital
        self.account_cash_start_day = config.starting_capital
        self.account_cash_activity_day = Money(0, self.account_currency)

        self._account = Account(self.reset_account_event())
        self.exchange_calculator = ExchangeRateCalculator()
        self.commission_model = commission_model
        self.rollover_calculator = RolloverInterestCalculator(config.short_term_interest_csv_path)
        self.rollover_spread = 0.0  # Bank + Broker spread markup
        self.total_commissions = Money(0, self.account_currency)
        self.total_rollover = Money(0, self.account_currency)
        self.fill_model = fill_model

        self._market = {}               # type: {Symbol, QuoteTick}
        self._working_orders = {}       # type: {ClientOrderId, Order}
        self._child_orders = {}         # type: {ClientOrderId, [Order]}
        self._oco_orders = {}           # type: {ClientOrderId, ClientOrderId}
        self._position_oco_orders = {}  # type: {ClientPositionId, [ClientOrderId]}

        self._set_slippages()
        self._set_min_distances()

    cdef void _set_slippages(self) except *:
        cdef dict slippage_index = {}  # type: {Symbol, Decimal64}

        for symbol, instrument in self.instruments.items():
            slippage_index[symbol] = instrument.tick_size

        self._slippages = slippage_index

    cdef void _set_min_distances(self) except *:
        cdef dict min_stops = {}   # type: {Symbol, Decimal64}
        cdef dict min_limits = {}  # type: {Symbol, Decimal64}

        for symbol, instrument in self.instruments.items():
            min_stops[symbol] = Decimal64(
                instrument.tick_size * instrument.min_stop_distance,
                instrument.price_precision)

            min_limits[symbol] = Decimal64(
                instrument.tick_size * instrument.min_limit_distance,
                instrument.price_precision)

        self._min_stops = min_stops
        self._min_limits = min_limits

    cdef dict _build_current_bid_rates(self):
        cdef Symbol symbol
        cdef QuoteTick tick
        return {symbol.code: tick.bid.as_double() for symbol, tick in self._market.items()}

    cdef dict _build_current_ask_rates(self):
        cdef Symbol symbol
        cdef QuoteTick tick
        return {symbol.code: tick.ask.as_double() for symbol, tick in self._market.items()}

    cpdef void check_residuals(self) except *:
        """
        Check for any residual objects and log warnings if any are found.
        """
        for order_list in self._child_orders.values():
            for order in order_list:
                self._log.warning(f"Residual child-order {order}")

        for order_id in self._oco_orders.values():
            self._log.warning(f"Residual OCO {order_id}")

    cpdef void reset(self) except *:
        """
        Return the client to its initial state preserving tick data.
        """
        self._log.debug(f"Resetting...")

        self.day_number = 0
        self.account_capital = self.starting_capital
        self.account_cash_start_day = self.account_capital
        self.account_cash_activity_day = Money(0, self.account_currency)
        self.total_commissions = Money(0, self.account_currency)
        self.total_rollover = Money(0, self.account_currency)

        self._market = {}               # type: {Symbol, QuoteTick}
        self._working_orders = {}       # type: {ClientOrderId, PassiveOrder}
        self._child_orders = {}         # type: {ClientOrderId, [PassiveOrder]}
        self._oco_orders = {}           # type: {ClientOrderId, ClientOrderId}
        self._position_oco_orders = {}  # type: {ClientPositionId, [ClientOrderId]}

        self._log.info("Reset.")

    cdef AccountState reset_account_event(self):
        """
        Resets the account.
        """
        return AccountState(
            self.exec_engine.account_id,
            self.account_currency,
            self.starting_capital,
            self.starting_capital,
            Money(0, self.account_currency),
            Money(0, self.account_currency),
            Money(0, self.account_currency),
            Decimal64(),
            'N',
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

    cpdef datetime time_now(self):
        """
        Return the current time for the execution client.

        :return: datetime.
        """
        return self._clock.utc_now()

    cpdef void change_fill_model(self, FillModel fill_model) except *:
        """
        Set the fill model to be the given model.

        :param fill_model: The fill model to set.
        """
        Condition.not_none(fill_model, "fill_model")

        self.fill_model = fill_model

    cpdef void process_tick(self, QuoteTick tick) except *:
        """
        Process the execution client with the given tick. Market dynamics are
        simulated against working orders.

        :param tick: The tick data to process with.
        """
        Condition.not_none(tick, "tick")

        self._clock.set_time(tick.timestamp)
        self._market[tick.symbol] = tick

        cdef datetime time_now = self._clock.utc_now()

        cdef datetime rollover_local
        if self.day_number != time_now.day:
            # Set account statistics for new day
            self.day_number = time_now.day
            self.account_cash_start_day = self._account.cash_balance
            self.account_cash_activity_day = Money(0, self.account_currency)
            self.rollover_applied = False

            rollover_local = time_now.astimezone(_TZ_US_EAST)
            self.rollover_time = _TZ_US_EAST.localize(datetime(
                rollover_local.year,
                rollover_local.month,
                rollover_local.day,
                17),
            ).astimezone(pytz.utc)

        # Check for and apply any rollover interest
        if not self.rollover_applied and time_now >= self.rollover_time:
            self.apply_rollover_interest(time_now, self.rollover_time.isoweekday())
            self.rollover_applied = True

        # Check for working orders
        if not self._working_orders:
            return

        # Simulate market
        cdef ClientOrderId order_id
        cdef Order order
        cdef Instrument instrument
        for order in self._working_orders.copy().values():  # Copies list to avoid resize during loop
            if not order.symbol.equals(tick.symbol):
                continue  # Order is for a different symbol
            if not order.is_working():
                continue  # Orders state has changed since the loop commenced

            instrument = self.instruments[order.symbol]

            # Check for order fill
            if order.side == OrderSide.BUY:
                if order.type == OrderType.STOP:
                    if tick.ask.ge(order.price) or self._is_marginal_buy_stop_fill(order.price, tick):
                        del self._working_orders[order.cl_ord_id]  # Remove order from working orders
                        if self.fill_model.is_slipped():
                            self._fill_order(
                                order,
                                order.price.add(self._slippages[order.symbol]),
                                LiquiditySide.TAKER,
                            )
                        else:
                            self._fill_order(
                                order,
                                order.price,
                                LiquiditySide.TAKER,
                            )
                        continue  # Continue loop to next order
                elif order.type == OrderType.LIMIT:
                    if tick.ask.le(order.price) or self._is_marginal_buy_limit_fill(order.price, tick):
                        del self._working_orders[order.cl_ord_id]  # Remove order from working orders
                        self._fill_order(
                            order,
                            order.price,
                            LiquiditySide.MAKER,
                        )
                        continue  # Continue loop to next order
            elif order.side == OrderSide.SELL:
                if order.type == OrderType.STOP:
                    if tick.bid.le(order.price) or self._is_marginal_sell_stop_fill(order.price, tick):
                        del self._working_orders[order.cl_ord_id]  # Remove order from working orders
                        if self.fill_model.is_slipped():
                            self._fill_order(
                                order,
                                order.price.sub(self._slippages[order.symbol]),
                                LiquiditySide.TAKER,
                            )
                        else:
                            self._fill_order(
                                order,
                                order.price,
                                LiquiditySide.TAKER,
                            )
                        continue  # Continue loop to next order
                elif order.type == OrderType.LIMIT:
                    if tick.bid.ge(order.price) or self._is_marginal_sell_limit_fill(order.price, tick):
                        del self._working_orders[order.cl_ord_id]  # Remove order from working orders
                        self._fill_order(
                            order,
                            order.price,
                            LiquiditySide.MAKER,
                        )
                        continue  # Continue loop to next order

            # Check for order expiry
            if order.expire_time is not None and time_now >= order.expire_time:
                if order.cl_ord_id in self._working_orders:  # Order may have been removed since loop started
                    del self._working_orders[order.cl_ord_id]
                    self._expire_order(order)

    cpdef void adjust_account(self, OrderFillEvent event, Position position) except *:
        Condition.not_none(event, "event")

        cdef Instrument instrument = self.instruments[event.symbol]
        cdef double exchange_rate = self.exchange_calculator.get_rate(
            from_currency=instrument.quote_currency,
            to_currency=self._account.currency,
            price_type=PriceType.BID if event.order_side is OrderSide.SELL else PriceType.ASK,
            bid_rates=self._build_current_bid_rates(),
            ask_rates=self._build_current_ask_rates(),
        )

        cdef MarketPosition direction
        cdef Money pnl = Money(0, self.account_currency)
        if position is not None and position.entry_direction != event.order_side:
            if position.entry_direction == OrderSide.BUY:
                direction = MarketPosition.LONG
            elif position.entry_direction == OrderSide.SELL:
                direction = MarketPosition.SHORT
            else:
                raise RuntimeError(f"Invalid entry direction")

            pnl = self.calculate_pnl(
                direction=direction,
                open_price=position.average_open_price,
                close_price=event.average_price.as_double(),
                quantity=event.filled_quantity,
                exchange_rate=exchange_rate,
            )

        self.total_commissions = self.total_commissions.sub(event.commission)
        pnl = pnl.sub(event.commission)

        cdef AccountState account_event
        if not self.frozen_account:
            self.account_capital = self.account_capital.add(pnl)
            self.account_cash_activity_day = self.account_cash_activity_day.add(pnl)

            account_event = AccountState(
                self._account.id,
                self._account.currency,
                self.account_capital,
                self.account_cash_start_day,
                self.account_cash_activity_day,
                margin_used_liquidation=Money(0, self.account_currency),
                margin_used_maintenance=Money(0, self.account_currency),
                margin_ratio=Decimal64(),
                margin_call_status='N',
                event_id=self._uuid_factory.generate(),
                event_timestamp=self._clock.utc_now(),
            )

            self.exec_engine.handle_event(account_event)

    cpdef Money calculate_pnl(
            self,
            MarketPosition direction,
            double open_price,
            double close_price,
            Quantity quantity,
            double exchange_rate):
        Condition.not_none(quantity, "quantity")

        cdef double difference
        if direction == MarketPosition.LONG:
            difference = close_price - open_price
        elif direction == MarketPosition.SHORT:
            difference = open_price - close_price
        else:
            raise ValueError(f"Cannot calculate the pnl of a "
                             f"{market_position_to_string(direction)} direction")

        return Money(difference * quantity.as_double() * exchange_rate, self.account_currency)

    cpdef void apply_rollover_interest(self, datetime timestamp, int iso_week_day) except *:
        Condition.not_none(timestamp, "timestamp")
        Condition.not_none(self.exec_engine, "_exec_engine")

        cdef dict open_positions = self.exec_engine.database.get_positions_open()

        cdef Instrument instrument
        cdef Currency base_currency
        cdef double interest_rate
        cdef double exchange_rate
        cdef double rollover
        cdef double rollover_cumulative = 0.0
        cdef double mid_price
        cdef dict mid_prices = {}
        cdef QuoteTick market
        for position in open_positions.values():
            instrument = self.instruments[position.symbol]
            if instrument.security_type == SecurityType.FOREX:
                mid_price = mid_prices.get(instrument.symbol, 0.0)
                if mid_price == 0.0:
                    market = self._market[instrument.symbol]
                    mid_price = (market.ask.as_double() + market.bid.as_double()) / 2.0
                    mid_prices[instrument.symbol] = mid_price
                interest_rate = self.rollover_calculator.calc_overnight_rate(
                    position.symbol,
                    timestamp)
                exchange_rate = self.exchange_calculator.get_rate(
                    from_currency=instrument.quote_currency,
                    to_currency=self._account.currency,
                    price_type=PriceType.MID,
                    bid_rates=self._build_current_bid_rates(),
                    ask_rates=self._build_current_ask_rates(),
                )
                rollover = mid_price * position.quantity.as_double() * interest_rate * exchange_rate
                # Apply any bank and broker spread markup (basis points)
                rollover_cumulative += rollover - (rollover * self.rollover_spread)

        if iso_week_day == 3:  # Book triple for Wednesdays
            rollover_cumulative = rollover_cumulative * 3.0
        elif iso_week_day == 5:  # Book triple for Fridays (holding over weekend)
            rollover_cumulative = rollover_cumulative * 3.0

        cdef Money rollover_final = Money(rollover_cumulative, self.account_currency)
        self.total_rollover = self.total_rollover.add(rollover_final)

        cdef AccountState account_event
        if not self.frozen_account:
            self.account_capital = self.account_capital.add(rollover_final)
            self.account_cash_activity_day = self.account_cash_activity_day.add(rollover_final)

            account_event = AccountState(
                self._account.id,
                self._account.currency,
                self.account_capital,
                self.account_cash_start_day,
                self.account_cash_activity_day,
                margin_used_liquidation=Money(0, self.account_currency),
                margin_used_maintenance=Money(0, self.account_currency),
                margin_ratio=Decimal64(),
                margin_call_status='N',
                event_id=self._uuid_factory.generate(),
                event_timestamp=self._clock.utc_now(),
            )

            self.exec_engine.handle_event(account_event)

# -- COMMAND EXECUTION -----------------------------------------------------------------------------

    cpdef void handle_account_inquiry(self, AccountInquiry command) except *:
        Condition.not_none(command, "command")

        # Generate event
        cdef AccountState event = AccountState(
            self._account.id,
            self._account.currency,
            self._account.cash_balance,
            self.account_cash_start_day,
            self.account_cash_activity_day,
            self._account.margin_used_liquidation,
            self._account.margin_used_maintenance,
            self._account.margin_ratio,
            self._account.margin_call_status,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(event)

    cpdef void handle_submit_order(self, SubmitOrder command) except *:
        Condition.not_none(command, "command")

        self._submit_order(command.order)
        self._process_order(command.order)

    cpdef void handle_submit_bracket_order(self, SubmitBracketOrder command) except *:
        Condition.not_none(command, "command")

        cdef list bracket_orders = [command.bracket_order.stop_loss]
        self._position_oco_orders[command.cl_pos_id] = []
        if command.bracket_order.has_take_profit:
            bracket_orders.append(command.bracket_order.take_profit)
            self._oco_orders[command.bracket_order.take_profit.cl_ord_id] = command.bracket_order.stop_loss.cl_ord_id
            self._oco_orders[command.bracket_order.stop_loss.cl_ord_id] = command.bracket_order.take_profit.cl_ord_id
            self._position_oco_orders[command.cl_pos_id].append(command.bracket_order.take_profit)

        self._child_orders[command.bracket_order.entry.cl_ord_id] = bracket_orders
        self._position_oco_orders[command.cl_pos_id].append(command.bracket_order.stop_loss)

        # Generate command
        cdef SubmitOrder submit_order = SubmitOrder(
            command.trader_id,
            command.account_id,
            command.strategy_id,
            command.cl_pos_id,
            command.bracket_order.entry,
            self._uuid_factory.generate(),
            self._clock.utc_now())

        self._submit_order(command.bracket_order.entry)
        self._submit_order(command.bracket_order.stop_loss)
        if command.bracket_order.has_take_profit:
            self._submit_order(command.bracket_order.take_profit)

        self._process_order(command.bracket_order.entry)

    cpdef void handle_cancel_order(self, CancelOrder command) except *:
        Condition.not_none(command, "command")

        if command.cl_ord_id not in self._working_orders:
            self._cancel_reject_order(command.cl_ord_id, "cancel order", "order not found")
            return  # Rejected the cancel order command

        cdef Order order = self._working_orders[command.cl_ord_id]

        # Generate event
        cdef OrderCancelled cancelled = OrderCancelled(
            command.account_id,
            order.cl_ord_id,
            OrderId(order.cl_ord_id.value.replace('O', 'B')),
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        # Remove from working orders (checked it was in dictionary above)
        del self._working_orders[command.cl_ord_id]

        self.exec_engine.handle_event(cancelled)
        self._check_oco_order(command.cl_ord_id)

    cpdef void handle_modify_order(self, ModifyOrder command) except *:
        Condition.not_none(command, "command")

        if command.cl_ord_id not in self._working_orders:
            self._cancel_reject_order(command.cl_ord_id, "modify order", "order not found")
            return  # Rejected the modify order command

        cdef Order order = self._working_orders[command.cl_ord_id]
        cdef Instrument instrument = self.instruments[order.symbol]

        if command.modified_quantity.as_double() == 0.0:
            self._cancel_reject_order(
                order,
                "modify order",
                f"modified quantity {command.modified_quantity} invalid")
            return  # Cannot modify order

        cdef QuoteTick current_market = self._market.get(order.symbol)

        # Check order price is valid and reject or fill
        if order.side == OrderSide.BUY:
            if order.type == OrderType.STOP:
                if order.price.lt(current_market.ask.add(self._min_stops[order.symbol])):
                    self._reject_order(order, f"BUY STOP order price of {order.price} is too "
                                              f"far from the market, ask={current_market.ask}")
                    return  # Invalid price
            elif order.type == OrderType.LIMIT:
                if order.price.ge(current_market.ask.sub(self._min_limits[order.symbol])):
                    if order.is_post_only:
                        self._reject_order(order, f"BUY LIMIT order price of {order.price} is too "
                                                  f"far from the market, ask={current_market.ask}")
                        return  # Invalid price
                    else:
                        self._accept_order(order)
                        self._fill_order(order, current_market.ask, LiquiditySide.TAKER)
                    return  # Filled
        elif order.side == OrderSide.SELL:
            if order.type == OrderType.STOP:
                if order.price.gt(current_market.bid.sub(self._min_stops[order.symbol])):

                    self._reject_order(order, f"SELL STOP order price of {order.price} is too "
                                              f"far from the market, bid={current_market.bid}")
                    return  # Invalid price
            elif order.type == OrderType.LIMIT:
                if order.price.le(current_market.bid.add(self._min_limits[order.symbol])):
                    if order.is_post_only:
                        self._reject_order(order, f"SELL LIMIT order price of {order.price} is too "
                                                  f"far from the market, bid={current_market.bid}")
                        return  # Invalid price
                    else:
                        self._accept_order(order)
                        self._fill_order(order, current_market.bid, LiquiditySide.TAKER)
                        return  # Filled

        # Generate event
        cdef OrderModified modified = OrderModified(
            command.account_id,
            order.cl_ord_id,
            order.id,
            command.modified_quantity,
            command.modified_price,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(modified)

# -- EVENT HANDLING --------------------------------------------------------------------------------

    cdef bint _is_marginal_buy_stop_fill(self, Price order_price, QuoteTick current_market):
        return current_market.ask.eq(order_price) and self.fill_model.is_stop_filled()

    cdef bint _is_marginal_buy_limit_fill(self, Price order_price, QuoteTick current_market):
        return current_market.ask.eq(order_price) and self.fill_model.is_limit_filled()

    cdef bint _is_marginal_sell_stop_fill(self, Price order_price, QuoteTick current_market):
        return current_market.bid.eq(order_price) and self.fill_model.is_stop_filled()

    cdef bint _is_marginal_sell_limit_fill(self, Price order_price, QuoteTick current_market):
        return current_market.bid.eq(order_price) and self.fill_model.is_limit_filled()

    cdef void _submit_order(self, Order order) except *:
        # Generate event
        cdef OrderSubmitted submitted = OrderSubmitted(
            self._account.id,
            order.cl_ord_id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(submitted)

    cdef void _accept_order(self, Order order) except *:
        # Generate event
        cdef OrderAccepted accepted = OrderAccepted(
            self._account.id,
            order.cl_ord_id,
            OrderId(order.cl_ord_id.value.replace('O', 'B')),
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(accepted)

    cdef void _reject_order(self, Order order, str reason) except *:
        if order.state() != OrderState.SUBMITTED:
            self._log.error(f"Cannot reject order, state was {order.state_as_string()}.")
            return

        # Generate event
        cdef OrderRejected rejected = OrderRejected(
            self._account.id,
            order.cl_ord_id,
            self._clock.utc_now(),
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(rejected)
        self._check_oco_order(order.cl_ord_id)
        self._clean_up_child_orders(order.cl_ord_id)

    cdef void _cancel_reject_order(
            self,
            ClientOrderId order_id,
            str response,
            str reason) except *:
        # Generate event
        cdef OrderCancelReject cancel_reject = OrderCancelReject(
            self._account.id,
            order_id,
            self._clock.utc_now(),
            response,
            reason,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(cancel_reject)

    cdef void _expire_order(self, PassiveOrder order) except *:
        # Generate event
        cdef OrderExpired expired = OrderExpired(
            self._account.id,
            order.cl_ord_id,
            order.id,
            order.expire_time,
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(expired)

        cdef ClientOrderId first_child_order_id
        cdef ClientOrderId other_oco_order_id
        if order.cl_ord_id in self._child_orders:
            # Remove any unprocessed bracket child order OCO identifiers
            first_child_order_id = self._child_orders[order.cl_ord_id][0].cl_ord_id
            if first_child_order_id in self._oco_orders:
                other_oco_order_id = self._oco_orders[first_child_order_id]
                del self._oco_orders[first_child_order_id]
                del self._oco_orders[other_oco_order_id]
        else:
            self._check_oco_order(order.cl_ord_id)
        self._clean_up_child_orders(order.cl_ord_id)

    cdef void _process_order(self, Order order) except *:
        """
        Work the given order.
        """
        Condition.not_in(order.cl_ord_id, self._working_orders, "order.id", "working_orders")

        cdef Instrument instrument = self.instruments[order.symbol]

        # Check order size is valid or reject
        if order.quantity > instrument.max_trade_size:
            self._reject_order(order, f"order quantity of {order.quantity} exceeds "
                                      f"the maximum trade size of {instrument.max_trade_size}")
            return  # Cannot accept order
        if order.quantity < instrument.min_trade_size:
            self._reject_order(order, f"order quantity of {order.quantity} is less than "
                                      f"the minimum trade size of {instrument.min_trade_size}")
            return  # Cannot accept order

        cdef QuoteTick current_market = self._market.get(order.symbol)

        # Check market exists
        if current_market is None:  # Market not initialized
            self._reject_order(order, f"no market for {order.symbol}")
            return  # Cannot accept order

        # Check if market order and accept and fill immediately
        if order.type == OrderType.MARKET:
            self._process_market_order(order, current_market)
            return  # Market order filled - nothing further to process
        elif order.type == OrderType.LIMIT:
            self._process_limit_order(order, current_market)
        else:
            self._process_passive_order(order, current_market)

    cdef void _process_market_order(self, MarketOrder order, QuoteTick current_market) except *:
        self._accept_order(order)

        if order.side == OrderSide.BUY:
            if self.fill_model.is_slipped():
                self._fill_order(
                    order,
                    current_market.ask.add(self._slippages[order.symbol]),
                    LiquiditySide.TAKER)
            else:
                self._fill_order(order, current_market.ask, LiquiditySide.TAKER)
        elif order.side == OrderSide.SELL:
            if self.fill_model.is_slipped():
                self._fill_order(
                    order,
                    current_market.bid.sub(self._slippages[order.symbol]),
                    LiquiditySide.TAKER)
            else:
                self._fill_order(order, current_market.bid, LiquiditySide.TAKER)
        else:
            raise RuntimeError(f"Invalid order side, was {order_side_to_string(order.side)}")

    cdef void _process_limit_order(self, LimitOrder order, QuoteTick current_market) except *:
        if order.side == OrderSide.BUY:
            if order.price.ge(current_market.ask.sub(self._min_limits[order.symbol])):
                if order.is_post_only:
                    self._reject_order(order, f"BUY LIMIT order price of {order.price} is too "
                                              f"far from the market, ask={current_market.ask}")
                    return  # Invalid price
            elif order.price.ge(current_market.ask):
                self._accept_order(order)
                self._fill_order(order, current_market.bid, LiquiditySide.TAKER)
                return  # Filled
        elif order.side == OrderSide.SELL:
            if order.price.le(current_market.bid.add(self._min_limits[order.symbol])):
                if order.is_post_only:
                    self._reject_order(order, f"SELL LIMIT order price of {order.price} is too "
                                              f"far from the market, bid={current_market.bid}")
                    return  # Invalid price
            elif order.price.le(current_market.bid):
                self._accept_order(order)
                self._fill_order(order, current_market.bid, LiquiditySide.TAKER)
                return  # Filled

        # Order is valid and accepted
        self._accept_order(order)
        self._work_order(order)

    cdef void _process_passive_order(self, PassiveOrder order, QuoteTick current_market) except *:
        if order.side == OrderSide.BUY:
            if order.price.lt(current_market.ask.add(self._min_stops[order.symbol])):
                self._reject_order(order, f"BUY STOP order price of {order.price} is too "
                                          f"far from the market, ask={current_market.ask}")
                return  # Invalid price
        elif order.side == OrderSide.SELL:
            if order.price.gt(current_market.bid.sub(self._min_stops[order.symbol])):
                self._reject_order(order, f"SELL STOP order price of {order.price} is too "
                                          f"far from the market, bid={current_market.bid}")
                return  # Invalid price

        # Order is valid and accepted
        self._accept_order(order)
        self._work_order(order)

    cdef void _work_order(self, Order order) except *:
        # Order now becomes working
        self._working_orders[order.cl_ord_id] = order

        # Generate event
        cdef OrderWorking working = OrderWorking(
            self._account.id,
            order.cl_ord_id,
            OrderId(order.cl_ord_id.value.replace('O', 'B')),
            order.symbol,
            order.side,
            order.type,
            order.quantity,
            order.price,
            order.time_in_force,
            order.expire_time,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(working)

    cdef Money _calculate_commission(self, Order order, Price fill_price, LiquiditySide liquidity_side):
        cdef Instrument instrument = self.instruments[order.symbol]
        cdef double exchange_rate = self.exchange_calculator.get_rate(
            from_currency=instrument.quote_currency,
            to_currency=self._account.currency,
            price_type=PriceType.BID if order.side is OrderSide.SELL else PriceType.ASK,
            bid_rates=self._build_current_bid_rates(),
            ask_rates=self._build_current_ask_rates(),
        )

        cdef Money commission = self.commission_model.calculate(
            symbol=order.symbol,
            filled_quantity=order.quantity,
            filled_price=fill_price,
            exchange_rate=exchange_rate,
            currency=self.account_currency,
            liquidity_side=liquidity_side
        )

        return commission

    cdef void _fill_order(
            self,
            Order order,
            Price fill_price,
            LiquiditySide liquidity_side) except *:
        # Generate event
        cdef Money commission = self._calculate_commission(order, fill_price, liquidity_side)
        cdef OrderFilled filled = OrderFilled(
            self._account.id,
            order.cl_ord_id,
            OrderId(order.cl_ord_id.value.replace('O', 'B')),
            ExecutionId("E-" + order.cl_ord_id.value),
            PositionId("ET-" + order.cl_ord_id.value),
            order.symbol,
            order.side,
            order.quantity,
            fill_price,
            commission,
            liquidity_side,
            self.instruments[order.symbol].quote_currency,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        cdef Position position = self.exec_engine.database.get_position_for_order(order.cl_ord_id)
        self.adjust_account(filled, position)

        self.exec_engine.handle_event(filled)
        self._check_oco_order(order.cl_ord_id)

        # Work any bracket child orders
        if order.cl_ord_id in self._child_orders:
            for child_order in self._child_orders[order.cl_ord_id]:
                if not child_order.is_completed():  # The order may already be cancelled or rejected
                    self._process_order(child_order)
            del self._child_orders[order.cl_ord_id]

        if position is not None and position.is_closed():
            oco_orders = self._position_oco_orders.get(position.cl_pos_id)
            if oco_orders is not None:
                for order in self._position_oco_orders[position.cl_pos_id]:
                    if order.is_working():
                        self._cancel_order(order)
                del self._position_oco_orders[position.cl_pos_id]

    cdef void _clean_up_child_orders(self, ClientOrderId order_id) except *:
        # Clean up any residual child orders from the completed order associated
        # with the given identifier.
        if order_id in self._child_orders:
            del self._child_orders[order_id]

    cdef void _check_oco_order(self, ClientOrderId order_id) except *:
        # Check held OCO orders and remove any paired with the given order_id
        cdef ClientOrderId oco_order_id
        cdef Order oco_order

        if order_id in self._oco_orders:
            oco_order_id = self._oco_orders[order_id]
            oco_order = self.exec_engine.database.get_order(oco_order_id)
            del self._oco_orders[order_id]
            del self._oco_orders[oco_order_id]

            # Reject any latent bracket child orders
            for bracket_order_id, child_orders in self._child_orders.items():
                for order in child_orders:
                    if oco_order.equals(order) and order.state() != OrderState.WORKING:
                        self._reject_oco_order(order, order_id)

            # Cancel any working OCO orders
            if oco_order_id in self._working_orders:
                self._cancel_oco_order(self._working_orders[oco_order_id], order_id)
                del self._working_orders[oco_order_id]

    cdef void _reject_oco_order(self, PassiveOrder order, ClientOrderId oco_order_id) except *:
        # order is the OCO order to reject
        # oco_order_id is the other order_id for this OCO pair

        if order.state() != OrderState.WORKING:
            self._log.debug(f"Cannot reject order, state was already {order.state_as_string()}.")
            return

        # Generate event
        cdef OrderRejected event = OrderRejected(
            self._account.id,
            order.cl_ord_id,
            self._clock.utc_now(),
            f"OCO order rejected from {oco_order_id}",
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self.exec_engine.handle_event(event)

    cdef void _cancel_oco_order(self, PassiveOrder order, ClientOrderId oco_order_id) except *:
        # order is the OCO order to cancel
        # oco_order_id is the other order_id for this OCO pair
        if order.state() != OrderState.WORKING:
            self._log.debug(f"Cannot cancel order, state was already {order.state_as_string()}.")
            return

        # Generate event
        cdef OrderCancelled event = OrderCancelled(
            self._account.id,
            order.cl_ord_id,
            order.id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.debug(f"Cancelling {order.cl_ord_id} OCO order from {oco_order_id}.")
        self.exec_engine.handle_event(event)

    cdef void _cancel_order(self, PassiveOrder order) except *:
        if order.state() != OrderState.WORKING:
            self._log.debug(f"Cannot cancel order, state was already {order.state_as_string()}.")
            return

        # Generate event
        cdef OrderCancelled event = OrderCancelled(
            self._account.id,
            order.cl_ord_id,
            order.id,
            self._clock.utc_now(),
            self._uuid_factory.generate(),
            self._clock.utc_now(),
        )

        self._log.debug(f"Cancelling {order.cl_ord_id} as linked position closed.")
        self.exec_engine.handle_event(event)
