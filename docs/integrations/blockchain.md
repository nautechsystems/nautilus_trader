# Blockchain

## Core Primitives

Nautilus Trader's blockchain integration is built on foundational primitives defined in the DeFi domain model (`nautilus_model::defi`). These building blocks provide type-safe abstractions for working with EVM-based blockchains.

### Chain

The `Chain` struct represents a blockchain network with its connection endpoints and metadata. Each chain instance contains:

**Fields:**

- `name` (`Blockchain`): The blockchain network type (enum of 80+ supported chains)
- `chain_id` (`u32`): Unique EVM chain identifier (e.g., 1 for Ethereum, 42161 for Arbitrum)
- `hypersync_url` (`String`): Endpoint for high-performance Hypersync data streaming
- `rpc_url` (`Option<String>`): Optional HTTP/WSS RPC endpoint for direct node communication
- `native_currency_decimals` (`u8`): Decimal precision for the chain's native gas token (typically 18)

**Chain Retrieval:**

Chains can be retrieved by numeric ID or string name (case-insensitive):

- **By Chain ID:** Lookup using EVM chain identifier with `from_chain_id`
- **By Name:** Lookup using blockchain name (case-insensitive: "ethereum", "Ethereum", "ETHEREUM" all work) with `from_chain_name`
- **Static Instances:** Pre-configured chains available as constants

Each chain has a native currency used for gas fees. The `native_currency()` method returns a properly configured Currency instance:

| Chain Family                                    | Code | Name         | Decimals |
|-------------------------------------------------|------|--------------|----------|
| Ethereum & L2s (Arbitrum, Base, Optimism, etc.) | ETH  | Ethereum     | 18       |
| Polygon                                         | POL  | Polygon      | 18       |
| Avalanche                                       | AVAX | Avalanche    | 18       |
| BSC                                             | BNB  | Binance Coin | 18       |

## Contracts

High-performance interface for querying EVM smart contracts with type-safe Rust abstractions. Supports token metadata, DEX pools, and DeFi protocols through efficient batch operations.

### Base (Multicall3)

Batches multiple contract calls into a single RPC request using Multicall3 (`0xcA11bde05977b3631167028862bE2a173976CA11`).

- Always uses `allow_failure: true` for partial success and detailed errors
- Executes atomically in the same block
- Errors: `RpcError` (network issues), `AbiDecodingError` (decode failures)

### ERC20

Inherits from `BaseContract` to leverage Multicall3 for efficient batch operations. Fetches token metadata with robust handling for non-standard implementations.

**Methods:**

- `fetch_token_info`: Single token metadata (uses multicall internally for name, symbol, decimals)
- `batch_fetch_token_info`: Multiple tokens in one multicall (3 calls per token)
- `enforce_token_fields`: Validate non-empty name/symbol

**Error Types:**

1. **`CallFailed`** - Contract missing or function not implemented → Skip token
2. **`DecodingError`** - Raw bytes instead of ABI encoding (e.g., `0x5269636f...`) → Skip token
3. **`EmptyTokenField`** - Function returns empty string → Skip if enforced

**Best Practices:**

- Skip pools with any token errors
- `raw_data` field preserves original response for debugging
- Non-standard tokens often have other issues (transfer fees, rebasing)

## Configuration

| Option                          | Default | Description |
|---------------------------------|---------|-------------|
| `chain`                         | Required | `nautilus_trader.model.Chain` to synchronize (e.g., `Chain.ETHEREUM`). |
| `dex_ids`                       | Required | Sequence of `DexType` identifiers describing which DEX integrations to enable. |
| `http_rpc_url`                  | Required | HTTPS RPC endpoint used for EVM calls and Multicall requests. |
| `wss_rpc_url`                   | `None`  | Optional WSS endpoint for streaming live updates. |
| `rpc_requests_per_second`       | `None`  | Optional throttle for outbound RPC calls (requests per second). |
| `multicall_calls_per_rpc_request` | `100` | Maximum number of Multicall targets batched per RPC request. |
| `use_hypersync_for_live_data`   | `True`  | When `True`, bootstrap and stream using Hypersync for lower-latency diffs. |
| `from_block`                    | `None`  | Optional starting block height for historical backfill. |
| `pool_filters`                  | `DexPoolFilters()` | Filtering rules applied when selecting DEX pools to monitor. |
| `postgres_cache_database_config`| `None`  | Optional `PostgresConnectOptions` enabling on-disk caching of decoded pool state. |
| `http_proxy_url`                | `None`  | Optional HTTP proxy URL for RPC requests. |
| `ws_proxy_url`                  | `None`  | Optional WebSocket proxy URL for RPC connections. |
