from collections.abc import Callable

from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.greeks_data import GreeksData
from nautilus_trader.model.greeks_data import PortfolioGreeks
from stubs.cache.base import CacheFacade
from stubs.common.component import Clock
from stubs.common.component import MessageBus
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import Venue
from stubs.model.position import Position

class GreeksCalculator:
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
    -----
    Currently implemented greeks are:
    - Delta (first derivative of price with respect to spot move).
    - Gamma (second derivative of price with respect to spot move).
    - Vega (first derivative of price with respect to implied volatility of an option).
    - Theta (first derivative of price with respect to time to expiry).

    Vega is expressed in terms of absolute percent changes ((dV / dVol) / 100).
    Theta is expressed in terms of daily changes ((dV / d(T-t)) / 365.25, where T is the expiry of an option and t is the current time).

    Also note that for ease of implementation we consider that american options (for stock options for example) are european for the computation of greeks.

    """

    def __init__(self, msgbus: MessageBus, cache: CacheFacade, clock: Clock) -> None: ...
    def instrument_greeks(self, instrument_id: InstrumentId, flat_interest_rate: float = 0.0425, flat_dividend_yield: float | None = None, spot_shock: float = 0.0, vol_shock: float = 0.0, time_to_expiry_shock: float = 0.0, use_cached_greeks: bool = False, cache_greeks: bool = False, publish_greeks: bool = False, ts_event: int = 0, position: Position | None = None, percent_greeks: bool = False, index_instrument_id: InstrumentId | None = None, beta_weights: dict[InstrumentId, float] | None = None) -> GreeksData:
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
        ...
    def modify_greeks(self, delta_input: float, gamma_input: float, underlying_instrument_id: InstrumentId, underlying_price: float, unshocked_underlying_price: float, percent_greeks: bool, index_instrument_id: InstrumentId | None, beta_weights: dict[InstrumentId, float] | None) -> tuple[float, float]:
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
        ...
    def portfolio_greeks(self, underlyings: list[str] | None = None, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: PositionSide = ..., flat_interest_rate: float = 0.0425, flat_dividend_yield: float | None = None, spot_shock: float = 0.0, vol_shock: float = 0.0, time_to_expiry_shock: float = 0.0, use_cached_greeks: bool = False, cache_greeks: bool = False, publish_greeks: bool = False, percent_greeks: bool = False, index_instrument_id: InstrumentId | None = None, beta_weights: dict[InstrumentId, float] | None = None, greeks_filter: Callable | None = None) -> PortfolioGreeks:
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
        greeks_filter : callable, optional
            Filter function to select which greeks to add to the portfolio_greeks.

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
        ...
    def subscribe_greeks(self, instrument_id: InstrumentId | None = None, handler: Callable[[GreeksData], None] | None = None) -> None:
        """
        Subscribe to Greeks data for a given underlying instrument.

        Useful for reading greeks from a backtesting data catalog and caching them for later use.

        Parameters
        ----------
        instrument_id : str, optional
            The underlying instrument ID subscribe to.
            Use for example InstrumentId.from_str("ES*.GLBX") to cache all ES greeks.
            If empty, subscribes to all Greeks data.
        handler : Callable[[GreeksData], None], optional
            The callback function to handle received Greeks data.
            If None, defaults to adding greeks to the cache.

        Returns
        -------
        None

        """
        ...
