# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.stdint cimport int64_t

from decimal import Decimal

import pandas as pd
import pytz

from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport nanos_to_unix_dt
from nautilus_trader.core.functions cimport pad_string
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.calculators cimport RolloverInterestCalculator


cdef class SimulationModule:
    """
    The abstract base class for all simulation modules.

    This class should not be used directly, but through a concrete subclass.
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

    cpdef void process(self, int64_t now_ns) except *:
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
        self._rollover_time = None  # Initialized at first rollover
        self._rollover_applied = False
        self._rollover_totals = {}
        self._day_number = 0

    cpdef void process(self, int64_t now_ns) except *:
        """
        Process the given tick through the module.

        Parameters
        ----------
        now_ns : int64
            The current time in the simulated exchange.

        """
        cdef datetime now = nanos_to_unix_dt(nanos=now_ns)
        cdef datetime rollover_local
        if self._day_number != now.day:
            # Set account statistics for new day
            self._day_number = now.day
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

        cdef Position position
        cdef Instrument instrument
        cdef OrderBook book
        cdef dict mid_prices = {}  # type: dict[InstrumentId, Decimal]
        cdef Currency currency
        for position in open_positions:
            instrument = self._exchange.instruments[position.instrument_id]
            if instrument.asset_class != AssetClass.FX:
                continue  # Only applicable to FX

            mid: Decimal = mid_prices.get(instrument.id)
            if mid is None:
                book = self._exchange.get_book(instrument.id)
                mid = book.midpoint()
                if mid is None:
                    mid = book.best_bid_price()
                if mid is None:
                    mid = book.best_ask_price()
                if mid is None:
                    raise RuntimeError("Cannot apply rollover interest, no market prices")
                mid_prices[instrument.id] = Price(mid, precision=instrument.price_precision)

            interest_rate = self._calculator.calc_overnight_rate(
                position.instrument_id,
                timestamp,
            )

            rollover: Decimal = position.quantity * mid_prices[instrument.id] * interest_rate

            if iso_week_day == 3:  # Book triple for Wednesdays
                rollover *= 3
            elif iso_week_day == 5:  # Book triple for Fridays (holding over weekend)
                rollover *= 3

            if self._exchange.default_currency is not None:
                currency = self._exchange.default_currency
                xrate = self._exchange.get_xrate(
                    from_currency=instrument.settlement_currency,
                    to_currency=currency,
                    price_type=PriceType.MID,
                )
                rollover *= xrate
            else:
                currency = instrument.settlement_currency

            rollover_total = self._rollover_totals.get(currency, Decimal())
            rollover_total = Money(rollover_total + rollover, currency)
            self._rollover_totals[currency] = rollover_total

            self._exchange.adjust_account(Money(-rollover, currency))

    cpdef void log_diagnostics(self, LoggerAdapter log) except *:
        """
        Log diagnostics out to the `BacktestEngine` logger.

        Parameters
        ----------
        log : LoggerAdapter
            The logger to log to.

        """
        account_balances_starting = ', '.join([b.to_str() for b in self._exchange.starting_balances])
        account_starting_length = len(account_balances_starting)
        rollover_totals = ', '.join([b.to_str() for b in self._rollover_totals.values()])
        rollover_interest = pad_string(rollover_totals, account_starting_length)
        log.info(f"Rollover interest (totals):  {rollover_interest}")

    cpdef void reset(self) except *:
        self._rollover_time = None  # Initialized at first rollover
        self._rollover_applied = False
        self._rollover_totals = {}
        self._day_number = 0
