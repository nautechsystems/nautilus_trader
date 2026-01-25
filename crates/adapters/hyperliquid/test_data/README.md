# Hyperliquid Test Data

This directory contains real API response samples for testing.

## Files

### HTTP Public Data (No Account Required)

- `http_meta_perp_sample.json` - Perpetuals market metadata (sample of 3 markets)
- `http_meta_spot_sample.json` - Spot market metadata (sample of 3 markets)
- `http_l2_book_btc.json` - BTC order book snapshot (5 levels each side)
- `http_l2_book_snapshot.json` - Existing order book test data

### WebSocket Public Data (No Account Required)

- `ws_trades_sample.json` - Real-time trade message sample
- `ws_l2_book_sample.json` - Order book update message sample
- `ws_book_data.json` - Existing book data test sample

## Capturing New Test Data

### HTTP Data

```bash
cargo run --bin capture-test-data
```

### WebSocket Data

```bash
cargo run --bin capture-ws-test-data
```

## Usage in Tests

```rust
fn load_test_data<T>(filename: &str) -> T
where
    T: serde::de::DeserializeOwned,
{
    let path = format!("test_data/{}", filename);
    let content = std::fs::read_to_string(path).expect("Failed to read test data");
    serde_json::from_str(&content).expect("Failed to parse test data")
}

#[rstest]
fn test_parse_perpetuals_metadata() {
    let meta: PerpMetadata = load_test_data("http_meta_perp_sample.json");
    // assertions...
}
```

## Data Size Policy

- Keep files small (< 50KB each)
- Sample only 3-5 items from large arrays
- Use real mainnet data when possible
- Update files when API response format changes
