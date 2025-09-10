# -------------------------------------------------------------------------------------------------
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
"""
Option exercise simulation module for backtesting.

This module provides automatic option exercise functionality during backtesting,
handling both cash-settled and physically-settled options with unified exercise logic.

## Settlement Types

- **Cash Settlement (Index Options)**: Options are closed at intrinsic value with no underlying position created
- **Physical Settlement (Equity Options)**: Options are closed and underlying positions are created at strike price

## Exercise Logic

For both long and short positions:
1. Check if option is in-the-money (ITM) using strict price comparison
2. If ITM, exercise the option using appropriate settlement method
3. If not exercised, option expires worthless (closed at zero value)

"""


from nautilus_trader.backtest.config import SimulationModuleConfig
from nautilus_trader.common.events import TimeEvent

from libc.stdint cimport uint64_t

from nautilus_trader.backtest.engine cimport SimulatedExchange
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport LiquiditySide
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.position cimport PositionClosed
from nautilus_trader.model.events.position cimport PositionOpened
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_option cimport CryptoOption
from nautilus_trader.model.instruments.index cimport IndexInstrument
from nautilus_trader.model.instruments.option_contract cimport OptionContract
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


class OptionExerciseConfig(SimulationModuleConfig, frozen=True):
    auto_exercise_enabled: bool = True


cdef class OptionExerciseModule(SimulationModule):
    def __init__(self, config: OptionExerciseConfig) -> None:
        super().__init__(config)

        self.config = config
        self.cache = None
        self.expiry_timers = {}  # expiry_ns -> timer_name
        self.processed_expiries = set()  # Track processed expiry timestamps

    cpdef void reset(self):
        """Reset the module state."""
        self.expiry_timers.clear()
        self.processed_expiries.clear()

    cpdef void log_diagnostics(self, Logger logger):
        """
        Log diagnostic information about the module state.
        """
        logger.info(f"OptionExerciseModule: {len(self.expiry_timers)} expiry timers set")
        logger.info(f"OptionExerciseModule: {len(self.processed_expiries)} expiries processed")

    cpdef void register_venue(self, SimulatedExchange exchange):
        """
        Register the exchange and subscribe to position events.
        """
        SimulationModule.register_venue(self, exchange)

        # Subscribe to position events to detect new option positions
        if self._msgbus:
            # Subscribe to all position events
            self._msgbus.subscribe(
                topic="events.position.*",
                handler=self.on_position_event,
            )

    cpdef void pre_process(self, Data data):
        """
        Pre-process method - not needed for option exercise.
        """
        pass

    cpdef void process(self, uint64_t ts_now):
        """
        Process method called by backtesting engine.
        """
        pass

    def on_position_event(self, event) -> None:
        """
        Handle position events to set up exercise timers for new option positions.
        """
        if not self.config.auto_exercise_enabled or not self.exchange:
            return

        # Check if this is an option position
        instrument = self.exchange.cache.instrument(event.instrument_id)

        if not isinstance(instrument, (OptionContract, CryptoOption)):
            return

        expiry_ns = instrument.expiration_ns

        # Handle different position event types
        if isinstance(event, PositionOpened):
            # Set up timer for option expiry if not already set
            if expiry_ns not in self.expiry_timers:
                timer_name = f"option_expiry_{expiry_ns}"
                self.clock.set_time_alert_ns(
                    name=timer_name,
                    alert_time_ns=expiry_ns,
                    callback=self._on_expiry_timer,
                )
                self.expiry_timers[expiry_ns] = timer_name
                self._log.debug(f"Set expiry timer for {instrument.id} at {expiry_ns}")
        elif isinstance(event, PositionClosed):
            # Check if there are any remaining positions for this expiry
            self._cleanup_timer_if_no_positions(expiry_ns)

    def _cleanup_timer_if_no_positions(self, expiry_ns: int) -> None:
        """
        Clean up expiry timer if no option positions remain for the given expiry.
        """
        if expiry_ns not in self.expiry_timers:
            return

        # Check if any option positions exist for this expiry
        has_positions = False

        if self.exchange and self.exchange.cache:
            positions = self.exchange.cache.positions_open()

            for position in positions:
                instrument = self.exchange.cache.instrument(position.instrument_id)

                if (
                        isinstance(instrument, (OptionContract, CryptoOption))
                        and instrument.expiration_ns == expiry_ns
                ):
                    has_positions = True
                    break

        # If no positions remain for this expiry, cancel the timer
        if not has_positions:
            timer_name = self.expiry_timers[expiry_ns]

            # Cancel the timer (if the clock supports it)
            self.clock.cancel_timer(timer_name)

            # Remove from our tracking
            del self.expiry_timers[expiry_ns]
            self._log.debug(f"Cleaned up expiry timer for {expiry_ns} - no positions remaining")

    def _on_expiry_timer(self, event: TimeEvent) -> None:
        """
        Handle timer events for option expiry.
        """
        if not self.config.auto_exercise_enabled or not self.exchange:
            return

        expiry_ns = event.ts_event

        # Skip if already processed
        if expiry_ns in self.processed_expiries:
            return

        # Process all options expiring at this timestamp
        self._process_expiring_options(expiry_ns)
        self.processed_expiries.add(expiry_ns)

    def _process_expiring_options(self, ts_now: int) -> None:
        """
        Process options expiring at the current timestamp.
        """
        if not self.exchange or not self.exchange.cache:
            return

        # Find options expiring at this timestamp
        expiring_options = []

        for instrument in self.exchange.cache.instruments():
            if isinstance(instrument, (OptionContract, CryptoOption)):
                if instrument.expiration_ns == ts_now:
                    expiring_options.append(instrument)

        if not expiring_options:
            return

        # Process each expiring option
        for option in expiring_options:
            self._process_option_expiry(option, ts_now)

    def _process_option_expiry(self, option: OptionContract | CryptoOption, ts_now: int) -> None:
        """
        Process the expiry of a single option.
        """
        if not self.cache:
            return

        # Get option positions
        positions = self.cache.positions_open(venue=None, instrument_id=option.id)

        if not positions:
            return

        # Get underlying price for exercise decision
        underlying_price = self._get_underlying_price(option)

        if underlying_price is None:
            self._log.debug(f"Skipping exercise of {option.id}: no underlying price available")
            return

        # Process each position (both long and short)
        for position in positions:
            # Check if option should be exercised (applies to both long and short positions)
            if self._should_exercise(option, underlying_price):
                self._exercise(option, position, underlying_price, ts_now)
            # If not exercised, option expires worthless and position is closed at zero value
            else:
                self._log.debug(
                    f"Expiring OTM {option.id}: {position.side} {position.quantity} @ strike {option.strike_price} "
                    f"(expires worthless)",
                )
                self._generate_otm_expiry_events(option, position, ts_now)

    def _get_underlying_price(self, option: OptionContract | CryptoOption) -> Price | None:
        """
        Get the current underlying price for exercise evaluation.
        """
        # Find underlying instrument
        underlying_instrument = self._get_underlying_instrument(option)

        if underlying_instrument is None:
            return None

        return self.cache.price(underlying_instrument.id, PriceType.LAST)

    def _should_exercise(self, option: OptionContract | CryptoOption, underlying_price: Price) -> bool:
        """
        Determine if option should be exercised based on strict price comparison.

        An option is exercised if the underlying price is strictly greater than (calls)
        or strictly less than (puts) the strike price.
        """
        strike = option.strike_price.as_double()
        spot = underlying_price.as_double()

        if option.option_kind == OptionKind.CALL:
            is_itm = spot > strike
        else:  # PUT
            is_itm = strike > spot

        if not is_itm:
            self._log.debug(
                f"Skipping exercise of {option.id}: OTM "
                f"(underlying: {spot}, strike: {strike})",
            )
            return False

        return True

    def _is_option_itm(
        self,
        option: OptionContract | CryptoOption,
        underlying_price: Price,
    ) -> tuple[bool, float]:
        """
        Check if option is in-the-money and calculate intrinsic value.
        """
        strike = option.strike_price.as_double()
        spot = underlying_price.as_double()

        if option.option_kind == OptionKind.CALL:
            intrinsic_value = max(0.0, spot - strike)
            is_itm = spot > strike
        else:  # PUT
            intrinsic_value = max(0.0, strike - spot)
            is_itm = strike > spot

        return is_itm, intrinsic_value

    def _generate_otm_expiry_events(
        self,
        option: OptionContract | CryptoOption,
        position: Position,
        ts_now: int,
    ) -> None:
        """Generate OrderFilled events for OTM option expiry (expires worthless)."""
        # Close option position at zero value since it expires worthless
        option_close_fill = self._create_option_fill(
            option, position, f"OTM-EXPIRY-{ts_now}",
            f"OTM-EXPIRY-{ts_now}", ts_now, False  # use_avg_price=False for zero value
        )
        self._send_events([option_close_fill])
        self._log.debug(f"OTM expiry complete: Closed {option.id} position @ {option_close_fill.last_px} (worthless)")

    def _exercise(
        self,
        option: OptionContract | CryptoOption,
        position: Position,
        underlying_price: Price,
        ts_now: int,
    ) -> None:
        """
        Process option exercise with unified logic for both long and short positions.

        This method handles all option exercise scenarios using a single, unified approach.
        There is no distinction between "exercise" (long positions) and "assignment"
        (short positions) - both are processed identically as option exercise.

        Settlement type is automatically determined based on underlying instrument:
        - IndexInstrument: Cash settlement (close option at intrinsic value)
        - Other instruments: Physical settlement (close option + create underlying position)
        """
        underlying_instrument = self._get_underlying_instrument(option)
        is_cash_settled = isinstance(underlying_instrument, IndexInstrument)
        settlement_type = "cash" if is_cash_settled else "physical"
        self._log.debug(
            f"Exercising {option.id}: {position.side} {position.quantity} @ strike {option.strike_price} "
            f"(underlying: {underlying_price}, settlement: {settlement_type})",
        )

        if is_cash_settled:
            # Cash settlement: close option at intrinsic value
            self._generate_cash_settlement_events(option, position, underlying_price, ts_now)
        else:
            # Physical settlement: create underlying position
            underlying_quantity, underlying_side = self._calculate_underlying_position(option, position)

            if underlying_instrument is None:
                self._log.error(f"Cannot exercise {option.id}: underlying instrument not found")
                return

            self._generate_physical_settlement_events(
                option, position, underlying_instrument, underlying_quantity,
                underlying_side, underlying_price, ts_now
            )

    def _calculate_underlying_position(
        self,
        option: OptionContract | CryptoOption,
        position,
    ) -> tuple[Quantity, PositionSide]:
        """
        Calculate the underlying position quantity and side from option exercise.
        """
        # Base quantity from option multiplier and position size
        base_quantity = position.quantity.as_double() * option.multiplier.as_double()

        # Determine side based on option type and position side
        if option.option_kind == OptionKind.CALL:
            # Call exercise: long option -> long underlying, short option -> short underlying
            underlying_side = position.side
        else:  # PUT
            # Put exercise: long option -> short underlying, short option -> long underlying
            underlying_side = (
                PositionSide.SHORT if position.side == PositionSide.LONG else PositionSide.LONG
            )

        # Create quantity with appropriate precision
        underlying_quantity = Quantity.from_str(str(base_quantity))

        return underlying_quantity, underlying_side

    def _generate_cash_settlement_events(
        self,
        option: OptionContract | CryptoOption,
        position: Position,
        underlying_price: Price,
        ts_now: int,
    ) -> None:
        """
        Generate OrderFilled events for cash settlement (close option at intrinsic value).

        For cash-settled options (typically index options), the option position is closed
        at the intrinsic value with no underlying position created. The cash payment
        represents the profit/loss from the option's intrinsic value.
        """
        settlement_id = UUID4()

        # Calculate intrinsic value for cash settlement
        intrinsic_value = self._calculate_settlement_price(option, underlying_price)

        # Close option position at intrinsic value
        option_close_fill = self._create_cash_settlement_fill(
            option, position, intrinsic_value, f"CASH_SETTLE_{settlement_id}",
            f"CASH_SETTLE_{settlement_id}", ts_now
        )
        self._send_events([option_close_fill])
        self._log.debug(
            f"Cash settlement complete: exercised {option.id} position @ {intrinsic_value} "
            f"(intrinsic value from underlying: {underlying_price})"
        )

    def _create_cash_settlement_fill(self, option, position, settlement_price: Price,
                                     trade_id_suffix: str, venue_id_suffix: str, ts_now: int) -> OrderFilled:
        """Create OrderFilled event for cash settlement (closing option at intrinsic value)."""
        # Determine the order side to close the position
        close_side = OrderSide.SELL if position.side == PositionSide.LONG else OrderSide.BUY

        return OrderFilled(
            trader_id=position.trader_id,
            strategy_id=position.strategy_id,
            instrument_id=option.id,
            client_order_id=ClientOrderId(trade_id_suffix),
            venue_order_id=VenueOrderId(venue_id_suffix),
            account_id=position.account_id,
            trade_id=TradeId(trade_id_suffix),
            position_id=position.id,
            order_side=close_side,
            order_type=OrderType.MARKET,
            last_qty=position.quantity,
            last_px=settlement_price,
            currency=option.quote_currency,
            commission=Money(0, option.quote_currency),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

    def _generate_physical_settlement_events(
        self,
        option: OptionContract | CryptoOption,
        position: Position,
        underlying_instrument,
        underlying_quantity: Quantity,
        underlying_side: PositionSide,
        underlying_price: Price,
        ts_now: int,
    ) -> None:
        """
        Generate OrderFilled events for physical settlement (close option and open underlying).

        For physically-settled options (typically equity options), the option position is closed
        and a corresponding underlying position is created at the strike price. This simulates
        the actual delivery of the underlying asset.
        """
        settlement_id = UUID4()

        # Close option position
        option_close_fill = self._create_option_fill(
            option, position, f"EXERCISE_CLOSE_{settlement_id}",
            f"EXERCISE_{settlement_id}", ts_now, True
        )

        # Open underlying position
        settlement_price = self._calculate_settlement_price(option, underlying_price)
        underlying_open_fill = self._create_underlying_fill(
            position, underlying_instrument, underlying_quantity, underlying_side,
            settlement_price, f"EXERCISE_OPEN_{settlement_id}", f"EXERCISE_{settlement_id}", ts_now
        )
        self._send_events([option_close_fill, underlying_open_fill])
        self._log.debug(
            f"Physical settlement complete: exercised {option.id} position, "
            f"opened {underlying_instrument.id} {underlying_side} {underlying_quantity} @ {settlement_price}",
        )

    def _calculate_settlement_price(self, option, underlying_price: Price) -> Price:
        """
        Calculate settlement price based on option type (cash vs physical settlement).

        For cash-settled options (IndexInstrument underlying):
        - Returns the intrinsic value of the option
        - Call: max(0, underlying_price - strike_price)
        - Put: max(0, strike_price - underlying_price)

        For physically-settled options (other instruments):
        - Returns the strike price (price at which underlying is delivered)
        """
        underlying_instrument = self._get_underlying_instrument(option)
        is_cash_settled = isinstance(underlying_instrument, IndexInstrument)

        if is_cash_settled:
            # Cash settlement: use intrinsic value
            if option.option_kind == OptionKind.CALL:
                settlement_price = Price(max(0.0, underlying_price.as_double() - option.strike_price.as_double()), option.strike_price.precision)
            else:  # PUT
                settlement_price = Price(max(0.0, option.strike_price.as_double() - underlying_price.as_double()), option.strike_price.precision)

            self._log.debug(
                f"Cash settlement for {option.id}: intrinsic value {settlement_price} "
                f"(underlying: {underlying_price}, strike: {option.strike_price})"
            )
            return settlement_price
        else:
            # Physical settlement: use strike price
            self._log.debug(
                f"Physical settlement for {option.id}: strike price {option.strike_price} "
                f"(underlying: {underlying_price})"
            )
            return option.strike_price

    cpdef Instrument _get_underlying_instrument(self, object option):
        """Get the underlying instrument for the option."""
        underlying_instrument_id = InstrumentId.from_str(f"{option.underlying}.{option.id.venue}")

        return self._cache.instrument(underlying_instrument_id)

    def _create_option_fill(self, option, position, trade_id_suffix: str, venue_id_suffix: str, ts_now: int, use_avg_price: bool = True) -> OrderFilled:
        """Create OrderFilled event for option position closure."""
        if use_avg_price:
            # Use the average opening price to ensure unrealized PnL becomes 0
            price = Price(position.avg_px_open, option.price_precision)
        else:
            # For OTM expiry, option is worthless
            price = Price(0.0, option.price_precision)

        return OrderFilled(
            trader_id=position.trader_id,
            strategy_id=position.strategy_id,
            instrument_id=option.id,
            client_order_id=ClientOrderId(trade_id_suffix),
            venue_order_id=VenueOrderId(venue_id_suffix),
            account_id=position.account_id,
            trade_id=TradeId(trade_id_suffix),
            position_id=position.id,
            order_side=OrderSide.SELL if position.side == PositionSide.LONG else OrderSide.BUY,
            order_type=OrderType.MARKET,
            last_qty=position.quantity,
            last_px=price,
            currency=option.quote_currency,
            commission=Money(0, option.quote_currency),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

    def _create_underlying_fill(self, position, underlying_instrument, quantity: Quantity, side: PositionSide,
                               price: Price, trade_id_suffix: str, venue_id_suffix: str, ts_now: int) -> OrderFilled:
        """Create OrderFilled event for underlying position opening."""
        return OrderFilled(
            trader_id=position.trader_id,
            strategy_id=position.strategy_id,
            instrument_id=underlying_instrument.id,
            client_order_id=ClientOrderId(trade_id_suffix),
            venue_order_id=VenueOrderId(venue_id_suffix),
            account_id=position.account_id,
            trade_id=TradeId(trade_id_suffix),
            position_id=None,  # New underlying position will get its own ID
            order_side=OrderSide.BUY if side == PositionSide.LONG else OrderSide.SELL,
            order_type=OrderType.MARKET,
            last_qty=quantity,
            last_px=price,
            currency=underlying_instrument.quote_currency,
            commission=Money(0, underlying_instrument.quote_currency),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=ts_now,
            ts_init=ts_now,
        )

    def _send_events(self, events: list) -> None:
        """Send events to the execution engine for processing."""
        if self.exchange:
            for event in events:
                self.exchange.msgbus.send(endpoint="ExecEngine.process", msg=event)
