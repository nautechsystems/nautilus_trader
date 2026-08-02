# Blockchain

## Overview

The blockchain adapter ingests DeFi data from EVM chains and exposes it through the
NautilusTrader data model. An execution client for locally signed Uniswap V3 swaps is in active
development (see [Execution](#execution)). The adapter uses three backends:

- HyperSync: high-throughput historical blocks and contract logs. See the
  [Envio HyperSync docs](https://docs.envio.dev/docs/HyperSync/hypersync-usage) for query shape,
  pagination, and tuning.
- HTTP RPC: contract calls, Multicall reads, and final on-chain state hydration.
- Postgres: optional durable cache state, pool metadata, decoded events, and snapshots.

## Core primitives

The DeFi domain model lives in `nautilus_model::defi`.

### Chain

`Chain` defines the target blockchain and its default service endpoints.

| Field                      | Type         | Description                                                        |
| -------------------------- | ------------ | ------------------------------------------------------------------ |
| `name`                     | `Blockchain` | Chain enum value, such as `Ethereum` or `Arbitrum`.                |
| `chain_id`                 | `u32`        | EVM chain ID, such as `1` for Ethereum.                            |
| `hypersync_url`            | `String`     | HyperSync endpoint, by default `https://{chain_id}.hypersync.xyz`. |
| `rpc_url`                  | `Option`     | Optional direct RPC endpoint stored on the chain model.            |
| `native_currency_decimals` | `u8`         | Native gas token decimal precision, usually `18`.                  |

Chains can be loaded by numeric ID with `Chain::from_chain_id` or by name with
`Chain::from_chain_name`.

| Chain family     | Code | Name         | Decimals |
| ---------------- | ---- | ------------ | -------- |
| Ethereum and L2s | ETH  | Ethereum     | 18       |
| Polygon          | POL  | Polygon      | 18       |
| Avalanche        | AVAX | Avalanche    | 18       |
| BSC              | BNB  | Binance Coin | 18       |

### DEX and pools

DEX integrations register:

- Factory addresses.
- Event signatures and parser functions.
- AMM type.

Pool definitions bind the chain and DEX to a pool contract address or protocol pool ID to form a
stable Nautilus instrument ID. The token pair, fee tier, tick spacing, and creation block remain
pool metadata.

When the data engine processes a pool definition, it caches and publishes a `CurrencyPair` under
the same pool instrument ID. The instrument keeps the raw pool `token0`/`token1` order as base/quote,
derives price and size precision from token decimals up to `FIXED_PRECISION`, and exposes the fee
tier divided by 1,000,000 as `taker_fee`. Distinct pool identifiers let same‑token pools coexist in
the cache and on the message bus.

Uniswap V3 and compatible concentrated-liquidity pools also use:

- `Initialize(uint160,int24)` for initial price state.
- `Mint` and `Burn` events for position and tick state replay.
- `Swap` events for live pool price movement.
- HTTP RPC final-state reads for `slot0`, liquidity, active ticks, and position data.

## Configuration

| Option                            | Default            | Description                                            |
| --------------------------------- | ------------------ | ------------------------------------------------------ |
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

Set credentials outside the repository:

```bash
export ENVIO_API_TOKEN="<envio-token>"
export RPC_HTTP_URL="https://your-rpc.example"
export RPC_WSS_URL="wss://your-rpc.example"
```

For local `.env` usage, keep the file out of version control:

```dotenv
ENVIO_API_TOKEN=<envio-token>
RPC_HTTP_URL=https://your-rpc.example
RPC_WSS_URL=wss://your-rpc.example
```

- `ENVIO_API_TOKEN` is required by the Rust HyperSync client. Missing or malformed tokens fail
  client construction before any query is sent.
- `RPC_HTTP_URL` or `--rpc-url` is required for contract reads and snapshot hydration.
- `RPC_WSS_URL` is only needed for WSS RPC live streams.

Execution adds further variables (see [Execution](#execution)):

- The signer private key is read from the variable named by the `signer_private_key_env`
  configuration field, never from configuration directly.
- `BLOCKCHAIN_FORK_TESTS=1` and `BLOCKCHAIN_FORK_RPC_URL` gate the pinned‑block Anvil integration
  suite, which points Anvil's `--fork-url` at a read‑only Arbitrum RPC and sends signed
  transactions to localhost only.

For token setup and quota details, see Envio's
[HyperSync API token docs](https://docs.envio.dev/docs/HyperSync/api-tokens).

### RPC endpoints

`RPC_HTTP_URL` or `--rpc-url` must point at an EVM JSON-RPC endpoint for the target chain.
The data client resolves it at construction, and first-time pool syncs read on-chain state through it.
The HyperSync endpoint is derived from the chain ID (`https://{chain_id}.hypersync.xyz`).

Verified free public HTTP endpoints (June 2026, no API key):

| Chain        | HTTP endpoint                          | Archive |
| ------------ | -------------------------------------- | ------- |
| Arbitrum One | `https://arb1.arbitrum.io/rpc`         | No      |
| Arbitrum One | `https://arbitrum.gateway.tenderly.co` | Yes     |
| Ethereum     | `https://ethereum-rpc.publicnode.com`  | No      |

Free archive endpoints exist, but availability and limits change. Snapshot validation usually needs
only a small number of `eth_call`s per pool, so a free archive endpoint can be enough to get
`validation_state = on_chain`.

Archive support affects validation, not whether event sync runs:

- On an archive node, a historical-block snapshot validates against on-chain state and is stored with
  `validation_state = on_chain`.
- On a non-archive node, the historical read fails and the snapshot stays `validation_state = replay`,
  which is still usable as a replay start point.
- A first-time sync on a non-archive node must run to a recent `--to-block`, because non-archive
  nodes only serve recent state and bootstrap reads on-chain state at the target block.

For other chains or archive access, use a directory such as [chainlist.org](https://chainlist.org) or
[comparenodes.com](https://www.comparenodes.com), or a keyed provider (Infura, Alchemy, dRPC).

## Local services

The development compose file starts Postgres, Redis, and pgAdmin.

```bash
make start-services
make init-db
```

Default Postgres connection:

- Host: `127.0.0.1:5432`
- Database: `nautilus`
- User: `nautilus`
- Password: `pass`

Check that the schema exists:

```bash
docker exec nautilus-database psql -U nautilus -d nautilus -Atc \
    "select count(*) from information_schema.tables where table_schema='public'"
```

For destructive DeFi tests, use a separate database or resettable Docker volume. Pool discovery and
snapshot tests can write many rows to `token`, `pool`, `pool_*_event`, `pool_snapshot`,
`pool_position`, and `pool_tick`.

## Data flow

### Architecture

`sync-dex` discovers pools and tokens once. `analyze-pool(s)` then generates `pool_snapshot` rows.
The diagram shows the default replay path and the `--snapshot-from-rpc` path.

```mermaid
flowchart TD
    HS["HyperSync (Envio): logs and events"]
    RPC["HTTP RPC + Multicall3: on-chain reads"]
    PG[("Postgres cache")]

    subgraph discovery["sync-dex (one-time discovery)"]
        direction TB
        D1["Stream factory PoolCreated logs"]
        D2["Fetch ERC-20 token metadata"]
        D3["Write pool and token rows"]
        D1 --> D2 --> D3
    end

    subgraph analyze["analyze-pool(s) (snapshot generation, one task per pool)"]
        direction TB
        AP0{"Mode"}
        AP1["Default: sync full pool events"]
        AP2["Bootstrap from cache snapshot, replay events"]
        AP3["extract_snapshot per --checkpoint-blocks"]
        AP4["Persist snapshot + ticks + positions"]
        AP5{"check_snapshot_validity"}
        RP1["--snapshot-from-rpc: stream state events"]
        RP2["Hydrate checkpoint from RPC"]
        RP3["Persist snapshot + ticks + positions"]
        AP0 --> AP1 --> AP2 --> AP3 --> AP4 --> AP5
        AP0 --> RP1 --> RP2 --> RP3
        AP5 -->|"matches chain"| V1["validation_state = on_chain"]
        AP5 -->|"RPC cannot reach block, or --skip-validation"| V2["validation_state = replay"]
        AP5 -->|"structural mismatch"| V3["validation_state = invalid"]
        RP3 -->|"validated from RPC"| V1
    end

    R["Backtest replay: load latest usable snapshot (not invalid), replay forward"]

    HS --> D1
    RPC --> D2
    D3 --> PG
    HS --> AP1
    HS --> RP1
    PG --> AP2
    AP4 --> PG
    RP3 --> PG
    RPC --> AP5
    RPC --> RP2
    PG --> R
```

`analyze-pools` runs one task per pool, bounded by `--concurrency`. Each task owns its data client.
A snapshot is usable as a replay start point unless its `validation_state` is `invalid`.

### Pool discovery

Pool discovery:

- Streams DEX factory events from HyperSync.
- Fetches ERC-20 metadata through RPC.
- Stores valid tokens and pools in the cache.
- Can skip pools with invalid or empty token metadata via `DexPoolFilters`.

### Live data

- `use_hypersync_for_live_data = true`: subscribe to blocks through HyperSync for live timestamps
  and hold one open-ended HyperSync DEX-event stream per subscribed DEX filter.
- `use_hypersync_for_live_data = false`: use WSS RPC block and pool-log subscriptions for live
  swaps, liquidity updates, fee collections, flash events, and fee-protocol events.

### Snapshot bootstrap

For Uniswap V3-compatible snapshots, bootstrap:

- Replay historical Initialize, Mint, and Burn events from HyperSync to rebuild ticks and
  positions.
- Fetch the final on-chain state through HTTP RPC and Multicall, then restore the profiler from
  that snapshot.

Bootstrap modes:

- Default: store the full pool event history up to the target block, then bootstrap from the
  database.
- `--snapshot-from-rpc`: skip full swap storage, stream Initialize, Mint, Burn, SetFeeProtocol, and
  CollectProtocol events from HyperSync to enumerate ticks and positions, then hydrate the exact
  checkpoint block from RPC.

Use `--snapshot-from-rpc` for old high-volume pools when the required output is the final snapshot,
not a stored swap history. It cannot be combined with `--from-block`, `--reset`, or
`--require-existing-snapshot`.

If final RPC hydration fails, the adapter must fail closed. It must not emit a snapshot built from
replayed events with stale price state.

### Snapshot validation

Before marking a snapshot valid, bootstrap compares the replayed profiler against on-chain state.
These structural fields must match exactly:

- Current tick.
- Active liquidity.
- Per-tick net and gross liquidity.
- Position liquidity.

A structural mismatch fails closed and the snapshot is not marked valid.

Non-structural mismatches are accepted with a warning:

- Sqrt price, which differs when replay is event-scoped but the RPC snapshot is block-scoped.
- Fee protocol, which can differ on forks or when a fee-protocol event is not in the replayed range.
- Protocol-fee balances, which can differ from replay rounding while the RPC snapshot reads the
  on-chain accumulator directly.

If only non-structural fields differ, the snapshot is accepted. This matches backtest replay behavior.

### Snapshot bootstrap guard

Use `--require-existing-snapshot` when analysis should run only from the local snapshot cache:

- Checks for the latest usable `pool_snapshot` at or before the target block.
- Returns `needs_bootstrap` if no usable snapshot exists.
- Treats an empty creation-block snapshot with no positions or ticks as unusable.
- Skips the creation-to-target bootstrap for that pool.

```bash
nautilus blockchain analyze-pools \
    --chain ethereum \
    --dex UniswapV3 \
    --addresses-file pools.txt \
    --to-block 25218797 \
    --require-existing-snapshot \
    --rpc-url "$RPC_HTTP_URL"
```

`analyze-pool(s)` prints:

- One JSON result per `--checkpoint-blocks` entry.
- One JSON result at `--to-block` when no checkpoints are given.

A pool that needs a first-time bootstrap has this shape:

```json
{
  "chain": "Ethereum",
  "dex": "UniswapV3",
  "pool_address": "0x1111111111111111111111111111111111111111",
  "target_block": 25218797,
  "status": "needs_bootstrap"
}
```

A successful result includes `validation_state`:

- `on_chain`: hydrated and matched against chain.
- `replay`: replay-derived or unchecked, still usable as a replay start point.
- `invalid`: hydrated and mismatched, not usable.

```json
{
  "chain": "Ethereum",
  "dex": "UniswapV3",
  "pool_address": "0x1111111111111111111111111111111111111111",
  "target_block": 25218797,
  "status": "success",
  "snapshot_block": 25218790,
  "positions": 2,
  "ticks": 7,
  "validation_state": "replay",
  "already_valid": false,
  "liquidity_utilization_rate": 0.25
}
```

### Checkpoints and concurrency

- `--checkpoint-blocks b1,b2,...`: produces snapshots in one bootstrap pass. Blocks are sorted,
  deduped, and clamped to `--to-block`.
- `--concurrency`: controls `analyze-pools` parallelism. Default: `4`.
- `--skip-validation`: skips the on-chain compare and keeps replay-derived snapshots as `replay`.
- `--snapshot-from-rpc`: hydrates from chain at the checkpoint block and records snapshots as
  `on_chain`.

Snapshot keys:

- Default mode: keyed to the last pool event at or before the checkpoint. Checkpoints with no events
  between them can share one stored row.
- `--snapshot-from-rpc`: keyed to the requested checkpoint block with a block-scoped sentinel
  transaction/log index.

### Backtest replay

Backtest replay needs a snapshot in the input data. The adapter does not service live snapshot
requests during backtests.

`load_pool_snapshot` reads a full snapshot, including positions and ticks, from Postgres:

```python
from nautilus_trader.adapters.blockchain import load_pool_snapshot

snapshot = load_pool_snapshot(
    pg_config=postgres_config,
    chain_id=chain_id,
    pool_address=pool_address,
    before_block=replay_start_block,  # latest snapshot at or before this block
)
```

Replay rules:

- By default, only `on_chain` snapshots are returned. Pass `require_valid=False` to accept replay
  snapshots.
- Treat `None` as setup failure. Do not replay without profiler state.
- Wrap the result as `DefiData.PoolSnapshot(snapshot)` and pass it to
  `BacktestEngine.add_defi_data` with the pool events.
- Replay every pool event from the snapshot block forward. Starting after the snapshot block can leave
  the profiler stale.

Cached block timestamps load into Nautilus data objects as UNIX nanoseconds. Cache rows written with
second-resolution block timestamps are normalized to nanoseconds when snapshots and pool events are
loaded, while nanosecond rows preserve their stored precision.

## Contracts

### Base contract and Multicall3

`BaseContract` batches contract calls through Multicall3
(`0xcA11bde05977b3631167028862bE2a173976CA11`):

- Calls use `allow_failure: true` so individual contract call failures can be reported.
- Reads execute against a single block context.
- Transport and provider failures surface as RPC errors.

### ERC-20 metadata

`Erc20Contract` reads `name`, `symbol`, and `decimals` through Multicall. The adapter can skip pools
whose token metadata is malformed, raw bytes, or empty.

### Uniswap V3 pools

`UniswapV3PoolContract` reads global pool state, active ticks, and positions.

- Large pools can exceed provider payload, gas, or timeout limits.
- Hydration fails closed if the final-state read fails.
- Very large pools may need a stronger provider or future chunked/minimal hydration.

PancakeSwap V3 reuses the Uniswap V3 read contract because `slot0`, `ticks`, `positions`,
`liquidity`, and fee-growth reads share the same ABI. Fee-protocol encoding differs:

- Uniswap V3 packs two 4-bit fee denominators into one `uint8`.
- PancakeSwap V3 stores two 16-bit basis-point shares in `slot0.feeProtocol` and emits
  `SetFeeProtocol(uint32,uint32,uint32,uint32)`.
- PancakeSwap V3 snapshots store `fee_protocol0_basis_points` and
  `fee_protocol1_basis_points`, and replay computes protocol fees as `fee * basis_points / 10000`.

## Execution

:::note
Execution support is under active development. Preflight, wrap, and approve operations and the
shared EIP-1559 transaction path are implemented; order submission, swaps, and reconciliation are
planned and described here before they land.
:::

The `BlockchainExecutionClient` implements `connect`/`disconnect` with a wallet balance refresh at
connect (native currency and, when a token universe is configured, ERC-20 tokens), RPC chain ID
verification against configuration, and signer initialization from `signer_private_key_env` (see
[Transaction signing and broadcast](#transaction-signing-and-broadcast)). Disconnect removes the
signer, and transaction operations reject a disconnected client before any execution RPC call.

### Supported order slice

The first execution slice will support Arbitrum as the chain and Uniswap V3 as the DEX, with a
single order flow:

- The caller selects a pool by its address‑based instrument ID, for example
  `0xC6962004f452bE9203591991D15f6b388e09E8D0.Arbitrum:UniswapV3`. The venue must parse as
  `<Chain>:<DexType>` and the symbol as an address `PoolIdentifier`, and the pool must resolve from
  the shared engine cache populated by the data engine (`Cache::pool`). Unknown pools, V4 pool‑ID
  symbols, and pools without a fee tier will be rejected.
- Only a SELL `MarketOrder` with a base‑denominated `Quantity` will be accepted; every other
  combination will be rejected before any RPC call. Base and quote will resolve through the model's
  token‑priority convention (`Pool::get_base_token`, `Pool::get_quote_token`): stablecoins price as
  quote, wrapped native assets next, and all other tokens as base against them. A pool whose tokens
  share a priority is ambiguous and will be rejected, detected by comparing
  `Token::get_token_priority` for both pool tokens because the helpers resolve ties silently.

The order will map to a single `exactInputSingle` call on the original Uniswap SwapRouter (the
deployment whose signature carries a deadline):

| Parameter           | Source                                                      |
| ------------------- | ----------------------------------------------------------- |
| `tokenIn`           | Pool base token address                                     |
| `tokenOut`          | Pool quote token address                                    |
| `fee`               | Pool fee tier                                               |
| `recipient`         | Execution wallet address                                    |
| `deadline`          | Latest block timestamp plus configured `deadline_seconds`   |
| `amountIn`          | `Quantity` converted to raw `U256` with base token decimals |
| `amountOutMinimum`  | Derived from a current quote (see below)                    |
| `sqrtPriceLimitX96` | `0` (slippage is bounded by `amountOutMinimum`)             |

### Slippage protection

`amountOutMinimum` will always be derived, never caller‑supplied:

1. Require an active data-side subscription to the pool so the `PoolProfiler` in the shared engine
   cache (`Cache::pool_profiler`) is live; without a live profiler no quote exists and the order
   will be rejected.
1. Simulate the exact‑input swap locally with `PoolProfiler::swap_exact_in` on that profiler.
1. Require the pool state to be fresh within `max_quote_age_blocks` of the latest block the data
   engine has processed for the chain; with no running data engine the quote is stale and the
   order will be rejected.
1. Compute `amountOutMinimum = quoted_amount_out * (10_000 - slippage_bps) / 10_000` in integer
   arithmetic and reject the order when the result is zero.

### Preflight, wrapping, and approval

Preflight, WETH wrapping, and router approval are explicit operations on the client, separate from
`submit_order`:

- **Preflight** is read‑only: it resolves the pool from its `InstrumentId` through the shared
  engine cache, verifies the chain ID, deployed bytecode at the router, pool, and token addresses,
  wallet native and token balances, the exact router allowance of the pool's base (input) token,
  and current fee conditions, and returns a structured, sanitized report. It contains no RPC URLs,
  keys, or raw signed transactions, and it changes no state.
- **Wrap** submits a WETH `deposit()` transaction carrying native value, only on explicit command.
- **Approve** submits an ERC-20 `approve(router, amount)` transaction, only on explicit command,
  with the approval amount policy set by `unlimited_approval`. The router must be in the
  `router_addresses` allowlist.

Wrap and approve each build their transaction through the shared EIP-1559 path, persist a
transaction record before broadcast, and return after the transaction is included on-chain.

### Transaction signing and broadcast

Transactions are built and signed locally as EIP-1559 typed transactions through Alloy primitives
(the workspace Alloy dependency's `consensus` and `eips` features are enabled for these paths):

- Building an `alloy::consensus::TxEip1559` (chain ID, nonce, gas, fees, `to`, `value`, `input`).
- Signing with `alloy::signers::local::PrivateKeySigner` (k256) over
  `SignableTransaction::signature_hash()`, producing a `Signed<TxEip1559>`.
- Encoding with `alloy::eips::eip2718::Encodable2718::encoded_2718()` and broadcasting the raw
  bytes with `eth_sendRawTransaction` through the adapter's HTTP RPC client.

Signer and transaction policy:

- The private key comes from the environment variable named by `signer_private_key_env`; it is
  never logged, serialized, or stored in configuration. One signer is supported. At connect, the
  address derived from the key must equal the configured `wallet_address`; a mismatch is a
  configuration error.
- At most one transaction is in flight across wraps and approvals, and across swaps once they
  land: a submission guard rejects any new transaction while another awaits inclusion, keeping the
  `pending` nonce authoritative. After signing, the client occupies this slot before the
  cancellable persistence write and keeps it through broadcast. A persistence error also keeps the
  slot because the database may have committed before its acknowledgement was lost. Dropping an
  in‑progress operation while persistence or broadcast is pending therefore cannot admit another
  transaction. The nonce comes from `eth_getTransactionCount` with the `pending` tag.
- Fees derive from `eth_maxPriorityFeePerGas` plus the latest base fee with `base_fee_buffer_bps`;
  `max_fee_per_gas_wei` is a required hard ceiling that rejects the transaction when current
  conditions exceed it.
- Gas comes from `eth_estimateGas` plus `gas_buffer_bps`; a buffered estimate above the `gas_limit`
  ceiling rejects the transaction before signing rather than clamping to the ceiling.
- The client persists the transaction record (nonce, transaction hash, chain ID, purpose, status)
  to the adapter's Postgres cache database before broadcast; with no durable store configured the
  client refuses to submit. Wrap and approve return only after the receipt confirms inclusion, and
  the persisted status moves from `pending` to `included` or `reverted`. A definitive node rejection
  moves the status to `rejected` and releases the in‑flight slot only when that update succeeds.
  Order submission (planned) will ack as submitted only after broadcast acceptance.

Execution RPC calls use per‑request timeouts, and a `null` result is a legitimate pending response
(a receipt that does not exist yet), not an error. Receipt observation retries RPC errors within the
bounded poll window without rebroadcasting. Exhaustion returns the last error if every observation
failed, or an inclusion timeout after any `null` response; both outcomes keep the in‑flight slot
occupied. Broadcast failures classify before retry: an `already known` response is acceptance, a
node‑level rejection is definitive, and an ambiguous failure after sending (timeout, reset, or an
unreadable response) reconciles through the persisted record rather than rebroadcasting.

### Risk and validation boundaries

Generic pre‑trade risk will stay in the engine. Venue‑specific gates will live in the adapter as a
configuration‑driven limiter. Chain ID verification, the router allowlist, gas and fee ceilings,
and the in‑flight limit are enforced today; the remaining limiter rows land with order submission:

| Check                 | Boundary       | Enforcement                                               |
| --------------------- | -------------- | --------------------------------------------------------- |
| Chain ID              | Adapter        | Preflight at connect and before every signature           |
| Router allowlist      | Risk (adapter) | `router_addresses` only                                   |
| Token‑pair allowlist  | Risk (adapter) | `allowed_token_pairs` only                                |
| Order amount          | Risk (adapter) | `max_order_amount` in raw units of the order's base token |
| Gas and fee           | Risk (adapter) | `gas_limit` and `max_fee_per_gas_wei` ceilings            |
| Balance sufficiency   | Adapter + risk | Wallet balances as account state; limiter rejects short   |
| Allowance sufficiency | Adapter        | Preflight and submission check against router allowance   |
| Slippage              | Risk (adapter) | `max_slippage_bps` ceiling; quote derives the minimum     |
| In‑flight limit       | Adapter        | Submission guard: one signer, one in‑flight transaction   |

Every limiter rejection will refuse the order before signing and report a structured reason.

### Persistence and reconciliation

Execution persistence adds only new keys and tables: existing Redis, PostgreSQL, and other state is
never mutated or cleared, and upgrades load existing data unchanged. Before any broadcast, the
client persists a transaction record carrying the nonce, the signed transaction hash, the chain ID,
the purpose, and a status in the `execution_transaction` table; wrap and approve records persist
before broadcast today, and order submission will key its records by `client_order_id` with the
venue parameters. On restart (planned), pending records will reload and resume from on‑chain
observation.

Wrap and approve records use `pending`, `rejected`, `included`, and `reverted`. A rejected broadcast
is terminal only after its status update succeeds.

Order state will derive from transaction observation:

| Outcome   | Detection                                                | Order result                                             |
| --------- | -------------------------------------------------------- | -------------------------------------------------------- |
| Included  | Receipt with status 1, not yet finalized                 | Order stays submitted; inclusion recorded                |
| Finalized | Inclusion block at or behind the chain's `finalized` tag | Fill report; wallet balances refresh; terminal           |
| Reverted  | Receipt with status 0                                    | Rejected with venue reason                               |
| Replaced  | Nonce consumed by a different hash before inclusion      | Canceled with replacement reason                         |
| Dropped   | No receipt within `receipt_timeout_secs`                 | Record marked suspect and alerted; observation continues |
| Restart   | Pending records reloaded and re‑observed                 | Resumes the matching path                                |
| Reorg     | Inclusion block no longer canonical before finality      | Inclusion record cleared; observation resumes            |

Finality is observed through the chain's `finalized` block tag (`eth_getBlockByNumber`), not a raw
L2 confirmation count: on Arbitrum, L2 blocks remain revocable until their batch posts to L1, and
the tag exposes exactly that boundary. Fills will emit only at finality, so a reorg never voids a
fill. A dropped record keeps the order submitted and the in‑flight slot occupied until inclusion,
replacement, or operator intervention; a signed transaction is never forgotten, and replacement
assumes the key is used only by this client. Fill quantities will come from the receipt and the
pool's Swap event amounts, decoded through the existing event parsing; the fill price derives from
the executed amounts at the pool's price and size precision, and the transaction's gas cost maps
to commission.

### Execution configuration

The `BlockchainExecutionClientConfig` gains additive fields, exposed to Python following the
`BlockchainDataClientConfig` pattern. These fields exist for preflight, wrap, and approve:

| Field                            | Default  | Description                                              |
| -------------------------------- | -------- | -------------------------------------------------------- |
| `signer_private_key_env`         | Required | Name of the environment variable holding the signer key  |
| `router_addresses`               | Required | Allowlist of SwapRouter addresses; at least one required |
| `max_fee_per_gas_wei`            | Required | Fee ceiling                                              |
| `base_fee_buffer_bps`            | Required | Buffer over the derived base fee                         |
| `gas_limit`                      | Required | Gas ceiling; buffered estimates above it reject          |
| `gas_buffer_bps`                 | Required | Buffer over `eth_estimateGas`                            |
| `unlimited_approval`             | `false`  | Approve the router unlimited instead of exact need       |
| `weth_address`                   | Required | Wrapped native token for wrap operations                 |
| `postgres_cache_database_config` | `None`   | Durable store for execution records; required to submit  |

These fields are planned with order submission support:

| Field                  | Default  | Description                                             |
| ---------------------- | -------- | ------------------------------------------------------- |
| `allowed_token_pairs`  | Required | Allowed (token in, token out) address pairs             |
| `slippage_bps`         | Required | Default slippage applied to quotes                      |
| `max_slippage_bps`     | Required | Limiter ceiling for slippage                            |
| `max_order_amount`     | Required | Limiter per‑order raw amount ceiling (base token units) |
| `deadline_seconds`     | Required | Swap deadline offset from the latest block timestamp    |
| `max_quote_age_blocks` | Required | Freshness bound for the local quote                     |
| `receipt_timeout_secs` | Required | Inclusion timeout before the dropped path               |

### Execution testing

Automated execution tests never use a live network:

- Mocked-RPC unit tests in the adapter crate cover calldata encoding for `deposit`, `approve`,
  and `allowance`, EIP-1559 signing against a fixed-key reference vector, nonce selection with
  the `pending` tag, fee and gas policy including ceiling rejections, preflight ready and
  not-ready states, receipt parsing including the null-pending and reverted cases, receipt RPC
  retry without rebroadcast, broadcast classification (`already known`, rejection, timeout after
  send), disconnect signer revocation, cancellation during persistence and after request dispatch,
  and successful and failed initial and terminal-status persistence against a temporary Postgres
  schema (skipped when Postgres is unavailable). JSON-RPC fixtures live as files under the crate's
  `test_data/execution/` directory.
- A pinned‑block Anvil integration forks Arbitrum at block 489000000 with
  `anvil --fork-url <RPC> --fork-block-number 489000000 --chain-id 42161` and runs wrap, approve,
  and preflight against localhost only, asserting receipt status 1, positive gas usage, the WETH
  balance delta, the router allowance, and the persisted records. The suite is gated behind
  `BLOCKCHAIN_FORK_TESTS=1` and `BLOCKCHAIN_FORK_RPC_URL` and never runs in default CI. The
  fork‑source RPC only reads chain state and needs archive access at the pinned block, so signed
  transactions never leave localhost. Anvil does not emulate Arbitrum's ArbOS gas pricing, so gas
  estimation behavior is covered by mocked RPC responses, not the fork suite.

To run the fork suite with Foundry's Anvil installed:

```bash
BLOCKCHAIN_FORK_TESTS=1 BLOCKCHAIN_FORK_RPC_URL="https://your-archive-capable-arbitrum-rpc.example.com" \
cargo nextest run -p nautilus-blockchain --features hypersync --test execution_fork
```

The suite spawns Anvil itself on a random localhost port, requires a reachable Postgres for the
persistence path, and writes an evidence packet (commit, fork block, Anvil version, transaction
hashes with gas used and receipt status, and SHA-256 sums) to
`target/blockchain-fork-evidence/`. A missing gate variable, unreachable Postgres, or absent
`anvil` makes the test skip while still reporting success; the evidence packet is the proof it
ran.

## Smoke tests

### HyperSync authentication

```bash
curl -fsS --max-time 15 \
    -H "Authorization: Bearer $ENVIO_API_TOKEN" \
    https://1.hypersync.xyz/height
```

Expected result: JSON with a numeric `height`.

### Small HyperSync query

```bash
query='{"from_block":25170900,"to_block":25170901,"include_all_blocks":true,"field_selection":{"block":["number","timestamp","hash"]}}'

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

```bash
cargo check -p nautilus-blockchain --features hypersync
```

### Live fail-closed regression

This ignored test uses real HyperSync replay plus an invalid local HTTP RPC URL. It verifies that
final RPC hydration fails closed instead of emitting a stale snapshot.

```bash
cargo test -p nautilus-blockchain --features hypersync \
    live_hypersync_bootstrap_fails_closed_when_rpc_hydration_fails \
    -- --ignored --nocapture
```

Expected result: one ignored test passes. This can take several minutes.

## Operational notes

- Use HyperSync for high-volume historical log scans. See
  [Envio HyperSync docs](https://docs.envio.dev/docs/HyperSync/hypersync-usage) for request shape and
  tuning details.
- Use HTTP RPC for final contract state and validation.
- Use a paid or high-limit RPC provider for large Uniswap V3 pools.
- Keep `ENVIO_API_TOKEN`, RPC keys, and Postgres credentials outside version control.
- Use a separate Postgres database for repeatable DeFi test runs that write pool snapshots.
- Treat failed final-state hydration as a hard failure for emitted snapshots.

### Pool analysis prerequisites and gotchas

These surface as `analyze-pool(s)` failures with a clear cause and fix.

#### Discover pools before analysis

`analyze-pool(s)` reads pool metadata from the cache and fails with `Pool <address> is not registered`
if the pool was never discovered. Run `sync-dex` for the chain/DEX once to populate the `pool` table
first.

#### Unsupported DEX combinations fail before sync

A DEX can be registered for a chain yet lack the parsers a command needs. The CLI fails fast:

- `sync-dex` (discovery) needs a `PoolCreated` parser.
- `analyze-pool(s)` (snapshots) need Initialize, Swap, Mint, Burn, and Collect parsers.
- Replay-ready DEXes additionally parse `SetFeeProtocol`, so replay keeps fee-protocol settings
  correct.
- DEXes that also parse `CollectProtocol` can replay protocol-fee balance withdrawals.

Current support:

- Uniswap V3 is replay-ready on Ethereum, Base, Arbitrum, and BSC.
- PancakeSwap V3 is replay-ready on Ethereum, Base, Arbitrum, and BSC.
- Aerodrome Slipstream is snapshot-capable on Base, but has no `PoolCreated` parser. Register pools
  another way before `analyze-pool(s)`.
- Uniswap V2/V4, Camelot, and Fluid currently support discovery only.
- Polygon works for `sync-blocks`, but has no DEX registrations.

`blockchain analyze-pool --help` and `blockchain sync-dex --help` print the current supported chain
and DEX combinations, derived from the registered parsers.

#### Use checksummed pool addresses

Addresses must be EIP-55 checksummed; a lowercase address fails with
`Blockchain address '<address>' has incorrect checksum`. Resolving a pool from
`UniswapV3Factory.getPool` returns lowercase, so checksum it before passing `--address`.

#### Lower the multicall batch on capped RPCs

Public nodes enforce a per-call gas limit, so a large multicall returns `out of gas` and the adapter
falls back to slow per-item fetches. Pass a smaller `--multicall-calls-per-rpc-request` (for example
`50` on `https://arb1.arbitrum.io/rpc`) to keep batches under the cap.

#### Use a recent target block on non-archive RPCs

A first-time sync reads on-chain state at `--to-block`, and a non-archive node only serves recent
state, so historical targets fail the on-chain read. See [RPC endpoints](#rpc-endpoints).

#### HyperSync rate limits are shared per token

HyperSync rate limits apply per token. See Envio's
[HyperSync API token docs](https://docs.envio.dev/docs/HyperSync/api-tokens) for token and usage
details.

- Keep `--concurrency` low on free or low-quota tokens.
- A full first-time sync of a large old pool can need thousands of requests.
- Use `--snapshot-from-rpc` when an exact checkpoint snapshot is enough and full swap storage is not
  needed.

#### Pools with no liquidity events fail cleanly

A pool with no processed Mint/Burn events up to the target block has no state to snapshot:

- `analyze-pools` emits a per-pool `"status": "failure"` JSON line and keeps other pools running.
- `analyze-pool` returns the error.
- Choose pools with liquidity activity to avoid this failure.

#### Exit code reflects per-pool failures

`analyze-pool(s)` exits non-zero when any pool fails, and each failed pool is also reported as a JSON
line with `"status": "failure"`. Rely on the exit code for an overall pass/fail signal, and parse
each result line's `status` for per-pool detail.

## Runbook: live pool-sync smoke test

Use this to check pool discovery, event parsing, and snapshot generation for one DEX on one chain.
The example uses PancakeSwap V3 on Arbitrum.

### Prerequisites

- `ENVIO_API_TOKEN` exported.
- RPC HTTP URL for the chain (`--rpc-url` or `RPC_HTTP_URL`).
- Postgres up with schema (`make start-services && make init-db`).
- Built CLI: `cargo build -p nautilus-cli --features defi --bin nautilus`.

### Steps

Discover pools first, then analyze specific pools:

```bash
./target/debug/nautilus blockchain sync-dex --chain arbitrum --dex PancakeSwapV3 \
    --rpc-url https://arb1.arbitrum.io/rpc \
    --host 127.0.0.1 --port 5432 --username nautilus --password pass --database nautilus

./target/debug/nautilus blockchain analyze-pools --chain arbitrum --dex PancakeSwapV3 \
    --address <pool-address> --address <pool-address> \
    --rpc-url https://arb1.arbitrum.io/rpc \
    --host 127.0.0.1 --port 5432 --username nautilus --password pass --database nautilus \
    --concurrency 1
```

Verify by counting rows in:

- `pool_swap_event`
- `pool_liquidity_event`
- `pool_collect_event`
- `pool_flash_event`
- `pool_fee_protocol_update_event`
- `pool_fee_protocol_collect_event`
- `pool_snapshot`
- `pool_position`
- `pool_tick`

Fee-protocol tables are often empty or small because `SetFeeProtocol` and `CollectProtocol` rarely
fire.

### Gotchas

- Free or low-quota Envio tokens can spend most time backing off on high-activity pools. Pick
  short-history pools, lower `--concurrency`, or use `--snapshot-from-rpc`.
- Development Postgres data can disappear mid-session while the schema remains. Run `sync-dex`
  immediately before `analyze-pool(s)` when in doubt.
- `--from-block` at a mid-life block skips `Initialize`, so snapshot bootstrap can fail with
  `Pool is not initialized and it doesn't contain initial price, cannot bootstrap profiler`. Sync
  from creation when a snapshot is required.
- Addresses must be EIP-55 checksummed. Use the CLI or `count(*)` to inspect pool rows.
- Capability guards fail unsupported DEX/parser combinations before sync. See
  [Unsupported DEX combinations fail before sync](#unsupported-dex-combinations-fail-before-sync).

## Extending the adapter

The event model currently targets Uniswap V3 concentrated-liquidity pools:

- `PoolSwap` carries `sqrt_price_x96` and `tick`.
- `PoolLiquidityUpdate` carries `tick_lower` and `tick_upper`.
- Other `DexType` and `AmmType` families exist, but most are not wired beyond discovery.

### Adding an event or protocol family

Design the taxonomy before writing a parser. Most families do not fit the V3 structs:

- Uniswap V2 emits `Sync`.
- Uniswap V4 uses `ModifyLiquidity` and `Donate`.
- Curve and Balancer pools can hold more than two tokens.

Adding events piecemeal tends to create optional fields, duplicate variants, and later renames.

The design pass should:

- Map the protocol's events and decide, per event, whether each reuses, extends, or adds a
  `DexPoolData` variant.
- Decide whether the family needs a new taxonomy axis. Singleton or `poolId` protocols (Uniswap V4,
  Balancer) and multi-token pools (Curve) break the per-pool-address, token-pair assumptions.
- Name events with the `<concept>_<verb>` convention, such as `fee_protocol_update`. Reserve the
  literal on-chain event name for signatures and error labels.

Then wire each event through the full path, mirroring an existing one such as `fee_protocol_collect`:

- Event struct
- HyperSync and RPC parsers
- `DexExtended` parser slot
- `DexPoolData` and `DefiData` variants
- Profiler apply method
- Event table and its insert
- `stream_pool_events` UNION arm and row mapper
- PyO3 binding

Cover it with a parser round-trip test, a profiler apply test, and the parser-parity test.

Incremental sync resumes from each pool's last-synced block. Adding an event type does not backfill
already-synced history; run a reset sync from creation to populate the new table.

### Adding a chain

A new chain is registration only if its DEXes reuse modeled events:

- Add the `Chain`.
- Add its RPC client.
- Add per-DEX registrations.

A new protocol family needs the design pass above.

## Current limitations

- Order submission is not yet implemented: the client connects, refreshes wallet balances, and
  supports explicit preflight, wrap, and approve operations; order methods, account state, and
  reconciliation are still to land. See [Execution](#execution).
- Very large Uniswap V3 pools can still hit provider payload, timeout, or rate limits during
  final-state Multicall hydration.
- `multicall_calls_per_rpc_request` documents the intended batching limit, but some final snapshot
  paths still need chunking hardening.
- A full successful WETH/USDT or WETH/USDC delivery test needs a real HTTP RPC provider that can
  serve the final-state reads, or the adapter needs minimal/chunked hydration first.
- On-chain snapshot validation covers Uniswap V3 and PancakeSwap V3 (shared V3 pool read ABI). Forks
  with a different pool ABI sync events and produce replay snapshots, but cannot reach
  `validation_state = on_chain` until the final-state hydration covers their pool contracts.
