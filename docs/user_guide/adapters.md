# Adapters

The NautilusTrader design allows for integrating data publishers and/or trading venues
through adapter implementations, these can be found in the top level `adapters` subpackage. 

A full integration adapter is typically comprised of the following main components:

- `InstrumentProvider`
- `DataClient`
- `ExecutionClient`

## Instrument Providers

Instrument providers do as their name suggests - instantiating Nautilus 
`Instrument` objects by parsing the publisher or venues raw API.

The use cases for the instruments available from an `InstrumentProvider` are either:
- Used standalone to discover the instruments available for an integration, using these for research or backtesting purposes
- Used in a sandbox or live trading environment context for consumption by actors/strategies

### Research/Backtesting

Here is an example of discovering the current instruments for the Binance Futures testnet:
```python
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.factories import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


clock = LiveClock()
account_type = BinanceAccountType.FUTURES_USDT

client = get_cached_binance_http_client(
    loop=asyncio.get_event_loop(),
    clock=clock,
    logger=Logger(clock=clock),
    account_type=account_type,
    key=os.getenv("BINANCE_FUTURES_TESTNET_API_KEY"),
    secret=os.getenv("BINANCE_FUTURES_TESTNET_API_SECRET"),
    is_testnet=True,
)
await client.connect()

provider = BinanceFuturesInstrumentProvider(
    client=client,
    logger=Logger(clock=clock),
    account_type=BinanceAccountType.FUTURES_USDT,
)

await provider.load_all_async()
```

### Live Trading

Each integration is implementation specific, and there are generally two options for the behavior of an `InstrumentProvider` within a `TradingNode` for live trading,
as configured:

- All instruments are automatically loaded on start:

```python
InstrumentProviderConfig(load_all=True)
```

- Only those instruments explicitly specified in the configuration are loaded on start:

```python
InstrumentProviderConfig(load_ids=["BTCUSDT-PERP.FTX", "ETHUSDT-PERP.FTX"])
```

## Data Clients

### Requests

An `Actor` or `Strategy` can request custom data from a `DataClient` by sending a `DataRequest`. If the client that receives the 
`DataRequest` implements a handler for the request, data will be returned to the `Actor` or `Strategy`.

#### Example

An example of this is a `DataRequest` for an `Instrument`, which the `Actor` class implements (copied below). Any `Actor` or
`Strategy` can call a `request_instrument` method with an `InstrumentId` to request the instrument from a `DataClient`.

In this particular case, the `Actor` implements a separate method `request_instrument`. A similar type of 
`DataRequest` could be instantiated and called from anywhere and/or anytime in the actor/strategy code.

On the actor/strategy:

```cython
# nautilus_trader/common/actor.pyx

cpdef void request_instrument(self, InstrumentId instrument_id, ClientId client_id=None) except *:
    """
    Request `Instrument` data for the given instrument ID.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the request.
    client_id : ClientId, optional
        The specific client ID for the command.
        If ``None`` then will be inferred from the venue in the instrument ID.

    """
    Condition.not_none(instrument_id, "instrument_id")

    cdef DataRequest request = DataRequest(
        client_id=client_id,
        venue=instrument_id.venue,
        data_type=DataType(Instrument, metadata={
            "instrument_id": instrument_id,
        }),
        callback=self._handle_instrument_response,
        request_id=UUID4(),
        ts_init=self._clock.timestamp_ns(),
    )

    self._send_data_req(request)

```

The handler on the `ExecutionClient`:

```python
# nautilus_trader/adapters/binance/spot/data.py
def request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4):
    instrument: Optional[Instrument] = self._instrument_provider.find(instrument_id)
    if instrument is None:
        self._log.error(f"Cannot find instrument for {instrument_id}.")
        return

    data_type = DataType(
        type=Instrument,
        metadata={"instrument_id": instrument_id},
    )

    self._handle_data_response(
        data_type=data_type,
        data=[instrument],  # Data engine handles lists of instruments
        correlation_id=correlation_id,
    )

```
