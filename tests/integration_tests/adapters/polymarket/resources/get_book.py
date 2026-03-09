from pathlib import Path

import msgspec

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client


def main():
    print("Requesting book")
    client = get_polymarket_http_client()
    token_id = 23360939988679364027624185518382759743328544433592111535569478055890815567848
    response = client.get_order_book(token_id)

    data = msgspec.json.encode(response)
    Path("http_responses/book.json").write_bytes(data)


if __name__ == "__main__":
    main()
