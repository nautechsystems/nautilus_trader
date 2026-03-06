from pathlib import Path

import msgspec

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def save_historical_trades() -> None:
    client = get_polymarket_http_client()

    response = client.get_trades()
    print(response)

    path = Path("trades_history.json")
    path.write_bytes(msgspec.json.encode(response))


if __name__ == "__main__":
    save_historical_trades()
