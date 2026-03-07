# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import PositiveInt


class BitgetDataClientConfig(LiveDataClientConfig):
    """Configuration for ``BitgetDataClient`` instances."""

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    product_types: tuple[object, ...] | None = None
    base_url_http: str | None = None
    base_url_ws_public: str | None = None
    base_url_ws_private: str | None = None
    demo: bool = False
    update_instruments_interval_mins: PositiveInt | None = 60
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None


class BitgetExecClientConfig(LiveExecClientConfig):
    """Configuration for ``BitgetExecutionClient`` instances."""

    api_key: str | None = None
    api_secret: str | None = None
    api_passphrase: str | None = None
    product_types: tuple[object, ...] | None = None
    base_url_http: str | None = None
    base_url_ws_private: str | None = None
    demo: bool = False
    max_retries: PositiveInt | None = None
    retry_delay_initial_ms: PositiveInt | None = None
    retry_delay_max_ms: PositiveInt | None = None
