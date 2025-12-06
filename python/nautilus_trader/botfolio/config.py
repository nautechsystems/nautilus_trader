"""
Bot-folio configuration helpers.

Provides easy access to credentials and settings injected by run_strategy.py.
"""
import os
from dataclasses import dataclass
from typing import Optional


@dataclass
class BotfolioConfig:
    """Configuration loaded from environment variables set by the trading engine."""

    bot_id: str
    provider: str
    trading_mode: str  # 'paper' or 'live'
    initial_capital: float
    virtual_cash: float

    # Alpaca credentials (may be None if using a different provider)
    alpaca_api_key: Optional[str] = None
    alpaca_api_secret: Optional[str] = None
    alpaca_access_token: Optional[str] = None
    alpaca_base_url: Optional[str] = None

    @property
    def is_paper(self) -> bool:
        return self.trading_mode == "paper"

    @property
    def is_live(self) -> bool:
        return self.trading_mode == "live"


def get_config() -> BotfolioConfig:
    """
    Get the bot-folio configuration from environment variables.

    These are set by run_strategy.py before executing the user's strategy.

    Returns
    -------
    BotfolioConfig
        The configuration object with credentials and settings.

    """
    return BotfolioConfig(
        bot_id=os.environ.get("BOTFOLIO_BOT_ID", ""),
        provider=os.environ.get("BOTFOLIO_PROVIDER", ""),
        trading_mode=os.environ.get("BOTFOLIO_TRADING_MODE", "paper"),
        initial_capital=float(os.environ.get("BOTFOLIO_INITIAL_CAPITAL", "100000")),
        virtual_cash=float(os.environ.get("BOTFOLIO_VIRTUAL_CASH", "100000")),
        alpaca_api_key=os.environ.get("APCA_API_KEY_ID"),
        alpaca_api_secret=os.environ.get("APCA_API_SECRET_KEY"),
        alpaca_access_token=os.environ.get("APCA_API_ACCESS_TOKEN"),
        alpaca_base_url=os.environ.get("APCA_API_BASE_URL"),
    )

