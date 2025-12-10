# 02_TECHNICAL_DISCOVERY_Nautilus.md

## Nautilus Trader Adapter Architecture Deep Dive

### Existing Adapters Overview

| Adapter | Type | Status | Best For Reference |
|---------|------|--------|-------------------|
| **dYdX** | Perp DEX | beta | DEX auth patterns, `CryptoPerpetual` |
| **Hyperliquid** | Perp DEX | beta | Newest DEX implementation |
| **Bybit** | CEX | stable | Mature WS patterns |
| **OKX** | CEX | beta | Rust layer patterns |
| **BitMEX** | CEX | beta | Test surface patterns |

**Recommended Primary Analog**: **dYdX** — Pure perpetual DEX, wallet-based auth, funding rate streams

**Secondary Reference**: **Hyperliquid** — Most recent DEX adapter (v1.220.0), similar architecture

**Implementation Approach**: Rust-first adapter crate with PyO3 bindings; Python layer is kept thin
(configs/factories/tests). Avoid new Python networking deps unless proven necessary.

### Adapter Surface Area Components

```
┌─────────────────────────────────────────────────────────┐
│                   NAUTILUS ADAPTER                       │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────────────────┐   │
│  │ InstrumentProvider│  │ LiveMarketDataClient       │   │
│  │ - load_all_async  │  │ - _subscribe_order_book_*  │   │
│  │ - load_async      │  │ - _subscribe_trade_ticks   │   │
│  │ - get_all         │  │ - _subscribe_mark_prices   │   │
│  └─────────────────┘  │ - _subscribe_funding_rates  │   │
│                        └─────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────┐    │
│  │            LiveExecutionClient                   │    │
│  │ - _submit_order      - generate_order_status_*   │    │
│  │ - _cancel_order      - generate_fill_reports     │    │
│  │ - _modify_order      - generate_position_status_*│    │
│  └─────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────┘
```

### Required Base Classes

| Class | File Path | Purpose |
|-------|-----------|---------|
| `InstrumentProvider` | `nautilus_trader/common/providers.py` | Instrument loading |
| `LiveMarketDataClient` | `nautilus_trader/live/data_client.pyx` | Market data subscriptions |
| `LiveExecutionClient` | `nautilus_trader/live/execution_client.pyx` | Order execution |
| `LiveDataClientConfig` | `nautilus_trader/config.py` | Data client config |
| `LiveExecClientConfig` | `nautilus_trader/config.py` | Execution client config |

### InstrumentProvider Interface

```python
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId

class LighterInstrumentProvider(InstrumentProvider):
    async def load_all_async(self, filters: dict | None = None) -> None:
        """Load all Lighter perpetual instruments."""
        
    async def load_ids_async(
        self, 
        instrument_ids: list[InstrumentId],
        filters: dict | None = None
    ) -> None:
        """Load specific instruments by ID."""
        
    async def load_async(
        self, 
        instrument_id: InstrumentId,
        filters: dict | None = None
    ) -> None:
        """Load single instrument."""
```

### LiveMarketDataClient Required Methods

```python
from nautilus_trader.live.data_client import LiveMarketDataClient

class LighterDataClient(LiveMarketDataClient):
    # Connection lifecycle
    async def _connect(self) -> None: ...
    async def _disconnect(self) -> None: ...
    
    # Order book subscriptions
    async def _subscribe_order_book_deltas(self, command) -> None: ...
    async def _subscribe_order_book_snapshots(self, command) -> None: ...
    async def _unsubscribe_order_book_deltas(self, command) -> None: ...
    
    # Trade subscriptions
    async def _subscribe_trade_ticks(self, command) -> None: ...
    async def _unsubscribe_trade_ticks(self, command) -> None: ...
    
    # Perp-specific subscriptions
    async def _subscribe_mark_prices(self, command) -> None: ...
    async def _subscribe_funding_rates(self, command) -> None: ...
    async def _subscribe_index_prices(self, command) -> None: ...
    
    # Historical requests
    async def _request_bars(self, request) -> None: ...
    async def _request_order_book_snapshot(self, request) -> None: ...
```

### LiveExecutionClient Required Methods

```python
from nautilus_trader.live.execution_client import LiveExecutionClient

class LighterExecutionClient(LiveExecutionClient):
    # Connection
    async def _connect(self) -> None: ...
    async def _disconnect(self) -> None: ...
    
    # Order management
    async def _submit_order(self, command: SubmitOrder) -> None: ...
    async def _cancel_order(self, command: CancelOrder) -> None: ...
    async def _cancel_all_orders(self, command: CancelAllOrders) -> None: ...
    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None: ...
    
    # Reconciliation reports
    async def generate_order_status_report(
        self, command
    ) -> OrderStatusReport | None: ...
    
    async def generate_order_status_reports(
        self, command
    ) -> list[OrderStatusReport]: ...
    
    async def generate_fill_reports(
        self, command
    ) -> list[FillReport]: ...
    
    async def generate_position_status_reports(
        self, command
    ) -> list[PositionStatusReport]: ...
```

### Recommended Module Layout

```
nautilus_trader/adapters/lighter/
├── __init__.py              # Re-exports: LIGHTER, configs, factories
├── config.py                # LighterDataClientConfig, LighterExecClientConfig
├── constants.py             # LIGHTER venue ID, enums
├── data.py                  # LighterDataClient
├── execution.py             # LighterExecutionClient
├── factories.py             # LighterLiveDataClientFactory, LighterLiveExecClientFactory
├── providers.py             # LighterInstrumentProvider
├── http/
│   ├── __init__.py
│   ├── client.py           # LighterHttpClient
│   ├── endpoints.py        # Endpoint definitions
│   └── errors.py           # HTTP error handling
├── websocket/
│   ├── __init__.py
│   ├── client.py           # LighterWebSocketClient
│   ├── messages.py         # Message type definitions
│   └── handlers.py         # Message handlers
└── common/
    ├── __init__.py
    ├── enums.py            # LighterOrderType, LighterTimeInForce, etc.
    ├── parsing.py          # Venue-to-Nautilus type conversion
    ├── credentials.py      # API key + signing utilities
    └── symbol.py           # Symbol normalization
```

### Configuration Pattern

```python
from nautilus_trader.config import LiveDataClientConfig, LiveExecClientConfig

class LighterDataClientConfig(LiveDataClientConfig):
    """Configuration for Lighter data client."""
    
    # Credentials (can use env vars: LIGHTER_API_KEY_PRIVATE_KEY)
    api_key_private_key: str | None = None
    account_index: int | None = None
    api_key_index: int = 2  # Default to first user key slot
    
    # Endpoints
    base_url_http: str | None = None
    base_url_ws: str | None = None
    
    # Environment
    testnet: bool = False
    
    # Adapter options
    update_instrument_interval_ms: int = 3600000  # 1 hour

class LighterExecClientConfig(LiveExecClientConfig):
    """Configuration for Lighter execution client."""
    
    api_key_private_key: str | None = None
    account_index: int | None = None
    api_key_index: int = 2
    
    base_url_http: str | None = None
    base_url_ws: str | None = None
    
    testnet: bool = False
    
    # Execution options
    max_retries: int = 3
    retry_delay_ms: int = 1000
```

### Testing Patterns

**Unit Tests**: Use `pytest` with fixtures for parsing/mapping logic

```python
# tests/unit_tests/adapters/lighter/test_parsing.py
def test_parse_order_book_update():
    raw = {"asks": [{"price": "100.00", "size": "1.5"}], ...}
    result = parse_order_book_update(raw)
    assert result.asks[0].price == Decimal("100.00")
```

**Integration Tests**: Use Axum mock servers (Rust) or `aioresponses` (Python)

```python
# tests/integration_tests/adapters/lighter/test_http_client.py
@pytest.fixture
def mock_lighter_api():
    with aioresponses() as m:
        m.get(f"{BASE_URL}/api/v1/orderBooks", payload={...})
        yield m
```

**Test Data Convention**:

```
tests/test_data/lighter/
├── http_get_orderbooks.json
├── http_post_sendtx.json
├── ws_order_book_update.json
├── ws_trade_update.json
└── ws_account_orders_update.json
```

**Fixture-first**: After the validation spike, prefer recorded HTTP/WS fixtures for both Rust and
Python tests to avoid brittle live dependencies.

---
