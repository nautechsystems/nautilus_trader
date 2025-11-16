# dYdX Data Client - Testing Strategy

## Implementation Status

- [x] `request_instruments` - COMPLETE
- [x] `request_instrument` - COMPLETE
- [x] `request_trades` - COMPLETE
- [x] `request_bars` - COMPLETE (partitioning + Candle → Bar)

## Testing Checklist

### Unit Tests

#### request_instruments

- [x] Test successful fetch of all instruments ✅ (COMPILES - needs async runtime)
- [x] Test empty response handling ✅ (COMPILES - needs singleton fix)
- [x] Test instrument caching after fetch ✅ (PASSES)
- [x] Test correlation_id matching in response ✅ (COMPILES - needs async runtime)
- [x] Test error handling when HTTP call fails ✅ (COMPILES - needs singleton fix)
- [x] Verify InstrumentsResponse format ✅ (COMPILES)
- [x] Test venue assignment ✅ (COMPILES - needs async runtime)
- [x] Test timestamp handling (start_nanos, end_nanos) ✅ (COMPILES - needs singleton fix)
- [x] Test client_id fallback ✅ (PASSES)
- [x] Test params handling ✅ (COMPILES - needs singleton fix)

**Status**: 8/8 tests implemented, 2/8 passing, 6/8 compile but fail due to:

- Tokio runtime not available in sync tests (need #[tokio::test])
- Data event sender singleton can only be set once (need proper teardown)

#### request_instrument

- [x] Test cache hit (instrument already cached) ✅ (COMPILES - needs singleton fix)
- [x] Test cache miss (fetch from API) ✅ (COMPILES - needs singleton fix)
- [x] Test instrument not found scenario ✅ (COMPILES - needs singleton fix)
- [x] Test bulk caching when fetching from API ✅ (PASSES)
- [x] Test correlation_id matching ✅ (COMPILES - needs singleton fix)
- [x] Verify InstrumentResponse format (boxed) ✅ (COMPILES - needs singleton fix)
- [x] Test symbol extraction from InstrumentId ✅ (PASSES)
- [x] Test client_id fallback to default ✅ (COMPILES - needs singleton fix)

**Status**: 8/8 tests implemented, 2/8 passing, 6/8 compile but fail due to:

- Data event sender singleton can only be set once (need proper teardown)

#### request_trades

- [x] Test successful trade fetch with limit ✅ (PASSES)
- [x] Test timestamp filtering (start/end) ✅ (PASSES)
- [x] Test limit parameter handling ✅ (PASSES)
- [x] Test empty trades response ✅ (PASSES)
- [x] Test correlation_id matching ✅ (PASSES)
- [x] Verify TradesResponse format ✅ (PASSES)
- [x] Test trade parsing to TradeTick ✅ (PASSES - included in success test)
- [x] Test symbol conversion (strip -PERP suffix) ✅ (PASSES)
- [x] Test instrument not in cache ✅ (PASSES)

**Status**: 8/8 tests implemented, 8/8 PASSING ✅

### Integration Tests (Testnet)

- [ ] End-to-end: request_instruments → cache → verify all instruments
- [ ] End-to-end: request_instrument from cold cache (fetch from API)
- [ ] End-to-end: request_instrument from warm cache (instant lookup)
- [ ] End-to-end: request_trades with various date ranges
- [ ] Test all methods with actual dYdX testnet connection
- [ ] Verify data flows to DataEngine correctly via message bus
- [ ] Test concurrent requests (ensure thread safety)
- [ ] Test reconnection scenarios

### Error Coverage

#### HTTP Errors

- [x] HTTP 404 handling (instrument not found) - ✅ PASSING
- [x] HTTP 429 handling (rate limit exceeded) - ✅ PASSING
- [x] HTTP 500 handling (internal server error) - ✅ PASSING
- [x] HTTP 502/503 handling (bad gateway/service unavailable) - ✅ PASSING
- [x] Network timeout scenarios - ✅ PASSING
- [x] Connection refused errors - ✅ PASSING
- [x] DNS resolution failures - ✅ PASSING
- [x] Error handling without panics (all methods) - ✅ PASSING

**Status: 8/8 implemented, 8 PASSING ✅**

#### Parse Errors

- [x] Malformed JSON response - ✅ PASSING
- [x] Missing required fields - ✅ PASSING
- [x] Invalid data types - ✅ PASSING
- [x] Unexpected response structure - ✅ PASSING
- [x] Empty markets object (valid JSON, no data) - ✅ PASSING
- [x] Null values in critical fields - ✅ PASSING

**Status: 6/6 implemented, 6 PASSING ✅**

#### Validation Errors

- [x] Invalid instrument_id format (non-existent instrument) - ✅ PASSING
- [x] Invalid date range (end before start) - ✅ PASSING
- [x] Negative limit values (minimum valid limit = 1) - ✅ PASSING
- [x] Zero or empty limit (None = use API default) - ✅ PASSING
- [x] Very large limit values (boundary testing) - ✅ PASSING
- [x] None limit uses default behavior - ✅ PASSING
- [x] Validation edge cases don't panic - ✅ PASSING

**Status: 7/7 implemented, 7 PASSING ✅**

**Note**: Rust's type system (NonZeroUsize) prevents negative/zero limits at compile time.

### Response Format Verification

#### InstrumentsResponse

- [x] Has correct venue (DYDX) - ✅ PASSING
- [x] Contains Vec<InstrumentAny> - ✅ PASSING
- [x] Includes correlation_id (request_id) - ✅ PASSING
- [x] Includes client_id - ✅ PASSING
- [x] Includes start (UnixNanos) - ✅ PASSING
- [x] Includes end (UnixNanos) - ✅ PASSING
- [x] Includes ts_init - ✅ PASSING
- [x] Includes params (if provided) - ✅ PASSING
- [x] Complete structure validation - ✅ PASSING

**Status: 8/8 implemented, 8 PASSING ✅**

#### InstrumentResponse

- [x] Properly boxed (Box<InstrumentResponse>) - ✅ PASSING
- [x] Contains single InstrumentAny - ✅ PASSING
- [x] Has correct instrument_id - ✅ PASSING
- [x] Includes all metadata (correlation_id, timestamps, params) - ✅ PASSING
- [x] Matches requested instrument exactly - ✅ PASSING
- [x] Complete structure validation - ✅ PASSING

**Status: 6/6 implemented, 6 PASSING ✅**

#### TradesResponse

- [x] Contains Vec<TradeTick> - ✅ PASSING
- [x] Has correct instrument_id - ✅ PASSING
- [x] Trades are properly ordered (by timestamp) - ✅ PASSING
- [x] All TradeTick fields populated correctly - ✅ PASSING
- [x] Includes all metadata - ✅ PASSING

**Status: 5/5 implemented, 5 PASSING ✅**

### Parameter Combinations

#### request_instruments

- [x] No start/end (fetch all current instruments) - ✅ IMPLEMENTED (singleton issue)
- [x] With start only (all instruments since start) - ✅ IMPLEMENTED (singleton issue)
- [x] With end only (all instruments until end) - ✅ IMPLEMENTED (singleton issue)
- [x] With start and end range - ✅ IMPLEMENTED (singleton issue)
- [x] With custom params dict - ✅ IMPLEMENTED (singleton issue)
- [x] Different client_id values - ✅ IMPLEMENTED (singleton issue)
- [x] None vs Some(client_id) - ✅ IMPLEMENTED (passes)

**Status: 7/7 implemented (1 passes, 6 compile but fail due to singleton issue)**

#### request_instrument

- [ ] Valid instrument_id (cached) - instant return
- [ ] Valid instrument_id (not cached) - API fetch
- [ ] Invalid instrument_id (non-existent)
- [ ] Malformed instrument_id
- [ ] With start/end timestamps
- [ ] With custom params
- [ ] Different symbols (BTC-USD, ETH-USD, etc.)

#### request_trades

- [ ] No limit (default behavior)
- [ ] Limit = 100 (small dataset)
- [ ] Limit = 500 (medium dataset)
- [ ] Limit = 1000 (large dataset)
- [ ] Limit > 1000 (test API limits)
- [ ] With start only (recent trades)
- [ ] With end only (trades until date)
- [ ] With start and end (specific range)
- [ ] With future end date (should handle gracefully)
- [ ] With very old start date (may have no data)
- [ ] Different instruments (BTC-USD vs ETH-USD)

## Test Implementation Priority

### Phase 1: Critical Happy Paths (HIGH Priority)

- [ ] request_instruments: successful fetch
- [ ] request_instrument: cache hit
- [ ] request_instrument: cache miss + API fetch
- [ ] request_trades: successful fetch with limit

### Phase 2: Error Handling (HIGH Priority)

- [ ] HTTP error handling for all methods
- [ ] Empty response handling
- [ ] Instrument not found handling
- [ ] Parse error handling

### Phase 3: Integration Tests (MEDIUM Priority)

- [ ] Testnet end-to-end tests
- [ ] Message bus integration
- [ ] Cache verification
- [ ] Concurrent request handling

### Phase 4: Edge Cases (MEDIUM Priority)

- [ ] Parameter validation
- [ ] Boundary conditions
- [ ] Various parameter combinations
- [ ] Symbol format variations

### Phase 5: Performance & Stress (LOW Priority)

- [ ] Large dataset handling
- [ ] Cache performance with many instruments
- [ ] Concurrent request stress test
- [ ] Memory leak detection

## Test File Locations

```
tests/unit_tests/adapters/dydx/
├── test_data_request_instruments.py
├── test_data_request_instrument.py
└── test_data_request_trades.py

tests/integration_tests/adapters/dydx/
├── test_dydx_data_requests_live.py
└── test_dydx_cache_integration.py
```

## Example Test Patterns

### Unit Test Example (request_instrument cache hit)

```python
@pytest.mark.asyncio
async def test_request_instrument_cache_hit(dydx_data_client, btc_instrument):
    # Pre-populate cache
    dydx_data_client._cache.add_instrument(btc_instrument)

    # Request instrument
    request = RequestInstrument(
        client_id=dydx_data_client.id,
        venue=Venue("DYDX"),
        instrument_id=btc_instrument.id,
        request_id=UUID4(),
    )

    dydx_data_client.request_instrument(request)

    # Should not call HTTP (already cached)
    # Verify response sent to message bus
    # Assert correlation_id matches
```

### Integration Test Example

```python
@pytest.mark.integration
@pytest.mark.asyncio
async def test_request_trades_testnet(dydx_testnet_client):
    # Request last 100 trades
    request = RequestTrades(
        client_id=dydx_testnet_client.id,
        venue=Venue("DYDX"),
        instrument_id=InstrumentId.from_str("BTC-USD.DYDX"),
        limit=100,
        request_id=UUID4(),
    )

    dydx_testnet_client.request_trades(request)

    # Wait for response
    await asyncio.sleep(1.0)

    # Verify trades received
    # Check trade format
    # Validate timestamps
```

## Success Criteria

- [ ] All unit tests pass (100% of implemented tests)
- [ ] Integration tests pass against testnet
- [ ] Error handling tested for all failure modes
- [ ] Response formats validated
- [ ] Cache behavior verified
- [ ] No memory leaks detected
- [ ] Concurrent access safe
- [ ] Documentation complete

## Notes

- Use `pytest.mark.asyncio` for async tests
- Mock HTTP client for unit tests
- Use actual testnet for integration tests
- Set `DYDX_TESTNET_INTEGRATION=1` for integration tests
- Tests should be independent (no shared state)
- Clean up cache between tests
- Use fixtures for common setup
