# Adapters

The NautilusTrader design integrates data providers and/or trading venues
through adapter implementations. These can be found in the top-level `adapters` subpackage.

An integration adapter is *typically* comprised of the following main components:

- `HttpClient`
- `WebSocketClient`
- `InstrumentProvider`
- `DataClient`
- `ExecutionClient`

## Instrument providers

Instrument providers do as their name suggests - instantiating Nautilus
`Instrument` objects by parsing the raw API of the publisher or venue.

The use cases for the instruments available from an `InstrumentProvider` are either:

- Used standalone to discover the instruments available for an integration, using these for research or backtesting purposes
- Used in a `sandbox` or `live` [environment context](architecture.md#environment-contexts) for consumption by actors/strategies

### Research and backtesting

Here is an example of discovering the current instruments for the Binance Futures testnet:

```python
import asyncio
import os

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance import get_cached_binance_http_client
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.common.component import LiveClock


clock = LiveClock()
account_type = BinanceAccountType.USDT_FUTURES

client = get_cached_binance_http_client(
    loop=asyncio.get_event_loop(),
    clock=clock,
    account_type=account_type,
    key=os.getenv("BINANCE_FUTURES_TESTNET_API_KEY"),
    secret=os.getenv("BINANCE_FUTURES_TESTNET_API_SECRET"),
    is_testnet=True,
)
await client.connect()

provider = BinanceFuturesInstrumentProvider(
    client=client,
    account_type=BinanceAccountType.USDT_FUTURES,
)

await provider.load_all_async()
```

### Live trading

Each integration is implementation specific, and there are generally two options for the behavior of an `InstrumentProvider` within a `TradingNode` for live trading,
as configured:

- All instruments are automatically loaded on start:

```python
from nautilus_trader.config import InstrumentProviderConfig

InstrumentProviderConfig(load_all=True)
```

- Only those instruments explicitly specified in the configuration are loaded on start:

```python
InstrumentProviderConfig(load_ids=["BTCUSDT-PERP.BINANCE", "ETHUSDT-PERP.BINANCE"])
```

## Data clients

### Requests

An `Actor` or `Strategy` can request custom data from a `DataClient` by sending a `DataRequest`. If the client that receives the
`DataRequest` implements a handler for the request, data will be returned to the `Actor` or `Strategy`.

#### Example

An example of this is a `DataRequest` for an `Instrument`, which the `Actor` class implements (copied below). Any `Actor` or
`Strategy` can call a `request_instrument` method with an `InstrumentId` to request the instrument from a `DataClient`.

In this particular case, the `Actor` implements a separate method `request_instrument`. A similar type of
`DataRequest` could be instantiated and called from anywhere and/or anytime in the actor/strategy code.

A simplified version of `request_instrument` for an actor/strategy is:

```python
# nautilus_trader/common/actor.pyx

cpdef void request_instrument(self, InstrumentId instrument_id, ClientId client_id=None):
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

    cdef RequestInstrument request = RequestInstrument(
        instrument_id=instrument_id,
        start=None,
        end=None,
        client_id=client_id,
        venue=instrument_id.venue,
        callback=self._handle_instrument_response,
        request_id=UUID4(),
        ts_init=self._clock.timestamp_ns(),
        params=None,
    )

    self._send_data_req(request)
```

A simplified version of the request handler implemented in a `LiveMarketDataClient` that will retrieve the data
and send it back to actors/strategies is for example:

```python
# nautilus_trader/live/data_client.py

def request_instrument(self, request: RequestInstrument) -> None:
    self.create_task(self._request_instrument(request))

# nautilus_trader/adapters/binance/data.py

async def _request_instrument(self, request: RequestInstrument) -> None:
    instrument: Instrument | None = self._instrument_provider.find(request.instrument_id)

    if instrument is None:
        self._log.error(f"Cannot find instrument for {request.instrument_id}")
        return

    self._handle_instrument(instrument, request.id, request.params)
```

The `DataEngine` which is an important component in Nautilus links a request with a `DataClient`.
For example a simplified version of handling an instrument request is:

```python
# nautilus_trader/data/engine.pyx

self._msgbus.register(endpoint="DataEngine.request", handler=self.request)

cpdef void request(self, RequestData request):
    self._handle_request(request)

cpdef void _handle_request(self, RequestData request):
    cdef DataClient client = self._clients.get(request.client_id)

    if client is None:
        client = self._routing_map.get(request.venue, self._default_client)

    if isinstance(request, RequestInstrument):
        self._handle_request_instrument(client, request)

cpdef void _handle_request_instrument(self, DataClient client, RequestInstrument request):
    client.request_instrument(request)
```
