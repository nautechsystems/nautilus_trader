import asyncio

from nautilus_trader.adapters.databento.factories import get_cached_databento_http_client
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core import nautilus_pyo3


async def test_databento_instrument_provider():
    http_client = get_cached_databento_http_client()
    clock = LiveClock()

    provider = DatabentoInstrumentProvider(
        http_client=http_client,
        clock=clock,
    )

    # await provider.load_async(InstrumentId.from_str("ESH4.GLBX"))

    # SR3Z4.CME SR3M5.CME and CLM5.CME
    instrument_ids = [
        nautilus_pyo3.InstrumentId.from_str("SR3Z4.GLBX"),
        nautilus_pyo3.InstrumentId.from_str("SR3M5.GLBX"),
        nautilus_pyo3.InstrumentId.from_str("CLM5.GLBX"),
        # nautilus_pyo3.InstrumentId.from_str("AAPL.XNAS"),
    ]
    await provider.load_ids_async(instrument_ids)
    instruments = provider.list_all()

    # instruments = await provider.get_range(
    #     instrument_ids=instrument_ids,
    #     start=(pd.Timestamp.utcnow() - pd.Timedelta(days=5)).date().isoformat(),
    # )

    print(instruments)
    await asyncio.sleep(1.0)


if __name__ == "__main__":
    asyncio.run(test_databento_instrument_provider())
