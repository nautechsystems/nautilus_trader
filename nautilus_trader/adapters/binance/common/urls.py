from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily


def get_http_base_url(  # noqa: C901 (URL dispatch)
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return "https://testnet.binance.vision"
        elif (
            account_type == BinanceAccountType.USDT_FUTURES
            or account_type == BinanceAccountType.COIN_FUTURES
        ):
            return "https://testnet.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    if environment == BinanceEnvironment.DEMO:
        if account_type.is_spot_or_margin:
            return "https://demo-api.binance.com"
        elif account_type.is_futures:
            # Futures demo uses same URLs as futures testnet
            return "https://testnet.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    top_level_domain: str = "us" if is_us else "com"
    if account_type.is_spot:
        return f"https://api.binance.{top_level_domain}"
    elif account_type.is_margin:
        return f"https://api.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.USDT_FUTURES:
        return f"https://fapi.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"https://dapi.binance.{top_level_domain}"
    else:
        raise RuntimeError(  # pragma: no cover (design-time error)
            f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
        )


def get_private_http_base_url(
    account_type: BinanceAccountType,
    private_api_family: BinancePrivateApiFamily,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    if account_type == BinanceAccountType.PORTFOLIO_MARGIN:
        if environment != BinanceEnvironment.LIVE:
            raise ValueError("Portfolio margin private API routing is only supported on Binance live")
        if is_us:
            raise ValueError("Portfolio margin private API routing is not supported on Binance US")
        return "https://papi.binance.com"

    if not account_type.is_futures or private_api_family != BinancePrivateApiFamily.PORTFOLIO_MARGIN:
        return get_http_base_url(account_type, environment, is_us)

    if environment != BinanceEnvironment.LIVE:
        raise ValueError("Portfolio margin private API routing is only supported on Binance live")
    if is_us:
        raise ValueError("Portfolio margin private API routing is not supported on Binance US")

    return "https://papi.binance.com"


def get_ws_api_base_url(  # noqa: C901 (URL dispatch)
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    """
    Return the WebSocket API base URL for user data streams.

    This is the new authenticated WebSocket API endpoint that replaces listenKey.

    """
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return "wss://ws-api.testnet.binance.vision/ws-api/v3"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            return "wss://testnet.binancefuture.com/ws-fapi/v1"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            raise ValueError("no WS API testnet for COIN-M futures")
        else:
            raise RuntimeError(
                f"invalid `BinanceAccountType`, was {account_type}",
            )

    if environment == BinanceEnvironment.DEMO:
        if account_type.is_spot_or_margin:
            return "wss://demo-ws-api.binance.com/ws-api/v3"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            # Futures demo uses same WS API as futures testnet
            return "wss://testnet.binancefuture.com/ws-fapi/v1"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            raise ValueError("no WS API demo for COIN-M futures")
        else:
            raise RuntimeError(
                f"invalid `BinanceAccountType`, was {account_type}",
            )

    top_level_domain: str = "us" if is_us else "com"
    if account_type.is_spot_or_margin:
        return f"wss://ws-api.binance.{top_level_domain}:443/ws-api/v3"
    elif account_type == BinanceAccountType.USDT_FUTURES:
        return f"wss://ws-fapi.binance.{top_level_domain}/ws-fapi/v1"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"wss://ws-dapi.binance.{top_level_domain}/ws-dapi/v1"
    else:
        raise RuntimeError(
            f"invalid `BinanceAccountType`, was {account_type}",
        )


def get_ws_base_url(  # noqa: C901 (URL dispatch)
    account_type: BinanceAccountType,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    if environment == BinanceEnvironment.TESTNET:
        if account_type.is_spot_or_margin:
            return "wss://stream.testnet.binance.vision"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            return "wss://stream.binancefuture.com"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            return "wss://dstream.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    if environment == BinanceEnvironment.DEMO:
        if account_type.is_spot_or_margin:
            return "wss://demo-stream.binance.com"
        elif account_type == BinanceAccountType.USDT_FUTURES:
            # Futures demo uses same WS URLs as futures testnet
            return "wss://stream.binancefuture.com"
        elif account_type == BinanceAccountType.COIN_FUTURES:
            return "wss://dstream.binancefuture.com"
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"invalid `BinanceAccountType`, was {account_type}",  # pragma: no cover
            )

    top_level_domain: str = "us" if is_us else "com"
    if account_type.is_spot_or_margin:
        return f"wss://stream.binance.{top_level_domain}:9443"
    elif account_type == BinanceAccountType.USDT_FUTURES:
        return f"wss://fstream.binance.{top_level_domain}"
    elif account_type == BinanceAccountType.COIN_FUTURES:
        return f"wss://dstream.binance.{top_level_domain}"
    else:
        raise RuntimeError(
            f"invalid `BinanceAccountType`, was {account_type}",
        )  # pragma: no cover (design-time error)


def get_user_stream_base_url(
    account_type: BinanceAccountType,
    private_api_family: BinancePrivateApiFamily,
    environment: BinanceEnvironment,
    is_us: bool,
) -> str:
    if account_type == BinanceAccountType.PORTFOLIO_MARGIN:
        if environment != BinanceEnvironment.LIVE:
            raise ValueError("Portfolio margin user stream routing is only supported on Binance live")
        if is_us:
            raise ValueError("Portfolio margin user stream routing is not supported on Binance US")
        return "wss://fstream.binance.com/pm"

    if not account_type.is_futures or private_api_family != BinancePrivateApiFamily.PORTFOLIO_MARGIN:
        return get_ws_base_url(account_type, environment, is_us)

    if environment != BinanceEnvironment.LIVE:
        raise ValueError("Portfolio margin user stream routing is only supported on Binance live")
    if is_us:
        raise ValueError("Portfolio margin user stream routing is not supported on Binance US")
    if account_type != BinanceAccountType.USDT_FUTURES:
        raise ValueError("Portfolio margin user stream routing is currently supported for UM futures only")

    return "wss://fstream.binance.com/pm"
