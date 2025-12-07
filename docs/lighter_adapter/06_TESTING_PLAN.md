# 06_TESTING_PLAN.md

## Testing Plan

### Unit Tests

**Coverage Target**: &gt;80% for parsers, mappers, state machines

| Component | Test File | Focus Areas |
|-----------|-----------|-------------|
| Config | `test_config.py` | Validation, env var loading |
| Parsing | `test_parsing.py` | All message types, edge cases |
| Enums | `test_enums.py` | Bidirectional mapping |
| Nonce | `test_nonce.py` | Increment, persistence, recovery |
| Symbol | `test_symbol.py` | Normalization, round-trip |

**Example Unit Test**:
```python
def test_parse_order_book_delta():
    raw = {
        "asks": [{"price": "3327.46", "size": "29.0915"}],
        "bids": [{"price": "3326.00", "size": "0"}],  # Removal
        "offset": 12345
    }
    
    delta = parse_order_book_delta(raw, instrument_id)
    
    assert len(delta.asks) == 1
    assert delta.asks[0].price == Price.from_str("3327.46")
    assert len(delta.bids) == 1
    assert delta.bids[0].size == Quantity.from_int(0)  # Removal
```

### Contract Tests (API Schema)

**Goal**: Validate adapter handles actual API responses

```python
@pytest.mark.contract
async def test_orderbooks_response_schema():
    """Verify orderBooks endpoint response matches expected schema."""
    async with LighterHttpClient(testnet=True) as client:
        response = await client.get_order_books()
        
        # Validate structure
        assert "order_books" in response
        for market in response["order_books"]:
            assert "market_index" in market
            assert "supported_price_decimals" in market
            assert "supported_size_decimals" in market
            assert "min_base_amount" in market
```

### Integration Tests

**Environment**: Lighter Testnet (`https://testnet.zklighter.elliot.ai`)

| Test Case | Description |
|-----------|-------------|
| `test_load_instruments` | Load all instruments successfully |
| `test_subscribe_order_book` | Receive order book updates |
| `test_subscribe_trades` | Receive trade events |
| `test_submit_limit_order` | Place and verify limit order |
| `test_cancel_order` | Cancel and verify cancellation |
| `test_full_fill` | Order fills completely |
| `test_partial_fill` | Order fills partially |
| `test_account_balance` | Balance updates correctly |

### Failure Mode Tests

| Scenario | Test | Expected Behavior |
|----------|------|-------------------|
| WS disconnect | `test_ws_reconnect` | Auto-reconnect within 10s |
| Offset gap | `test_orderbook_gap` | Refetch snapshot |
| Rate limit | `test_rate_limit_backoff` | Exponential backoff |
| Auth expired | `test_auth_refresh` | Refresh and continue |
| Invalid nonce | `test_nonce_recovery` | Fetch and retry |
| Insufficient margin | `test_insufficient_margin` | Reject with reason |

**Disconnect Test Example**:
```python
async def test_ws_reconnect(lighter_client, mock_ws):
    await lighter_client.connect()
    await lighter_client.subscribe_order_book("BTCUSD-PERP")
    
    # Simulate disconnect
    await mock_ws.close()
    
    # Wait for reconnect
    await asyncio.sleep(5)
    
    assert lighter_client.is_connected
    assert "order_book/0" in lighter_client.subscriptions
```

### Performance Tests

| Metric | Target | Test Method |
|--------|--------|-------------|
| Message throughput | &gt;1000 msg/s | Send synthetic messages |
| Order latency | &lt;500ms (Premium) | Measure round-trip |
| Memory stability | No growth over 1hr | Monitor RSS |
| Reconnect time | &lt;10s | Force disconnect |

### CI Configuration

```yaml
# .github/workflows/lighter_tests.yml
name: Lighter Adapter Tests

on: [push, pull_request]

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
        with:
          python-version: '3.11'
      - run: pip install -e .[dev]
      - run: pytest tests/unit_tests/adapters/lighter -v --cov

  integration-tests:
    runs-on: ubuntu-latest
    if: github.event_name == 'push' && github.ref == 'refs/heads/main'
    env:
      LIGHTER_TESTNET_API_KEY_PRIVATE_KEY: ${{ secrets.LIGHTER_TESTNET_KEY }}
      LIGHTER_TESTNET_ACCOUNT_INDEX: ${{ secrets.LIGHTER_TESTNET_ACCOUNT }}
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-python@v5
      - run: pip install -e .[dev]
      - run: pytest tests/integration_tests/adapters/lighter -v --timeout=300
```

---