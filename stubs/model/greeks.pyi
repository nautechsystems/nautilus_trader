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

    def __init__(self, msgbus: MessageBus, cache: CacheFacade, clock: Clock) -> None: ...
    def instrument_greeks(self, instrument_id: InstrumentId, flat_interest_rate: float = 0.0425, flat_dividend_yield: float | None = None, spot_shock: float = 0.0, vol_shock: float = 0.0, time_to_expiry_shock: float = 0.0, use_cached_greeks: bool = False, cache_greeks: bool = False, publish_greeks: bool = False, ts_event: int = 0, position: Position | None = None, percent_greeks: bool = False, index_instrument_id: InstrumentId | None = None, beta_weights: dict[InstrumentId, float] | None = None) -> GreeksData: ...
    def modify_greeks(self, delta_input: float, gamma_input: float, underlying_instrument_id: InstrumentId, underlying_price: float, unshocked_underlying_price: float, percent_greeks: bool, index_instrument_id: InstrumentId | None, beta_weights: dict[InstrumentId, float] | None) -> tuple[float, float]: ...
    def portfolio_greeks(self, underlyings: list[str] | None = None, venue: Venue | None = None, instrument_id: InstrumentId | None = None, strategy_id: StrategyId | None = None, side: PositionSide = ..., flat_interest_rate: float = 0.0425, flat_dividend_yield: float | None = None, spot_shock: float = 0.0, vol_shock: float = 0.0, time_to_expiry_shock: float = 0.0, use_cached_greeks: bool = False, cache_greeks: bool = False, publish_greeks: bool = False, percent_greeks: bool = False, index_instrument_id: InstrumentId | None = None, beta_weights: dict[InstrumentId, float] | None = None, greeks_filter: Callable | None = None) -> PortfolioGreeks: ...
    def subscribe_greeks(self, instrument_id: InstrumentId | None = None, handler: Callable[[GreeksData], None] | None = None) -> None: ...
