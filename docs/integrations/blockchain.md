# Blockchain

## Overview

The blockchain adapter ingests DeFi data from EVM chains and exposes it through the
NautilusTrader data model. It combines three services:

- HyperSync for high-throughput historical blocks and contract logs.
- HTTP RPC for contract calls, Multicall reads, and final on-chain state hydration.
- Postgres for optional durable cache state, pool metadata, decoded events, and snapshots.

HyperSync and RPC serve different roles. HyperSync is the fast event source. HTTP RPC remains the
source of truth for current contract state, including Uniswap V3 slot state, active ticks, and
positions.

## Core primitives

The DeFi domain model lives in `nautilus_model::defi`.

### Chain

`Chain` defines the target blockchain and its default service endpoints.

| Field                       | Type         | Description                                                        |
|-----------------------------|--------------|--------------------------------------------------------------------|
| `name`                      | `Blockchain` | Chain enum value, such as `Ethereum` or `Arbitrum`.                |
| `chain_id`                  | `u32`        | EVM chain ID, such as `1` for Ethereum.                            |
| `hypersync_url`             | `String`     | HyperSync endpoint, by default `https://{chain_id}.hypersync.xyz`. |
| `rpc_url`                   | `Option`     | Optional direct RPC endpoint stored on the chain model.            |
| `native_currency_decimals`  | `u8`         | Native gas token decimal precision, usually `18`.                  |

Chains can be loaded by numeric ID with `Chain::from_chain_id` or by name with
`Chain::from_chain_name`.

| Chain family                                    | Code | Name         | Decimals |
|-------------------------------------------------|------|--------------|----------|
| Ethereum and L2s                                | ETH  | Ethereum     | 18       |
| Polygon                                         | POL  | Polygon      | 18       |
| Avalanche                                       | AVAX | Avalanche    | 18       |
| BSC                                             | BNB  | Binance Coin | 18       |

### DEX and pools

DEX integrations register factory addresses, event signatures, parser functions, and AMM type.
Pool definitions bind a chain, DEX, pool contract, token pair, fee tier, tick spacing, and creation
block into a stable Nautilus instrument ID.

Uniswap V3 and compatible concentrated-liquidity pools also use:

- `Initialize(uint160,int24)` for initial price state.
- `Mint` and `Burn` events for position and tick state replay.
- `Swap` events for live pool price movement.
- HTTP RPC final-state reads for `slot0`, liquidity, active ticks, and position data.

## Configuration

| Option                            | Default            | Description                                            |
|-----------------------------------|--------------------|--------------------------------------------------------|
| `chain`                           | Required           | Target `Chain`, such as Ethereum or Arbitrum.          |
| `dex_ids`                         | `[]`               | DEX integrations to register and sync.                 |
| `http_rpc_url`                    | Required           | HTTP RPC endpoint for contract reads and Multicall.    |
| `wss_rpc_url`                     | `None`             | Optional WSS RPC endpoint for RPC live streams.        |
| `rpc_requests_per_second`         | `None`             | Optional RPC request throttle.                         |
| `multicall_calls_per_rpc_request` | `200`              | Requested maximum Multicall targets per RPC request.   |
| `use_hypersync_for_live_data`     | `false` in Rust    | When true, live block and event streams use HyperSync. |
| `from_block`                      | `None`             | Optional start block for historical sync.              |
| `pool_filters`                    | `DexPoolFilters()` | Pool universe filtering rules.                         |
| `postgres_cache_database_config`  | `None`             | Optional Postgres cache configuration.                 |
| `proxy_url`                       | `None`             | Optional HTTP and WebSocket proxy URL.                 |
| `transport_backend`               | `Tungstenite`      | WebSocket transport backend.                           |

:::note
Pool snapshot requests currently require a Postgres cache database. The in-memory cache can hold
tokens and pools, but latest pool profiler bootstrap reads snapshot and event state through the
cache database path.
:::

## Environment

Set the HyperSync token and RPC URLs outside the repository. Do not commit `.env` files containing
secrets.

```fish
set -x ENVIO_API_TOKEN "<envio-token>"
set -x RPC_HTTP_URL "https://your-rpc.example"
set -x RPC_WSS_URL "wss://your-rpc.example"
```

For local `.env` usage:

```dotenv
ENVIO_API_TOKEN=<envio-token>
RPC_HTTP_URL=https://your-rpc.example
RPC_WSS_URL=wss://your-rpc.example
```

`ENVIO_API_TOKEN` is required by the Rust HyperSync client. Missing or malformed tokens fail client
construction before any query is sent.

## Local services

The development compose file starts Postgres, Redis, and pgAdmin.

```fish
make start-services
make init-db
```

The default Postgres service listens on `127.0.0.1:5432` with database `nautilus`, user
`nautilus`, and password `pass`.

Check that the schema exists:

```fish
docker exec nautilus-database psql -U nautilus -d nautilus -Atc \
    "select count(*) from information_schema.tables where table_schema='public'"
```

For destructive DeFi test runs, use a separate database or resettable Docker volume. Pool discovery
and snapshot tests can write many rows to `token`, `pool`, `pool_*_event`, `pool_snapshot`,
`pool_position`, and `pool_tick`.

## Data flow

### Pool discovery

Pool discovery streams DEX factory events from HyperSync, fetches ERC-20 metadata through RPC, and
stores valid tokens and pools in the cache. Pools with invalid or empty token metadata can be
filtered out through `DexPoolFilters`.

### Live data

When `use_hypersync_for_live_data` is true, the adapter subscribes to blocks through HyperSync and
then fetches matching DEX contract events for subscribed pools. When false, WSS RPC is used where a
streaming implementation exists.

### Snapshot bootstrap

For Uniswap V3 snapshots, bootstrap uses a two-stage process:

- Replay historical Initialize, Mint, and Burn events from HyperSync to rebuild ticks and
  positions.
- Fetch the final on-chain state through HTTP RPC and Multicall, then restore the profiler from
  that snapshot.

If final RPC hydration fails, the adapter must fail closed. It must not emit a snapshot built from
replayed events with stale price state.

## Contracts

### Base contract and Multicall3

`BaseContract` batches contract calls through Multicall3 at
`0xcA11bde05977b3631167028862bE2a173976CA11`.

- Calls use `allow_failure: true` so individual contract call failures can be reported.
- Reads execute against a single block context.
- Transport and provider failures surface as RPC errors.

### ERC-20 metadata

`Erc20Contract` reads `name`, `symbol`, and `decimals` through Multicall. Non-standard token
contracts may return malformed strings, raw bytes, or empty fields. The adapter can skip pools with
tokens that fail metadata validation.

### Uniswap V3 pools

`UniswapV3PoolContract` reads global pool state, active ticks, and positions. Large pools can exceed
provider limits if too many ticks or positions are packed into a single RPC call. The current safety
behavior is fail-closed on hydration failure; successful delivery for very large pools depends on
provider limits or future chunked/minimal hydration work.

## Smoke tests

### HyperSync authentication

```fish
curl -fsS --max-time 15 \
    -H "Authorization: Bearer $ENVIO_API_TOKEN" \
    https://1.hypersync.xyz/height
```

Expected result: JSON with a numeric `height`.

### Small HyperSync query

```fish
set query (string join '' \
    '{"from_block":25170900,' \
    '"to_block":25170901,' \
    '"include_all_blocks":true,' \
    '"field_selection":{"block":["number","timestamp","hash"]}}')

curl -sS --max-time 30 \
    -H "Authorization: Bearer $ENVIO_API_TOKEN" \
    -H "Content-Type: application/json" \
    --data "$query" \
    https://1.hypersync.xyz/query/arrow-ipc \
    -o /dev/null \
    -w "http_code=%{http_code} size_download=%{size_download}\n"
```

Expected result: HTTP `200` with a non-zero response size.

### Adapter compile check

```fish
cargo check -p nautilus-blockchain --features hypersync
```

### Live fail-closed regression

This ignored test uses real HyperSync replay for the Ethereum WETH/USDT Uniswap V3 pool and a
deliberately invalid local HTTP RPC URL. It verifies that final RPC hydration failure returns an
error instead of allowing a stale snapshot through the construction path.

```fish
cargo test -p nautilus-blockchain --features hypersync \
    live_hypersync_bootstrap_fails_closed_when_rpc_hydration_fails \
    -- --ignored --nocapture
```

Expected result: one ignored test passes. On a live network this can take several minutes.

## Operational notes

- Use HyperSync for high-volume historical log scans.
- Use HTTP RPC for final contract state and validation.
- Use a paid or high-limit RPC provider for large Uniswap V3 pools.
- Keep `ENVIO_API_TOKEN`, RPC keys, and Postgres credentials outside version control.
- Use a separate Postgres database for repeatable DeFi test runs that write pool snapshots.
- Treat failed final-state hydration as a hard failure for emitted snapshots.

## Current limitations

- Very large Uniswap V3 pools can still hit provider payload, timeout, or rate limits during
  final-state Multicall hydration.
- `multicall_calls_per_rpc_request` documents the intended batching limit, but some final snapshot
  paths still need chunking hardening.
- A full successful WETH/USDT or WETH/USDC delivery test needs a real HTTP RPC provider that can
  serve the final-state reads, or the adapter needs minimal/chunked hydration first.
