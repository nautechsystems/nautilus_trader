from typing import Optional

from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config.validation import PositiveFloat
from nautilus_trader.config.validation import PositiveInt


class BybitDataClientConfig(LiveDataClientConfig, frozen=True):
    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    account_type: BybitAccountType = BybitAccountType.SPOT
    base_url_http: Optional[str] = None
    base_url_ws: Optional[str] = None
    # us: bool = False
    testnet: bool = False
    # use_agg_trade_ticks: bool = False


class BybitExecClientConfig(LiveExecClientConfig, frozen=True):
    api_key: Optional[str] = None
    api_secret: Optional[str] = None
    account_type: BybitAccountType = BybitAccountType.SPOT
    base_url_http: Optional[str] = None
    base_url_ws: Optional[str] = None
    testnet: bool = False
    clock_sync_interval_secs: int = 0
    use_reduce_only: bool = True
    use_position_ids: bool = True
    treat_expired_as_canceled: bool = False
    max_retries: Optional[PositiveInt] = None
    retry_delay: Optional[PositiveFloat] = None
