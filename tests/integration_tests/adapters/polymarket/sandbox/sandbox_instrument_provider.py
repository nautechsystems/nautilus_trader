import asyncio

from nautilus_trader.adapters.polymarket.factories import get_polymarket_http_client
from nautilus_trader.adapters.polymarket.providers import PolymarketInstrumentProvider
from nautilus_trader.common.component import LiveClock


async def test_polymarket_instrument_provider():
    clock = LiveClock()
    client = get_polymarket_http_client()

    provider = PolymarketInstrumentProvider(
        client=client,
        clock=clock,
    )

    filters = {
        "next_cursor": "MTEyMDA=",
        "is_active": True,
    }

    await provider.load_all_async(filters=filters)

    instruments = provider.list_all()
    await provider.load_async(instruments[0].id)


if __name__ == "__main__":
    asyncio.run(test_polymarket_instrument_provider())
