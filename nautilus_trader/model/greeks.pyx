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

    Also note that for ease of implementation we consider that american options (for stock options for example) are european for the computation of greeks.

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
        flat_interest_rate: float = 0.0425,
        flat_dividend_yield: float | None = None,
        spot_shock: float = 0.,
        vol_shock: float = 0.,
        time_to_expiry_shock: float = 0.,
        use_cached_greeks: bool = False,
        cache_greeks: bool = False,
        publish_greeks: bool = False,
        ts_event: int = 0,
        position: Position | None = None,
        percent_greeks: bool = False,
        index_instrument_id: InstrumentId | None = None,
        beta_weights: dict[InstrumentId, float] | None = None,
    ) -> GreeksData:
        """
        Calculate option or underlying greeks for a given instrument and a quantity of 1.

        Additional features:
        - Apply shocks to the spot value of the instrument's underlying, implied volatility or time to expiry.
        - Compute percent greeks.
        - Compute beta-weighted delta and gamma with respect to an index.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ID of the instrument to calculate greeks for.
        flat_interest_rate : float, default 0.0425
            The interest rate to use for calculations.
            The function first searches if an interest rate curve for the currency of the option is stored in cache;
            if not, flat_interest_rate is used.
        flat_dividend_yield : float, optional
            The dividend yield to use for calculations.
            The function first searches if a dividend yield curve is stored in cache using the instrument id of the underlying as key;
            if not, flat_dividend_yield is used if it's not None.
        spot_shock : float, default 0.0
            Shock to apply to spot price.
        vol_shock : float, default 0.0
            Shock to apply to implied volatility.
        time_to_expiry_shock : float, default 0.0
            Shock in years to apply to time to expiry.
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
        percent_greeks : bool, optional
            Whether to compute greeks as percentage of the underlying price, by default False.
        index_instrument_id : InstrumentId, optional
            The reference instrument id beta is computed with respect to.
        beta_weights : dict[InstrumentId, float], optional
            Dictionary of beta weights used to compute portfolio delta and gamma.

        Returns
        -------
        GreeksData
          The calculated option greeks data
          Contains price, delta, gamma, vega, theta as well as additional information used for the computation.

        """
        instrument = self._cache.instrument(instrument_id)

        if instrument.instrument_class is not InstrumentClass.OPTION:
            multiplier = float(instrument.multiplier)
            underlying_instrument_id = instrument.id
            underlying_price = float(self._cache.price(underlying_instrument_id, PriceType.LAST))

            delta = self.modify_greeks(multiplier, 0., underlying_instrument_id, underlying_price + spot_shock, underlying_price,
                                       percent_greeks, index_instrument_id, beta_weights)[0]
            greeks_data = GreeksData.from_delta(instrument_id, delta, multiplier, ts_event)

            if position is not None:
                greeks_data.pnl = multiplier * ((underlying_price + spot_shock) - position.avg_px_open)
                greeks_data.price = greeks_data.pnl

            return greeks_data

        greeks_data = None
        underlying_instrument_id = InstrumentId.from_str(f"{instrument.underlying}.{instrument_id.venue}")

        if use_cached_greeks and (greeks_data := self._cache.greeks(instrument_id)) is not None:
            pass
        else:
            utc_now_ns = ts_event if ts_event is not None else self._clock.timestamp_ns()
            utc_now = unix_nanos_to_dt(utc_now_ns)

            expiry_utc = instrument.expiration_utc
            expiry_int = int(expiry_utc.strftime("%Y%m%d"))
            expiry_in_years = min((expiry_utc - utc_now).days, 1) / 365.25

            currency = instrument.quote_currency.code

            if (yield_curve := self._cache.yield_curve(currency)) is not None:
                interest_rate = yield_curve(expiry_in_years)
            else:
                interest_rate = flat_interest_rate

            # cost of carry is 0 for futures
            cost_of_carry = 0.

            if (dividend_curve := self._cache.yield_curve(str(underlying_instrument_id))) is not None:
                dividend_yield = dividend_curve(expiry_in_years)
                cost_of_carry = interest_rate - dividend_yield
            elif flat_dividend_yield is not None:
                # Use a dividend rate of 0. to have a cost of carry of interest rate for options on stocks
                cost_of_carry = interest_rate - flat_dividend_yield

            multiplier = float(instrument.multiplier)
            is_call = instrument.option_kind is OptionKind.CALL
            strike = float(instrument.strike_price)

            option_mid_price = float(self._cache.price(instrument_id, PriceType.MID))
            underlying_price = float(self._cache.price(underlying_instrument_id, PriceType.LAST))

            greeks = imply_vol_and_greeks(underlying_price, interest_rate, cost_of_carry, is_call, strike, expiry_in_years, option_mid_price, multiplier)
            delta, gamma =  self.modify_greeks(greeks.delta, greeks.gamma, underlying_instrument_id, underlying_price, underlying_price,
                                               percent_greeks, index_instrument_id, beta_weights)

            greeks_data = GreeksData(utc_now_ns, utc_now_ns, instrument_id, is_call, strike, expiry_int, expiry_in_years, multiplier, 1.0,
                                     underlying_price, interest_rate, cost_of_carry, greeks.vol, 0., greeks.price, delta, gamma, greeks.vega, greeks.theta,
                                     abs(greeks.delta / multiplier))

            # adding greeks to cache
            if cache_greeks:
                self._cache.add_greeks(greeks_data)

            # publishing greeks on the message bus so they can be written to a catalog from streamed objects
            if publish_greeks:
                data_type = DataType(GreeksData, metadata={"instrument_id": instrument_id.value})
                self._msgbus.publish_c(topic=f"data.{data_type.topic}", msg=greeks_data)

        if spot_shock != 0. or vol_shock != 0. or time_to_expiry_shock != 0.:
            underlying_price = greeks_data.underlying_price
            shocked_underlying_price = underlying_price + spot_shock
            shocked_vol = greeks_data.vol + vol_shock
            shocked_time_to_expiry = greeks_data.expiry_in_years - time_to_expiry_shock

            greeks = black_scholes_greeks(shocked_underlying_price, greeks_data.interest_rate, greeks_data.cost_of_carry,
                                          shocked_vol, greeks_data.is_call, greeks_data.strike, shocked_time_to_expiry, greeks_data.multiplier)
            delta, gamma = self.modify_greeks(greeks.delta, greeks.gamma, underlying_instrument_id, shocked_underlying_price, underlying_price,
                                              percent_greeks, index_instrument_id, beta_weights)

            greeks_data = GreeksData(greeks_data.ts_event, greeks_data.ts_event,
                                     greeks_data.instrument_id, greeks_data.is_call, greeks_data.strike, greeks_data.expiry,
                                     shocked_time_to_expiry, greeks_data.multiplier, greeks_data.quantity, shocked_underlying_price,
                                     greeks_data.interest_rate, greeks_data.cost_of_carry, shocked_vol, 0., greeks.price, delta, gamma, greeks.vega,
                                     greeks.theta, abs(greeks.delta / greeks_data.multiplier))

        if position is not None:
            greeks_data.pnl = greeks_data.price - greeks_data.multiplier * position.avg_px_open

        return greeks_data

    def modify_greeks(
        self,
        delta_input: float,
        gamma_input: float,
        underlying_instrument_id: InstrumentId,
        underlying_price: float,
        unshocked_underlying_price: float,
        percent_greeks: bool,
        index_instrument_id: InstrumentId | None,
        beta_weights: dict[InstrumentId, float] | None,
    ) -> tuple[float, float]:
        """
        Modify delta and gamma based on beta weighting and percentage calculations.

        Parameters
        ----------
        delta_input : float
            The input delta value.
        gamma_input : float
            The input gamma value.
        underlying_instrument_id : InstrumentId
            The ID of the underlying instrument.
        underlying_price : float
            The current price of the underlying asset.
        unshocked_underlying_price : float
            The base (non-shocked) price of the underlying asset.
        percent_greeks : bool, optional
            Whether to compute greeks as percentage of the underlying price, by default False.
        index_instrument_id : InstrumentId, optional
            The reference instrument id beta is computed with respect to.
        beta_weights : dict[InstrumentId, float], optional
            Dictionary of beta weights used to compute portfolio delta and gamma.

        Returns
        -------
        tuple[float, float]
            Modified delta and gamma values.

        Notes
        -----
        The beta weighting of delta and gamma follows this equation linking the returns of a stock x to the ones of an index I:
        (x - x0) / x0 = alpha + beta (I - I0) / I0 + epsilon

        beta can be obtained by linear regression of stock_return = alpha + beta index_return, it's equal to:
        beta = Covariance(stock_returns, index_returns) / Variance(index_returns)

        Considering alpha == 0:
        x = x0 + beta x0 / I0 (I-I0)
        I = I0 + 1 / beta I0 / x0 (x - x0)

        These two last equations explain the beta weighting below, considering the price of an option is V(x) and delta and gamma
        are the first and second derivatives respectively of V.

        Also percent greeks assume a change of variable to percent returns by writing:
        V(x = x0 * (1 + stock_percent_return / 100))
        or V(I = I0 * (1 + index_percent_return / 100))
        """
        delta = delta_input
        gamma = gamma_input

        index_price = None
        delta_multiplier = 1.0

        if index_instrument_id is not None:
            index_price = float(self._cache.price(index_instrument_id, PriceType.LAST))

            beta = 1.
            if beta_weights is not None:
                beta = beta_weights.get(underlying_instrument_id, 1.0)

            if underlying_price != unshocked_underlying_price:
                index_price += 1. / beta * (index_price / unshocked_underlying_price) * (underlying_price - unshocked_underlying_price)

            delta_multiplier = beta * underlying_price / index_price
            delta *= delta_multiplier
            gamma *= delta_multiplier ** 2

        if percent_greeks:
            if index_price is None:
                delta *= underlying_price / 100.
                gamma *= (underlying_price / 100.) ** 2
            else:
                delta *= index_price / 100.
                gamma *= (index_price / 100.) ** 2

        return delta, gamma

    def portfolio_greeks(
        self,
        underlyings : list[str] = None,
        Venue venue = None,
        InstrumentId instrument_id = None,
        StrategyId strategy_id = None,
        PositionSide side = PositionSide.NO_POSITION_SIDE,
        flat_interest_rate: float = 0.0425,
        flat_dividend_yield: float | None = None,
        spot_shock: float = 0.0,
        vol_shock: float = 0.0,
        time_to_expiry_shock: float = 0.0,
        use_cached_greeks: bool = False,
        cache_greeks: bool = False,
        publish_greeks: bool = False,
        percent_greeks: bool = False,
        index_instrument_id: InstrumentId | None = None,
        beta_weights: dict[InstrumentId, float] | None = None,
    ) -> PortfolioGreeks:
        """
        Calculate the portfolio Greeks for a given set of positions.

        Aggregates the Greeks data for all open positions that match the specified criteria.

        Additional features:
        - Apply shocks to the spot value of an instrument's underlying, implied volatility or time to expiry.
        - Compute percent greeks.
        - Compute beta-weighted delta and gamma with respect to an index.

        Parameters
        ----------
        underlyings : list, optional
            A list of underlying asset symbol prefixes as strings to filter positions.
            For example, ["AAPL", "MSFT"] would include positions for AAPL and MSFT stocks and options.
            Only positions with instruments starting with one of these symbols will be included.
            If more than one underlying is provided, using beta-weighted greeks is recommended.
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
            The interest rate to use for calculations when no curve is available.
        flat_dividend_yield : float, optional
            The dividend yield to use for calculations when no dividend curve is available.
        spot_shock : float, default 0.0
            Shock to apply to the underlying price.
        vol_shock : float, default 0.0
            Shock to apply to implied volatility.
        time_to_expiry_shock : float, default 0.0
            Shock in years to apply to time to expiry.
        use_cached_greeks : bool, default False
            Whether to use cached Greeks calculations if available.
        cache_greeks : bool, default False
            Whether to cache the calculated Greeks.
        publish_greeks : bool, default False
            Whether to publish the Greeks data to the message bus.
        percent_greeks : bool, optional
            Whether to compute greeks as percentage of the underlying price, by default False.
        index_instrument_id : InstrumentId, optional
            The reference instrument id beta is computed with respect to.
        beta_weights : dict[InstrumentId, float], optional
            Dictionary of beta weights used to compute portfolio delta and gamma.

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

            if underlyings is not None:
                skip_position = True

                for underlying in underlyings:
                    if position_instrument_id.value.startswith(underlying):
                        skip_position = False
                        break

                if skip_position:
                    continue

            quantity = position.signed_qty
            instrument_greeks = self.instrument_greeks(
                position_instrument_id,
                flat_interest_rate,
                flat_dividend_yield,
                spot_shock,
                vol_shock,
                time_to_expiry_shock,
                use_cached_greeks,
                cache_greeks,
                publish_greeks,
                ts_event,
                position,
                percent_greeks,
                index_instrument_id,
                beta_weights,
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
