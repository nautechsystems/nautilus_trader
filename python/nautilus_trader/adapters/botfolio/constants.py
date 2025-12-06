# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers import Venue


# Venue identifier for local paper trading
BOTFOLIO_VENUE = Venue("BOTFOLIO")

# Default Redis channel prefixes
REDIS_BAR_CHANNEL_PREFIX = "market:bar:"
REDIS_QUOTE_CHANNEL_PREFIX = "market:quote:"

# Default configuration values
DEFAULT_REDIS_URL = "redis://localhost:6379"
DEFAULT_STARTING_BALANCE = "100000 USD"
DEFAULT_BASE_LATENCY_MS = 50
DEFAULT_SLIPPAGE_BPS = 5.0

