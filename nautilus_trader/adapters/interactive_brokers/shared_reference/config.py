from __future__ import annotations

from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataClientConfig


class InteractiveBrokersSharedReferenceDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for the Flux-owned shared IBKR reference data client.
    """

    profile_id: str = "equities"
    account_scope_id: str = "ibkr.reference.main"
    redis_host: str = "127.0.0.1"
    redis_port: int = 6380
    redis_db: int = 0
    redis_username: str | None = None
    redis_password: str | None = None
    redis_ssl: bool = False
    redis_connect_timeout_secs: float = 5.0
    redis_read_timeout_secs: float = 5.0
    subscription_poll_interval_secs: float = 0.1
    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
