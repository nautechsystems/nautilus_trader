from pathlib import Path

import msgspec

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def main():
    print("Requesting markets")
    client = get_polymarket_http_client()
    response = client.get_markets()

    data = msgspec.json.encode(response)
    Path("http_responses/markets.json").write_bytes(data)

    # print(client.get_simplified_markets())
    # print(client.get_sampling_markets())
    # print(client.get_sampling_simplified_markets())
    # print(client.get_market("condition_id"))


if __name__ == "__main__":
    main()
