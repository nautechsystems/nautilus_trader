# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import os

from nautilus_trader.common.config import PositiveInt
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig

from nautilus_trader.adapters.lmex.constants import (
    LMEX_API_KEY_ENV,
    LMEX_API_SECRET_ENV,
    LMEX_SANDBOX_API_KEY_ENV,
    LMEX_SANDBOX_API_SECRET_ENV,
)


def resolve_api_key(api_key: str | None, is_sandbox: bool) -> str | None:
    """
    Resolve the LMEX API key from argument or environment variable.

    Parameters
    ----------
    api_key : str or None
        Explicit API key. If not ``None`` this is returned as-is.
    is_sandbox : bool
        When ``True`` the sandbox environment variable is consulted instead of
        the live one.

    Returns
    -------
    str or None

    """
    if api_key is not None:
        return api_key
    env_var = LMEX_SANDBOX_API_KEY_ENV if is_sandbox else LMEX_API_KEY_ENV
    return os.environ.get(env_var)


def resolve_api_secret(api_secret: str | None, is_sandbox: bool) -> str | None:
    """
    Resolve the LMEX API secret from argument or environment variable.

    Parameters
    ----------
    api_secret : str or None
        Explicit API secret. If not ``None`` this is returned as-is.
    is_sandbox : bool
        When ``True`` the sandbox environment variable is consulted instead of
        the live one.

    Returns
    -------
    str or None

    """
    if api_secret is not None:
        return api_secret
    env_var = LMEX_SANDBOX_API_SECRET_ENV if is_sandbox else LMEX_API_SECRET_ENV
    return os.environ.get(env_var)


class LmexDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``LmexLiveMarketDataClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The LMEX API public key.
        If ``None`` then sourced from the ``LMEX_API_KEY`` (or
        ``LMEX_SANDBOX_API_KEY`` when ``is_sandbox=True``) environment variable.
    api_secret : str, optional
        The LMEX API secret.
        If ``None`` then sourced from the ``LMEX_API_SECRET`` (or
        ``LMEX_SANDBOX_API_SECRET`` when ``is_sandbox=True``) environment variable.
    is_sandbox : bool, default False
        When ``True`` the client connects to ``test-api.lmex.io`` instead of the
        live endpoint.
    base_url_http : str, optional
        Override the HTTP base URL.  Defaults to the live or sandbox URL based on
        ``is_sandbox``.
    base_url_ws : str, optional
        Override the WebSocket base URL. Defaults to the live or sandbox URL based
        on ``is_sandbox``.
    update_instruments_interval_mins : PositiveInt or None, default 60
        Interval (minutes) at which instrument definitions are refreshed from the
        venue.  Set to ``None`` to disable background refresh.
    max_retries : PositiveInt or None, default 3
        Maximum number of times a failed HTTP request is retried.
    retry_delay_initial_ms : PositiveInt or None, default 1000
        Initial delay (milliseconds) before the first retry.
    retry_delay_max_ms : PositiveInt or None, default 10000
        Maximum delay (milliseconds) between retries (exponential back-off cap).
    proxy_url : str, optional
        Optional HTTP/WebSocket proxy URL.

    """

    api_key: str | None = None
    api_secret: str | None = None
    is_sandbox: bool = False
    base_url_http: str | None = None
    base_url_ws: str | None = None
    update_instruments_interval_mins: PositiveInt | None = 60
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    proxy_url: str | None = None


class LmexExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``LmexLiveExecutionClient`` instances.

    Parameters
    ----------
    api_key : str, optional
        The LMEX API public key.
        If ``None`` then sourced from the ``LMEX_API_KEY`` (or
        ``LMEX_SANDBOX_API_KEY`` when ``is_sandbox=True``) environment variable.
    api_secret : str, optional
        The LMEX API secret.
        If ``None`` then sourced from the ``LMEX_API_SECRET`` (or
        ``LMEX_SANDBOX_API_SECRET`` when ``is_sandbox=True``) environment variable.
    is_sandbox : bool, default False
        When ``True`` the client connects to ``test-api.lmex.io``.
    base_url_http : str, optional
        Override the HTTP base URL.
    base_url_ws : str, optional
        Override the WebSocket base URL.
    max_retries : PositiveInt or None, default 3
        Maximum retries for order submission / cancellation requests.
    retry_delay_initial_ms : PositiveInt or None, default 1000
        Initial delay (milliseconds) before the first retry.
    retry_delay_max_ms : PositiveInt or None, default 10000
        Maximum delay (milliseconds) between retries.
    proxy_url : str, optional
        Optional HTTP/WebSocket proxy URL.

    Warnings
    --------
    A short ``retry_delay_initial_ms`` with many ``max_retries`` may result in
    account bans if the exchange enforces rate limits aggressively.

    """

    api_key: str | None = None
    api_secret: str | None = None
    is_sandbox: bool = False
    base_url_http: str | None = None
    base_url_ws: str | None = None
    max_retries: PositiveInt | None = 3
    retry_delay_initial_ms: PositiveInt | None = 1_000
    retry_delay_max_ms: PositiveInt | None = 10_000
    proxy_url: str | None = None
