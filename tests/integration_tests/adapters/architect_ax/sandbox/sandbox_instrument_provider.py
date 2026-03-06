import asyncio

from nautilus_trader.adapters.architect_ax.factories import get_cached_ax_http_client
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider


async def test_ax_instrument_provider():
    client = get_cached_ax_http_client()

    provider = AxInstrumentProvider(client=client)

    await provider.load_all_async()

    instruments = provider.list_all()
    print(f"Loaded {len(instruments)} instruments")

    for instrument in instruments:
        print(instrument)


if __name__ == "__main__":
    asyncio.run(test_ax_instrument_provider())
