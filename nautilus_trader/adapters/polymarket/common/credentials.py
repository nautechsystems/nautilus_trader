import msgspec

from nautilus_trader.adapters.env import get_env_key


def get_polymarket_api_key() -> str:
    return get_env_key("POLYMARKET_API_KEY")


def get_polymarket_api_secret() -> str:
    return get_env_key("POLYMARKET_API_SECRET")


def get_polymarket_passphrase() -> str:
    return get_env_key("POLYMARKET_PASSPHRASE")


def get_polymarket_private_key() -> str:
    return get_env_key("POLYMARKET_PK")


def get_polymarket_funder() -> str:
    return get_env_key("POLYMARKET_FUNDER")


class PolymarketWebSocketAuth(msgspec.Struct, frozen=True):
    apiKey: str
    secret: str
    passphrase: str
