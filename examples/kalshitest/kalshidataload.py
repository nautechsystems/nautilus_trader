import asyncio
from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider


async def main():
    config = KalshiDataClientConfig(series_tickers=("KXBTC",))
    provider = KalshiInstrumentProvider(config=config)
    await provider.load_all_async()
    print(f"Loaded {len(provider.get_all())} instruments")
    for inst in list(provider.get_all().values())[:3]:
        print(inst)


asyncio.run(main())