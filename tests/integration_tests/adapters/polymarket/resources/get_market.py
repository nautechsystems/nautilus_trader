from pathlib import Path

import msgspec

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def main():
    print("Requesting market")
    client = get_polymarket_http_client()

    # Trump Election 2024 Winner market
    condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
    response = client.get_market(condition_id)

    data = msgspec.json.encode(response)
    Path("http_responses/market.json").write_bytes(data)


if __name__ == "__main__":
    main()
