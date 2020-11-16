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

from cpython.datetime cimport datetime
from decimal import Decimal

import pandas as pd
import pytz

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport pad_string
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.trading.calculators cimport RolloverInterestCalculator


cdef class SimulationModule:
    """
    The base class for all simulation modules
    """

    def __init__(self):
        """
        Initialize a new instance of the `SimulationModule` class.
        """
        self._exchange = None  # Must be registered

    def __repr__(self) -> str:
        return f"{type(self).__name__}"

    cpdef void register_exchange(self, SimulatedExchange exchange) except *:
        """
        Register the given simulated exchange with the module.

        Parameters
        ----------
        exchange : SimulatedExchange
            The exchange to register.

        """
        Condition.not_none(exchange, "exchange")

        self._exchange = exchange

    cpdef void process(self, QuoteTick tick, datetime now) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void log_diagnostics(self, LoggerAdapter log) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


_TZ_US_EAST = pytz.timezone("US/Eastern")

cdef class FXRolloverInterestModule(SimulationModule):
    """
    Provides an FX rollover interest simulation module.
    """

    def __init__(self, rate_data not None: pd.DataFrame):
        """
        Initialize a new instance of the `FXRolloverInterestModule` class.

        Parameters
        ----------
        rate_data : pd.DataFrame
            The interest rate data for the internal rollover interest calculator.

        """
        super().__init__()
        self._calculator = RolloverInterestCalculator(data=rate_data)
        self._rollover_spread = Decimal()  # Bank + Broker spread markup
        self._rollover_time = None  # Initialized at first rollover
        self._rollover_applied = False
        self._rollover_total = None
        self._day_number = 0

    cpdef void process(self, QuoteTick tick, datetime now) except *:
        """
        Process the given tick through the module.

        Parameters
        ----------
        tick : QuoteTick
            The quote tick to process.
        now : datetime
            The current time in the simulated exchange.

        """
        Condition.not_none(tick, "tick")
        Condition.not_none(now, "now")

        cdef datetime rollover_local
        if self._day_number != now.day:
            # Set account statistics for new day
            self._day_number = now.day
            self._exchange.account_start_day = self._exchange.account.balance()
            self._exchange.account_activity_day = Money(0, self._exchange.account_currency)
            self._rollover_applied = False

            rollover_local = now.astimezone(_TZ_US_EAST)
            self._rollover_time = _TZ_US_EAST.localize(datetime(
                rollover_local.year,
                rollover_local.month,
                rollover_local.day,
                17),
            ).astimezone(pytz.utc)

        # Check for and apply any rollover interest
        if not self._rollover_applied and now >= self._rollover_time:
            self._apply_rollover_interest(now, self._rollover_time.isoweekday())
            self._rollover_applied = True

    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day) except *:
        cdef list open_positions = self._exchange.exec_cache.positions_open()

        rollover_cumulative = Decimal()

        cdef Position position
        cdef Instrument instrument
        cdef Price bid
        cdef Price ask
        cdef dict mid_prices = {}  # type: {Symbol, Decimal}
        for position in open_positions:
            instrument = self._exchange.instruments[position.symbol]
            if instrument.asset_class != AssetClass.FX:
                continue  # Only applicable to FX

            mid: Decimal = mid_prices.get(instrument.symbol)
            if mid is None:
                bid = self._exchange.get_current_bid(instrument.symbol)
                ask = self._exchange.get_current_ask(instrument.symbol)
                if bid is None or ask is None:
                    raise RuntimeError("Cannot apply rollover interest, no market prices")
                mid: Decimal = (bid + ask) / 2
                mid_prices[instrument.symbol] = mid
            interest_rate = self._calculator.calc_overnight_rate(
                position.symbol,
                timestamp,
            )

            xrate = self._exchange.get_xrate(
                from_currency=instrument.quote_currency,
                to_currency=self._exchange.account.currency,
                price_type=PriceType.MID,
            )

            rollover = mid * position.quantity * interest_rate * xrate
            # Apply any bank and broker spread markup (basis points)
            rollover_cumulative += rollover - (rollover * self._rollover_spread)

        if iso_week_day == 3:  # Book triple for Wednesdays
            rollover_cumulative = rollover_cumulative * 3
        elif iso_week_day == 5:  # Book triple for Fridays (holding over weekend)
            rollover_cumulative = rollover_cumulative * 3

        cdef Money rollover_final = Money(rollover_cumulative, self._exchange.account_currency)
        if self._rollover_total is None:
            self._rollover_total = Money(rollover_final, self._exchange.account_currency)
        else:
            self._rollover_total = Money(self._rollover_total + rollover_final, self._exchange.account_currency)

        self._exchange.adjust_account(rollover_final)

    cpdef void log_diagnostics(self, LoggerAdapter log) except *:
        """
        Log diagnostics out to the `BacktestEngine` logger.

        Parameters
        ----------
        log : LoggerAdapter
            The logger to log to.

        """
        account_balance_starting = self._exchange.starting_capital.to_string()
        account_starting_length = len(account_balance_starting)
        rollover_total = self._rollover_total.to_string() if self._rollover_total is not None else "0"
        rollover_interest = pad_string(rollover_total, account_starting_length)
        log.info(f"Rollover interest (total):  {rollover_interest}")

    cpdef void reset(self) except *:
        self._rollover_spread = Decimal()  # Bank + Broker spread markup
        self._rollover_time = None  # Initialized at first rollover
        self._rollover_applied = False
        self._rollover_total = None
        self._day_number = 0
