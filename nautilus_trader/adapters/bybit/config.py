from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config.validation import PositiveFloat
from nautilus_trader.config.validation import PositiveInt


class BybitDataClientConfig(LiveDataClientConfig, frozen=True):
    api_key: str | None = None
    api_secret: str | None = None
    instrument_types: list[BybitInstrumentType] = []
    base_url_http: str | None = None
    base_url_ws: str | None = None
    testnet: bool = False


class BybitExecClientConfig(LiveExecClientConfig, frozen=True):
    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    instrument_type: BybitInstrumentType = BybitInstrumentType.SPOT
    base_url_http: Optional[str] = None
    base_url_ws: Optional[str] = None
    testnet: bool = False
    clock_sync_interval_secs: int = 0
    use_reduce_only: bool = True
    use_position_ids: bool = True
    treat_expired_as_canceled: bool = False
    max_retries: Optional[PositiveInt] = None
    retry_delay: Optional[PositiveFloat] = None
