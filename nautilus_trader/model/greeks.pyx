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

from libc.math cimport exp
from libc.stdint cimport uint64_t

from nautilus_trader.core.nautilus_pyo3 import black_scholes_greeks
from nautilus_trader.core.nautilus_pyo3 import imply_vol_and_greeks
from nautilus_trader.core.nautilus_pyo3 import refine_vol_and_greeks
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.greeks_data import GreeksData
from nautilus_trader.model.greeks_data import PortfolioGreeks

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.datetime cimport unix_nanos_to_dt
from nautilus_trader.core.rust.model cimport OptionKind
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.model.data cimport IndexPriceUpdate
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Symbol
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
    cache : CacheFacade
        The cache for the calculator.
    clock : LiveClock
        The clock for the calculator.
    logger : Logger
        The logger for logging messages.

    Notes
    ----------
    Currently implemented greeks are:
    - Delta (first derivative of price with respect to spot move).
    - Gamma (second derivative of price with respect to spot move).
    - Vega (first derivative of price with respect to implied volatility of an option).
    - Theta (first derivative of price with respect to time to expiry).

    Vega is expressed in terms of absolute percent changes ((dV / dVol) / 100).
    Theta is expressed in terms of daily changes ((dV / d(T-t)) / 365.25, where T is the expiry of an option and t is the current time).

    Also note that for ease of implementation we consider that american options (for stock options for example) are european for the computation of greeks.

    """

    def __init__(
        self,
        CacheFacade cache not None,
        Clock clock not None,
    ) -> None:
        self._cache = cache
        self._clock = clock
        self._log = Logger(type(self).__name__)
        self._cached_futures_spreads = {}

    def instrument_greeks(
        self,
        instrument_id: InstrumentId,
        flat_interest_rate: float = 0.0425,
        flat_dividend_yield: float | None = None,
        spot_shock: float = 0.,
        vol_shock: float = 0.,
        time_to_expiry_shock: float = 0.,
        use_cached_greeks: bool = False,
        update_vol: bool = False,
        cache_greeks: bool = False,
        ts_event: int = 0,
        position: Position | None = None,
        percent_greeks: bool = False,
        index_instrument_id: InstrumentId | None = None,
        beta_weights: dict[InstrumentId, float] | None = None,
        vega_time_weight_base: int | None = None,
    ) -> GreeksData | None:
        """
        Calculate option or underlying greeks for a given instrument and a quantity of 1.

        Additional features:
        - Apply shocks to the spot value of the instrument's underlying, implied volatility or time to expiry.
        - Compute percent greeks.
        - Compute beta-weighted delta and gamma with respect to an index.
        - Update volatility to a target price from a previously calculated volatility.

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
        update_vol : bool, default False
            Whether to update the volatility to a target price using the previously calculated volatility.
        cache_greeks : bool, default False
            Whether to cache the calculated greeks.
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
        vega_time_weight_base : int, optional
            Base value in days for time-weighting vega. When provided, vega is multiplied by sqrt(vega_time_weight_base / expiry_in_days).
            Also enables percent vega calculation where vega is multiplied by vol / 100.

        Returns
        -------
        GreeksData
          The calculated option greeks data
          Contains price, delta, gamma, vega, theta as well as additional information used for the computation.

        """
        cdef object instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot calculate greeks: instrument {instrument_id!r} not found")
            return None

        if instrument.instrument_class is not InstrumentClass.OPTION:
            return self._calculate_non_option_greeks(
                instrument, instrument_id, spot_shock, ts_event, position, percent_greeks,
                index_instrument_id, beta_weights
            )

        cdef InstrumentId underlying_instrument_id = InstrumentId.from_str(f"{instrument.underlying}.{instrument_id.venue}")
        cdef object greeks_data = self._calculate_option_greeks(
            instrument, instrument_id, underlying_instrument_id, flat_interest_rate, flat_dividend_yield,
            use_cached_greeks, update_vol, cache_greeks, ts_event, percent_greeks, index_instrument_id,
            beta_weights, vega_time_weight_base
        )
        if greeks_data is None:
            return None

        if spot_shock != 0. or vol_shock != 0. or time_to_expiry_shock != 0.:
            greeks_data = self._apply_option_greeks_shocks(
                greeks_data, underlying_instrument_id, spot_shock, vol_shock, time_to_expiry_shock, percent_greeks,
                index_instrument_id, beta_weights, vega_time_weight_base
            )

        if position is not None:
            greeks_data.pnl = greeks_data.price - position.avg_px_open

        return greeks_data

    cdef object _calculate_non_option_greeks(
        self,
        object instrument,
        InstrumentId instrument_id,
        double spot_shock,
        uint64_t ts_event,
        object position,
        bint percent_greeks,
        object index_instrument_id,
        object beta_weights,
    ):
        cdef double multiplier = float(instrument.multiplier)
        cdef InstrumentId underlying_instrument_id = instrument.id
        cdef Price underlying_price_obj = self._get_price(underlying_instrument_id)
        cdef double underlying_price
        cdef double delta

        if underlying_price_obj is None:
            return None

        underlying_price = float(underlying_price_obj)
        delta, _, _ = self.modify_greeks(
            1., 0., underlying_instrument_id, underlying_price + spot_shock, underlying_price,
            percent_greeks, index_instrument_id, beta_weights, 0.0, 0.0, 0,
            None
        )
        cdef object greeks_data = GreeksData.from_delta(instrument_id, delta, multiplier, ts_event)

        if position is not None:
            greeks_data.pnl = (underlying_price + spot_shock) - position.avg_px_open
            greeks_data.price = greeks_data.pnl

        return greeks_data

    cdef object _calculate_option_greeks(
        self,
        object instrument,
        InstrumentId instrument_id,
        InstrumentId underlying_instrument_id,
        double flat_interest_rate,
        object flat_dividend_yield,
        bint use_cached_greeks,
        bint update_vol,
        bint cache_greeks,
        uint64_t ts_event,
        bint percent_greeks,
        object index_instrument_id,
        object beta_weights,
        object vega_time_weight_base,
    ):
        cdef double cost_of_carry = 0.
        cdef object greeks_data = None
        cdef object cached_greeks = None
        cdef object greeks = None
        cdef double initial_vol
        cdef double delta
        cdef double gamma
        cdef double vega

        if use_cached_greeks:
            greeks_data = self._cache.greeks(instrument_id)
            if greeks_data is not None:
                self._log.debug(f"Using cached greeks for {instrument_id=}")
                return greeks_data

        cdef uint64_t utc_now_ns = ts_event if ts_event else self._clock.timestamp_ns()
        cdef object utc_now = unix_nanos_to_dt(utc_now_ns)
        cdef object expiry_utc = instrument.expiration_utc
        cdef int expiry_int = int(expiry_utc.strftime("%Y%m%d"))
        cdef int expiry_in_days = max((expiry_utc - utc_now).days, 1)
        cdef double expiry_in_years = expiry_in_days / 365.25
        cdef str currency = instrument.quote_currency.code
        cdef object yield_curve = self._cache.yield_curve(currency)
        cdef double multiplier = float(instrument.multiplier)
        cdef bint is_call = instrument.option_kind is OptionKind.CALL
        cdef double strike = float(instrument.strike_price)

        cdef Price option_price_obj = self._get_price(instrument_id)
        if option_price_obj is None:
            return None

        cdef double interest_rate = yield_curve(expiry_in_years) if yield_curve is not None else flat_interest_rate
        cdef object dividend_curve = self._cache.yield_curve(str(underlying_instrument_id))
        if dividend_curve is not None:
            cost_of_carry = interest_rate - dividend_curve(expiry_in_years)
        elif flat_dividend_yield is not None:
            cost_of_carry = interest_rate - flat_dividend_yield

        cdef Price underlying_price_obj = self._get_underlying_price(underlying_instrument_id)
        if underlying_price_obj is None:
            return None

        cdef double option_price = float(option_price_obj)
        cdef double underlying_price = float(underlying_price_obj)

        if update_vol and (cached_greeks := self._cache.greeks(instrument_id)) is not None:
            initial_vol = cached_greeks.vol
            greeks = refine_vol_and_greeks(
                underlying_price, interest_rate, cost_of_carry, is_call, strike, expiry_in_years,
                option_price, initial_vol
            )
            if greeks is not None:
                self._log.debug(
                    f"Updated vol from cached greeks for {instrument_id=}: {initial_vol:.4f} -> {greeks.vol:.4f}",
                )
            else:
                greeks = imply_vol_and_greeks(
                    underlying_price, interest_rate, cost_of_carry, is_call, strike,
                    expiry_in_years, option_price
                )
        else:
            greeks = imply_vol_and_greeks(
                underlying_price, interest_rate, cost_of_carry, is_call, strike,
                expiry_in_years, option_price
            )

        delta, gamma, vega = self.modify_greeks(
            greeks.delta, greeks.gamma, underlying_instrument_id, underlying_price, underlying_price, percent_greeks,
            index_instrument_id, beta_weights, greeks.vega, greeks.vol, expiry_in_days, vega_time_weight_base
        )

        greeks_data = GreeksData(
            utc_now_ns, utc_now_ns, instrument_id, is_call, strike, expiry_int, expiry_in_days, expiry_in_years,
            multiplier, 1.0, underlying_price, interest_rate, cost_of_carry, greeks.vol, 0., greeks.price, delta,
            gamma, vega, greeks.theta, greeks.itm_prob
        )

        if cache_greeks:
            self._cache.add_greeks(greeks_data)

        return greeks_data

    cdef object _apply_option_greeks_shocks(
        self,
        object greeks_data,
        InstrumentId underlying_instrument_id,
        double spot_shock,
        double vol_shock,
        double time_to_expiry_shock,
        bint percent_greeks,
        object index_instrument_id,
        object beta_weights,
        object vega_time_weight_base,
    ):
        cdef double underlying_price = greeks_data.underlying_price
        cdef double shocked_underlying_price = underlying_price + spot_shock
        cdef double shocked_vol = greeks_data.vol + vol_shock
        cdef double shocked_time_to_expiry = greeks_data.expiry_in_years - time_to_expiry_shock
        cdef object greeks = black_scholes_greeks(
            shocked_underlying_price, greeks_data.interest_rate, greeks_data.cost_of_carry, shocked_vol,
            greeks_data.is_call, greeks_data.strike, shocked_time_to_expiry
        )
        cdef int shocked_expiry_in_days = int(shocked_time_to_expiry * 365.25)

        cdef double delta
        cdef double gamma
        cdef double vega
        delta, gamma, vega = self.modify_greeks(
            greeks.delta, greeks.gamma, underlying_instrument_id, shocked_underlying_price, underlying_price,
            percent_greeks, index_instrument_id, beta_weights, greeks.vega, shocked_vol, shocked_expiry_in_days,
            vega_time_weight_base
        )

        return GreeksData(
            greeks_data.ts_event, greeks_data.ts_event, greeks_data.instrument_id, greeks_data.is_call,
            greeks_data.strike, greeks_data.expiry, shocked_expiry_in_days, shocked_time_to_expiry,
            greeks_data.multiplier, greeks_data.quantity, shocked_underlying_price, greeks_data.interest_rate,
            greeks_data.cost_of_carry, shocked_vol, 0., greeks.price, delta, gamma, vega, greeks.theta, greeks.itm_prob
        )

    cdef Price _get_underlying_price(self, InstrumentId underlying_instrument_id):
        cdef Price price = self._get_price(underlying_instrument_id)
        if price is not None:
            return price

        # Only fall back to cached futures spread when the underlying is a future
        # (or absent from the cache, since the spread was explicitly cached)
        cdef object underlying_instrument = self._cache.instrument(underlying_instrument_id)
        cdef bint is_future_or_absent = (
            underlying_instrument is None
            or underlying_instrument.instrument_class is InstrumentClass.FUTURE
        )

        if is_future_or_absent:
            price = self.get_cached_futures_spread_price(underlying_instrument_id)
            if price is not None:
                self._log.debug(f"Using cached futures spread for {underlying_instrument_id=}: {float(price)=:.6f}")
                return price

        self._log.warning(f"No price available for {underlying_instrument_id}")
        return None

    cdef Price _get_price(self, InstrumentId instrument_id):
        # For index-class futures, prefer tradable quotes over the published index
        # price since index price is the spot level and may diverge from futures basis.
        # For true index instruments (non-futures), prefer the published index price.
        cdef object instrument = self._cache.instrument(instrument_id)
        cdef IndexPriceUpdate index_price
        cdef Price price
        if instrument is not None and instrument.asset_class is AssetClass.INDEX:
            if instrument.instrument_class is InstrumentClass.FUTURE:
                price = self._cache.price(instrument_id, PriceType.MID)
                if price is not None:
                    return price
                price = self._cache.price(instrument_id, PriceType.LAST)
                if price is not None:
                    return price
            index_price = self._cache.index_price(instrument_id)
            if index_price is not None:
                return index_price.value

        price = self._cache.price(instrument_id, PriceType.MID)
        if price is not None:
            return price

        price = self._cache.price(instrument_id, PriceType.LAST)
        if price is not None:
            return price

        self._log.warning(f"No price available for {instrument_id}")
        return None

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
        vega_input: float = 0.0,
        vol: float = 0.0,
        expiry_in_days: int = 0,
        vega_time_weight_base: int | None = None,
    ) -> tuple[float, float, float]:
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
        vega_input : float, default 0.0
            The original vega value.
        vol : float, default 0.0
            The implied volatility.
        expiry_in_days : int, default 0
            Days to expiry.
        vega_time_weight_base : int, optional
            Base value in days for time-weighting vega.

        Returns
        -------
        tuple[float, float, float]
            Modified delta, gamma, and vega values.

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
        cdef double delta = delta_input
        cdef double gamma = gamma_input
        cdef double vega = vega_input
        cdef object index_price = None
        cdef double delta_multiplier = 1.0
        cdef double beta = 1.

        if index_instrument_id is not None:
            index_price = float(self._cache.price(index_instrument_id, PriceType.LAST))

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

            vega = vega * vol / 100.0

        # Apply time weighting to vega if vega_time_weight_base is provided
        cdef double time_weight
        if vega_time_weight_base is not None and expiry_in_days > 0:
            time_weight = (vega_time_weight_base / expiry_in_days) ** 0.5
            vega *= time_weight

        return delta, gamma, vega

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
        update_vol: bool = False,
        cache_greeks: bool = False,
        percent_greeks: bool = False,
        index_instrument_id: InstrumentId | None = None,
        beta_weights: dict[InstrumentId, float] | None = None,
        greeks_filter: callable | None = None,
        vega_time_weight_base: int | None = None,
    ) -> PortfolioGreeks | None:
        """
        Calculate the portfolio Greeks for a given set of positions.

        Aggregates the Greeks data for all open positions that match the specified criteria.

        Additional features:
        - Apply shocks to the spot value of an instrument's underlying, implied volatility or time to expiry.
        - Compute percent greeks.
        - Compute beta-weighted delta and gamma with respect to an index.
        - Update volatility to a target price from a previously calculated volatility.

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
        flat_interest_rate : float, default 0.0425
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
        update_vol : bool, default False
            Whether to update the volatility to a target price using the previously calculated volatility.
        cache_greeks : bool, default False
            Whether to cache the calculated Greeks.
        percent_greeks : bool, optional
            Whether to compute greeks as percentage of the underlying price, by default False.
        index_instrument_id : InstrumentId, optional
            The reference instrument id beta is computed with respect to.
        beta_weights : dict[InstrumentId, float], optional
            Dictionary of beta weights used to compute portfolio delta and gamma.
        greeks_filter : callable, optional
            Filter function to select which greeks to add to the portfolio_greeks.
        vega_time_weight_base : int, optional
            Base value in days for time-weighting vega. When provided, vega is multiplied by sqrt(vega_time_weight_base / expiry_in_days).
            Also enables percent vega calculation where vega is multiplied by vol / 100.

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
        cdef uint64_t ts_event = self._clock.timestamp_ns()
        cdef object portfolio_greeks = PortfolioGreeks(ts_event, ts_event)
        cdef list open_positions = self._cache.positions_open(venue, instrument_id, strategy_id, side)
        cdef InstrumentId position_instrument_id
        cdef bint skip_position
        cdef double quantity
        cdef object instrument_greeks
        cdef object position_greeks

        for position in open_positions:
            position_instrument_id = position.instrument_id

            # Filtering underlyings
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
                position_instrument_id, flat_interest_rate, flat_dividend_yield, spot_shock, vol_shock,
                time_to_expiry_shock, use_cached_greeks, update_vol, cache_greeks, ts_event, position,
                percent_greeks, index_instrument_id, beta_weights, vega_time_weight_base,
            )
            if instrument_greeks is None:
                self._log.warning(f"No greeks available for underlying {position_instrument_id}")
                continue

            position_greeks = quantity * instrument_greeks

            if greeks_filter is None or greeks_filter(position_greeks):
                portfolio_greeks += position_greeks

        return portfolio_greeks

    cpdef object cache_futures_spread(
        self,
        InstrumentId call_instrument_id,
        InstrumentId put_instrument_id,
        InstrumentId futures_instrument_id,
    ):
        cdef object call_instrument = self._cache.instrument(call_instrument_id)
        cdef object put_instrument = self._cache.instrument(put_instrument_id)
        cdef object reference_future_instrument = self._cache.instrument(futures_instrument_id)
        cdef Price reference_future_price = self._get_price(futures_instrument_id)

        if call_instrument is None or put_instrument is None:
            self._log.warning(
                f"Cannot cache futures spread: missing option instruments {call_instrument_id=} {put_instrument_id=}",
            )
            return None

        if call_instrument.instrument_class is not InstrumentClass.OPTION or put_instrument.instrument_class is not InstrumentClass.OPTION:
            self._log.warning(
                f"Cannot cache futures spread: non-option instruments provided {call_instrument_id=} {put_instrument_id=}",
            )
            return None

        if call_instrument.option_kind is not OptionKind.CALL or put_instrument.option_kind is not OptionKind.PUT:
            self._log.warning(
                f"Cannot cache futures spread: expected call/put pair {call_instrument_id=} {put_instrument_id=}",
            )
            return None

        if call_instrument.underlying != put_instrument.underlying:
            self._log.warning(
                f"Cannot cache futures spread: option underlyings differ {call_instrument_id=} {put_instrument_id=}",
            )
            return None

        if call_instrument.strike_price != put_instrument.strike_price:
            self._log.warning(
                f"Cannot cache futures spread: strike prices differ {call_instrument_id=} {put_instrument_id=}",
            )
            return None

        if call_instrument.expiration_ns != put_instrument.expiration_ns:
            self._log.warning(
                f"Cannot cache futures spread: expiration dates differ {call_instrument_id=} {put_instrument_id=}",
            )
            return None

        if reference_future_instrument is None:
            self._log.warning(f"Cannot cache futures spread: no reference futures instrument for {futures_instrument_id}")
            return None

        if reference_future_price is None:
            self._log.warning(f"Cannot cache futures spread: no reference futures price for {futures_instrument_id}")
            return None

        cdef Price call_price = self._get_price(call_instrument_id)
        cdef Price put_price = self._get_price(put_instrument_id)
        if call_price is None or put_price is None:
            self._log.warning(
                f"Cannot cache futures spread: missing option price for {call_instrument_id=} or {put_instrument_id=}",
            )
            return None

        cdef InstrumentId underlying_instrument_id = InstrumentId.from_str(f"{call_instrument.underlying}.{call_instrument_id.venue}")

        # Reject if the underlying is present in cache but is not a future
        cdef object underlying_check = self._cache.instrument(underlying_instrument_id)
        if underlying_check is not None and underlying_check.instrument_class is not InstrumentClass.FUTURE:
            self._log.warning(
                f"Cannot cache futures spread: underlying {underlying_instrument_id} is not a futures contract",
            )
            return None

        cdef double implied_future_price = self._calculate_implied_future_price(call_instrument, call_price, put_price)

        cdef double spread = implied_future_price - float(reference_future_price)
        cdef Price spread_price = reference_future_instrument.make_price(spread)
        self._cached_futures_spreads[underlying_instrument_id] = (futures_instrument_id, spread_price)

        return reference_future_price.add(spread_price)

    cdef double _calculate_implied_future_price(self, object call_instrument, Price call_price, Price put_price):
        cdef object expiry_utc = call_instrument.expiration_utc
        cdef int expiry_in_days = max((expiry_utc - unix_nanos_to_dt(self._clock.timestamp_ns())).days, 1)
        cdef double expiry_in_years = expiry_in_days / 365.25
        cdef str currency = call_instrument.quote_currency.code
        cdef object yield_curve = self._cache.yield_curve(currency)
        cdef double interest_rate = yield_curve(expiry_in_years) if yield_curve is not None else 0.0425
        cdef double strike = float(call_instrument.strike_price)

        return strike + exp(interest_rate * expiry_in_years) * (float(call_price) - float(put_price))

    cpdef object get_cached_futures_spread_price(self, InstrumentId underlying_instrument_id):
        cdef Price spread
        cdef object futures_instrument_id
        cdef Price reference_future_price
        cdef object cached_spread

        cached_spread = self._cached_futures_spreads.get(underlying_instrument_id)
        if cached_spread is None:
            return None

        futures_instrument_id, spread = cached_spread

        reference_future_price = self._get_price(futures_instrument_id)
        if reference_future_price is None:
            return None

        return reference_future_price.add(spread)
