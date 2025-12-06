# -------------------------------------------------------------------------------------------------
#  Bot-folio Alpaca Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from nautilus_trader.adapters.alpaca.constants import ALPACA_VENUE
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveInt
from nautilus_trader.model.identifiers import Venue


class AlpacaInstrumentProviderConfig(InstrumentProviderConfig, frozen=True):
    """
    Configuration for ``AlpacaInstrumentProvider`` instances.

    Parameters
    ----------
    load_all : bool, default False
        If all venue instruments should be loaded on start.
        For Alpaca, this loads both US equities and crypto assets.
        WARNING: This loads thousands of instruments and is slow - prefer using load_ids.
    load_ids : frozenset[InstrumentId], optional
        The list of instrument IDs to be loaded on start (if `load_all` is False).
    filters : frozendict or dict[str, Any], optional
        The venue specific instrument loading filters to apply.
    log_warnings : bool, default True
        If parser warnings should be logged.

    """

    load_all: bool = False  # Default to False - strategies should specify what they need


class AlpacaDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``AlpacaDataClient`` instances.

    Parameters
    ----------
    venue : Venue, default ALPACA_VENUE
        The venue for the client.
    api_key : str, optional
        The Alpaca API key.
        If ``None`` then will source the `APCA_API_KEY_ID` environment variable.
    api_secret : str, optional
        The Alpaca API secret.
        If ``None`` then will source the `APCA_API_SECRET_KEY` environment variable.
    access_token : str, optional
        The Alpaca OAuth access token.
        If ``None`` then will source the `APCA_API_ACCESS_TOKEN` environment variable.
        Takes precedence over api_key/api_secret if provided.
    paper : bool, default True
        If the client is connecting to paper trading.
    data_feed : str, default "iex"
        The data feed to use: "iex" (free stocks), "sip" (paid stocks), or "crypto".
    update_instruments_interval_mins : PositiveInt or None, default 60
        The interval (minutes) between reloading instruments from the venue.

    """

    venue: Venue = ALPACA_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    access_token: str | None = None
    paper: bool = True
    data_feed: str = "iex"
    update_instruments_interval_mins: PositiveInt | None = 60


class AlpacaExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``AlpacaExecutionClient`` instances.

    Parameters
    ----------
    venue : Venue, default ALPACA_VENUE
        The venue for the client.
    api_key : str, optional
        The Alpaca API key.
        If ``None`` then will source the `APCA_API_KEY_ID` environment variable.
    api_secret : str, optional
        The Alpaca API secret.
        If ``None`` then will source the `APCA_API_SECRET_KEY` environment variable.
    access_token : str, optional
        The Alpaca OAuth access token.
        If ``None`` then will source the `APCA_API_ACCESS_TOKEN` environment variable.
        Takes precedence over api_key/api_secret if provided.
    paper : bool, default True
        If the client is connecting to paper trading.
    max_retries : PositiveInt, optional
        The maximum number of times a submit or cancel order request will be retried.
    retry_delay_secs : float, default 1.0
        The delay (seconds) between retries.

    """

    venue: Venue = ALPACA_VENUE
    api_key: str | None = None
    api_secret: str | None = None
    access_token: str | None = None
    paper: bool = True
    max_retries: PositiveInt | None = 3
    retry_delay_secs: float = 1.0

