from nautilus_trader.adapters.env import get_env_key


def get_api_key(is_demo: bool) -> str:
    if is_demo:
        return get_env_key("OKX_DEMO_API_KEY")
    return get_env_key("OKX_API_KEY")


def get_api_secret(is_demo: bool) -> str:
    if is_demo:
        return get_env_key("OKX_DEMO_API_SECRET")
    return get_env_key("OKX_API_SECRET")


def get_passphrase(is_demo: bool) -> str:
    if is_demo:
        return get_env_key("OKX_DEMO_PASSPHRASE")
    return get_env_key("OKX_PASSPHRASE")
