from __future__ import annotations

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveInt


class PortfolioConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``Portfolio`` instances.

    Parameters
    ----------
    use_mark_prices : bool, default False
        The type of prices used for P&L and net exposure calculations.
        If False (default), uses quote prices if available; otherwise, last trade prices
        (or falls back to bar prices if `bar_updates` is True).
        If True, uses mark prices.
    use_mark_xrates : bool, default False
        The type of exchange rates used for P&L and net exposure calculations.
        If False (default), uses quote prices.
        If True, uses mark prices.
    bar_updates : bool, default True
        If external bar prices should be considered for calculations.
    convert_to_account_base_currency : bool, default True
        If calculations should be converted into each account's base currency.
        This setting is only effective for accounts with a specified base currency.
    min_account_state_logging_interval_ms : PositiveInt, optional
        The minimum interval (milliseconds) between logging account state events for the same account.
        When set, account state updates will only be logged if this much time has passed since the last log.
        Useful for HFT deployments to prevent excessive logging when account states change rapidly.
        Default is None (no throttling).
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    use_mark_prices: bool = False
    use_mark_xrates: bool = False
    bar_updates: bool = True
    convert_to_account_base_currency: bool = True
    min_account_state_logging_interval_ms: PositiveInt | None = None
    debug: bool = False
