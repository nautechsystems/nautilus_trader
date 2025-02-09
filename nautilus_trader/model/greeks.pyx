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

from typing import Callable

from nautilus_trader.core.nautilus_pyo3 import black_scholes_greeks
from nautilus_trader.core.nautilus_pyo3 import imply_vol_and_greeks
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.greeks_data import GreeksData
from nautilus_trader.model.greeks_data import PortfolioGreeks

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.position cimport Position


cdef class GreeksCalculator:
    """
    Class used to calculate instrument and portfolio greeks (sensitivities of price moves with respect to market data moves).

    Useful for risk management of options and futures portfolios.
    Accessible from any class inheriting from the actor class including strategies.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the calculator.
    cache : CacheFacade
        The cache for the calculator.
    clock : LiveClock
        The clock for the calculator.
    logger : Logger
        The logger for logging messages.

    Notes
    ----------
    Currently implemented greeks are:
    - Delta (first derivative of price with respect to spot move)
    - Gamma (second derivative of price with respect to spot move)
    - Vega (first derivative of price with respect to implied volatility of an option)
    - Theta (first derivative of price with respect to time to expiry).

    Vega is expressed in terms of absolute percent changes ((dV / dVol) / 100).
    Theta is expressed in terms of daily changes ((dV / d(T-t)) / 365.25, where T is the expiry of an option and t is the current time).

    """

    def __init__(
        self,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        Clock clock not None,
        Logger logger not None,
    ) -> None:
        self._msgbus = msgbus
        self._cache = cache
        self._clock = clock
        self._log = logger

    def instrument_greeks(
        self,
        instrument_id: InstrumentId,
        flat_interest_rate: float = 0.05,
        spot_shock: float = 0.,
        vol_shock: float = 0.,
        expiry_in_years_shock: float = 0.,
        use_cached_greeks: bool = False,
        cache_greeks: bool = False,
        publish_greeks: bool = False,
        ts_event: int = 0,
        position: Position | None = None
    ) -> GreeksData:
        """
        Calculate option greeks for a given instrument.

        Also allows to apply shocks to spot, volatility and time to expiry.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ID of the instrument to calculate greeks for.
        flat_interest_rate : float, default 0.05
            The interest rate to use for calculations.
            The function also searches if an interest rate curve for the currency of the option is stored in cache;
            if not, flat_interest_rate is used.
        spot_shock : float, default 0.0
            Shock to apply to spot price.
        vol_shock : float, default 0.0
            Shock to apply to volatility.
        expiry_in_years_shock : float, default 0.0
            Shock to apply to time to expiry.
        use_cached_greeks : bool, default False
            Whether to use cached greeks values if available.
        cache_greeks : bool, default False
            Whether to cache the calculated greeks.
        publish_greeks : bool, default False
            Whether to publish the calculated greeks.
        ts_event : int, default 0
            Timestamp of the event triggering the calculation, by default 0.
        position : Position, optional
            Optional position used to calculate the pnl of a Future when necessary.

        Returns
        -------
        GreeksData
          The calculated option greeks data
          Contains price, delta, gamma, vega, theta as well as additional information used for the computation.

        """
        instrument_definition = self._cache.instrument(instrument_id)
        if instrument_definition.instrument_class is not InstrumentClass.OPTION:
            if instrument_definition.instrument_class is not InstrumentClass.FUTURE:
                self._log.error(f"instrument_greeks only works with futures for now.")
                return

            greeks_data = GreeksData.from_multiplier(instrument_id, float(instrument_definition.multiplier), ts_event)

            if position is not None:
                # we set as price the pnl of a unit position so we can see how the price of a portfolio evolves with shocks
                underlying_price = float(self._cache.price(instrument_definition.id, PriceType.LAST))
                greeks_data.price = float(position.unrealized_pnl(Price.from_str(str(underlying_price + spot_shock)))) / float(position.signed_qty)

            return greeks_data

        greeks_data = None

        if use_cached_greeks and (greeks_data := self._cache.greeks(instrument_id)) is not None:
            pass
        else:
            utc_now_ns = ts_event if ts_event is not None else self._clock.timestamp_ns()
            utc_now = unix_nanos_to_dt(utc_now_ns)

            expiry_utc = instrument_definition.expiration_utc
            expiry_int = int(expiry_utc.strftime("%Y%m%d"))
            expiry_in_years = min((expiry_utc - utc_now).days, 1) / 365.25

            currency = instrument_definition.quote_currency.code
            if (interest_rate_curve := self._cache.interest_rate_curve(currency)) is not None:
                interest_rate = interest_rate_curve(expiry_in_years)
            else:
                interest_rate = flat_interest_rate

            multiplier = float(instrument_definition.multiplier)
            is_call = instrument_definition.option_kind is OptionKind.CALL
            strike = float(instrument_definition.strike_price)

            option_mid_price = float(self._cache.price(instrument_id, PriceType.MID))

            underlying_instrument_id = InstrumentId.from_str(f"{instrument_definition.underlying}.{instrument_id.venue}")
            underlying_price = float(self._cache.price(underlying_instrument_id, PriceType.LAST))

            greeks = imply_vol_and_greeks(underlying_price, interest_rate, 0.0, is_call, strike, expiry_in_years, option_mid_price, multiplier)
            greeks_data = GreeksData(utc_now_ns, utc_now_ns, instrument_id, is_call, strike, expiry_int, expiry_in_years, multiplier, 1.0,
                                     underlying_price, interest_rate, greeks.vol, greeks.price, greeks.delta, greeks.gamma, greeks.vega, greeks.theta,
                                     abs(greeks.delta / multiplier))

            # adding greeks to cache
            if cache_greeks:
                self._cache.add_greeks(greeks_data)

            # publishing greeks on the message bus so they can be written to a catalog from streamed objects
            if publish_greeks:
                data_type = DataType(GreeksData, metadata={"instrument_id": instrument_id.value})
                self._msgbus.publish_c(topic=f"data.{data_type.topic}", msg=greeks_data)

        if spot_shock != 0. or vol_shock != 0. or expiry_in_years_shock != 0.:
            greeks = black_scholes_greeks(greeks_data.underlying_price + spot_shock, greeks_data.interest_rate, 0.0, greeks_data.vol + vol_shock,
                                          greeks_data.is_call, greeks_data.strike, greeks_data.expiry_in_years - expiry_in_years_shock,
                                          greeks_data.multiplier)
            greeks_data = GreeksData(greeks_data.ts_event, greeks_data.ts_event,
                                     greeks_data.instrument_id, greeks_data.is_call, greeks_data.strike, greeks_data.expiry,
                                     greeks_data.expiry_in_years - expiry_in_years_shock,
                                     greeks_data.multiplier, greeks_data.quantity, greeks_data.underlying_price + spot_shock,
                                     greeks_data.interest_rate,
                                     greeks_data.vol + vol_shock, greeks.price, greeks.delta, greeks.gamma, greeks.vega, greeks.theta,
                                     abs(greeks.delta / greeks_data.multiplier))

        return greeks_data

    def portfolio_greeks(
        self, str underlying = "",
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        PositionSide side = PositionSide.NO_POSITION_SIDE,
        flat_interest_rate: float = 0.05,
        spot_shock: float = 0.0,
        vol_shock: float = 0.0,
        expiry_in_years_shock: float = 0.0,
        use_cached_greeks: bool = False,
        cache_greeks: bool = False,
        publish_greeks: bool = False,
    ) -> PortfolioGreeks:
        """
        Calculate the portfolio Greeks for a given set of positions.

        Aggregates the Greeks data for all open positions that match the specified criteria.
        Also allows to apply shocks to spot, volatility and time to expiry.

        Parameters
        ----------
        underlying : str, default ""
            The underlying asset symbol to filter positions.
            Only positions with instruments starting with this symbol will be included.
        venue : Venue, optional
            The venue to filter positions.
            Only positions from this venue will be included.
        instrument_id : InstrumentId, optional
            The instrument ID to filter positions.
            Only positions for this instrument will be included.
        strategy_id : StrategyId, optional
            The strategy ID to filter positions.
            Only positions for this strategy will be included.
        side : PositionSide, default PositionSide.NO_POSITION_SIDE
            The position side to filter.
            Only positions with this side will be included.
        flat_interest_rate : float, default 0.05
            The flat/constant interest rate to use for calculations when no curve is available.
        spot_shock : float, default 0.0
            The shock to apply to the underlying price.
        vol_shock : float, default 0.0
            The shock to apply to the implied volatility.
        expiry_in_years_shock : float, default 0.0
            The shock to apply to the time to expiry.
        use_cached_greeks : bool, default False
            Whether to use cached Greeks calculations if available.
        cache_greeks : bool, default False
            Whether to cache the calculated Greeks.
        publish_greeks : bool, default False
            Whether to publish the Greeks data to the message bus.

        Returns
        -------
        PortfolioGreeks
            The aggregated Greeks data for the portfolio.
            Contains price, delta, gamma, vega, theta.

        Notes
        -----
        The method filters positions based on the provided parameters and calculates
        Greeks for each matching position. The Greeks are then weighted by position
        size and aggregated into portfolio-level risk metrics.

        """
        ts_event = self._clock.timestamp_ns()
        portfolio_greeks = PortfolioGreeks(ts_event, ts_event)
        open_positions = self._cache.positions_open(venue, instrument_id, strategy_id, side)

        for position in open_positions:
            position_instrument_id = position.instrument_id

            if underlying != "" and not position_instrument_id.value.startswith(underlying):
                continue

            quantity = position.signed_qty
            instrument_greeks = self.instrument_greeks(
                position_instrument_id,
                flat_interest_rate,
                spot_shock,
                vol_shock,
                expiry_in_years_shock,
                use_cached_greeks,
                cache_greeks,
                publish_greeks,
                ts_event,
                position,
            )

            position_greeks = quantity * instrument_greeks
            portfolio_greeks += position_greeks

        return portfolio_greeks

    def subscribe_greeks(self, underlying: str = "", handler: Callable[[GreeksData], None] = None) -> None:
        """
        Subscribe to Greeks data for a given underlying instrument.

        Useful for reading greeks from a backtesting data catalog and caching them for later use.

        Parameters
        ----------
        underlying : str, default ""
            The underlying instrument ID prefix to subscribe to.
            If empty, subscribes to all Greeks data.
        handler : Callable[[GreeksData], None], optional
            The callback function to handle received Greeks data.
            If None, defaults to adding greeks to the cache.

        Returns
        -------
        None

        """
        used_handler = handler or (lambda greeks: self._cache.add_greeks(greeks))
        self._msgbus.subscribe(
            topic=f"data.GreeksData.instrument_id={underlying}*",
            handler=used_handler,
        )
