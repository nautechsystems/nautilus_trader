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

import msgspec
import pandas as pd
import pytz

from nautilus_trader.backtest.config import FXRolloverInterestConfig
from nautilus_trader.backtest.config import SimulationModuleConfig
from nautilus_trader.common.config import ActorConfig

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.calculators cimport RolloverInterestCalculator
from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position


cdef class SimulationModule(Actor):
    """
    The base class for all simulation modules.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: SimulationModuleConfig):
        super().__init__(config)
        self.exchange = None  # Must be registered

    def __repr__(self) -> str:
        return f"{type(self).__name__}"

    cpdef void register_venue(self, SimulatedExchange exchange):
        """
        Register the given simulated exchange with the module.

        Parameters
        ----------
        exchange : SimulatedExchange
            The exchange to register.

        """
        Condition.not_none(exchange, "exchange")

        self.exchange = exchange

    cpdef void pre_process(self, Data data):
        """Abstract method `pre_process` (implement in subclass)."""
        pass

    cpdef void process(self, uint64_t ts_now):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `process` must be implemented in the subclass")  # pragma: no cover

    cpdef void log_diagnostics(self, Logger logger):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `log_diagnostics` must be implemented in the subclass")  # pragma: no cover

    cpdef void reset(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `reset` must be implemented in the subclass")  # pragma: no cover


_TZ_US_EAST = pytz.timezone("US/Eastern")


cdef class FXRolloverInterestModule(SimulationModule):
    """
    Provides an FX rollover interest simulation module.

    Parameters
    ----------
    config  : FXRolloverInterestConfig
    """

    def __init__(self, config: FXRolloverInterestConfig):
        super().__init__(config)

        rate_data = config.rate_data
        if not isinstance(rate_data, pd.DataFrame):
            rate_data = pd.read_json(msgspec.json.decode(rate_data))

        self._calculator = RolloverInterestCalculator(data=config.rate_data)
        self._rollover_time = None  # Initialized at first rollover
        self._rollover_applied = False
        self._rollover_totals = {}
        self._day_number = 0

    cpdef void process(self, uint64_t ts_now):
        """
        Process the given tick through the module.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds) in the simulated exchange.

        """
        cdef datetime now = pd.Timestamp(ts_now, tz="UTC")
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

    cdef void _apply_rollover_interest(self, datetime timestamp, int iso_week_day):
        cdef list open_positions = self.exchange.cache.positions_open()

        cdef Position position
        cdef Instrument instrument
        cdef OrderBook book
        cdef dict mid_prices = {}  # type: dict[InstrumentId, float]
        cdef Currency currency
        cdef double mid
        cdef double rollover
        cdef double xrate
        cdef Money rollover_total
        for position in open_positions:
            instrument = self.exchange.instruments[position.instrument_id]
            if instrument.asset_class != AssetClass.FX:
                continue  # Only applicable to FX

            mid = mid_prices.get(instrument.id, 0.0)
            if mid == 0.0:
                book = self.exchange.get_book(instrument.id)
                mid = book.midpoint()
                if mid is None:
                    mid = book.best_bid_price()
                if mid is None:
                    mid = book.best_ask_price()
                if mid is None:  # pragma: no cover
                    raise RuntimeError("cannot apply rollover interest, no market prices")
                mid_prices[instrument.id] = Price(float(mid), precision=instrument.price_precision)

            interest_rate = self._calculator.calc_overnight_rate(
                position.instrument_id,
                timestamp,
            )

            rollover = position.quantity.as_f64_c() * mid_prices[instrument.id] * float(interest_rate)

            if iso_week_day == 3:  # Book triple for Wednesdays
                rollover *= 3
            elif iso_week_day == 5:  # Book triple for Fridays (holding over weekend)
                rollover *= 3

            if self.exchange.base_currency is not None:
                currency = self.exchange.base_currency
                xrate = self.exchange.cache.get_xrate(
                    venue=instrument.id.venue,
                    from_currency=instrument.quote_currency,
                    to_currency=currency,
                    price_type=PriceType.MID,
                ) or 0.0  # Retain original behavior of returning zero for now
                rollover *= xrate
            else:
                currency = instrument.quote_currency

            rollover_total = Money(self._rollover_totals.get(currency, 0.0) + rollover, currency)
            self._rollover_totals[currency] = rollover_total

            self.exchange.adjust_account(Money(-rollover, currency))

    cpdef void log_diagnostics(self, Logger logger):
        """
        Log diagnostics out to the `BacktestEngine` logger.

        Parameters
        ----------
        logger : Logger
            The logger to log to.

        """
        account_balances_starting = ', '.join([b.to_formatted_str() for b in self.exchange.starting_balances])
        account_starting_length = len(account_balances_starting)
        rollover_totals = ', '.join([b.to_formatted_str() for b in self._rollover_totals.values()])
        logger.info(f"Rollover interest (totals): {rollover_totals}")

    cpdef void reset(self):
        self._rollover_time = None  # Initialized at first rollover
        self._rollover_applied = False
        self._rollover_totals = {}
        self._day_number = 0
