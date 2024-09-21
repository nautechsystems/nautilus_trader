from nautilus_trader.adapters.env import get_env_key_or
from nautilus_trader.adapters.okx.common.enums import OKXWsBaseUrlType


def get_http_base_url() -> str:
    return get_env_key_or("OKX_BASE_URL_HTTP", "https://www.okx.com")


def get_ws_base_url(ws_base_url_type: OKXWsBaseUrlType, is_demo: bool) -> str:
    if is_demo:
        match ws_base_url_type:
            case OKXWsBaseUrlType.PUBLIC:
                return get_env_key_or(
                    "OKX_DEMO_BASE_URL_PUBLIC_WS",
                    "wss://wspap.okx.com:8443/ws/v5/public",
                )
            case OKXWsBaseUrlType.PRIVATE:
                return get_env_key_or(
                    "OKX_DEMO_BASE_URL_PRIVATE_WS",
                    "wss://wspap.okx.com:8443/ws/v5/private",
                )
            case OKXWsBaseUrlType.BUSINESS:
                return get_env_key_or(
                    "OKX_DEMO_BASE_URL_BUSINESS_WS",
                    "wss://wspap.okx.com:8443/ws/v5/business",
                )
            case _:
                raise ValueError(
                    f"unknown websocket base url type {ws_base_url_type} - must be one of "
                    f"{list(OKXWsBaseUrlType)}",
                )

    match ws_base_url_type:
        case OKXWsBaseUrlType.PUBLIC:
            return get_env_key_or("OKX_BASE_URL_PUBLIC_WS", "wss://ws.okx.com:8443/ws/v5/public")
        case OKXWsBaseUrlType.PRIVATE:
            return get_env_key_or("OKX_BASE_URL_PRIVATE_WS", "wss://ws.okx.com:8443/ws/v5/private")
        case OKXWsBaseUrlType.BUSINESS:
            return get_env_key_or(
                "OKX_BASE_URL_BUSINESS_WS",
                "wss://ws.okx.com:8443/ws/v5/business",
            )
        case _:
            raise ValueError(
                f"unknown websocket base url type {ws_base_url_type} - must be one of "
                f"{list(OKXWsBaseUrlType)}",
            )
