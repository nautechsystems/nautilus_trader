# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from nautilus_trader.adapters.botfolio.constants import BOTFOLIO_VENUE
from nautilus_trader.adapters.botfolio.constants import DEFAULT_BASE_LATENCY_MS
from nautilus_trader.adapters.botfolio.constants import DEFAULT_REDIS_URL
from nautilus_trader.adapters.botfolio.constants import DEFAULT_SLIPPAGE_BPS
from nautilus_trader.adapters.botfolio.constants import DEFAULT_STARTING_BALANCE
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveInt
from nautilus_trader.model.identifiers import Venue


class BotfolioDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``BotfolioDataClient`` instances.

    Parameters
    ----------
    venue : Venue, default BOTFOLIO_VENUE
        The venue for the client.
    redis_url : str, default "redis://localhost:6379"
        The Redis connection URL for subscribing to market data.
    symbols : list[str], optional
        List of symbols to subscribe to on startup.

    """

    venue: Venue = BOTFOLIO_VENUE
    redis_url: str = DEFAULT_REDIS_URL
    symbols: list[str] | None = None


class BotfolioExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``BotfolioExecutionClient`` instances.

    Parameters
    ----------
    venue : Venue, default BOTFOLIO_VENUE
        The venue for the client.
    redis_url : str, default "redis://localhost:6379"
        The Redis connection URL for receiving price data (used for fill simulation).
    starting_balance : str, default "100000 USD"
        The starting balance for the paper trading account.
    base_latency_ms : PositiveInt, default 50
        Base execution latency in milliseconds.
    slippage_bps : float, default 5.0
        Slippage in basis points per $10K notional.
    partial_fill_prob : float, default 0.0
        Probability of partial fill (0.0 to 1.0).

    """

    venue: Venue = BOTFOLIO_VENUE
    redis_url: str = DEFAULT_REDIS_URL
    starting_balance: str = DEFAULT_STARTING_BALANCE
    base_latency_ms: PositiveInt = DEFAULT_BASE_LATENCY_MS
    slippage_bps: float = DEFAULT_SLIPPAGE_BPS
    partial_fill_prob: float = 0.0

