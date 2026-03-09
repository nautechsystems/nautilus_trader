from __future__ import annotations

from nautilus_trader.common.config import NautilusConfig


class RiskEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``RiskEngine`` instances.

    Parameters
    ----------
    bypass : bool, default False
        If True, then will bypass all pre-trade risk checks and rate limits (will still check for duplicate IDs).
    max_order_submit_rate : str, default 100/00:00:01
        The maximum rate of submit order commands per timedelta.
    max_order_modify_rate : str, default 100/00:00:01
        The maximum rate of modify order commands per timedelta.
    max_notional_per_order : dict[str, int], default empty dict
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    bypass: bool = False
    max_order_submit_rate: str = "100/00:00:01"
    max_order_modify_rate: str = "100/00:00:01"
    max_notional_per_order: dict[str, int] = {}
    debug: bool = False
