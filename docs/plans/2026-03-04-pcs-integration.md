# PancakeSwap (PCS) Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate PancakeSwap AMM trading into NautilusTrader with *signer-only* execution, while laying a reusable framework for additional DEX (AMM) integrations.

**Architecture:** Build on Nautilus’ existing DeFi primitives (`crates/model/src/defi/*`) and blockchain adapter (`crates/adapters/blockchain/*`). Add a signer-driven AMM execution layer (generic) + a PancakeSwap protocol implementation (specific). Use the existing “DEX venue” convention (`<Chain>:<DexType>`) and represent pools as instruments keyed by pool address.

**Tech Stack:** Rust (adapter core + DeFi model + RPC), Python (adapter wiring/config/examples), PyO3 bindings, `nautilus-network` HTTP client + rate limiting, `alloy` ABI encoding, `tokio`, `pytest`, `cargo test`, optional `anvil`/Hardhat for live integration tests.

---

## 0) What “PCS integration” means in Nautilus

In Nautilus, a “venue integration” generally includes:

1. **Instrument universe**: how instruments are identified and loaded (InstrumentProvider).
2. **Market data** (optional for MVP): subscriptions/requests and normalized market data output.
3. **Execution**: submitting orders and emitting order/fill/position/account events.
4. **Config + factories**: creating clients from config and wiring into a `TradingNode`.
5. **Testing + examples**: reproducible tests and runnable scripts.

For AMMs, there is no native order book. Execution becomes “submit swap transaction(s)” and emit fills after on-chain confirmation.

---

## 1) Hard requirements (from prompt)

### 1.1 Must trade through a signer (like `~/chainsaw`)

No private keys inside Nautilus. All state-changing transactions MUST be:

1. Built as **unsigned tx intent** (to/data/value/gas/fee/nonce/chainId + policy metadata),
2. **Signed by remote signer service** (HTTP, optional mTLS),
3. Broadcast as `eth_sendRawTransaction`,
4. Monitored until receipt (or definitively failed/replaced),
5. Parsed to produce order/fill events.

### 1.2 Lay framework for more DEX (AMM) integrations

PCS should *not* be a one-off. The architecture must support (at minimum):

- UniswapV2-like routers (PCS V2, Sushi V2, Uniswap V2 forks),
- UniswapV3-like routers (PCS V3, Sushi V3, Uniswap V3),
- Future: smart routers / mixed routing / stable pools (PCS SmartRouter + MixedRouteQuoter, PCS StableSwap, Curve-style),
- Future: “universal router” style aggregators (PCS Infinity Universal Router) which can compose swaps and approvals into one tx.

---

## 2) Existing Nautilus foundations to reuse (important)

### 2.1 DeFi model is already present

Nautilus already contains DeFi abstractions in Rust:

- `DexType` enum includes `PancakeSwapV3`: `crates/model/src/defi/dex.rs`
- Chain registry includes BSC (56) and BSC testnet (97): `crates/model/src/defi/chain.rs`
- Venue “DEX encoding” exists: a DEX venue contains `:` and is parsed as `Chain:DexId`
  - `Venue::is_dex()` / `Venue::parse_dex()`: `crates/model/src/identifiers/venue.rs`
- `InstrumentId` parsing is DEX-aware:
  - If venue is a DEX venue, the symbol is expected to be an EVM address (validated): `crates/model/src/identifiers/instrument_id.rs`

**Consequence:** the most “native” PCS integration uses:

- `venue = Venue("Bsc:PancakeSwapV2")` (to be added) or `Venue("Bsc:PancakeSwapV3")`
- `instrument_id.symbol = <pool_address>` (string form `0x…`)

This is already aligned with the blockchain data client which expects pool address symbols (see below).

### 2.2 Blockchain adapter already streams pool events (data path)

The Rust blockchain adapter (`nautilus-blockchain`) already supports:

- RPC client scaffolding: `crates/adapters/blockchain/src/rpc/*`
- ERC20 metadata/balance reads via multicall: `crates/adapters/blockchain/src/contracts/erc20.rs`
- Pool event subscription manager and swap/mint/burn/etc events:
  - Subscriptions are keyed by `(DexType, pool_address)` and use DEX venue parsing:
    - `crates/adapters/blockchain/src/data/client.rs` (see `cmd.instrument_id.venue.parse_dex()` and `validate_address(cmd.instrument_id.symbol.as_str())`)

**Important note (feature flags):** as of `2026-03-04`, the `nautilus-blockchain` modules
`cache`, `execution`, `data`, `exchanges`, `factories`, `services` are compiled only when
`--features hypersync` is enabled (`crates/adapters/blockchain/src/lib.rs`). This means the data-path
described above is currently *hypersync-feature-gated*, even if the functionality itself is not
strictly hypersync-dependent.

**Gap:** PCS/BSC is not wired into `crates/adapters/blockchain/src/exchanges/mod.rs` yet (currently only Ethereum/Base/Arbitrum).

### 2.3 Execution path is incomplete today

`BlockchainExecutionClient` exists but does not implement order submission:

- `crates/adapters/blockchain/src/execution/client.rs` (look for TODOs / unimplemented submit handlers)

**Important note (feature flags):** `execution` is also behind `--features hypersync` today, and
`BlockchainExecutionClient` currently depends on `crate::cache::BlockchainCache` which is also
behind `--features hypersync`. PCS execution therefore requires a feature-gating cleanup
(see section 2.4 + Milestone 0a).

**Consequence:** PCS execution will require new implementation work (which is expected and central to this plan).

### 2.4 Feature-flag reality check (must resolve early)

PCS execution + Python usability will be blocked unless we make the *execution* surface buildable
without requiring HyperSync.

**Current repo reality:**

- `nautilus-blockchain` gates core adapter modules behind `feature = "hypersync"`:
  - `crates/adapters/blockchain/src/lib.rs`
- Python `nautilus-pyo3` enables DeFi via `--features defi`, which **does not** enable hypersync:
  - `crates/pyo3/Cargo.toml` → `defi = ["dep:nautilus-blockchain", "nautilus-blockchain/python"]`
  - `hypersync = ["nautilus-blockchain/hypersync"]` is separate

**Implication:** today, `cargo test -p nautilus-pyo3 --features defi` cannot expose a usable
blockchain **execution** client factory (and may also block data factories) because the relevant
Rust modules never compile.

**Plan decision (recommended):**

- Treat `hypersync` as an *optional indexer/streaming integration* feature.
- Make the **execution core** (signer → sendRawTx → receipts → fills) build with:
  - `nautilus-blockchain` default features (no hypersync), and
  - `nautilus-blockchain --features python` (so `nautilus-pyo3 --features defi` can use it).

**Acceptance criteria (build matrix):**

- Rust core builds:
  - `cargo test -p nautilus-model --features defi`
  - `cargo test -p nautilus-blockchain` (no `--features hypersync`)
- Rust + PyO3 surface builds:
  - `cargo test -p nautilus-blockchain --features python`
  - `cargo test -p nautilus-blockchain --features "python,hypersync"`
- Python bindings build:
  - `cargo test -p nautilus-pyo3 --features defi`
  - (optional) `cargo test -p nautilus-pyo3 --features "defi,hypersync"`
- Hypersync-enabled builds still work (for data backfill / pool discovery):
  - `cargo test -p nautilus-blockchain --features hypersync`

This plan includes an explicit Milestone (0a) to re-scope feature flags so PCS execution and its
PyO3 surface do not depend on `--features hypersync`.

### 2.5 External docs cross-check (DeepWiki) (avoid duplicating work)

DeepWiki’s “Blockchain and DeFi Adapters” page (indexed `2026-01-18`) describes Nautilus’ DeFi
building blocks: pool discovery, event parsing, pool profiling/simulation, and adapter scaffolding:
https://deepwiki.com/nautechsystems/nautilus_trader/8.5-blockchain-and-defi-adapters

**What appears to already exist (and should be reused):**

- `DexType` includes `UniswapV2`/`UniswapV3`/`UniswapV4`, `SushiSwapV2`/`SushiSwapV3`, and
  `PancakeSwapV3` (but not `PancakeSwapV2`) → we will extend rather than replace.
- Pool discovery/parsing infrastructure exists under `crates/adapters/blockchain/src/exchanges/*`
  and `crates/adapters/blockchain/src/services/*` and is already DEX-agnostic in shape.

**What is *not* implemented end-to-end today (and this plan adds):**

- **Signer-only trade execution** for AMMs (build tx → sign remotely → send raw tx → confirm receipt
  → parse fills) is not present today. The current `BlockchainExecutionClient` is largely TODO for
  order submission (`crates/adapters/blockchain/src/execution/client.rs`).
- **PCS V2 (UniswapV2-style) execution + fill decoding** is not present today; existing UniswapV2
  support is primarily focused on pool discovery/parsing for a subset of chains (not BSC).

### 2.6 Codebase audit deltas to bake into implementation plan

The focused audit of:

- `crates/adapters/blockchain/src/execution/*`
- `crates/adapters/blockchain/src/exchanges/*`
- DeFi model venue/enums parsing (`crates/model/src/defi/*`, `crates/model/src/identifiers/*`)

shows these concrete “already exists vs missing” deltas.

**Already exists (reusable immediately):**

- DEX venue parsing + validation (`Venue::is_dex`, `Venue::parse_dex`) and DEX-aware instrument address parsing.
- `DexExtended` abstraction with pluggable HyperSync/RPC event parsers.
- UniswapV2 `PairCreated` parser and pool-discovery path.
- `BlockchainExecutionClient`/factory scaffolding and wallet balance bootstrap.

**Missing / fragile (must be addressed explicitly):**

- `DexType` lacks `PancakeSwapV2`, so `Bsc:PancakeSwapV2` venue cannot be parsed yet.
- Exchange registry has no BSC map/branch (`exchanges/mod.rs` only Ethereum/Base/Arbitrum).
- V2 exchange definitions frequently pass empty event signatures (`""`) into `Dex::new`; subscription registration normalizes/hashes these strings, creating bogus topic filters instead of “event absent”.
- Data core currently assumes swap/mint/burn/collect signatures always exist when syncing pool events.
- Data core on-chain snapshot path is UniswapV3-only (`get_on_chain_snapshot`), so CPAMM profiling/snapshoting must be explicitly skipped or separately implemented.
- RPC chain websocket initialization excludes BSC (`initialize_rpc_client` supports only Ethereum/Polygon/Base/Arbitrum).
- `BlockchainHttpRpcClient` lacks execution-critical methods (`send_raw_transaction`, `get_transaction_receipt`, `get_transaction_count`, etc.) and needs Milestone 4 expansion.
- Execution factory currently hardcodes venue to `BLOCKCHAIN` instead of config-driven DEX venue, which breaks venue-based routing for AMM clients.
- Python blockchain module currently exposes only data config/factory extraction paths; execution config/factory registration is missing.

**Plan adjustment (required):** add dedicated hardening milestones for (a) CPAMM-safe event capability modeling (no empty-signature hashing), and (b) venue-correct execution factory wiring.

### 2.7 Targeted audit: `DexType::PancakeSwapV3` support by chain (as of current codebase)

This is the current status of PCS V3 definitions under `crates/adapters/blockchain/src/exchanges/*`:

- **Ethereum:** `exchanges/ethereum/pancakeswap_v3.rs` exists, but `PoolCreated/Swap/Mint/Burn/Collect`
  signatures are all empty strings and no parsers are registered.
- **Base:** `exchanges/base/pancakeswap_v3.rs` has the same issue (empty signatures, no parsers).
- **Arbitrum:** `exchanges/arbitrum/pancakeswap_v3.rs` sets `PoolCreated` and a HyperSync
  pool-created parser, but leaves swap/mint/burn/collect signatures empty and does not wire
  swap/mint/burn/collect parser functions.
- **BSC:** no exchange module/map branch exists, so `get_dex_extended(Blockchain::Bsc, PancakeSwapV3)` returns `None`.

Protocol-surface implication (important):

- PancakeSwap V3 pool `Swap` event signature includes **two extra protocol fee fields**
  (`protocolFeesToken0`, `protocolFeesToken1`), so it is **not** ABI-compatible with Nautilus’ current UniswapV3 swap parser.
  PCS V3 requires its own swap topic + decoder (or a generalized “V3 swap variants” decoder).

Streaming-path implications:

- DEX registration (`register_dex_for_subscriptions`) always normalizes/hashes provided signatures.
  Empty strings are hashed into synthetic topics, so subscriptions silently use wrong topic filters.
- Live block fan-out (`data/client.rs`) currently requests only swap/mint/burn topics and uses `unwrap()`
  on those signatures; collect/flash subscriptions are never fed from this path.
- WebSocket RPC chain client initialization has no BSC variant; non-HyperSync live path cannot run on BSC.

Execution implications:

- No AMM execution modules exist yet under `execution/*`; `submit_order` in `BlockchainExecutionClient`
  remains TODO. PCS V3 execution on BSC requires dedicated router/quoter/fill-decoding adapter work.

**Plan adjustment (required for PCS V3 on BSC):**

1. Add BSC exchange map + PCS V3 definition.
2. Harden PCS V3 definitions on all currently-declared chains (remove empty-signature placeholders, wire parsers).
3. Add PCS V3 execution adapter (BSC-first) with explicit router/quoter defaults and startup validation.
4. Add optional live-streaming completion milestone for BSC (HyperSync-first, RPC optional).

---

## 3) PCS protocol surface (what must be implemented)

This section is the concrete PancakeSwap contract interface we need to support for trading.

### 3.1 Recommended MVP: PancakeSwap V2 exact-in/out swaps

Start with PCS V2, token-token only, single-hop only (one pool), because it’s the smallest correct surface:

- Quote:
  - `getAmountsOut(amountIn, path)`
  - `getAmountsIn(amountOut, path)`
- Swap:
  - `swapExactTokensForTokens(amountIn, amountOutMin, path, to, deadline)`
  - `swapTokensForExactTokens(amountOut, amountInMax, path, to, deadline)`

Reference interfaces:

- `IPancakeRouter01.sol`, `IPancakeRouter02.sol`: https://github.com/pancakeswap/pancake-swap-periphery

### 3.1a MVP support matrix (explicit + enforced)

The adapter MUST hard-reject unsupported paths/tokens at *submit-time* (before signing/broadcast),
and MUST hard-reject “unexpected receipt shape” at *reconcile-time* (to avoid emitting incorrect fills).

| Capability / Case | MVP | Phase 2 | Enforcement notes |
|---|---:|---:|---|
| Token-token swap (ERC20 → ERC20) | ✅ | ✅ | Required baseline |
| Native-value swaps (`tx.value > 0`) | ❌ | ✅ (optional) | Guard with `enable_native_value_swaps=false` by default |
| Explicit wrap/unwrap (BNB↔WBNB) | ❌ | ✅ (optional) | Guard with `enable_wrap_unwrap=false` by default |
| Single-hop path only (`path.len()==2`) | ✅ | ✅ | Reject `path.len()!=2` |
| Multi-hop routing | ❌ | ⚠️ later | Requires deterministic fill attribution across hops |
| Exact-input swaps (`swapExact*`) | ✅ | ✅ | Recommended MVP default |
| Exact-output swaps (`swap*ForExact*`) | ✅ | ✅ | Add quote-freshness guard and strong revert mapping |
| Fee-on-transfer / taxed tokens | ❌ | ✅ (optional) | MVP rejects unless explicit FoT mode enabled + router FoT variants used |
| Rebasing / elastic-supply tokens | ❌ | ⚠️ later | Reject in MVP/Phase 2 (high ambiguity for fills) |
| “Non-standard” ERC20s (no `bool` return, reset-first approve) | ✅ | ✅ | Use SafeERC20-like decoding rules (see Milestone 6) |
| Unlimited approvals | ❌ (recommended) | ✅ (optional) | MVP can enforce `ApprovalPolicy::Exact` only; see Milestone 0 |

### 3.2 Approvals / allowances

Swaps use `transferFrom` under the hood; the wallet must approve router as spender for `tokenIn`.

MVP approach:

- Before swap: `allowance(owner, router)` via `eth_call`
- If insufficient:
  - Send `approve(router, amount)` tx via signer
  - Wait receipt
  - Then send swap tx

**ERC20 compatibility quirks (must be handled, not “best effort”):**

- Some widely-used tokens do **not** return a `bool` from `approve/transfer/transferFrom` (or return
  malformed data). Treat *empty return data* as success if the call did not revert (SafeERC20 rule).
- Some tokens require `approve(spender, 0)` before `approve(spender, amount)` (“reset-first”).
- Some tokens revert on non-zero → non-zero approvals; some enforce allowance race-mitigation patterns.

This plan treats these as first-class: implement tolerant ABI decoding and configurable
`ApprovalPolicy` patterns (Milestone 6).

Later optimization:

- Permit (EIP-2612) via `SelfPermit` to combine approve+swap in one tx (PCS V3 periphery supports this pattern).

### 3.3 Fill detection

On-chain swaps are atomic: either fully executed or reverted.

For V2:

- Parse transaction receipt logs:
  - Pair `Swap` events (one per hop)
  - (MVP) use pair `Swap` amounts as source-of-truth and **disallow** fee-on-transfer/rebasing tokens (documented limitation)
  - (Phase 2) support taxed tokens by using `SupportingFeeOnTransferTokens` router variants and/or recipient `Transfer` delta accounting

Sources:

- Pair `Swap` event: https://github.com/pancakeswap/pancake-swap-core/blob/master/contracts/interfaces/IPancakePair.sol

### 3.4 V2 event signatures + ABI selectors (exact values)

Use these exact function selectors and topic0 hashes in encoding/decoding tests and policy metadata:

- Router/factory/pair function selectors:
  - `factory()` -> `0xc45a0155`
  - `WETH()` -> `0xad5c4648` (PCS uses `WETH` name; on BSC this returns WBNB)
  - `getPair(address,address)` -> `0xe6a43905`
  - `allPairs(uint256)` -> `0x1e3dd18b`
  - `allPairsLength()` -> `0x574f2ba3`
  - `token0()` -> `0x0dfe1681`
  - `token1()` -> `0xd21220a7`
  - `getReserves()` -> `0x0902f1ac`
  - `getAmountsOut(uint256,address[])` -> `0xd06ca61f`
  - `getAmountsIn(uint256,address[])` -> `0x1f00ca74`
  - `swapExactTokensForTokens(uint256,uint256,address[],address,uint256)` -> `0x38ed1739`
  - `swapTokensForExactTokens(uint256,uint256,address[],address,uint256)` -> `0x8803dbee`
  - (Phase 2, native-value) `swapExactETHForTokens(uint256,address[],address,uint256)` -> `0x7ff36ab5`
  - (Phase 2, native-value) `swapETHForExactTokens(uint256,address[],address,uint256)` -> `0xfb3bdb41`
  - (Phase 2, native-value) `swapExactTokensForETH(uint256,uint256,address[],address,uint256)` -> `0x18cbafe5`
  - (Phase 2, native-value) `swapTokensForExactETH(uint256,uint256,address[],address,uint256)` -> `0x4a25d94a`
  - `swapExactTokensForTokensSupportingFeeOnTransferTokens(uint256,uint256,address[],address,uint256)` -> `0x5c11d795`
    - **Note:** FoT router variants return **no** `uint256[] amounts` (no return data to decode).
  - `swapExactETHForTokensSupportingFeeOnTransferTokens(uint256,address[],address,uint256)` -> `0xb6f9de95`
  - `swapExactTokensForETHSupportingFeeOnTransferTokens(uint256,uint256,address[],address,uint256)` -> `0x791ac947`
  - `allowance(address,address)` -> `0xdd62ed3e`
  - `approve(address,uint256)` -> `0x095ea7b3`

- Event signatures and topic0 hashes:
  - `PairCreated(address,address,address,uint256)` -> `0x0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9`
  - `Swap(address,uint256,uint256,uint256,uint256,address)` -> `0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822`
  - `Sync(uint112,uint112)` -> `0x1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1`
  - `Mint(address,uint256,uint256)` -> `0x4c209b5fc8ad50758f13e2e1088ba56a560dff690a1c6fef26394f4c03821c4f`
  - `Burn(address,uint256,uint256,address)` -> `0xdccd412f0b1252819cb1fd330b93224ca42612892bb3f4f789976e6d81936496`
  - `Transfer(address,address,uint256)` -> `0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef`
  - `Approval(address,address,uint256)` -> `0x8c5be1e5ebec7d5bd14f71427d1e84f3dd0314c0f7b2291e5b200ac8c7c3b925`

Primary sources:

- Note: PancakeSwap’s V2 core/periphery repos are archived upstream. Treat them as **stable reference implementations**
  for ABI signatures, event topics, and revert strings; prefer runtime validation (`eth_getCode`, `router.WETH()`, `router.factory()`) for live deployments.
- `IPancakeFactory.sol`: https://github.com/pancakeswap/pancake-swap-core/blob/master/contracts/interfaces/IPancakeFactory.sol
- `IPancakePair.sol`: https://github.com/pancakeswap/pancake-swap-core/blob/master/contracts/interfaces/IPancakePair.sol
- `IPancakeRouter01.sol`: https://github.com/pancakeswap/pancake-swap-periphery/blob/master/contracts/interfaces/IPancakeRouter01.sol
- `IPancakeRouter02.sol`: https://github.com/pancakeswap/pancake-swap-periphery/blob/master/contracts/interfaces/IPancakeRouter02.sol

### 3.5 On-chain calls to discover `token0`/`token1` for V2 pairs

If pool address is already known (InstrumentProvider config-driven pool):

1. `eth_call` to pair with `0x0dfe1681` (`token0()`),
2. `eth_call` to pair with `0xd21220a7` (`token1()`),
3. (recommended) `eth_call` to pair with `0xc45a0155` (`factory()`) and verify expected factory.

If only token pair is known:

1. `eth_call` to factory with selector `0xe6a43905` (`getPair(tokenA, tokenB)`),
2. reject if returned address is `0x0000000000000000000000000000000000000000`,
3. call pair `token0()` + `token1()` as above.

If discovering pools from logs:

1. `eth_getLogs` on factory with topic0=`PairCreated`,
2. decode `token0` and `token1` from indexed topics and pair address from event data,
3. optionally verify by calling pair `token0()` and `token1()` (defensive consistency check).

### 3.6 Canonical BSC defaults from PancakeSwap sources

Use source-backed defaults (not strategy hardcodes) for PCS V2:

- BSC mainnet (`chain_id=56`):
  - router v2: `0x10ED43C718714eb63d5aA57B78B54704E256024E`
  - factory v2: `0xcA143Ce32Fe78f1f7019d7d551a6402fC5350c73`
  - WBNB: `0xbb4CdB9CBd36B01bD1cBaEBF2De08d9173bc095c`
- BSC testnet (`chain_id=97`):
  - router v2: `0xD99D1c33F9fC3444f8101754aBC46c52416550D1`
  - factory v2: `0x6725f303b657a9451d8ba641348b6761a6cc7a17`
  - WBNB: `0xae13d989daC2f0dEbFf460aC112a837C89BAa7cd` (used by Pancake V3 config and commonly returned by `router.WETH()` for the testnet router)
    - Note: Pancake’s own repos contain multiple historical testnet WBNB addresses (e.g. `0x0946…` in `pancake-smart-contracts` exchange-protocol config).
      **Plan rule:** treat `router.WETH()` as source-of-truth at startup; hard-fail on mismatches unless explicitly overridden.

Sources:

- Factory map + chain IDs (historical; `pancake-swap-sdk` is archived): https://github.com/pancakeswap/pancake-swap-sdk/blob/master/src/constants.ts
- WBNB tokens map (historical; `pancake-swap-sdk` is archived): https://github.com/pancakeswap/pancake-swap-sdk/blob/master/src/entities/token.ts
- Router + alt testnet WBNB config: https://github.com/pancakeswap/pancake-smart-contracts/blob/master/projects/exchange-protocol/config.ts
- V2/V3/stable routing config (actively maintained): https://github.com/pancakeswap/pancake-v3-contracts/blob/main/common/config.ts
- PancakeSwap V3 deployments by chain (router/quoter/smart-router): https://github.com/pancakeswap/pancake-v3-contracts/tree/main/deployments

### 3.7 Native token (BNB) vs wrapped-native (WBNB) (end-to-end semantics)

On BSC, the router exposes **“ETH” named** methods which actually mean **BNB** on-chain, and uses
`WETH()` to return the **WBNB** contract address.

There are three distinct “assets” to keep straight:

- **Native BNB** (used to pay gas, held as an EOA balance; transferred via `value`)
- **WBNB** (ERC20 wrapper; held as ERC20 balance; requires allowance for router spends)
- **Pool token** which may be WBNB or non-WBNB

**MVP policy (recommended):**

- Execution supports **ERC20-only swaps** (`swapExactTokensForTokens` / `swapTokensForExactTokens`).
- If a pool leg is WBNB, treat it as a normal ERC20 token:
  - user must already hold WBNB (or pre-wrap manually)
  - router must be approved as spender for WBNB when WBNB is `tokenIn`
- Do not send swaps with non-zero `tx.value` in MVP; reserve that for native-swap support.

**Phase 2 options (pick one; keep consistent across future DEXs):**

1) **Router-native methods** (preferred UX; fewer transactions):
   - `swapExactETHForTokens(amountOutMin, path, to, deadline)` with `tx.value = amountIn`
   - `swapExactTokensForETH(amountIn, amountOutMin, path, to, deadline)` with `tx.value = 0`
   - `swapETHForExactTokens(amountOut, path, to, deadline)` with `tx.value = amountInMax`
   - `swapTokensForExactETH(amountOut, amountInMax, path, to, deadline)` with `tx.value = 0`
   - Path rules: for single-hop, `path = [WBNB, token]` or `[token, WBNB]`
2) **Explicit wrap/unwrap txs** (more control, more transactions):
   - wrap BNB → WBNB via `deposit()` then do token-token swap
   - unwrap WBNB → BNB via `withdraw(uint256)` after swap if desired

Chainsaw reference pointers (for implementation patterns, not direct porting):

- Native / wrapped symbol conventions: `~/chainsaw/engine/config/registry.py`
- Wrap/unwrap tx build patterns: `~/chainsaw/engine/gateways/rooster_gateway.py`

### 3.8 Wrap/unwrap contract surface (Phase 2, optional)

WBNB is WETH9-style. Support these calls if implementing explicit wrap/unwrap:

- `deposit()` (wrap): requires `tx.value = amount_in_wei`, `to = WBNB`, `data = deposit_selector`
- `withdraw(uint256)` (unwrap): `tx.value = 0`, `to = WBNB`, `data = withdraw_selector + amount`

**Selectors (WETH9 standard):**

- `deposit()` -> `0xd0e30db0`
- `withdraw(uint256)` -> `0x2e1a7d4d`

**Signer/local preflight implications:**

- extend allowlists/selectors to include wrap/unwrap if enabled
- enforce `value > 0` only for `deposit()` and router-native `swapExactETH*`/`swapETHForExact*`

### 3.9 Fees (what they mean and how to account for them)

Execution must account for *multiple kinds* of “fees”, each with different semantics:

1) **AMM LP fee** (e.g., UniV2/PCS V2 style): embedded in swap math and therefore embedded in the realized
   in/out amounts. Do **not** attempt to subtract it separately in Nautilus; report realized amounts and price.
   - PancakeSwap V2 has shipped multiple fee variants across codebases/forks:
     - legacy `pancake-swap-periphery` uses **0.20%** (`998/1000`),
     - `pancake-smart-contracts` “exchange-protocol” uses **0.25%** (`9975/10000`) (commonly documented for PCS V2 on BSC).
     Treat this as *protocol metadata only*; correctness must come from on-chain quotes/receipts, not from hardcoding the fee.
     - **MVP rule:** use router `getAmountsOut/getAmountsIn` for quoting (fee is inherently applied by the on-chain router).
     - If a local quote path is implemented later, make `amm.v2_fee_bps` explicit and default it to `25` for `Bsc:PancakeSwapV2`.
     - Primary sources:
       - 0.25% math (exchange-protocol): https://raw.githubusercontent.com/pancakeswap/pancake-smart-contracts/master/projects/exchange-protocol/contracts/libraries/PancakeLibrary.sol
       - 0.20% math (legacy periphery): https://raw.githubusercontent.com/pancakeswap/pancake-swap-periphery/master/contracts/libraries/PancakeLibrary.sol
2) **Gas fee** (native token): compute from receipt (`gasUsed * effectiveGasPrice`) and report separately
   from trade notional (see section 8.3).
3) **Token transfer taxes / rebasing**: can invalidate “swap event == balance delta” assumptions.
   MVP disallows; Phase 2 requires supporting router variants and/or transfer-delta accounting (section 10.5).

### 3.10 PancakeSwap beyond V2 (V3 + SmartRouter + StableSwap + Infinity) (post-MVP notes)

PancakeSwap is no longer “just V2 on BSC”. The official GitHub org publishes:

- **V3 deployments + routing stack** (factory, swap router, quoter, smart router, mixed-route quoter, stable factories):
  - `pancakeswap/pancake-v3-contracts` (deployments + config)
- **Infinity / Universal Router** (V4-style ecosystem):
  - `pancakeswap/infinity-*` repos (core/periphery/hooks) and `pancakeswap/infinity-universal-router` (deploy addresses)
- (Related) `pancakeswap/permit2` (EIP-712 permit flow used by router-style aggregators in many ecosystems)

**Plan stance:**

- MVP executes **direct PCS V2 router swaps** only (this document).
- The AMM adapter framework must make it straightforward to add (in order of increasing complexity):
  1) **PCS V3 execution** (UniswapV3-like, config-driven, signer-only),
  2) **PCS SmartRouter** (mixed routing across V2/V3/StableSwap),
  3) **PCS StableSwap pools** (non-constant-product; separate pricing model + fill decode),
  4) **PCS Infinity Universal Router** (command encoding + possibly Permit2/EIP-712 signing support).

**ABI gotchas (important if you add SmartRouter/StableSwap later):**

- PCS SmartRouter is *not* the classic `PancakeRouter` ABI. For example (from `pancake-v3-contracts/projects/router`):
  - V2 swaps use `swapExactTokensForTokens(uint256,uint256,address[],address)` (no `deadline`, selector `0x472b43f3`) and
    `swapTokensForExactTokens(uint256,uint256,address[],address)` (selector `0x42712a67`), not classic V2 selectors
    `0x38ed1739`/`0x8803dbee`.
  - SmartRouter V2 swap methods are `payable`; treat non-zero `tx.value` as invalid for ERC20-only flows.
  - SmartRouter V2/V3/stable routers support sentinel values:
    - `amountIn == 0` means “use contract balance” (not zero-amount swap),
    - `to == address(1)` means `msg.sender`,
    - `to == address(2)` means router contract itself.
    - **Adapter rule:** reject these sentinel encodings by default unless explicitly enabled for advanced multicall flows.
  - SmartRouter V2 has no per-method deadline argument; deadline guard comes from `multicall(uint256,bytes[])` (`0x5ae401dc`).
    Direct V2 method calls should be treated as unsafe unless wrapped in deadline-checked multicall.
  - V3 swaps use `exactInputSingle(ExactInputSingleParams)` / `exactInput(ExactInputParams)` (struct calldata, encoded paths).
  - StableSwap swaps use `exactInputStableSwap(address[],uint256[],uint256,uint256,address)` (extra `flag` array).
  - V3 Quoter-style contracts may use an internal **revert payload** during simulation; PancakeSwap `QuoterV2`
    catches and returns normally. **Adapter rule:** decode normal ABI returns first; treat RPC errors as failures
    (but always capture `error.data` when present for diagnostics).
  - Quoters are typically **not `view`** and are not gas-efficient; call only via `eth_call` (never broadcast) with strict timeouts/backoff.
  - `MixedRouteQuoterV1` is **exact-input only** (exact-output is explicitly unsupported) and requires a `flag` array:
    - `flag[i] = 0` for V3, `1` for V2, `2` for Stable 2-pool, `3` for Stable 3-pool.
    Treat it as a best-effort quote helper for SmartRouter; do not use it to justify exact-output `amountInMax` safety without additional validation.
  - SmartRouter also includes `SelfPermit` + multicall helpers, which can reduce tx count but requires message signing (EIP-712) for permit flows.
  - Native-value flows in router stacks often require explicit “sweep/refund” semantics for leftover native value; plan for correct sequencing if/when SmartRouter/UniversalRouter native paths are implemented.
  - `StableSwapRouter` owner can mutate `stableSwapFactory`/`stableSwapInfo` via `setStableSwap(...)`; do not assume those are immutable.
  - Router code contains many bare `require(...)` checks (empty revert data); error handling must not depend on reason strings.
  - Do not derive V2 pair addresses from hardcoded init-code constants copied from router libs; use `factory.getPair(...)` or on-chain validation.

**Primary sources (SmartRouter / StableSwap / quoters):**

- Note: links below reference upstream `main` branches as of **2026-03-04**; when implementing, prefer commit-pinned URLs for long-lived reproducibility.
- SmartRouter composition (V2/V3/StableSwap + multicall):  
  - https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/SmartRouter.sol
  - https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/base/MulticallExtended.sol
- SmartRouter swap interfaces (note lack of `deadline` on swap methods):  
  - https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/interfaces/IV2SwapRouter.sol
  - https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/interfaces/IV3SwapRouter.sol
  - https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/interfaces/IStableSwapRouter.sol
- V3 quote surface (internal callback revert is caught/decoded; RPC returns normally):  
  - https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/lens/QuoterV2.sol

**Canonical addresses (do not hardcode in strategies; treat as defaults + validate at startup):**

- PCS V3 / routing stack on BSC (from `pancake-v3-contracts/deployments/bscMainnet.json`):
  - `PancakeV3Factory`: `0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865`
  - `PancakeV3PoolDeployer`: `0x41ff9AA7e16B8B1a8a8dc4f0eFacd93D02d071c9`
  - `SwapRouter`: `0x1b81D678ffb9C0263b24A97847620C99d213eB14`
  - `QuoterV2`: `0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997`
  - `SmartRouter`: `0x13f4EA83D0bd40E75C8222255bc855a974568Dd4`
  - `MixedRouteQuoterV1`: `0x678Aa4bF4E210cf2166753e054d5b7c31cc7fa86`
  - `TokenValidator`: `0x864ED564875BdDD6F421e226494a0E7c071C06f8` (SmartRouter helper)
  - `PancakeInterfaceMulticall`: `0xac1cE734566f390A94b00eb9bf561c2625BF44ea` (optional; prefer Multicall3 when available)
- PCS V3 / routing stack on BSC testnet (from `pancake-v3-contracts/deployments/bscTestnet.json`):
  - `PancakeV3Factory`: `0x0BFbCF9fa4f9C56B0F40a671Ad40E0805A091865`
  - `PancakeV3PoolDeployer`: `0x41ff9AA7e16B8B1a8a8dc4f0eFacd93D02d071c9`
  - `SwapRouter`: `0x1b81D678ffb9C0263b24A97847620C99d213eB14`
  - `QuoterV2`: `0xbC203d7f83677c7ed3F7acEc959963E7F4ECC5C2`
  - `SmartRouter`: `0x9a489505a00cE272eAa5e07Dba6491314CaE3796`
  - `MixedRouteQuoterV1`: `0xB048Bbc1Ee6b733FFfCFb9e9CeF7375518e25997`
  - `TokenValidator`: `0x678Aa4bF4E210cf2166753e054d5b7c31cc7fa86` (SmartRouter helper)
  - `PancakeInterfaceMulticall`: `0x3D00CdB4785F0ef20C903A13596e0b9B2c652227` (optional; prefer Multicall3 when available)
- PCS “stable” routing config on BSC (from `pancake-v3-contracts/common/config.ts`):
  - `stableFactory` (BSC mainnet): `0x25a55f9f2279a54951133d503490342b50e5cd15`
  - `stableInfo` (BSC mainnet): `0x150c8AbEB487137acCC541925408e73b92F39A50`
- PCS Infinity Universal Router (from `infinity-universal-router/deploy-addresses/*.json`):
  - BSC mainnet: `UniversalRouter = 0xd9c500dff816a1da21a48a732d3498bf09dc9aeb`
    - `UnsupportedProtocol = 0x2979d1ea8f04C60423eb7735Cc3ed1BF74b565b8` (revert helper contract)
  - BSC testnet: `UniversalRouter = 0x87FD5305E6a40F378da124864B2D479c2028BD86`
    - `UnsupportedProtocol = 0xe4da88F38C11C1450c720b8aDeDd94956610a4e5` (revert helper contract)
- PCS Permit2 (often used by router-style aggregators, including UniversalRouter command set):
  - BSC mainnet/testnet: `Permit2 = 0x31c2F6fcFf4F8759b3Bd5Bf0e1084A055615c768`

**Implication for this plan’s framework:**

- Treat “router-like” execution backends as adapters (not as special-cased clients).
- Keep signer integration generic enough that future work can add **typed-data signing** (EIP-712) if Permit2/UniversalRouter is adopted.

**Address source precedence for SmartRouter stack (required):**

1. Explicit user config override (highest).
2. Root deployment manifests (`pancake-v3-contracts/deployments/<chain>.json`) for chain defaults.
   - **Explicitly do not** use `pancake-v3-contracts/projects/router/deployments/*.json` as a default source; treat it as potentially stale/conflicting.
3. Runtime immutable reads from deployed SmartRouter (`factoryV2()`, `factory()`, `WETH9()`, `positionManager()`, `stableSwapFactory()`, `stableSwapInfo()`).

**Why this precedence is required:**

- `pancake-v3-contracts/projects/router/config.ts`, `projects/router/deployments/*.json`,
  `common/config.ts`, and root `deployments/*.json` can diverge by chain/environment.
- The root `deployments/<chain>.json` files are assembled from multiple component manifests; duplicate keys can exist
  across sources. **Plan rule:** treat the root `deployments/<chain>.json` as the canonical “single source” for defaults,
  and still validate at startup via runtime contract reads.
- At startup, always verify configured/default addresses against runtime immutable reads and fail closed on mismatches unless explicitly overridden.
- When implementing, record the upstream commit SHA used for any baked-in address defaults/ABIs (reproducibility), and consider an optional code-hash allowlist check (`keccak256(eth_getCode)`) for router/factory/quoter in production deployments.

**Primary sources (addresses + universal router command surface):**

 - V3 deployments + routing stack addresses (BSC mainnet): https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/deployments/bscMainnet.json
 - V3 deployments + routing stack addresses (BSC testnet): https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/deployments/bscTestnet.json
- Chain config including `WNATIVE`, `stableFactory`, `stableInfo`: https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/common/config.ts
- Infinity UniversalRouter deployment addresses:
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/deploy-addresses/bsc-mainnet.json
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/deploy-addresses/bsc-testnet.json
- Infinity UniversalRouter contract + commands (for future integration):
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/src/UniversalRouter.sol
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/src/interfaces/IUniversalRouter.sol
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/src/libraries/Commands.sol
- Permit2 deployments + overview (for typed-data signing needs): https://raw.githubusercontent.com/pancakeswap/permit2/main/README.md

---

## 4) Proposed architecture (recommended)

### 4.1 High-level components

**A) Generic EVM transaction layer (reusable across DEXs)**

- **EvmRpcClient**: JSON-RPC methods needed for execution:
  - `eth_call`
  - `eth_getTransactionCount` (pending nonce)
  - `eth_estimateGas`
  - `eth_feeHistory` (optional)
  - `eth_gasPrice` (legacy)
  - `eth_sendRawTransaction`
  - `eth_getTransactionReceipt`
  - `eth_getBlockByNumber` (for baseFee / confirmations)
  - `eth_getLogs` (PairCreated discovery and log backfills)
  - `eth_getCode` (startup verification of router/factory/token contracts)
  - `eth_chainId` (startup chain guardrail)

**B) Signer integration layer (reusable across chains + DEXs)**

- **RemoteSignerClient**: POST tx intent → get `raw_tx_hex`
  - Must support retries/backoff and optional mTLS (like chainsaw `SignerClient`)
  - Must provide structured logs (req id, latency, decision)

**C) AMM execution framework (protocol-agnostic)**

- **AmmExecutionCore** (engine-facing):
  - Maps Nautilus `SubmitOrder` → AMM swap intent
  - Handles nonce management, approval flow, tx submission, receipt monitoring
  - Produces Nautilus order/fill/position/account events
- **AmmProtocolAdapter** (protocol-facing):
  - Encodes contract calls (router + ERC20)
  - Decodes receipts into fills
  - Implements quoting (getAmountsOut / getAmountsIn or local math)

**D) PancakeSwap implementation (protocol-specific)**

- `PancakeSwapV2Adapter`: implements the above for PCS V2 router and pair swap logs

### 4.2 Why this is “framework for more DEX”

Most EVM AMM integrations share:

- ERC20 allowance/approve flow
- Router call encoding + deadline + slippage
- Broadcast + receipt monitoring
- Log decoding to compute realized fills

By separating protocol-specific call encoding/decoding into a `AmmProtocolAdapter`, we can implement:

- `UniswapV2LikeAdapter` base (PCS V2 / Sushi V2 / Uni V2 forks)
- `UniswapV3LikeAdapter` base (PCS V3 / Sushi V3 / Uni V3)
- Later: StableSwap / weighted pools / aggregators

### 4.3 Rust execution integration decision (validated)

For **Rust Nautilus live execution/reconciliation paths**, implement PCS by **extending**
`BlockchainExecutionClient` and adding AMM protocol modules under the blockchain adapter,
rather than creating a second top-level execution client type in this phase.

**Why this is the right minimum-risk path:**

- Existing execution factory wiring already instantiates `BlockchainExecutionClient`
  (`crates/adapters/blockchain/src/factories.rs`), so extending it keeps registration/routing stable.
- **But:** Nautilus routes execution by `Venue` (`crates/execution/src/engine/mod.rs` stores a `routing_map: Venue -> ClientId`),
  and the current blockchain execution factory hardcodes `Venue("BLOCKCHAIN")` (`crates/adapters/blockchain/src/constants.rs`).
  For DEX execution, we must make the execution client venue **config-driven** (e.g., `Venue("Bsc:PancakeSwapV2")`)
  so orders for DEX instruments route to the correct client.
- Startup reconciliation in Rust uses `generate_mass_status` from registered execution clients
  (`crates/live/src/node.rs` + `crates/execution/src/engine/mod.rs`); we need to make this path correct first.
- `ExecutionManager` can create external orders during reconciliation and then calls
  `register_external_order`; AMM execution must support this tracking path.
- Rust `ExecutionManager` periodic helpers (`check_open_orders`, `check_positions_consistency`) exist,
  but wiring/scheduling can be phase-2 after startup reconciliation correctness.

**Important design guardrail (for multi-DEX):**

- Keep `BlockchainExecutionClient` **DEX-agnostic** and add per-DEX encoding/decoding in protocol adapters
  (`execution/amm/*`), not in client trait surface.
- Do not add PCS-specific public execution methods; keep generic order intent + adapter dispatch by `dex_id`.

### 4.4 V2 swap event model alignment (new work)

PCS V2 (UniswapV2-like) **does not** match the existing swap data model in Nautilus today:

- Existing Rust swap event/data types are UniswapV3-like:
  - `crates/adapters/blockchain/src/events/swap.rs` includes `sqrt_price_x96`, `liquidity`, `tick`
  - `crates/model/src/defi/data/swap.rs` (`PoolSwap`) also includes V3 fields
- UniswapV2-like swaps emit:
  - `Swap(address indexed sender, uint amount0In, uint amount1In, uint amount0Out, uint amount1Out, address indexed to)`
  - plus optional `Sync(uint112 reserve0, uint112 reserve1)` and `Transfer` events for ERC20 movements

**Plan decision (recommended):**

- Do **not** shoehorn V2 into V3 structs.
- For execution fills, introduce a protocol-agnostic internal representation:
  - Define `AmmFill` in `crates/adapters/blockchain/src/execution/amm/mod.rs` (canonical schema shared by all adapters)
  - Minimum fields (MVP):
    - `token_in`, `token_out` (addresses)
    - `amount_in`, `amount_out` (on-chain integers)
    - `tx_hash`, `log_index`
    - optional: `pool_address`, `recipient`
  - Ordering invariant: adapters must return fills sorted by `log_index` and with `tx_hash` matching the decoded receipt.
  - Protocol adapters produce `Vec<AmmFill>`; core execution maps `AmmFill -> FillReport`.
- If/when PCS V2 market data is implemented, add a dedicated V2 swap data type (e.g., `PoolSwapV2`) alongside existing `PoolSwap`
  rather than mutating the V3 schema.

This plan includes an explicit milestone for V2 swap log decoding and fill extraction (Milestone 7/8),
and an optional milestone for “first-class V2 swap market data” (Milestone 10).

### 4.5 AMM adapter contract (capability matrix + contract tests)

To make “framework for more DEX” enforceable (not aspirational), define an adapter contract that all AMM integrations must satisfy.

**Core trait (Rust):** `AmmProtocolAdapter`

Minimum responsibilities:

- Capability declaration:
  - `dex_type() -> DexType`
  - `amm_type() -> AmmType` (CPAMM vs CLAMM)
  - `supports_quote_exact_in/out`, `supports_single_hop`, `supports_multi_hop`
  - `supports_deadline_arg` (classic V2 router yes; SmartRouter V2-style swaps no; UniversalRouter uses a top-level deadline)
  - `supports_recipient_override` (MVP: false; enforce recipient == wallet; future: allow explicit recipient only with strict policy)
  - `swap_call_returns_amounts` (classic V2 exact-in/out: true; FoT variants: false; SmartRouter/UniversalRouter: varies)
  - `quote_revert_as_return` (V2 routers: false; V3-style quoters: true)
  - `supports_order_types` (MVP: market only)
- Call encoding:
  - `encode_quote_exact_in(...) -> CallData`
  - `encode_quote_exact_out(...) -> CallData`
  - `encode_swap_exact_in(...) -> TxCall`
  - `encode_swap_exact_out(...) -> TxCall`
  - `encode_approve(...) -> TxCall`
- Receipt decoding:
  - `decode_fills_from_receipt(receipt, expected_path) -> Vec<AmmFill>`
  - Must be deterministic and validated against ABI/topic0 constants

**Capability matrix (MVP target):**

| Adapter | Quote | Swap exact-in | Swap exact-out | Multi-hop | Receipt → fills | Streaming data | Notes |
|---|---:|---:|---:|---:|---:|---:|---|
| PCS V2 (UniswapV2-like) | ✅ | ✅ | ✅ | ❌ | ✅ | ⏳ | MVP is single-hop, market-only |
| PCS V3 (CLAMM, BSC-first; post-MVP) | ✅ | ✅ | ⚠️ | ❌ | ✅ | ⏳ | Pancake V3 `Swap` event differs from UniV3; exact-out behind config flag |

**Registry rule:** core execution code must select adapters via registry dispatch (`DexType -> adapter`),
not via `match` statements scattered across `BlockchainExecutionClient`.

**Contract tests (required):**

- “Golden vector” tests per adapter:
  - function selector bytes for each encoded call (router + ERC20 approve)
  - argument ordering and encoding correctness
  - swap log decoding correctness for representative receipts
- A shared “adapter capability” test that fails if an adapter claims unsupported functionality
  (e.g., V2 adapter claiming `supports_multi_hop=true` in MVP).

---

## 5) How PCS fits Nautilus identifiers & instruments

### 5.1 Venue encoding (use existing “DEX venue” scheme)

Use `Venue("<Chain>:<DexType>")`:

- Example (target): `Venue("Bsc:PancakeSwapV2")` (requires adding `DexType::PancakeSwapV2`)
- Already supported today: `Venue("Base:PancakeSwapV3")` (exists in `DexType`)

This is required because the Rust model treats venues with `:` as DEX venues and expects symbols to be addresses.

**Execution routing implication:** ensure the registered execution client’s `venue()` matches the instrument venue
exactly (one execution client instance per DEX venue for clean multi-DEX routing).

### 5.2 InstrumentId symbol is pool address

For DEX venues, `InstrumentId.from_str("0xPOOL….<Chain>:<DexType>")` is valid and validated as an address in `InstrumentId::from_str`.

Instrument modeling recommendation:

- Represent a pool as a Spot `CurrencyPair` instrument:
  - `instrument_id.symbol = Symbol(<pool_address>)`
  - `instrument_id.venue = Venue(<chain>:<dex>)`
  - `base_currency = token0`, `quote_currency = token1`
  - Store pool metadata in `instrument.info`:
    - `token0_address`, `token1_address`
    - `pool_address`
    - `fee_bps` (if known)
    - `router_address`
    - `chain_id`

**Precision constraint:** `Currency`, `Price`, `Quantity` are capped at 16 decimals. For tokens with >16 decimals, store full on-chain decimals in `instrument.info` and quantize Nautilus-facing precision to 16 while converting to/from U256 internally.

### 5.3 InstrumentProvider strategy (pool universe)

AMM instruments are not “listed” like CEX symbols. We need an explicit pool universe strategy.

**MVP recommendation (config-driven pools):**

- User supplies a small list of pool addresses in config (one instrument per pool).
- InstrumentProvider:
  - validates each pool address
  - fetches token0/token1 addresses and token decimals/symbol/name (on-chain calls or cache)
  - required call sequence per pool:
    - `pair.token0()` (`0x0dfe1681`)
    - `pair.token1()` (`0xd21220a7`)
    - `pair.factory()` (`0xc45a0155`) and assert equals configured/default factory
    - `factory.getPair(token0, token1)` (`0xe6a43905`) and assert equals `pool_address`
    - ERC20 metadata calls for each token (`decimals`, `symbol`, `name`)
  - builds `CurrencyPair` instruments:
    - `instrument_id.symbol = pool_address`
    - `venue = "<Chain>:<DexType>"`
    - `base_currency = token0`, `quote_currency = token1`
    - `info` contains: token addresses, full decimals, router address, chain id, optional fee

**Phase 2 (discovery-driven pools):**

- Reuse the existing pool discovery and caching path:
  - `PoolDiscoveryService`: `crates/adapters/blockchain/src/services/pool_discovery.rs`
  - `BlockchainCache`: `crates/adapters/blockchain/src/cache/*`
- Expose a Rust → Python API to list cached pools/tokens (allowlist + filters), then build instruments from that.

### 5.4 Operational workflow: adding/removing pools to trade (MVP + Phase 2)

We need an operator-friendly process to onboard new pools without compromising safety.

**MVP stance (recommended):** pool universe is **static at startup** (config-driven). Adding/removing pools requires
updating config and restarting the node. This avoids hot-reload complexity in execution-critical paths.

**Source of truth (required):**

- Maintain a single, explicit pool allowlist in the Python adapter config (the list of pool addresses to load/trade).
- Execution MUST reject orders for instruments not present in this configured pool universe (fail closed).

**How to find a pool (operator steps):**

- PCS V2: determine pool address from token addresses:
  - call `factory.getPair(tokenA, tokenB)` and use the returned pair address (if `0x000…`, there is no V2 pair).
- PCS V3 (post-MVP): determine pool address from `(tokenA, tokenB, fee)`:
  - call `factory.getPool(tokenA, tokenB, fee)`; reject if `0x000…` or if pool immutables don’t match.
- UI/subgraph sources can be used for convenience, but onboarding MUST verify on-chain before trading.

**On-chain validation checklist (required before enabling trading for a new pool):**

1) `eth_getCode(pool) != 0x` (contract exists)  
2) `pair.factory()` equals configured/default PCS factory  
3) `pair.token0()` / `pair.token1()` resolve to ERC20 contracts with `eth_getCode != 0x`  
4) token metadata loads (decimals/symbol/name) and passes filters (e.g. non-empty fields if configured)  
5) token safety policy:
   - MVP default: deny fee-on-transfer/rebasing unless explicitly allowlisted (section 10.5 / Milestone 7b)
6) optional but recommended: liquidity sanity check (avoid “dead” pools)
   - V2: read reserves (`getReserves`) or run a dust `getAmountsOut` quote and require non-zero output.

**Trading-direction clarity (required):**

- In Nautilus, this plan models a pool instrument as `token0/token1` (base/quote). This is deterministic but can be
  surprising (token0/token1 ordering is by address, not “stablecoin as quote” conventions).
- On onboarding, record `token0/token1` + symbols in the config/docs and use helper output to determine which
  Nautilus `OrderSide` corresponds to swapping A→B.

**Adding a pool (MVP):**

1) Add pool address to the adapter config allowlist.  
2) Run a local validation tool (planned) to print token0/token1, symbols/decimals, factory match, and a config snippet.  
3) Restart the node and verify instrument is loaded + quoting succeeds.  
4) Execute a small “smoke swap” in a non-production wallet to validate end-to-end receipt decoding.

**Removing a pool (MVP):**

1) Remove it from the allowlist.  
2) Restart the node.  
3) Ensure any in-flight tx journal entries for that pool are drained/terminalized before fully disabling the deployment.

**RPC/streaming impact (ops note):**

- If using RPC-based streaming (Milestone 10 option B), adding pools increases per-block log workload.
  Enforce `max_pools_for_rpc_streaming` and prefer HyperSync for larger universes.

---

## 6) Order semantics mapping (AMM vs Nautilus)

### 6.1 Supported order types (MVP)

- `OrderType.MARKET` only.
- Single-pool only (no multi-hop pathing).
- No post-only / limit orders in MVP (AMM doesn’t support native limit orders).

### 6.2 Side/quantity mapping for pool instrument (token0/token1)

Treat the pool as a spot pair `token0/token1`:

- **SELL base (token0)** → exact-input swap:
  - `amount_in = order.quantity (token0 units)`
  - Quote `amount_out = getAmountsOut(amount_in, [token0, token1])` (already includes AMM LP fee + price impact)
  - `amount_out_min` slippage math (MVP, required):
    - compute in integer U256 only (no floats/Decimal), rounding **down** (conservative)
    - `amount_out_min = floor(amount_out * (10_000 - slippage_bps) / 10_000)`
  - Router call: `swapExactTokensForTokens(amount_in, amount_out_min, [token0, token1], wallet, deadline)`

- **BUY base (token0)** → exact-output swap:
  - `amount_out = order.quantity (token0 units)`
  - Quote `amount_in = getAmountsIn(amount_out, [token1, token0])` (already includes AMM LP fee + price impact)
  - `amount_in_max` slippage math (MVP, required):
    - compute in integer U256 only, rounding **up** (conservative)
    - `amount_in_max = ceil(amount_in * (10_000 + slippage_bps) / 10_000)`
      - recommended implementation: `(amount_in * (10_000 + slippage_bps) + 9_999) / 10_000`
  - Router call: `swapTokensForExactTokens(amount_out, amount_in_max, [token1, token0], wallet, deadline)`

**Adapter capability note (important):** not all AMM backends support exact-output safely in the first iteration.
For example, PCS V3 Phase 1 in this plan supports `exactInputSingle` only. In those cases:

- `BUY base` orders MUST be rejected as `Unsupported` unless the adapter explicitly enables exact-output
  (e.g., `v3_enable_exact_output=true` + tests proving quote/revert behavior is reliable).

### 6.3 Order lifecycle and reporting

Suggested lifecycle for swap tx:

1. `SUBMITTED` when Nautilus accepts the command.
2. `ACCEPTED` once the **swap tx** broadcast is acknowledged:
   - `eth_sendRawTransaction` returns success (tx hash), **or**
   - broadcast is ambiguous-but-actionable (e.g., timeout-after-send, “already known”); in this case accept and track by the computed `tx_hash`.
   - Do **not** emit `OrderAccepted` just because signing succeeded and `tx_hash` is derivable if broadcast definitively failed.
3. `FILLED` when receipt status=1 and logs decode to actual amounts.
4. `REJECTED` when:
   - signer denies (policy)
   - RPC rejects tx
   - receipt status=0 (revert)
5. `CANCELED` only if we implement replacement/nonce-cancel flows (Phase 2).

**Execution event contract (required, repo-aligned):**

- `submit_order` MUST be non-blocking: it validates and enqueues an async workflow, emits `OrderSubmitted`, and returns.
  - signing/broadcast/receipt-wait/decoding MUST happen on background tasks, emitting `OrderAccepted` / `OrderRejected` / `FillReport` asynchronously.
  - do not block the calling thread waiting for receipts (this is incompatible with Nautilus’ execution engine model and can deadlock under load).
- Event ordering MUST be monotonic per order: `Submitted` → (`Accepted` | `Rejected`) → (`Filled` | `Rejected`).
- On restart recovery, event emission MUST be idempotent:
  - never emit a second fill for the same `(tx_hash, log_index)`,
  - never emit `Accepted` twice for the same swap tx hash,
  - use the tx journal as the source of truth for what was already emitted.

**Multi-transaction reality (approve → swap):**

A single user “swap order” may require:

- `TxIntent(approve)` (optional, if allowance insufficient), then
- `TxIntent(swap)` (required).

MVP rule: approvals are **internal prerequisites**. The Nautilus order’s `venue_order_id` MUST refer
to the **swap tx hash** (not the approve tx hash), and `OrderAccepted` MUST be emitted only once the
swap tx hash is known.

**Repo-verified constraint:** `VenueOrderId` has no explicit length cap (ASCII-only). Using a `0x`-prefixed tx hash (66 chars)
as the `venue_order_id` is valid in Nautilus today. If this changes in the future, keep the full tx hash as the journal source of truth
and introduce a deterministic shortened mapping for `venue_order_id` (but do not truncate the journal key).

**Durability / restart recovery (required):**

- Persist a per-order “tx journal” (minimal durable store) containing:
  - `client_order_id` → ordered list of intents (`approve?`, `swap`) with `intent_hash`, `tx_hash`, `raw_tx_hash`, timestamps, and last-known status
  - last-known `nonce` reservations for each in-flight intent
- **MVP backend recommendation (durability semantics):**
  - append-only JSONL file per `(venue, wallet)` under a configured data dir (e.g., `.../execution_journal/<venue>/<wallet>.jsonl`)
  - write semantics: append one line per state transition, `flush()` + `fsync()` after each append (fail closed if fsync fails)
  - read semantics: on startup, replay lines in order; tolerate/ignore a final partial line (crash mid-write)
  - optional: periodic compaction into a snapshot file + rotate the log once it grows beyond a cap
- On startup, load the journal, reconstruct in-flight work, and poll receipts *before* emitting mass status:
  - if `tx_hash` exists but receipt missing → keep `ACCEPTED`/`SUBMITTED` and continue polling
  - if receipt exists and confirmed → emit fill/status deterministically (idempotent; never double-emit)

**Duplicate submit idempotency (required):**

- Within a process lifetime and across restarts, treat `(venue, wallet_address, client_order_id)` as an idempotency key.
- If a `SubmitOrder` arrives for an existing non-terminal key:
  - return success without re-signing/re-broadcasting (no-op),
  - re-emit nothing immediately; allow the async workflow to continue and eventually emit `Accepted/Filled/Rejected`.
- If a `SubmitOrder` arrives for an existing terminal key:
  - reject deterministically with `AmmError::Unsupported { capability=\"duplicate_submit\" ... }` (or a dedicated error kind),
  - include the terminal status and `tx_hash` in details for operator debugging.

**Ambiguous broadcast handling (required):**

- Always compute `tx_hash = keccak256(raw_tx_bytes)` immediately after signing (before RPC send):
  - hex-decode `raw_tx_hex` to bytes and hash the bytes directly (do **not** RLP-encode again).
  - For typed transactions (EIP-2718, type `0x02`), the type prefix is part of `raw_tx_bytes` and MUST be included in the hash.
- If `eth_sendRawTransaction` returns a tx hash, require it equals the computed `tx_hash`; on mismatch fail closed with `RAW_TX_MISMATCH` (do not proceed with receipt polling).
- If `eth_sendRawTransaction` times out or returns an “already known” style error, continue lifecycle tracking by `tx_hash` (do not re-sign/rebuild).

**Receipt identity checks (required, fail-closed):**

- Do not trust `receipt` alone to identify the tx: receipts do not include `from` or `nonce`.
- When a receipt is observed, also fetch `eth_getTransactionByHash(tx_hash)` and verify invariants:
  - `from == wallet_address`
  - `nonce == reserved_nonce`
  - `to` equals the expected contract (`router_address` for swap; token address for approve; WBNB for wrap/unwrap when enabled)
  - `chain_id` matches config (guard against provider misrouting)
- If these invariants fail, fail closed with `AmmError::TxLifecycle` and persist the mismatch in the journal (this indicates mis-signed tx, provider bug, or corruption).

**Confirmations gating (required):**

- Do not emit terminal `FILLED/REJECTED` until `confirmations_required` blocks have elapsed and the receipt is still present (reorg-safe default).

Fill should include:

- `trade_id` derived from `(tx_hash, log_index)` **and** guaranteed to fit Nautilus’ 36-char limit:
  - recommended: `trade_id = hex(keccak256(tx_hash_bytes || u32_be(log_index))[0..16])` (32 hex chars)
- actual in/out amounts from logs (not from quote)
- commissions: on-chain fee is embedded in execution price; gas fees should be represented separately (see “Gas accounting” below).

**Fill quantity/price mapping (MVP, single-hop V2, instrument = `token0/token1`):**

- Decode the pair `Swap` log and extract the non-zero in/out legs.
- Decode and require `Swap.to == wallet_address` (recipient invariant for single-hop swaps; fail-closed if not).
- Treat `token0` as Nautilus *base* and `token1` as *quote*:
  - SELL base (`token0 -> token1`): `base_qty = amount0_in`, `quote_qty = amount1_out`, `last_px = quote_qty / base_qty`
  - BUY base (`token1 -> token0`): `base_qty = amount0_out`, `quote_qty = amount1_in`, `last_px = quote_qty / base_qty`
- Convert on-chain integers to Nautilus `Quantity`/`Price` using token decimals stored on the instrument (quantize to ≤16 dp).

### 6.4 Rust reconciliation contract (minimum required)

This section is the **minimum execution/reports contract** PCS must satisfy for Rust reconciliation
to work reliably in Nautilus.

**Required `ExecutionClient` methods (phase-1):**

1. `submit_order` (market-only for MVP) with signer-driven tx lifecycle.
2. `generate_mass_status` returning `Some(ExecutionMassStatus)` for startup reconciliation.
3. `register_external_order` to track venue orders created during reconciliation.
4. `connect` / `disconnect` / `start` / `stop` implemented (non-panicking).
5. All other currently-unimplemented trait methods must return deterministic non-panicking
   `Unsupported`/empty results (no `todo!()` at runtime).

**Required report coverage (phase-1):**

- `ExecutionMassStatus` must include all three collections:
  - `order_reports`
  - `fill_reports`
  - `position_reports`
  (collections may be empty; they must exist and be typed correctly).
- Determinism requirement: when generating mass status, sort reports in a stable order to avoid reconciliation flapping:
  - fills: primary `(tx_hash, log_index)` (or `(block_number, tx_index, log_index)` if available)
  - orders: stable by `venue_order_id` then `ts_last`
- `OrderStatusReport` minimum fields for reconciliation:
  - `instrument_id`, `venue_order_id`, `order_side`, `order_type`, `time_in_force`,
    `order_status`, `quantity`, `filled_qty`, `post_only`, `reduce_only`, `ts_accepted`, `ts_last`
  - plus strongly recommended: `client_order_id`, `price`/`trigger_price`/`avg_px`, `cancel_reason`, `venue_position_id`.
  - reconciliation note: for AMM execution adapters, **successful receipts must always decode to real `FillReport`s**.
    Reconciliation “fill inference” should be treated as a last-resort fallback for historical/external orders, not a normal success-path.
- `FillReport` minimum fields:
  - `venue_order_id`, `trade_id`, `instrument_id`, `order_side`, `last_qty`, `last_px`,
    `liquidity_side`, `account_id`, `ts_event`
  - optional but recommended: `client_order_id`, `commission`, `venue_position_id`.
- `PositionStatusReport` minimum fields:
  - `instrument_id`, `signed_decimal_qty`, `ts_last`
  - strongly recommended: `position_side`, `quantity`, `avg_px_open`, `venue_position_id`.

**Lifecycle mapping for reconciliation compatibility:**

| Stage | Client action | Nautilus event/report output | Reconciliation dependency |
|---|---|---|---|
| Command accepted | `submit_order` validates + builds swap intent | `OrderSubmitted` event | `client_order_id`, `instrument_id`, `strategy_id` |
| Broadcast acknowledged | send signed tx (`eth_sendRawTransaction`) | `OrderAccepted` event + `tx_hash` tracking | `venue_order_id` is the **swap tx hash** |
| Receipt success | parse swap logs | `FillReport` + `OrderStatusReport(Filled/PartiallyFilled)` | `trade_id`, cumulative `filled_qty`, prices |
| Receipt failure | parse revert / failed receipt | `OrderStatusReport(Rejected)` (Phase 1; `Canceled` only once cancel/replace flows exist) | terminal `order_status`, `cancel_reason`, timestamps |
| Startup reconciliation | `generate_mass_status` | full `ExecutionMassStatus` | order/fill/position maps keyed consistently |
| External order sync | `register_external_order` | adapter-internal tracking update | `client_order_id` ↔ `venue_order_id` persistence |

---

## 7) Signer integration details (port the pattern from chainsaw)

### 7.1 Chainsaw reference implementation

Chainsaw signer client and schemas (port the *shape*, not the bugs):

- Transport/client: `/home/ubuntu/chainsaw/engine/evm/signer/client.py`
- Schemas: `/home/ubuntu/chainsaw/engine/evm/signer/schemas.py`
- Tx field builder: `/home/ubuntu/chainsaw/engine/evm/signer/adapter.py`

Key notes from chainsaw:

- Remote signer endpoints are per “route” (`/sign/eth`, `/sign/eip712`, …).
- mTLS is optional.
- 4xx should not be retried (policy denials).
- Include “intent” metadata for policy enforcement (slippage/deadline/router/function selector).

### 7.2 OSS signer-server API contract (verified against running binary)

Verified against `/home/ubuntu/signer-server` source and live probes (`POST /sign/eth`):

- Request shape is **flat only** (Go struct `SignETHRequest`), not nested:
  - `chainId` (`uint64`)
  - `to` (`string`)
  - `data` (`string`)
  - `maxFeePerGas` (`uint64`)
  - `maxPriorityFeePerGas` (`uint64`)
  - `gas` (`uint64`)
  - `nonce` (`uint64`)
  - `value` (`string`, parsed as hex bytes)
  - `deadline` (`int64`, unix seconds)
  - `expected_notional` (`string`)
- Response shape:
  - `{ "r": "0x…", "s": "0x…", "v": <0|1>, "raw_tx_hex": "0x…" }`
- Nested payloads are not understood (`{"tx":...,"intent":...}` yields `unsupported_chain` because `chainId` stays zero).
- Numeric fields must be JSON numbers, not hex strings (`"0x38"` for `chainId` fails as `bad_request`).
- `value` must be string; numeric JSON `value` fails as `bad_request`.
- `value` string is treated as hex (no decimal parsing); `"1000"` is accepted and interpreted as `0x1000`.
- Unknown fields are silently ignored by server (`max_slippage_bps`, `function_sig`, `router` are ignored today).
- Server always builds an **EIP-1559 DynamicFeeTx** internally; no legacy tx type path.
- `gasPrice` input is ignored (not part of request struct), which can bypass fee policy if client sends only `gasPrice`.

**Tx-type compatibility guard (required, fail-closed):**

- Because the OSS signer-server emits **type-2 (EIP-1559) dynamic-fee transactions only**, the chain/RPC must accept type-2 txs.
- On `connect/startup`, probe chain capabilities (avoid false negatives on BSC/provider quirks):
  1) Call `eth_getBlockByNumber("latest", false)`:
     - if the response contains a `baseFeePerGas` field (even `"0x0"`), treat the chain as **type-2 allowed**.
  2) Else, call `eth_feeHistory`:
     - if it succeeds, treat the chain as **type-2 allowed**.
  3) Else, treat tx-type support as **unknown**.
- If support is unknown and `signer_api_mode == oss_v1_flat` (type-2 only), fail closed by default with a deterministic
  `AmmError::Unsupported { capability = \"chain_tx_type\" }`, **unless** config sets `chain_tx_type_override = Type2Allowed`.
- Always log the probe results (methods attempted + error messages) so ops can fix mis-detection quickly.

### 7.3 Canonical Nautilus schema + compatibility mapping

Nautilus should keep a **canonical internal schema** and map it to OSS signer-server flat payload at the transport boundary.

**Canonical internal request (Nautilus-side):**

```json
{
  "tx": {
    "chain_id": 56,
    "nonce": 123,
    "to": "0xRouterOrToken",
    "data": "0x095ea7b3...",
    "value_wei_hex": "0x00",
    "gas_limit": 250000,
    "max_fee_per_gas_wei": 3000000000,
    "max_priority_fee_per_gas_wei": 1000000000
  },
  "intent": {
    "deadline_unix": 1710000000,
    "expected_notional": "123.45",
    "max_slippage_bps": 80,
    "function_sig": "swapExactTokensForTokens(uint256,uint256,address[],address,uint256)",
    "function_selector": "0x38ed1739",
    "router": "0x...",
    "policy_tag": "pcs-v2-swap"
  }
}
```

**Transport mapping for OSS signer-server (`signer_api_mode = oss_v1_flat`):**

- Flatten to the exact fields accepted by `SignETHRequest`.
- Drop unknown fields from payload (keep them for local checks and logs).
- Always send EIP-1559 fee fields; **never rely on `gasPrice` passthrough**.
- If upstream source is legacy `gasPrice`, map explicitly:
  - `maxFeePerGas = gasPrice`
  - `maxPriorityFeePerGas = min(config.priority_fee_cap, gasPrice)`

**Strict type/format requirements in Nautilus before POST:**

- `chainId`, `nonce`, `gas`, `maxFeePerGas`, `maxPriorityFeePerGas`: unsigned integers.
- `to`: normalized checksum address string.
- `data`: `0x`-prefixed, even-length hex.
- `value`: `0x`-prefixed, even-length hex string only (reject decimal-looking strings).
  - canonical zero MUST be `"0x00"` (reject `"0x"`, `"0"`, `""`, or other ambiguous forms).
- `deadline`: absolute unix timestamp (seconds).
- `expected_notional`: decimal string.

**`expected_notional` semantics (must be consistent):**

This field is used by OSS signer policy (`policy.go`) to enforce `max_notional` as a decimal-string comparison.
Because the signer does not know token units, Nautilus must define a local convention.

Recommended convention (matches `~/chainsaw` patterns in `engine/gateways/pancakeswap_gateway.py`):

- For swaps: set `expected_notional` to the **minimum expected output** (human-readable decimal string) in `token_out` units:
  - exact-in: `amount_out_min` converted using `token_out.decimals`
  - exact-out: `amount_out` converted using `token_out.decimals`
- For native-value transactions (router-native swaps or wraps): keep `expected_notional` aligned with the human-readable native amount being spent.

### 7.4 Signer policy expectations (must-have for safety)

The signer should be able to enforce:

- max gas price / max fee per gas
- max slippage bps
- max deadline seconds
- allowlisted `to` addresses (router, ERC20 tokens for approve)
- allowlisted function selectors / signatures (swap / approve only)
- optionally max notional (expected_notional)

Nautilus must pass these policy-relevant fields in every signing request.
Because current OSS signer-server enforces only `to + selector + maxFee + deadline + expected_notional`, Nautilus must also perform local preflight checks for:

- `maxPriorityFeePerGas <= maxFeePerGas`
- non-zero `maxFeePerGas` for execution paths
- selector derived from `data` matches expected function for operation
- slippage/deadline intent consistency (tx arguments vs intent metadata)

**Operational note (important):**

- In OSS signer-server mode, if the signer does not decode `approve(address,uint256)` calldata, it cannot enforce `spender == router`.
  This implies the signer policy must allowlist each ERC20 token contract address which may be approved (token `to`), which is operationally
  heavy for large universes.
- MVP is still viable because the pool universe is config-driven and small.
- Post-MVP hardening recommendation: extend signer-server to ABI-decode `approve(address,uint256)` and enforce:
  - spender == configured router (or router allowlist),
  - amount <= policy cap,
  - (optional) token codehash/interface checks.

### 7.5 OSS signer-server stress-test findings (2026-03-04) and required client-side guards

Validated against `/home/ubuntu/signer-server` source (`main.go`, `policy.go`, `eth_sign.go`) and live probes:

- payload must be flat `SignETHRequest`; nested payloads drop `chainId` to zero and fail as `unsupported_chain`
- unknown JSON fields are ignored silently (`json.Decoder.Decode` without `DisallowUnknownFields`)
- `chainId`/`nonce`/gas fields must be JSON integer numbers (string/float forms fail)
- `value` must be a JSON string and is parsed as hex bytes; decimal-looking strings like `"1000"` are accepted as `0x1000`
- signer always builds EIP-1559 `DynamicFeeTx`; `gasPrice` is ignored
- requests with no EIP-1559 fee fields can still be signed with zero fee caps
- requests with `maxPriorityFeePerGas > maxFeePerGas` can still be signed
- deadline check is only upper-bound; past/negative deadlines can still be signed
- `expected_notional` empty string is treated as zero

**Plan implication:** Nautilus must implement fail-closed preflight validation before any signer request:

- **schema/type preflight**
  - enforce canonical nested schema internally, then flatten only at transport boundary
  - reject any non-integer numeric fee/nonce/gas/chain fields
  - enforce `value` format as strict hex (`0x`-prefixed, even length, no decimal-like strings)
- **fee preflight**
  - require explicit EIP-1559 fields in OSS mode (`maxFeePerGas`, `maxPriorityFeePerGas`)
  - reject `maxFeePerGas == 0` (unless an explicit simulation mode is enabled)
  - reject `maxPriorityFeePerGas > maxFeePerGas`
  - if upstream source provides only `gasPrice`, map explicitly and record the mapping decision
- **balance preflight**
  - ERC20 balance:
    - for exact-in swaps: require `balance(token_in) >= amount_in`
    - for exact-out swaps: require `balance(token_in) >= amount_in_max`
  - native gas balance: require `native_balance >= worst_case_gas_cost + tx.value` (fail closed with `INSUFFICIENT_NATIVE_GAS_BALANCE`)
- **deadline/notional preflight**
  - if adapter `supports_deadline_arg=true`: require `deadline` to be absolute unix seconds, `deadline > now + min_ttl`, and `deadline <= now + max_ttl`
  - if adapter does **not** support a per-swap deadline arg (e.g., SmartRouter V2-style swaps): enforce TTL via higher-level mechanisms only (e.g., UniversalRouter top-level deadline) and tighten slippage caps
  - require `expected_notional` to be non-empty positive decimal for swaps
- **intent↔calldata preflight**
  - derive selector from calldata and require exact match with operation type
  - ABI-decode swap/approve args and verify policy-critical fields (`to`, selector, deadline, minOut/maxIn, recipient, spender)
- **post-sign verification pre-broadcast**
  - decode `raw_tx_hex` returned by signer and assert all tx fields equal the preflighted request
  - recover sender (`from`) from the signed tx and require it equals configured `wallet_address` (fail closed on mismatch)
  - reject broadcast on any mismatch (type, chain_id, to, data, value, nonce, gas, fee caps)

### 7.6 Intent binding + chain fingerprinting (reduce mixups/misroutes)

To make execution + reconciliation deterministic across restarts and retries, introduce a canonical
`TxIntent` concept and bind all lifecycle steps to it.

**Intent binding (required):**

- Define `TxIntent` as the canonical struct used throughout:
  - `chain_id`, `from`, `to`, `data`, `value`, `nonce`, `gas`, `max_fee_per_gas`, `max_priority_fee_per_gas`
  - `deadline`, `expected_notional` (policy fields)
  - adapter metadata: `client_order_id`, `operation` (`approve`/`swap`/`wrap`/`unwrap`), `pool_address`
- Compute `intent_hash = keccak256(abi_encode(TxIntentCoreFields...))` with a strictly defined field order.
- Persist `intent_hash` and use it as the internal idempotency key:
  - signer request tracking
  - raw tx verification
  - retries/backoff: all retries for the *same* intent MUST reuse the same `intent_hash` (if any core tx field changes, treat it as a new intent and persist a new record)
  - broadcast and receipt polling
  - reconciliation dedupe (never reconcile the same `intent_hash` twice)
- (Optional, future signer): include `intent_hash` as an additional JSON field in signer requests for audit logs.

**Chain fingerprinting (recommended, fail-closed):**

- In addition to `eth_chainId`, fetch `eth_getBlockByNumber(0)` at startup and compare the genesis block hash
  to a configured expected value per chain (protects against misrouting to a fork/alt chain with the same chainId).
- Re-check periodically (or on first tx send) and halt execution on mismatch.

**Typed-data signing (EIP-712) (explicitly out-of-scope for MVP, but plan for it):**

- PancakeSwap’s more advanced routing stacks (Permit2 / UniversalRouter / Infinity) may require **EIP-712** signatures.
- The current OSS signer-server used by this plan signs **transactions only**.
- If/when adopting these stacks, add a signer capability for typed-data signing (new signer route + strict domain/chain binding + allowlisted types),
  and extend the AMM framework to treat “permit-like” signatures as first-class prerequisites (similar to approve).

---

## 8) Gas / nonce / transaction management

### 8.1 Nonce management (required)

Concurrency is a primary source of DEX execution bugs.

Implement a per-wallet `NonceManager`:

- Initialize from `eth_getTransactionCount(wallet, "pending")`
- Reserve nonces for in-flight txs
- On replacement / reorg handling, reconcile by querying pending nonce and receipts

MVP simplification:

- Allow only one in-flight tx per wallet (serialize execution) to avoid complex replacement logic.
- Add a config flag later for parallel execution + robust nonce manager.

### 8.2 Fee model

Support both:

- **Legacy** fee: `gasPrice` (BSC may be legacy depending on node)
- **EIP-1559** fee: `maxFeePerGas`, `maxPriorityFeePerGas`

Implementation approach:

- Canonical internal representation is always EIP-1559-style.
- If chain supports *non-zero* baseFee: use estimator-derived EIP-1559 values.
- If chain returns `baseFeePerGas == 0` (BSC commonly observed; treat as “zero-basefee mode”):
  - do **not** assume Ethereum-like basefee dynamics
  - derive EIP-1559 caps from `eth_gasPrice` + policy floors/caps (e.g., `maxFee=gasPrice`, `maxPriority=min(priority_cap, gasPrice)`)
- If chain is effectively legacy (no baseFee): derive EIP-1559 fields from `gasPrice` (`maxFee=maxPriority=gasPrice` or configured capped mapping).
- Never send only `gasPrice` to signer-server in OSS mode.
- **Signer constraint:** the OSS signer-server always emits EIP-1559 dynamic-fee txs (type 2). There is no legacy-tx fallback unless the signer changes.

Expose config:

- `gas_limit_multiplier` (e.g., 1.2)
- `max_fee_gwei` / `gas_price_gwei`
- `priority_fee_gwei`

### 8.3 Gas accounting in Nautilus

**MVP decision (pick one and keep consistent across future DEXs): Option A**

- **MVP behavior:** do **not** represent gas costs as `FillReport.commission` and do not attempt to emit account state updates.
  - Set `FillReport.commission = Money(0, instrument.quote_currency())` (consistent with existing reconciliation defaults).
  - Compute gas cost precisely and store it in the tx journal / logs for auditability (per-tx: approve/wrap/swap).
- **Post-MVP option (Option B):** add explicit `AccountBalance`/`AccountState` updates for native gas token spend once the blockchain account model is implemented.

**Required computation (regardless of reporting option):**

- For each broadcasted tx (approve/wrap/swap), once receipt is available:
  - `gas_used = receipt.gasUsed`
  - `effective_gas_price_wei = receipt.effectiveGasPrice` (preferred when present)
  - `gas_cost_wei = gas_used * effective_gas_price_wei`
- If `effectiveGasPrice` is missing from the node/receipt:
  - fallback to `eth_getTransactionByHash` for `gasPrice` / EIP-1559 fields, and/or
  - fallback to `min(tx.maxFeePerGas, block.baseFeePerGas + tx.maxPriorityFeePerGas)` when baseFee is available.

**Attribution rule (MVP):**

- Treat approve + swap as part of one user order lifecycle, but track gas per-tx.
- If an approve tx is required and succeeds, but the swap tx later fails, still report the approve gas cost.
- If a tx is replaced (same nonce, different hash), **only** account for the *mined* tx and persist the replacement chain in the journal
  to avoid double-counting.

**Data plumbing (MVP):**

- Extend the RPC receipt type parsing to include `gasUsed` and `effectiveGasPrice` (Milestone 4).
- Store gas telemetry on the order/venue-order tracking structure (tx hash, gas used, gas cost) for auditability.

### 8.4 Execution configuration surface (MVP + production knobs)

PCS execution needs a config surface that is:

- safe by default (timeouts, confirmations, allowlists)
- explicit (no hidden global constants)
- reusable across future AMM integrations
- overrideable per-order *only* where safe

**Recommended shape:** extend `BlockchainExecutionClientConfig` to contain:

**Backward-compatibility requirement (repo-aligned):**

- All new config fields MUST have safe defaults (`Default`/optional fields) and preserve constructor compatibility across Rust + PyO3.
- Avoid “required new args” in Python unless the feature cannot be made safe by default; prefer explicit validation errors over breaking constructors.

1) **Core routing/identity**

- `venue: Venue` (must be the DEX venue, e.g., `Bsc:PancakeSwapV2`; reject non-DEX venues for AMM execution)
- `chain: Chain` (must match venue chain)
- `wallet_address: String`
- Wallet support (DeFi wallet snapshot; see section 8.7):
  - `wallet_extra_tokens: Vec<String>` (optional; additional ERC20 addresses to track beyond those implied by the pool universe)
  - `wallet_allowance_spenders: Vec<String>` (default: `[router_address]`; add Permit2 spender when enabled)
  - `wallet_snapshot_ttl_secs: u32` (preflight must refresh if older than TTL)
  - `wallet_max_tokens_per_refresh: u32` (protects managed RPC; fail-closed if universe exceeds cap in MVP)
  - `wallet_refresh_on_connect: bool` (default true)
  - `wallet_refresh_interval_secs: Option<u32>` (default None; if set, periodically refresh balances/allowances within RPC budgets)
- `http_rpc_url: String`
- `wss_rpc_url: Option<String>` (recommended in production; enables new-heads-driven receipt polling and (future) log streaming to reduce HTTP polling load)
- `provider_profile: Option<String>` (e.g., `chainstack_limited`; may override internal defaults for backoff/splitting/budgets)
- `rpc_requests_per_second: Option<u32>`
- `rpc_max_concurrent_requests: u32` (defensive cap to avoid bursty load; critical when RPC is rate-limited)
- `ws_max_subscriptions: u32` (defensive cap; avoid exhausting provider subscription limits)
- `ws_idle_timeout_ms: u64` (close/reconnect WS if idle; prevents half-open hangs)
- `ws_stale_after_ms: u64` (if no `newHeads` arrives within this window, enter degraded mode and use watchdog polling)
- `ws_reconnect_max_attempts: u32` (cap flapping; after cap, enter degraded mode and require operator intervention)
- `rpc_timeout_ms: u64` (avoid hung execution paths)
- `rpc_retry_config: nautilus_network::retry::RetryConfig` (bounded retries with jitter; method-specific non-retryable errors; implemented via `nautilus_network::retry::RetryManager`)
- `expected_genesis_hash: Option<String>` (recommended for production; used for chain fingerprinting, see section 7.6)
- Multicall controls (startup metadata/balance reads; protects managed RPC):
  - `multicall_max_batch_size: u32`
  - `multicall_min_batch_size: u32`
  - `multicall_max_response_bytes: u32`
  - `multicall_adaptive_split_on_timeout_or_revert: bool`

2) **Signer**

- `signer_endpoint: String`
- `signer_route: String` (default: `/sign/eth`)
- `signer_api_mode: SignerApiMode` (default: `oss_v1_flat`)
- `signer_timeout_ms: u64`
- optional mTLS fields (paths or inline PEMs, consistent with other Nautilus configs)
- `signer_require_tls: bool` (default true; reject `http://` unless explicitly allowed for local dev)
- (optional) `signer_tls_pinned_pubkey_sha256: Option<String>` (defense-in-depth against MITM if mTLS is not used)
- `signer_capabilities: SignerCapabilities`
  - `enforces_slippage: bool` (OSS today: false)
  - `enforces_deadline: bool` (OSS today: partial; upper-bound only)
  - `enforces_expected_notional: bool`
  - `supports_nested_payload: bool` (OSS today: false)
  - `supports_unknown_field_rejection: bool` (OSS today: false)
  - `supports_eip712: bool` (typed-data signing for Permit2/UniversalRouter; OSS today: false)
- `signer_fail_closed_on_capability_gap: bool` (default true)
- `signer_reject_gas_price_only: bool` (default true)
- `signer_require_eip1559_fields: bool` (default true)
- `signer_verify_raw_tx_response: bool` (default true)
- `signer_clock_skew_tolerance_secs: u32` (for deadline checks)
- `signer_retry_config: nautilus_network::retry::RetryConfig` (implemented via `nautilus_network::retry::RetryManager`)
  - explicit non-retry list for 4xx (`bad_request`, `unsupported_chain`, `policy_denied`)
  - bounded retries for network/5xx only

3) **AMM execution (DEX-agnostic)**

- `dex_type: DexType` (must match `venue.parse_dex().dex_type`; reject mismatch)
- `router_address: String`
- `router_abi_mode: RouterAbiMode`
  - `ClassicV2` (PancakeRouter02-style; swap methods include `deadline`; **MVP required**)
  - `SmartRouter` (swap methods omit `deadline`; deadline must be enforced via `multicall(uint256,bytes[])`)
  - `UniversalRouter` (`execute(bytes,bytes[],uint256 deadline)`; command encoding; optional Permit2 integration)
    - **Safety rule:** UniversalRouter deployments sometimes expose an `execute(bytes,bytes[])` overload (no deadline). The adapter MUST reject
      this selector and require the deadline-bearing selector only (see Milestone 7f).
- `permit_mode: PermitMode` (default: `Disabled`; used by UniversalRouter/Permit2 tracks)
  - `Disabled` (MVP default; approvals handled separately; UniversalRouter swap-only flows can run without typed-data)
  - `Permit2AllowancePreApproved` (no typed-data; requires pre-approved Permit2 allowance out-of-band)
  - `Permit2Signature` (requires EIP-712 typed-data signing; one-tx permit+swap)
- **Config validation rules (fail-closed):**
  - if `router_abi_mode == UniversalRouter` and `permit_mode == Permit2Signature`, require `signer_capabilities.supports_eip712 == true`
    and require explicit typed-data signer route configuration; otherwise fail closed.
  - if `router_abi_mode != UniversalRouter`, require `permit_mode == Disabled` in MVP (avoid accidental Permit2 coupling).
- optional `factory_address: String` (used for validation)
- optional `quoter_address: String` (V3-style quoting; unused for V2)
- `v3_enable_exact_output: bool` (default false; enable after quote reliability metrics are proven in production)
- optional `mixed_route_quoter_address: String` (SmartRouter-style quoting; unused for V2)
- optional `stable_factory_address: String` (StableSwap pool discovery/validation; unused for V2)
- optional `stable_info_address: String` (StableSwap info contract; unused for V2)
- optional `wnative_address: String` (WBNB/WETH; used for safety checks and future routes)
- `enable_native_value_swaps: bool` (default false; Phase 2: allow router-native ETH/BNB methods that require `tx.value > 0`)
- `enable_wrap_unwrap: bool` (default false; Phase 2: allow WETH9-style `deposit()`/`withdraw()` txs)
- `slippage_bps_default: u32`
- `slippage_bps_max: u32` (policy cap enforced locally; applies to defaults and per-order overrides)
- `quote_max_age_ms_before_sign: u32` (default ~2000; fail-closed if quote is too old at broadcast time)
- `requote_after_approve: bool` (default true; if an approve tx is mined, always refresh quote before building swap)
- `deadline_secs_default: u32`
- `deadline_min_ttl_secs: u32` (default >0; prevent stale deadlines)
- `deadline_max_ttl_secs: u32` (policy cap enforced locally; applies to defaults and per-order overrides)
- `approval_policy: ApprovalPolicy`
  - `Exact` (approve only needed delta)
  - `Unlimited` (approve `U256::MAX` once when allowance insufficient)
  - `UnlimitedResetFirst` (approve 0 then max; for non-standard tokens)
- (recommended safety if `Unlimited*` is enabled) `unlimited_approval_token_allowlist: Vec<String>` (empty by default; fail-closed)
- (optional) `unlimited_approval_max_amount: Option<String>` (per token, human units) to cap blast radius even when unlimited approvals are allowed
- `confirmations_required: u32` (default 1)
- `receipt_timeout_secs: u32`
- `receipt_poll_interval_ms: u32`
- `receipt_watchdog_ms: u32` (fallback poll interval when WS heads are stale; prevents “no heads ⇒ no receipts”)
- `max_receipt_polls_per_tx: u32` (hard cap; after cap, classify as `Receipt::Timeout` / `TX_STALLED` depending on probes)
- `max_null_receipt_blocks: u32` (BSC/provider quirk: tolerate a limited number of “null receipt” blocks before probing tx/nonce)
- (optional, provider-profile driven) `receipt_poll_backoff_initial_ms: u32`, `receipt_poll_backoff_max_ms: u32`, `receipt_poll_jitter: f64`
- `max_inflight_txs_per_wallet: u32` (MVP default 1, required for nonce safety)

**Token safety policy (MVP defaults):**

- `deny_fee_on_transfer_tokens: bool` (default true; allow override only via explicit allowlist)
- `deny_rebasing_tokens: bool` (default true; allow override only via explicit allowlist)
- `fee_on_transfer_allowlist: Vec<String>` (addresses; empty by default)
- `rebasing_allowlist: Vec<String>` (addresses; empty by default)

4) **Gas policy**

- `gas_limit_strategy: GasLimitStrategy` (recommended for managed RPC providers):
  - `Estimate` (always call `eth_estimateGas`)
  - `Fixed` (never call `eth_estimateGas`; use conservative fixed limits per tx kind)
  - `EstimateWithFallbackFixed` (try estimate; fallback to fixed on timeout/rate-limit/unsupported)
  - fixed defaults (tunable per chain):
    - `approve_gas_limit: u64`
    - `swap_gas_limit: u64`
- `gas_limit_multiplier: f64` (default 1.2)
- `fee_strategy: FeeStrategy`:
  - `LegacyGasPrice` (derive EIP-1559 fields from `gasPrice`)
  - `Eip1559` (use `feeHistory`/baseFee where available)
- fee caps:
  - `max_fee_gwei` (or `max_fee_per_gas_wei`)
  - `priority_fee_gwei` (or `max_priority_fee_per_gas_wei`)
- `require_priority_fee_lte_max_fee: bool` (default true)
- `require_nonzero_max_fee: bool` (default true)
- (post-MVP) `replacement_policy` for stuck txs (gas bump + same nonce)

5) **Signer preflight policy (local, fail-closed)**

- `preflight_mode: PreflightMode`
  - `Strict` (default, required for production)
  - `WarnOnly` (staging only; forbid in production profiles/CI)
- `preflight_require_selector_match: bool` (default true)
- `preflight_require_calldata_policy_match: bool` (default true; ABI decode and verify)
- `preflight_enforce_allowlisted_to_locally: bool` (default true)
- `preflight_enforce_approve_spender_is_router: bool` (default true)
- `preflight_require_swap_value_zero: bool` (default true for ERC20 swaps)
- `preflight_require_notional_nonempty: bool` (default true for swap actions)
- `preflight_validate_uint64_bounds: bool` (default true for signer numeric fields)

**Per-order overrides (via `SubmitOrder.params`):**

Allow only parameters that do not change *where funds go*:

- `amm.slippage_bps` (u64)
- `amm.deadline_secs` (u64, relative; client converts to absolute unix seconds)
- `amm.expected_notional` (string; for signer policy/telemetry only)

Disallow per-order overrides for:

- router/factory/recipient addresses
- chain/venue/dex_type
- fee caps that could bypass signer policy
- signer/preflight safety toggles

**Validation bounds (fail-fast, explicit errors):**

- `slippage_bps_default` and `amm.slippage_bps` must be within a sane range (e.g., `0..=2_000` bps) and optionally capped by config
- `deadline_secs_default` and `amm.deadline_secs` must be within a sane range (e.g., `1..=3_600` seconds)
- `confirmations_required >= 1`
- `receipt_poll_interval_ms >= 200` and `receipt_timeout_secs >= 10` (and timeout must exceed poll interval meaningfully)
- `max_inflight_txs_per_wallet >= 1` (MVP should enforce `== 1` unless nonce manager supports parallelism)
- `gas_limit_multiplier >= 1.0` (and cap to prevent unbounded overpay)

### 8.5 Error handling: “as many DEX errors as possible” (detailed + actionable)

AMM execution failures are common and often opaque (especially on BSC). The adapter MUST make errors:

- **actionable** (user can fix config/params),
- **deterministic** (same failure yields same classification),
- **safe** (never “guess fills” on a successful receipt),
- **auditable** (retain raw JSON-RPC + revert data in the tx journal/logs).

This plan therefore requires three layers of error handling:

1) **JSON-RPC error capture** (code/message/data), per method, with retry classification.  
2) **EVM revert decoding** (Error(string), Panic(uint256), custom errors).  
3) **DEX-aware mapping** (router/library/helper revert reasons → canonical `DexErrorCode`).

**Classification precedence (required for determinism):**

Always classify in this order so the same failure never maps differently:

1) transport/infrastructure (HTTP status, WS disconnects, timeouts)  
2) JSON-RPC error (code/message/data)  
3) EVM revert decode + DEX mapping (selector/reason/custom error)  
4) receipt/decode invariants (missing logs, wrong pool, reorg artifacts)

#### 8.5.1 Canonical error taxonomy (Rust types)

Introduce a shared error model for AMM execution so all adapters can reuse it:

- `AmmError` (top-level, surfaced to `submit_order` caller and used for order rejection)
  - `Config { field, details }`
  - `Unsupported { capability, details }`
  - `Signer { status, code, message, body_preview, retryable }`
  - `Rpc { method, code, message, data_hex, retryable }`
  - `EvmRevert { stage, selector, reason, raw_data_hex, mapped: Option<DexErrorCode> }`
  - `TxLifecycle { kind, details }` (nonce too low, replacement underpriced, etc)
  - `Receipt { kind, details }` (timeout, missing, reorg, status=0, etc)
  - `Decode { kind, details }` (missing swap log, wrong pool, bad topics, etc)

And a small canonical enum for the “DEX reason” layer:

- `DexErrorCode` (stable string form used in logs + `cancel_reason`)
  - `INSUFFICIENT_OUTPUT_AMOUNT` (minOut too high / price moved)
  - `INSUFFICIENT_INPUT_AMOUNT` (amountIn too low / dust / reserve constraints)
  - `INSUFFICIENT_AMOUNT` (dust / invalid amount; often from router/library guards)
  - `EXCESSIVE_INPUT_AMOUNT` (exact-out with maxIn too low)
  - `EXPIRED` (deadline)
  - `INVALID_PATH` (bad path; wrong WNATIVE end or length < 2)
  - `INSUFFICIENT_LIQUIDITY` (pair doesn’t exist or reserves empty)
  - `TRANSFER_FROM_FAILED` (allowance/balance/transferFrom issues)
  - `TRANSFER_FAILED` (token transfer failed)
  - `APPROVE_FAILED` (approval failed; non-standard token or reset-first required)
  - `POOL_LOCKED` (pool locked/reentrancy guard; often transient)
  - `INVALID_PRICE_LIMIT` (sqrtPriceLimitX96 / SPL; typically bug or unsupported param)
  - `INVALID_INPUTS` (router command/input mismatch; typically bug)
  - `INVALID_TO` (invalid recipient; usually misconfiguration)
  - `IDENTICAL_ADDRESSES` / `ZERO_ADDRESS` (bad path/config)
  - `K` (invariant failure; typically indicates broken pair/token behavior)
  - `OUT_OF_GAS` (execution consumed tx gas limit before completion)
  - `UNKNOWN_REVERT` (fallback)

**Non-DEX canonical error codes (stable strings for `cancel_reason` when failures are not DEX reverts):**

- `CHAIN_ID_MISMATCH` (RPC endpoint is not the configured chain)
- `RAW_TX_MISMATCH` (signer returned a tx that doesn’t match the intent; fail closed)
- `INSUFFICIENT_NATIVE_GAS_BALANCE` (wallet lacks native gas token)
- `RATE_LIMITED` (provider throttling: HTTP 429, JSON-RPC `-32005`, etc)
- `WS_DISCONNECTED` (loss of `newHeads`/logs subscription; execution must degrade safely)
- `REORG_DETECTED` (receipt/log removed or block hash mismatch before finality)
- `NONCE_TOO_LOW` / `NONCE_TOO_HIGH` (wallet nonce drift vs node state)
- `REPLACEMENT_UNDERPRICED` / `FEE_TOO_LOW` (replacement/broadcast fee too low)
- `TX_DROPPED` / `TX_REPLACED` (accepted tx disappeared or was superseded)
- `EMPTY_REVERT_DATA` (revert with no data; preserve selector context as empty)

**User-facing guidance mapping (examples):**

| `DexErrorCode` | Typical cause | Actionable guidance |
|---|---|---|
| `INSUFFICIENT_OUTPUT_AMOUNT` | price moved / sandwich / slippage too tight | increase `amm.slippage_bps` (within policy), or reduce trade size, or use shorter deadline/private routing |
| `EXPIRED` | deadline too short/clock skew | increase `deadline_secs` (within cap), verify system clock + `signer_clock_skew_tolerance_secs` |
| `TRANSFER_FROM_FAILED` | allowance/balance/taxed token quirks | ensure allowance, ensure wallet has `token_in`, verify token is not fee-on-transfer in MVP |
| `INSUFFICIENT_LIQUIDITY` | wrong pool/path or empty reserves | verify pool config, verify `factory.getPair(tokenA,tokenB)`, reduce size or choose different pool |
| `INVALID_PATH` | wrong WNATIVE end / path order / misconfig | ensure `path[0]==token_in` and `path[-1]==token_out`; for native swaps ensure WNATIVE is at the correct end |
| `INVALID_TO` | bad recipient/path | verify swap recipient is wallet, and not token/pair addresses; verify adapter preflight |
| `APPROVE_FAILED` | token approval reverted/failed | enable reset-first approve, reduce allowance scope, or blocklist token; inspect revert in tx journal |
| `POOL_LOCKED` | pool reentrancy lock | retry with backoff; if persistent, suspect router/pool incompatibility or blocked token |
| `UNKNOWN_REVERT` | unknown/custom error | inspect tx journal for raw data; run `eth_call`/trace tooling |

**Important nuance (quoters):**

- Some ecosystems have “revert-as-return” quoters; PancakeSwap `QuoterV2` **returns normally** (it reverts internally in the swap callback and catches/decodes it).
- Quote calls must therefore be method-aware:
  - For classic V2 `getAmountsOut/In`, treat revert as an actual error.
  - For PancakeSwap `QuoterV2`, decode the normal ABI return tuple; if the call reverts, capture `error.data` (when present) and map to a typed quote error.

#### 8.5.2 Solidity/EVM revert decoding (required)

When the node returns revert data (from `eth_call`, `eth_estimateGas`, or error `data`):

- Decode `Error(string)` selector `0x08c379a0` to a UTF-8 reason string.
- Decode `Panic(uint256)` selector `0x4e487b71` to the panic code.
- If revert data is empty (`0x`), classify as `EMPTY_REVERT_DATA` (common with Solidity `require(cond)` without message).
- Otherwise treat as “custom error” (store the 4-byte selector + raw hex).
  - For V3/SmartRouter/Infinity, many revert conditions are **custom errors** (no revert string). Maintain a per-adapter
    selector → `DexErrorCode` map for the most common cases (and fall back to `UNKNOWN_REVERT` with raw selector recorded).

**Mapping rule (DEX-aware):** parse known DEX prefixes but map by suffix tokens:

- Accept revert strings like:
  - `UniswapV2Router: INSUFFICIENT_OUTPUT_AMOUNT`
  - `PancakeRouter: INSUFFICIENT_OUTPUT_AMOUNT`
  - `PancakeRouter: EXCESSIVE_INPUT_AMOUNT`
  - `PancakeRouter: INVALID_PATH`
  - `PancakeRouter: EXPIRED`
  - `PancakeLibrary: INSUFFICIENT_LIQUIDITY`
  - `PancakeLibrary: INSUFFICIENT_INPUT_AMOUNT`
  - `PancakeLibrary: INSUFFICIENT_OUTPUT_AMOUNT`
  - `PancakeLibrary: INSUFFICIENT_AMOUNT`
  - `PancakeLibrary: IDENTICAL_ADDRESSES`
  - `PancakeLibrary: ZERO_ADDRESS`
  - `PancakeLibrary: INVALID_PATH`
  - `Pancake: INSUFFICIENT_LIQUIDITY`
  - `Pancake: INSUFFICIENT_INPUT_AMOUNT`
  - `Pancake: INSUFFICIENT_OUTPUT_AMOUNT`
  - `Pancake: INVALID_TO`
  - `Pancake: K`
  - `Pancake: LOCKED`
  - `Pancake: TRANSFER_FAILED`
  - `TransferHelper: TRANSFER_FROM_FAILED`
  - `UniswapV2Library: INSUFFICIENT_LIQUIDITY`
- Extract the last token after `:` and map it to `DexErrorCode` when it matches the allowlist above.

**Also accept “no prefix” revert strings (common in V3/Infinity stacks):**

- V3 periphery helper short codes:
  - `STF` → `TRANSFER_FROM_FAILED`
  - `ST` → `TRANSFER_FAILED`
  - `SA` → `APPROVE_FAILED`
  - `STE` → `TRANSFER_FAILED` (native transfer failed; treat as transfer failure unless native swaps enabled)
- V3 core/pool short codes:
  - `AS` → `INSUFFICIENT_AMOUNT`
  - `AI` → `INVALID_INPUTS`
  - `LOK` → `POOL_LOCKED`
  - `SPL` → `INVALID_PRICE_LIMIT`
  - `IIA` → `INSUFFICIENT_INPUT_AMOUNT`
  - `TF` → `TRANSFER_FAILED`
- V3 router human strings:
  - `Too little received` → `INSUFFICIENT_OUTPUT_AMOUNT`
  - `Too much requested` → `EXCESSIVE_INPUT_AMOUNT`
  - `Transaction too old` → `EXPIRED`
- SmartRouter/extended-router empty-message guards (common on BSC):
  - empty revert from `require(amountOut >= amountOutMinimum)` in exact-input methods → classify as `EMPTY_REVERT_DATA` with stage context (`quote|swap`) and params snapshot.
  - empty revert from `require(amountIn <= amountInMaximum)` in exact-output methods → classify as `EMPTY_REVERT_DATA` with inferred slippage-risk hint.

**Selector mapping strategy (required):**

- Maintain a per-family selector map fixture under adapter tests (V2, V3, SmartRouter, UniversalRouter, StableSwap) with:
  - `selector_hex` → `decoded_name` → `DexErrorCode` (plus optional severity/retryable hint).
- Resolution precedence:
  1) explicit selector map hit,  
  2) known short string/token mapping,  
  3) `Error(string)`/`Panic(uint256)` decode,  
  4) `UNKNOWN_REVERT` with full raw bytes.
- For wrapper errors like `ExecutionFailed(uint256,bytes)`, decode top-level selector first, then nested payload recursively (depth-limited).
- Persist both top-level and nested selectors in tx journal for later backfilling when selector maps are expanded.

**UniversalRouter wrapper errors (required for actionable diagnostics):**

- If the revert is `ExecutionFailed(commandIndex, message)`:
  - record `commandIndex` and attempt a *nested* revert decode on `message` (it is often encoded revert bytes),
  - map the nested error to `DexErrorCode` when possible; otherwise attach the nested selector/data to the journal.
- Map common UniversalRouter custom errors when seen at the top level:
  - `TransactionDeadlinePassed()` → `EXPIRED`
  - `LengthMismatch()` → `INVALID_INPUTS`
  - `ETHNotAccepted()` → `INVALID_INPUTS` (or preflight `Unsupported` if native `value` is disabled)
  - `InvalidEthSender()` → `INVALID_INPUTS`
- Also map common UniversalRouter **module** errors (selector-based; add to the selector-map fixture):
  - `V2TooLittleReceived()` → `INSUFFICIENT_OUTPUT_AMOUNT`
  - `V2TooMuchRequested()` → `EXCESSIVE_INPUT_AMOUNT`
  - `V2InvalidPath()` → `INVALID_PATH`
  - `V3TooLittleReceived()` → `INSUFFICIENT_OUTPUT_AMOUNT`
  - `V3TooMuchRequested()` → `EXCESSIVE_INPUT_AMOUNT`
  - `V3InvalidAmountOut()` → `INVALID_INPUTS`
  - `V3InvalidSwap()` → `INVALID_INPUTS`
  - `V3InvalidCaller()` → `INVALID_INPUTS`
  - `StableTooLittleReceived()` → `INSUFFICIENT_OUTPUT_AMOUNT`
  - `StableTooMuchRequested()` → `EXCESSIVE_INPUT_AMOUNT`
  - `StableInvalidPath()` → `INVALID_PATH`
- Permit2 errors (Phase 2; when `permit_mode == Permit2Signature`):
  - `SignatureExpired()` / `AllowanceExpired()` → `EXPIRED`
  - `InvalidNonce()` / `InvalidSignature()` → `INVALID_INPUTS`
  - `InsufficientAllowance()` → `TRANSFER_FROM_FAILED` (or `INVALID_INPUTS` if surfaced before transfer)

**StableSwap (future) errors (include once StableSwap execution is implemented):**

- Preserve the original selector/reason and map known strings/custom errors when possible:
  - `NotWBNBPair` / `NotWhitelist` / `InvalidNCOINS` → `INVALID_INPUTS`

**Primary sources (error strings and custom errors):**

- TransferHelper short codes: https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-periphery/contracts/libraries/TransferHelper.sol
- Pancake V3 pool short codes: https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-core/contracts/PancakeV3Pool.sol
- Pancake V3 SwapRouter revert strings: https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-periphery/contracts/SwapRouter.sol
- Pancake V3 deadline revert string (`Transaction too old`): https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-periphery/contracts/base/PeripheryValidation.sol
- UniversalRouter custom errors + command model (for selector-based mapping):  
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/src/interfaces/IUniversalRouter.sol
  - https://raw.githubusercontent.com/pancakeswap/infinity-universal-router/main/src/libraries/Commands.sol
- BSC JSON-RPC typed error codes (upstream client): https://raw.githubusercontent.com/bnb-chain/bsc/master/internal/ethapi/errors.go
- BSC txpool message strings (underpriced/replacement/already-known): https://raw.githubusercontent.com/bnb-chain/bsc/master/core/txpool/errors.go
- BSC core execution error strings (nonce/intrinsic/insufficient funds): https://raw.githubusercontent.com/bnb-chain/bsc/master/core/error.go

#### 8.5.3 Receipt revert detail recovery (best-effort, but recommended)

When a tx is mined with `receipt.status == 0`, the receipt typically does not contain a reason.

Best-effort recovery steps (do not block indefinitely):

1. Fetch `eth_getTransactionByHash(tx_hash)` to get `(from,to,value,input,gas,fee fields)`.
2. Attempt `eth_call` with the same call object at `blockNumber` (or `blockNumber-1` if required by the node) to recover revert data.
3. If revert data is obtained, decode and map as in 8.5.2; otherwise record the JSON-RPC error message as fallback.
4. (Optional, node-dependent) if supported, use `debug_traceTransaction` to recover richer revert context; keep behind a config flag and never require it for MVP correctness.

If the node does not support this reliably, keep this logic behind a config flag (default true in production).

#### 8.5.4 Broadcast/sendRawTransaction error mapping (actionable)

`eth_sendRawTransaction` can fail with non-deterministic JSON-RPC codes and message strings.
Implement message-pattern mapping to `TxLifecycle` kinds (at minimum):

- `nonce too low` → `NonceTooLow` (likely duplicate nonce or external tx; requires journal recovery and/or operator action)
- `nonce too high` → `NonceTooHigh` (gap nonce or stale local nonce view; re-sync nonce manager)
- `replacement transaction underpriced` → `ReplacementUnderpriced` (post-MVP replacement logic)
- `already known` / `known transaction` → **treat as broadcast-ack** and start polling by computed `tx_hash`
- `insufficient funds for gas * price + value` → `InsufficientFunds` (actionable)
- `intrinsic gas too low` → `IntrinsicGasTooLow` (gas estimation bug/misconfig)
- `transaction underpriced` / `fee cap too low` → `FeeTooLow` (actionable; increase caps)
- `exceeds block gas limit` / `transaction gas limit too high` → `GasLimitInvalid` (actionable; lower gas limit)
- `max priority fee per gas higher than max fee per gas` → `FeeParamsInvalid` (signer or local fee builder bug)
- `transaction type not supported` → `TxTypeNotSupported` (node/chain capability mismatch)
- rate limiting (`HTTP 429`, JSON-RPC `-32005`, “Too Many Requests”) → `RATE_LIMITED` (retry with bounded backoff + jitter; preserve raw error details)

Also map BSC-specific numeric RPC codes when present:

- `-38010` (`nonce too low`) → `NonceTooLow`
- `-38011` (`nonce too high`) → `NonceTooHigh`
- `-38013` (`intrinsic gas too low`) → `IntrinsicGasTooLow`
- `-38014` (`insufficient funds`) → `InsufficientFunds`
- `-32602` (invalid params; fee caps/tip caps) → `FeeParamsInvalid`
- `-32000` or `3` with revert data → route into EVM revert decoder (8.5.2), not generic RPC failure.

Always include the original RPC `code/message` in error details for debugging.

#### 8.5.4b Tx lifecycle edge cases (required)

- If broadcast is acknowledged (or “already known”), track by computed `tx_hash` and reserved nonce until terminal state.
- If receipt is missing for `N` new-heads (configurable), probe:
  - `eth_getTransactionByHash(tx_hash)` and
  - `eth_getTransactionCount(wallet, "pending")`.
- Classify outcomes deterministically:
  - tx missing + nonce advanced past reserved nonce → `TX_REPLACED` (likely external replacement/cancel),
  - tx missing + nonce unchanged for timeout window → `TX_DROPPED` (mempool eviction/provider restart),
  - tx present but never mined within policy window → `TX_STALLED` (operator intervention / optional bump logic).
- For `receipt.status == 0` with `gas_used == tx_gas_limit`, classify as `OUT_OF_GAS` unless a stronger decoded revert reason exists.
- If a previously seen receipt/log later disappears (`removed=true` or block-hash mismatch before confirmation threshold), classify as `REORG_DETECTED` and move order back to non-terminal tracking.

#### 8.5.5 How errors surface in Nautilus (reports + logs)

On any *terminal failure* for a user order, set:

- `OrderStatusReport.order_status = Rejected`
- `OrderStatusReport.cancel_reason = Some(<compact string>)`

Recommended compact format:

- `DEX::<DexErrorCode>::<stage>::<short_reason>` when mapped
- otherwise `RPC::<method>::<code>::<short_message>` / `SIGNER::<status>::<short_message>`

**Truncation contract (required):**

- Enforce a maximum length for `cancel_reason` (e.g., 200 UTF-8 chars).
- If truncation is required, keep the prefix tokens (`DEX::...` / `RPC::...`) and append a stable suffix
  like `::hash=<8-hex>` derived from the full untruncated string so operators can correlate to the tx journal.

The tx journal MUST store full structured `AmmError` details (including raw hex / full RPC message) so
operators can debug without relying on truncated `cancel_reason`.

Minimum structured log/journal fields for every failure:

- `stage`, `chain_id`, `dex_type`, `router_address`, `tx_hash`, `nonce`
- `rpc_method`, `rpc_code`, `rpc_message`, `rpc_data_hex`
- `revert_selector`, `revert_reason`, `nested_revert_selector` (if any), `dex_error_code`
- `block_number`, `receipt_status`, `gas_used`, `effective_gas_price`
- `replacement_of_tx_hash` / `replaced_by_tx_hash` (when applicable)

#### 8.5.6 Required tests (add to milestones where relevant)

Add deterministic golden vectors for:

- JSON-RPC error bodies including `error.code/message/data`.
- `Error(string)` and `Panic(uint256)` revert data decoding.
- Mapping of common DEX revert reasons to `DexErrorCode`.
- Mapping of `eth_sendRawTransaction` messages (“already known”, “nonce too low”, etc) to lifecycle behavior.
- Empty revert data (`0x`) classification with function-selector + stage context.
- V3 short-code mappings (`STF/ST/SA/STE/TF`, `AS/LOK/SPL/IIA`) and V2 pair lock mapping (`Pancake: LOCKED`).
- UniversalRouter nested decode vectors for `ExecutionFailed(commandIndex,message)`.
- BSC numeric RPC code mapping vectors (`-38010/-38011/-38013/-38014/-32602`).
- Tx lifecycle vectors: dropped tx, replaced tx, reorg-removed receipt/log, and out-of-gas (`gas_used == gas_limit`).

### 8.6 Live data streaming + RPC usage (production / Chainstack-friendly)

In production we will likely run against a managed RPC provider (e.g. Chainstack). These providers are **not unlimited**:
they enforce request-rate limits, compute limits on expensive methods, and WebSocket subscription limits.

This plan therefore treats RPC usage as a first-class design constraint.

**Guiding principles (required):**

1) **Execution must win over data.**  
   If RPC capacity is constrained, execution-critical calls (nonce, sendRawTx, receipts) must not be starved by
   live data streaming or historical backfills. Streaming/backfill must be best-effort and pause/backoff under load.
   - **Implementation requirement:** separate budgets/queues for execution vs data:
     - separate concurrency semaphores (e.g., `exec_inflight` vs `data_inflight`)
     - separate rate-limiter buckets using `nautilus_network::ratelimiter` (preferred):
       - pass method-scoped keys into `nautilus_network::http::HttpClient` (e.g., `rpc:exec:eth_sendRawTransaction`, `rpc:data:eth_getLogs`)
       - configure per-key `Quota` for “exec” vs “data” classes so streaming cannot starve receipts/nonces/sends
     - execution paths may borrow unused data capacity, but not vice versa.
   - **Hard exec reservation (recommended):**
     - reserve a minimum concurrency/RPS budget for execution that data is never allowed to consume,
     - data streaming/backfills may use only surplus capacity.
   - **Weighted budget (recommended for managed RPC):**
     - treat RPC calls as having different “cost”; `eth_getLogs` and `eth_estimateGas` should consume more budget than cheap calls,
     - keep an explicit per-method budget table so adding a new streaming feature cannot accidentally starve receipts.

2) **Prefer push (WebSocket) over pull (HTTP polling).**  
   - Use `wss_rpc_url` to subscribe to `newHeads` and drive:
     - receipt polling cadence (poll once per new block instead of every 200–500ms),
     - confirmation counting (avoid `eth_getBlockByNumber` spam).
   - If WS disconnects, execution must degrade safely:
     - fall back to bounded HTTP receipt polling with backoff/jitter,
     - re-establish WS in the background and resume newHeads-driven cadence once healthy.
   - **Receipt scheduler (dual-trigger, required):**
     - primary trigger: `newHeads` (poll once per head per in-flight tx),
     - watchdog trigger: if no heads arrive for `ws_stale_after_ms`, poll receipts on a separate timer (`receipt_watchdog_ms`)
       so “WS stale” does not block fills/rejects,
     - cap per-tx polling (`max_receipt_polls_per_tx`) and tolerate transient `null` receipts (`max_null_receipt_blocks`) before
       probing tx/nonce state (8.5.4b).
   - **WS reliability contract (required):**
     - apply `ws_idle_timeout_ms` and explicit heartbeat/keepalive,
     - reconnect with bounded backoff and stop flapping after `ws_reconnect_max_attempts` (enter degraded mode + alert operator).
   - If market data is sourced from RPC (instead of HyperSync), prefer WS log streaming where supported.

3) **Batch reads using Multicall3 wherever possible.**  
   Token metadata, balances, and pool static metadata (token0/token1) must be loaded via Multicall when initializing
   a universe of pools/tokens, rather than N× per-token RPC calls.

4) **Cache aggressively, but verify at boundaries.**
   - Cache `chain_id`, router immutables (`factory()`, `WETH()`), token decimals/symbol/name, and pool token0/token1.
   - When a cached value is safety-critical (e.g., router address, chain id), verify once at startup and fail closed.
   - For mutable values (allowances/balances), cache with a short TTL and always re-check before broadcasting a tx
     when safety requires it.

5) **Never rely on expensive RPC methods on the hot path.**
   - Avoid `eth_getLogs` except for bounded catch-up windows and explicit pool discovery tasks.
   - Keep `debug_traceTransaction` off by default and never required for correctness.

**Provider profiles + rate limiting (required for managed RPC like Chainstack):**

- Add `provider_profile` config with at least one preset: `chainstack_limited`.
- Required behaviors under `chainstack_limited`:
  - bounded retries with jitter for retryable infra failures (`502/503/504`, connect timeouts)
  - treat HTTP `429` and JSON-RPC `-32005` as `RATE_LIMITED` and back off; if provider returns `try_again_in`, honor it
  - header-aware throttling (recommended):
    - honor `Retry-After` when present,
    - when headers are absent, fall back to bounded exponential backoff with jitter (do not spin/poll aggressively)
  - circuit breaker after N consecutive rate-limit/infra failures (reduce concurrency; then reopen gradually)
  - adaptive `eth_getLogs` splitting:
    - if provider returns “too many results” / payload limits, shrink `address_chunk_size` and/or block range and retry
    - keep a hard ceiling on max retries per page (fail closed once exceeded)
  - WS disconnect handling:
    - on WS close codes like `1006/1009`, reconnect with backoff and run a bounded HTTP gap-backfill
    - if gap exceeds `max_catchup_blocks`, stop streaming and instruct operator to enable hypersync/backfill tooling
- Recommended config knobs to expose (even if only used by provider profiles initially):
  - `provider_profile`, `http_max_inflight`, `ws_max_subscriptions`
  - `receipt_poll_backoff_initial_ms`, `receipt_poll_backoff_max_ms`, `receipt_poll_jitter`
  - `eth_getlogs_max_range_by_provider`, `log_backfill_page_blocks`, `log_backfill_parallel_pages`

**Startup capacity check (recommended):**

- On startup, estimate projected RPC load from:
  - configured pool count + `get_logs_address_chunk_size` (if RPC streaming is enabled),
  - in-flight receipt polling budget (worst-case: `max_inflight_txs_per_wallet` × `confirmations_required`),
  - startup metadata reads (multicall batch sizes).
- If the projected steady-state load exceeds the active `provider_profile` budget, fail fast with an actionable error (or auto-switch to
  “execution-only / no streaming” mode) rather than running in a degraded state that risks starving execution.

Reference (Chainstack limits/errors; verify latest before production rollout):

- https://docs.chainstack.com/docs/limits
- https://docs.chainstack.com/reference/node-api-errors-reference
- https://docs.chainstack.com/docs/understanding-eth-getlogs-limitations

**Execution-path RPC call budget (rule-of-thumb):**

- Typical single-hop swap with sufficient allowance:
  - 1× `eth_getTransactionCount(pending)`
  - 1× quote (`eth_call` router/pair)
  - 0–1× `eth_estimateGas` (if enabled; see gas-limit fallback notes below)
  - 1× `eth_sendRawTransaction`
  - ~1× `eth_getTransactionReceipt` per new block until mined (via newHeads cadence; avoid high-frequency polling)
- If approval is required, add:
  - 1× `eth_call allowance`
  - 0–1× `eth_estimateGas` for approve
  - 1× `eth_sendRawTransaction` for approve + receipts for approve

**Gas estimation strategy (provider-friendly):**

- `eth_estimateGas` is often rate-limited/expensive. Implement `GasLimitStrategy`:
  - `Estimate` (default for safety when capacity allows),
  - `Fixed` (use conservative fixed limits per tx kind),
  - `EstimateWithFallbackFixed` (recommended for managed RPC providers).
- Even when `Fixed` is used, keep local preflight and `eth_call` simulation optional (best-effort) to avoid sending
  obviously reverting txs.

**Market-data streaming recommendations under RPC constraints:**

- For **large** universes (many pools) and/or strict provider limits: prefer `use_hypersync_for_live_data=true`
  (HyperSync/indexer path) and use RPC only for execution + light metadata reads.
- For **small** universes (tens to low hundreds of pools), RPC-based streaming can be viable:
  - Use WS `newHeads` + bounded `eth_getLogs` per block (chunked by address count) **or** WS logs subscriptions if supported.
  - Enforce explicit caps:
    - `max_pools_for_rpc_streaming`
    - `max_get_logs_blocks_per_request` and `max_catchup_blocks` (fail closed if gap exceeds cap; require hypersync/backfill tooling)

**Observability (required for ops):**

- Emit per-method RPC counters and latency histograms (by `method` and `success/error`).
- Emit throttling metrics (`429`s, provider-specific “rate limited” errors) and backoff decisions.
- Emit per-order “RPC spend” (how many calls were used before broadcast, how many receipt polls, etc) so operators can tune
  `rpc_requests_per_second`, poll intervals, and streaming caps.

### 8.7 DeFi wallet support (balances/allowances + safe preflight) (required)

PCS execution is ultimately “wallet trading”. Even with perfect swap encoding, production operation fails if the node
cannot answer basic questions reliably:

- do we have enough native gas token to submit/confirm?
- do we have enough `token_in` balance?
- do we have sufficient allowance to the correct spender (router and/or Permit2)?

This plan therefore includes **DeFi wallet support** as a first-class concern. It should be implemented in a DEX-agnostic
way inside the blockchain adapter so future AMMs reuse it.

**Wallet data model to reuse (already exists):**

- `WalletBalance` + `TokenBalance`: `crates/model/src/defi/wallet.rs`
- current bootstrap exists in `BlockchainExecutionClient::connect()` via `refresh_wallet_balances()`:
  `crates/adapters/blockchain/src/execution/client.rs`

#### 8.7.1 Repo-verified constraints (important)

- **Reuse, don’t fork:** wallet support should reuse `WalletBalance`/`TokenBalance` rather than introducing a parallel model.
- **Snapshot replacement (bug avoidance):** current `refresh_wallet_balances()` appends token balances and can duplicate entries across refreshes.
  Wallet refresh MUST replace the snapshot deterministically (clear/replace per token), not append.
- `query_account` and `generate_account_state` are currently `todo!()` in `BlockchainExecutionClient`; wallet support must implement them
  (non-panicking, deterministic).
- `AccountType::Wallet` is currently rejected by `AccountAny` (`crates/model/src/accounts/any.rs`) and portfolio ingestion uses `AccountAny::from_events`
  (`crates/portfolio/src/portfolio.rs`). If we emit `AccountType::Wallet` snapshots in MVP, they will not be persisted.

**MVP scope (required):**

1) **Token universe derivation (avoid “track the world”):**
   - derive a tracked token set from:
     - configured pool universe (token0/token1),
     - `wnative_address` (WBNB/WETH),
     - config `wallet_extra_tokens`,
     - (Phase 2) Permit2 spender token set when UniversalRouter/Permit2 is enabled.
   - do not attempt global token discovery from historic transfers (too expensive for managed RPC).

2) **Balance + allowance refresh (RPC-budget aware):**
   - fetch native balance with `eth_getBalance`,
   - fetch ERC20 `balanceOf(wallet)` and `allowance(wallet, spender)` via Multicall3 in bounded batches (with adaptive split on provider limits),
   - cache token metadata (decimals/symbol/name) and clamp user-facing precision to ≤16 decimals; preserve raw U256 in the wallet journal.
   - RPC budget formula (operators need this):
     - per refresh ≈ `1 * eth_getBalance + N * balanceOf + (N * S) * allowance` where `N=tokens`, `S=spenders`
     - wallet refresh must run in a dedicated low-priority budget bucket so it never starves `nonce/sendRawTx/receipt`.

3) **Refresh triggers (deterministic + minimal):**
   - on `connect` (if `wallet_refresh_on_connect=true`),
   - on `QueryAccount` (operator-requested refresh),
   - preflight before signing/broadcasting any tx (ensure balances/allowances not stale beyond a short TTL),
   - after any mined receipt that touched the wallet (approve/swap/wrap): refresh only the *touched* tokens + native gas.

4) **Execution preflight checks (fail-closed, actionable):**
   - if native gas balance < configured floor: reject with `INSUFFICIENT_NATIVE_GAS_BALANCE`,
   - if `token_in` balance < `amount_in`: reject with deterministic `AmmError::Rpc`/`AmmError::Config` (do not broadcast),
   - if allowance insufficient and approvals are disabled by policy: reject deterministically (never “auto approve” without explicit policy).

**MVP hard decision (required for working portfolio/cache integration):**

- Emit DeFi wallet snapshots as `AccountType::Cash` (tokens become `Currency` entries, registered dynamically) so Rust portfolio/cache ingestion works today.
- Treat true `AccountType::Wallet` ingestion as post-MVP: it requires `nautilus-model` + portfolio changes (see Milestone 6a addendum).
- `QueryAccount` MUST emit a deterministic snapshot event (or log+telemetry at minimum) so operators can validate balances and allowances.

---

## 9) Detailed implementation plan (tasks)

This section is intentionally explicit: files, tests, commands, expected outcomes.

### Milestone 0: Decide MVP target + boundaries (1–2h)

**Deliverable:** a single “MVP contract” written down and agreed:

- chain: `Bsc` (56) + `BscTestnet` (97) (or only testnet for initial)
- protocol: PancakeSwap V2 only
- instrument: single pool address per instrument (no multi-hop)
- orders: market only
- tokens: standard ERC20 only (MVP disallows fee-on-transfer / rebasing tokens)
- native token: no `tx.value` swaps; trade via ERC20 only (WBNB is treated as ERC20)
- approvals: recommended `ApprovalPolicy::Exact` only for MVP (defer unlimited approvals)
- gas accounting: Option A (section 8.3) — fill commission stays `0` in quote currency; gas tracked in tx journal/logs
- signer: required, no local key fallback
- see section 3.1a for the full support matrix; treat any deviation as an explicit product decision + new tests

**Files:**
- Modify: `docs/plans/2026-03-04-pcs-integration.md` (this document) if decisions differ.

---

### Milestone 0a: Unblock execution builds (feature flags + PyO3 exec wiring + venue routing) (1–3d)

**Goal:** make PCS execution implementable and usable from Python **without** requiring `--features hypersync`.

This milestone directly addresses:

- `nautilus-blockchain` core modules incorrectly gated behind `feature="hypersync"`
- missing PyO3 execution factory/config exposure
- execution client venue mismatch (`Venue("BLOCKCHAIN")` vs DEX venues like `Bsc:PancakeSwapV2`)
- execution coupling to cache/hypersync types: keep execution independent of optional caching/indexing accelerators

**Files:**
- Modify: `crates/adapters/blockchain/src/lib.rs`
  - Move `cache`, `execution`, and `factories` out of `#[cfg(feature = "hypersync")]`
  - Keep `hypersync`-specific modules gated (`hypersync`, and (for now) `data`/`exchanges`/`services` if they still require hypersync types)
- (Recommended) Introduce a small metadata-store trait boundary so execution does not hard-depend on `BlockchainCache`:
  - Create: `crates/adapters/blockchain/src/execution/metadata_store.rs`
    - `TokenMetadataStore` / `PoolMetadataStore` traits (get/set token decimals/symbol/name; pool token0/token1; etc)
    - `InMemoryMetadataStore` implementation (MVP default; makes unit tests easy)
    - optional `BlockchainCacheMetadataStore` adapter (post-MVP) to reuse cache/indexer-backed storage when hypersync is enabled
- Modify: `crates/adapters/blockchain/src/factories.rs`
  - Ensure `BlockchainExecutionClientFactory` is available without `--features hypersync`
  - Gate `BlockchainDataClientFactory` behind hypersync if `data` remains hypersync-gated
  - Fix any copy/paste error messages (execution factory currently says “Invalid config type for BlockchainDataClientFactory…”)
- Modify: `crates/adapters/blockchain/src/config.rs`
  - Add `venue: Venue` to `BlockchainExecutionClientConfig` and thread it into `new(...)`
  - Add PyO3 annotations to `BlockchainExecutionClientConfig` (match `BlockchainDataClientConfig`)
- Modify: `crates/adapters/blockchain/src/python/mod.rs`
  - Register execution factory/config extractors (pattern-match `crates/adapters/binance/src/python/mod.rs`)
  - Expose `BlockchainExecutionClientFactory` + `BlockchainExecutionClientConfig` in `#[pymodule]`
  - Do not gate execution exposure behind hypersync
- Modify: `crates/adapters/blockchain/src/python/factories.rs`
  - Gate `BlockchainDataClientFactory` PyO3 methods behind hypersync if the data factory is hypersync-gated
  - Keep `BlockchainExecutionClientFactory` PyO3 methods always available
  - Ensure the PyO3 execution-factory wrapper exposes `name()` and `config_type()` correctly (LiveNode wiring depends on these)
- Modify: `crates/adapters/blockchain/src/python/config.rs`
  - Add `#[pymethods]` for `BlockchainExecutionClientConfig` (constructor + getters + `__repr__`)
- Modify: `crates/adapters/blockchain/Cargo.toml`
  - fix `required-features` so `nautilus-blockchain` Python/DeFi surfaces build without hypersync once gating is corrected
- Modify: `crates/adapters/blockchain/bin/node_wallet.rs` (if it imports execution/cache modules)
  - ensure the binary still builds after feature-gate refactors (or gate the binary itself appropriately)

**Steps (TDD/verification):**

**Recommended PR split (de-risking):**

1) **Venue routing fix only**
   - execution factory propagates `config.venue` (no `BLOCKCHAIN_VENUE` hardcoding)
   - add unit test verifying venue propagation and DEX venue routing
2) **PyO3 execution config/factory exposure**
   - minimal exec config/factory exported under `--features defi` without changing hypersync gating yet
   - add PyO3 registry extractor roundtrip tests
3) **Feature gating refactor**
   - move execution/factories/cache out of hypersync gating
   - run the full feature-matrix build/tests

1. Add the minimal config+factory surface and fix feature gates so the following builds work:
   - `cargo test -p nautilus-blockchain` (no `--features hypersync`)
   - `cargo test -p nautilus-blockchain --features python`
   - `cargo test -p nautilus-blockchain --features "python,hypersync"`
   - `cargo test -p nautilus-pyo3 --features defi`
   - (optional) `cargo test -p nautilus-pyo3 --features "defi,hypersync"`
2. Verify venue routing correctness at the factory level:
   - ensure `BlockchainExecutionClientFactory` sets `ExecutionClientCore.venue = config.venue`
   - add a unit test in `crates/adapters/blockchain/src/factories.rs` verifying venue is propagated
3. Verify PyO3 registry extractors end-to-end (exec factory + exec config):
   - Create: `crates/adapters/blockchain/tests/pyo3_exec_registry.rs`
   - In test: initialize the `nautilus_blockchain::python::blockchain` module under `Python::with_gil`,
     then perform a round-trip extraction:
     - instantiate `BlockchainExecutionClientFactory()` as a `Py<PyAny>` and run the registered `"BLOCKCHAIN"` exec factory extractor;
       assert the returned boxed factory has `name() == "BLOCKCHAIN"` and `config_type() == "BlockchainExecutionClientConfig"`
     - instantiate `BlockchainExecutionClientConfig(...)` as a `Py<PyAny>` and run the registered config extractor for
       `"BlockchainExecutionClientConfig"`; assert it downcasts to the concrete Rust config type successfully

Run:
- `cargo test -p nautilus-blockchain --features python --test pyo3_exec_registry`

**Expected outcome:** after Milestone 0a, it is *possible* to implement PCS execution in Rust and use it from Python
without enabling hypersync; hypersync remains optional for data ingestion/backfills.

---

### Milestone 1: Extend core DeFi enums for PCS V2 (Rust model) (0.5–1d)

**Goal:** represent PCS V2 as a first-class `DexType` so venue strings can be `Bsc:PancakeSwapV2`.

**Files:**
- Modify: `crates/model/src/defi/dex.rs` (add `DexType::PancakeSwapV2`)
- Modify: `crates/model/src/identifiers/venue.rs` tests if necessary (validate parsing)
- Modify: `crates/model/src/identifiers/instrument_id.rs` tests (DEX address symbol path with `Bsc:PancakeSwapV2`)

**Step 1: Write failing test**
- Add a unit test asserting `DexType::from_dex_name("PancakeSwapV2").is_some()`.
- Add a venue parse test asserting `Venue("Bsc:PancakeSwapV2").parse_dex()` resolves `(Blockchain::Bsc, DexType::PancakeSwapV2)`.

Run: `cargo test -p nautilus-model defi::dex`
Expected: FAIL before enum variant exists.

**Step 2: Implement enum variant + string mapping**
- Add the enum variant and ensure `Display/EnumString` are consistent.

**Step 3: Run tests**
Run: `cargo test -p nautilus-model`
Expected: PASS.

---

### Milestone 2 (Optional, data/discovery): Add BSC exchange maps to blockchain adapter (PCS V2 + PCS V3) (1–2d)

**Goal:** enable the blockchain **data** adapter to discover/subscribe PCS pools on BSC.

**MVP note:** this milestone is **not required** for PCS execution MVP (which is config-driven pools + RPC + signer).
Do it later if/when you want:

- pool discovery / backfill workflows
- hypersync-backed event sync
- streaming market data (Milestone 10)

**Files:**
- Create: `crates/adapters/blockchain/src/exchanges/bsc/mod.rs`
- Create: `crates/adapters/blockchain/src/exchanges/bsc/pancakeswap_v2.rs`
- Create: `crates/adapters/blockchain/src/exchanges/bsc/pancakeswap_v3.rs`
- Modify: `crates/adapters/blockchain/src/exchanges/mod.rs` (add BSC map + branch)
- Modify: `crates/adapters/blockchain/src/data/core.rs` (`initialize_rpc_client` BSC branch) if websocket live-data path is enabled
- Create: `crates/adapters/blockchain/src/rpc/chains/bsc.rs` and update `crates/adapters/blockchain/src/rpc/chains/mod.rs` (if adding websocket BSC support)
- Modify: `crates/adapters/blockchain/src/rpc/mod.rs` (ensure BSC is supported end-to-end for HTTP+WS client initialization)

**Notes:**
- Start with minimal `DexExtended` definitions (factory + creation block + event signatures).
- For PCS V2, store exact signature/topic values from section 3.4:
  - `PairCreated` topic0: `0x0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9`
  - `Swap` topic0: `0xd78ad95fa46c994b6551d0da85fc275fe613ce37657fb8d5e3d130840159d822`
  - `Sync` topic0: `0x1c411e9a96e071241c2f21f7726b17ae89e3cab4c78be50e062b03a9fffbbad1`
- For PCS V3, use the PCS V3 factory + pool event signatures (note: PCS V3 `Swap` differs from vanilla Uniswap V3):  
  - `PoolCreated(address,address,uint24,int24,address)` topic0: `0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118`  
  - `Swap(address,address,int256,int256,uint160,uint128,int24,uint128,uint128)` topic0: `0x19b47279256b2a23a1665c810c8d55a1758940ee09377d4f8d26497a3577dc83`

**Tests:**
- Add test that `get_dex_extended(Blockchain::Bsc, &DexType::PancakeSwapV2)` returns Some.
- Add test that `get_dex_extended(Blockchain::Bsc, &DexType::PancakeSwapV3)` returns Some.

Run:
- If `exchanges` remains hypersync-gated: `cargo test -p nautilus-blockchain --features hypersync exchanges`
- If feature-gating is refactored to compile `exchanges` without hypersync: `cargo test -p nautilus-blockchain exchanges`

---

### Milestone 2a (Recommended): CPAMM-safe exchange/event capability model (1–3d)

**Goal:** remove empty-signature fragility and make non-CLAMM event surfaces explicit for V2-style DEXs.

**Files:**
- Prefer adapter-layer capabilities (recommended for MVP scope): avoid changing `nautilus-model` for this and implement optional signatures/capabilities in the blockchain adapter layer first.
- Modify: `crates/adapters/blockchain/src/exchanges/extended.rs` (capability-aware parser registration helpers; allow absent/optional signatures)
- Modify: `crates/adapters/blockchain/src/data/subscription.rs` (accept optional event signatures and skip registration when absent)
- Modify: `crates/adapters/blockchain/src/data/core.rs` (build event signature lists dynamically based on available capabilities/parsers)
- Modify: existing V2 exchange definitions under:
  - `crates/adapters/blockchain/src/exchanges/ethereum/uniswap_v2.rs`
  - `crates/adapters/blockchain/src/exchanges/arbitrum/uniswap_v2.rs`
  - `crates/adapters/blockchain/src/exchanges/base/uniswap_v2.rs`
  - `crates/adapters/blockchain/src/exchanges/base/baseswap_v2.rs`
  - `crates/adapters/blockchain/src/exchanges/arbitrum/sushiswap_v2.rs`

**Acceptance criteria:**
- No hashing/registration of empty event signatures.
- CPAMM DEXes can register only the events they truly support.
- Pool-event sync path does not assume mint/burn/collect/flash are always present.
- Existing CLAMM integrations remain behaviorally unchanged.

**Tests:**
- Add subscription-manager tests that `None`/absent signatures are not registered.
- Add pool-event sync tests for a CPAMM DEX with only `Swap` (and optionally `Sync`) configured.

Run:
- `cargo test -p nautilus-blockchain data::subscription`
- `cargo test -p nautilus-blockchain --features hypersync data::core`

---

### Milestone 2b (Recommended): PancakeSwap V3 exchange-definition hardening across chains (1–3d)

**Goal:** make `DexType::PancakeSwapV3` actually streamable/discoverable wherever it is declared.

**Files:**
- Modify: `crates/adapters/blockchain/src/exchanges/ethereum/pancakeswap_v3.rs`
- Modify: `crates/adapters/blockchain/src/exchanges/base/pancakeswap_v3.rs`
- Modify: `crates/adapters/blockchain/src/exchanges/arbitrum/pancakeswap_v3.rs`
- Modify: `crates/adapters/blockchain/src/exchanges/bsc/pancakeswap_v3.rs` (from Milestone 2)
- Modify: `crates/adapters/blockchain/src/exchanges/arbitrum/mod.rs` (use `PANCAKESWAP_V3.dex.name` consistently)
- Modify: `crates/adapters/blockchain/src/exchanges/mod.rs` (BSC map branch)

**Required content for each PCS V3 definition:**
- Non-empty CLAMM signatures:
  - `PoolCreated(address,address,uint24,int24,address)`
  - `Swap(address,address,int256,int256,uint160,uint128,int24,uint128,uint128)` (**PCS V3 differs from UniV3**)
  - `Mint(address,address,int24,int24,uint128,uint256,uint256)`
  - `Burn(address,int24,int24,uint128,uint256,uint256)`
  - `Collect(...)` (use chain-correct ABI shape)
  - set `Initialize(uint160,int24)` and optional `Flash(...)`
- Register parsers:
  - PoolCreated/Initialize can reuse `exchanges/parsing/uniswap_v3/*` (same event shapes)
  - Swap must use a PCS V3-aware parser (signature includes protocol fee fields; see Milestone 7c)

**Acceptance criteria:**
- No PCS V3 exchange definition contains `""` event signature placeholders.
- Pool discovery for PCS V3 works on all enabled chains via `PoolCreated`.
- Swap/mint/burn parser functions are wired for PCS V3 in all enabled chains.

**Tests:**
- Add chain coverage test:
  - `Ethereum/Base/Arbitrum/Bsc` + `DexType::PancakeSwapV3` all return `Some`.
- Add signature integrity test asserting PCS V3 `swap_created_event/mint_created_event/burn_created_event` are not empty-derived placeholders.

Run:
- `cargo test -p nautilus-blockchain --features hypersync exchanges`

---

### Milestone 3: Pool InstrumentProvider (config-driven) (1–2d)

**Goal:** Nautilus can load a small set of PCS pools as instruments and route orders to them.

**Files (Python-first, consistent with other adapters):**
- Create: `nautilus_trader/adapters/pancakeswap/providers.py`
- Create: `nautilus_trader/adapters/pancakeswap/symbol.py` (optional display helpers)

**If Rust helpers are preferred (for RPC/multicall reuse):**
- Create: `crates/adapters/blockchain/src/contracts/uniswap_v2_pair.rs` (token0/token1 calls)
- Create: `crates/adapters/blockchain/src/python/pools.rs` (PyO3: batch-fetch pool metadata)

**Tests:**
- Unit test provider builds instruments with correct:
  - venue string format (`<Chain>:<DexType>`)
  - pool address as `instrument_id.symbol`
  - address normalization + validation:
    - normalize to checksum where appropriate
    - reject invalid hex addresses early via `crates/model/src/defi/validation.rs` (`validate_address`)
  - `factory.getPair(token0, token1) == pool_address` validation (reject malformed pool config)
  - token decimals handling (store full decimals in metadata, cap Nautilus precision at 16)
  - (if using Rust helpers) prefer `Pool::create_instrument_id` (`crates/model/src/defi/amm.rs`) to avoid Python-side ID drift

Run: `pytest tests/integration_tests/adapters/pancakeswap -k provider -q`

---

### Milestone 4: Execution RPC completeness (send raw tx, receipts, nonce, gas) (1–2d)

**Goal:** the existing `BlockchainHttpRpcClient` must support the execution methods needed for swaps.

**Files:**
- Modify: `crates/adapters/blockchain/src/rpc/http.rs`
- Modify: `crates/model/src/defi/data/transaction.rs` (extend `Transaction` parsing to include fields needed for execution: `nonce`, `input`, and EIP-1559 fee fields as optional; keep backwards-compatible defaults)
- Create: `crates/model/src/defi/data/transaction_receipt.rs` (new `TransactionReceipt` + `ReceiptLog` models; include `status`, `blockNumber`, `gasUsed`, `effectiveGasPrice`, and `logs` fields needed for fill decoding)
- Modify: `crates/model/src/defi/data/mod.rs` (re-export `TransactionReceipt`)
- Modify: `crates/model/src/defi/rpc.rs` (extend `RpcError` to capture optional `data` payload for revert decoding)

**Add/extend methods:**
- `get_transaction_count(address, block_tag)` (pending nonce)
- `estimate_gas(call_obj, block_tag)`
- `send_raw_transaction(raw_tx_hex)`
- `get_transaction_by_hash(tx_hash)` (needed for revert detail recovery + gas fallbacks)
- `get_transaction_receipt(tx_hash)`
- `get_block_by_number(number_or_latest)`
- (for chain fingerprinting) `get_block_by_number(0)` must parse `hash` reliably
- `get_logs(filter_obj)` (already exists; harden parsing + provider-limit splitting support for backfills)
- `get_code(address, block_tag)` (contract existence validation)
- `chain_id()` (startup chain guardrail)

**Provider-budget requirement (tie-in to section 8.6):**

- Thread method-scoped quota keys into each RPC call (e.g., `rpc:exec:eth_sendRawTransaction`, `rpc:exec:eth_getTransactionReceipt`, `rpc:data:eth_getLogs`)
  so managed-RPC rate-limiting cannot starve execution.
- Ensure the HTTP client surfaces response headers (at minimum `Retry-After`) and attach them to the structured RPC error so provider profiles can honor them.

**Tests:**
- Reuse existing adapter patterns:
  - Axum ephemeral servers + readiness polling (`crates/adapters/betfair/tests/common/mod.rs`, `crates/adapters/kraken/tests/http.rs`)
  - fixture-backed responses (`crates/adapters/kraken/tests/test_data/*`)
  - blockchain integration-style harnesses (`crates/adapters/blockchain/tests/rpc_reconnection.rs`)
- Create: `crates/adapters/blockchain/tests/common/mod.rs`
  - `start_mock_rpc_server(state) -> SocketAddr` using `TcpListener::bind("127.0.0.1:0")` + `axum::serve`
  - `wait_for_server(addr)` using `nautilus_common::testing::wait_until_async`
  - shared state for request log + per-method call counters
- Create: `crates/adapters/blockchain/tests/rpc_http_execution_methods.rs`
  - `test_get_transaction_count_uses_pending_tag`
  - `test_estimate_gas_builds_expected_rpc_payload`
  - `test_send_raw_transaction_returns_tx_hash`
  - `test_send_raw_transaction_error_parses_code_message_and_data` (for error taxonomy, section 8.5)
  - `test_get_transaction_by_hash_parses_from_to_input_and_fee_fields`
  - `test_get_transaction_receipt_parses_status_logs_and_gas_fields`
  - `test_get_block_by_number_latest_parses_timestamp`
  - `test_get_block_by_number_zero_parses_hash` (for genesis fingerprinting)
  - `test_get_logs_parses_topic_filtered_results`
  - `test_get_code_returns_empty_or_bytecode`
  - `test_chain_id_parses_expected_network`
  - `test_rpc_error_body_maps_to_client_error`
  - `test_http_429_rate_limited_maps_to_retryable_rpc_error` (managed RPC providers like Chainstack)
  - `test_http_429_retry_after_header_is_respected_when_present` (if the HTTP client surfaces headers)
  - assert outbound JSON-RPC `method`/`params` and typed success/error mapping

Run:
- `cargo test -p nautilus-blockchain --test rpc_http_execution_methods`
- `cargo test -p nautilus-blockchain rpc::http`

---

### Milestone 5: Remote signer client (Rust, with optional Python bindings) (1–2d)

**Goal:** implement signer transport in Nautilus, mirroring chainsaw behavior.

**Files (recommended location):**
- Create: `crates/adapters/blockchain/src/execution/signer/mod.rs`
- Create: `crates/adapters/blockchain/src/execution/signer/client.rs`
- Create: `crates/adapters/blockchain/src/execution/signer/types.rs`
- (Optional) Create: `crates/adapters/blockchain/src/python/signer.rs` (PyO3 bindings)

**API:**
- `RemoteSignerClient::sign_evm_tx(SignRequest) -> SignedTx { raw_tx_hex, r,s,v }`
- `SignerPayloadMapper::to_oss_v1_flat(SignRequest) -> OssSignEthRequest`

**Required features:**
- retries with exponential backoff (no retry on 4xx), implemented via `nautilus_network::retry::RetryManager` + `RetryConfig`
- optional mTLS
- enforce `signer_require_tls=true` by default (reject `http://` endpoints unless explicitly allowed for local dev)
- structured logging with request id + decision
- explicit `signer_api_mode` switch (`oss_v1_flat` default for current OSS server)
- preflight validation before HTTP POST:
  - require EIP-1559 fields present (do not permit gasPrice-only request)
  - require `value` hex string formatting
  - require selector extracted from `data` and matched to expected operation
- post-sign verification:
  - decode returned `raw_tx_hex` and ensure it exactly matches requested `chain_id/nonce/to/data/value/gas/fees`
  - recover sender from the signed tx and ensure it matches configured `wallet_address`
- compute `tx_hash = keccak256(raw_tx_bytes)` from the signed raw tx bytes for downstream idempotency (do not trust RPC response for hash)
  - hex-decode `raw_tx_hex` to bytes and hash directly (typed-tx safe; do not RLP-wrap raw bytes again)
- (optional) attach `intent_hash` (section 7.6) into logs/telemetry and internal tx journal records

**Tests:**
- Create: `crates/adapters/blockchain/tests/common/mock_signer.rs`
  - `start_mock_signer_server(state) -> SocketAddr` with scripted responses for `/sign/eth`
  - state tracks request bodies + call counts for retry assertions
- Create: `crates/adapters/blockchain/tests/signer_client.rs`
  - `test_sign_evm_tx_success_returns_raw_tx_and_metadata`
  - `test_sign_evm_tx_403_policy_deny_does_not_retry`
  - `test_sign_evm_tx_500_retries_then_succeeds`
  - `test_sign_evm_tx_timeout_classified_as_retryable`
  - `test_sign_evm_tx_rejects_raw_tx_hex_not_matching_request`
  - `test_sign_evm_tx_rejects_http_endpoint_when_require_tls`
  - `test_sign_evm_tx_returns_tx_hash_deterministically`
  - `test_signer_tx_hash_keccak_raw_bytes_matches_known_type2_vector` (must-have; protects typed-tx idempotency)
  - `test_signer_tx_hash_mismatch_vs_rpc_response_fails_closed` (mock `eth_sendRawTransaction` returns different hash)
  - `test_sign_evm_tx_mtls_config_builds_client` (unit-only config wiring)
  - assert outbound payload shape (route, chain id, tx fields, policy metadata, request id), retry semantics, and mapped error taxonomy
- Add OSS-conformance tests with signer-server fixture behavior:
  - nested payload is rejected/unsupported
  - hex-string numeric fields are rejected (`bad_request`)
  - `gasPrice`-only payload would be unsafe; client must reject before sending
  - extra intent fields are not relied upon for policy enforcement

Run:
- `cargo test -p nautilus-blockchain --test signer_client`
- `cargo test -p nautilus-blockchain signer`

---

### Milestone 6: ERC20 allowance/approve support (call encoding + tx flow) (1–2d)

**Goal:** adapter can ensure allowance for router spending tokenIn, signer-only.

**Files:**
- Modify: `crates/adapters/blockchain/src/contracts/erc20.rs` (add `allowance`, `approve` ABI encoding)
- Create: `crates/adapters/blockchain/src/execution/erc20_allowance.rs` (helper flow)

**Flow:**
1. `eth_call allowance(owner, spender)`
2. If insufficient:
   - enforce spender == configured router (fail-closed)
   - choose approval amount per `approval_policy`:
     - `Exact`: approve only the missing delta (or the full required amount if delta math is risky)
     - `Unlimited`: approve `U256::MAX` **only if** token is allowlisted for unlimited approvals
     - `UnlimitedResetFirst`: if current allowance > 0, send `approve(spender, 0)` first, then `approve(spender, max)`
   - apply optional approval caps (`unlimited_approval_max_amount`) when configured
   - build approve tx call data (ABI encoding only; do not rely on token return data conventions)
   - estimate gas
   - sign via signer
   - send raw tx
   - wait receipt
   - **post-approve verification (required, fail-closed):**
     - re-read `allowance(owner, spender)` after the approve receipt is mined
     - require `allowance >= required_amount` (or >= the approved amount, depending on policy)
     - if allowance is still insufficient, classify as `APPROVE_FAILED` / `ALLOWANCE_NOT_UPDATED` and abort (do not attempt swap)

**Tests:**
- Unit ABI tests:
  - Extend `crates/adapters/blockchain/src/contracts/erc20.rs` tests with:
    - `test_allowance_call_encoding_matches_selector_dd62ed3e`
    - `test_approve_call_encoding_matches_selector_095ea7b3`
    - `test_approve_call_encoding_matches_amount_and_spender`
  - Assert selector bytes + encoded args + decode roundtrip.
- Flow tests with mock RPC + signer:
  - Create: `crates/adapters/blockchain/tests/erc20_allowance_flow.rs`
    - `test_allowance_sufficient_skips_approve_and_signer_not_called`
    - `test_allowance_insufficient_signs_and_sends_approve_tx`
    - `test_unlimited_approval_rejected_when_token_not_allowlisted`
    - `test_unlimited_reset_first_sends_zero_then_max_when_allowance_nonzero`
    - `test_approve_receipt_status_one_but_allowance_still_insufficient_fails_closed` (must-have)
    - `test_approve_receipt_status_zero_returns_error`
    - `test_approve_nonce_or_gas_rpc_failure_bubbles_context`
  - Assert exact RPC sequence (`eth_call` allowance -> optional `eth_estimateGas` -> signer -> `eth_sendRawTransaction` -> `eth_getTransactionReceipt`), call count, and terminal result.

Run:
- `cargo test -p nautilus-blockchain erc20::tests`
- `cargo test -p nautilus-blockchain --test erc20_allowance_flow`

---

### Milestone 6a (Recommended): DeFi wallet support (balances/allowances + QueryAccount) (1–3d)

**Goal:** make wallet state a first-class, DEX-agnostic capability in the blockchain adapter (section 8.7).

**Dependencies:** Milestones `0a`, `4`, and `6` (execution client builds, RPC methods, ERC20 allowance encoding).

**Files:**
- Create: `crates/adapters/blockchain/src/execution/wallet.rs`
  - `WalletTracker` (tracked token set + cached balances/allowances + refresh helpers)
  - deterministic refresh APIs used by execution preflight and `QueryAccount`
- Modify: `crates/adapters/blockchain/src/execution/client.rs`
  - replace ad-hoc `refresh_wallet_balances()` with `WalletTracker` usage
  - implement `query_account` to trigger a refresh (respecting provider budgets) and emit/log a deterministic snapshot
  - implement `generate_account_state` (non-panicking) so wallet snapshots can be emitted as `AccountState`
  - ensure `connect()` respects `wallet_refresh_on_connect`
- Modify: `crates/adapters/blockchain/src/config.rs`
  - add wallet knobs to the execution config (section 8.4): TTL, extra tokens, allowance spenders, caps
- Modify: `crates/adapters/blockchain/src/python/config.rs`
  - expose new execution wallet config fields in PyO3 constructors/getters (keep backward compatible defaults)
- Modify: `crates/adapters/blockchain/src/factories.rs`
  - MVP policy: construct the execution client with `AccountType::Cash` (not `Wallet`) so portfolio/cache ingestion works (section 8.7)
  - keep the “true Wallet account type” work as a post-MVP milestone
- Modify: `crates/adapters/blockchain/src/contracts/erc20.rs`
  - add multicall-batched helpers:
    - `batch_balance_of(tokens, wallet)`
    - `batch_allowance(tokens, owner, spender)`
  - reuse the existing “multicall then fallback to per-token calls” pattern (already used for token metadata)
- Modify: `crates/adapters/blockchain/bin/node_wallet.rs`
  - update the example to exercise `QueryAccount` refresh + wallet config knobs

**Required behaviors:**
- Token universe derivation:
  - union(pool token0/token1, `wnative_address`, `wallet_extra_tokens`)
  - never expand the universe implicitly during trading (avoid surprise RPC load); require operator config update + restart for new tokens in MVP.
- Snapshot replacement (required):
  - refresh must replace the wallet snapshot deterministically (no append duplicates across refresh cycles)
- Refresh triggers:
  - on `connect` if enabled,
  - on `QueryAccount`,
  - preflight before broadcast,
  - post-receipt refresh of touched tokens only.
- RPC budget protection:
  - use multicall batches with adaptive splitting and strict caps (section 8.6).
  - allow multiple spenders for allowance refresh (`wallet_allowance_spenders`, e.g., router and Permit2)

**Tests (mock RPC):**
- Create: `crates/adapters/blockchain/tests/wallet_tracker_refresh.rs`
  - `test_wallet_tracker_batches_balance_of_via_multicall`
  - `test_wallet_tracker_batches_allowance_via_multicall`
  - `test_refresh_replaces_snapshot_not_appends_duplicates`
  - `test_wallet_tracker_allowance_refresh_supports_multiple_spenders`
  - `test_wallet_tracker_adaptive_splits_on_provider_limit_error`
  - `test_query_account_triggers_refresh_and_does_not_panic`
  - `test_wallet_refresh_respects_budget_caps_under_chainstack_profile`

Run:
- `cargo test -p nautilus-blockchain --test wallet_tracker_refresh`

**Post-MVP (optional): true `AccountType::Wallet` support**

If the team wants to keep `AccountType::Wallet` as a first-class concept (instead of Cash-mode snapshots), add a separate milestone:

- Create: `crates/model/src/accounts/wallet.rs` (new `WalletAccount`)
- Modify: `crates/model/src/accounts/any.rs` (support `AccountType::Wallet`)
- Modify: `crates/portfolio/src/portfolio.rs` (ensure wallet accounts ingest/apply like cash accounts)
- Add tests proving wallet snapshots persist in cache and do not panic.

---

### Milestone 6b (Optional, post-MVP): Wrapped-native + native swap support (BNB/WBNB) (1–3d)

**Goal:** remove the “must pre-wrap BNB to WBNB” operational burden and correctly account for native-value semantics.

This milestone implements the “Phase 2” options in section 3.7/3.8:

- router-native swap methods (`swapExactETHForTokens`, `swapExactTokensForETH`, etc), and/or
- explicit wrap/unwrap (`deposit()` / `withdraw(uint256)`) transactions

**Files:**
- Create: `crates/adapters/blockchain/src/contracts/weth9.rs` (WETH/WBNB deposit/withdraw encoding + selectors)
- Modify: `crates/adapters/blockchain/src/contracts/pancakeswap_v2_router.rs`
  - add encoding for ETH/BNB methods if enabling router-native swaps
- Modify: `crates/adapters/blockchain/src/execution/amm/pancakeswap_v2.rs`
  - add “native swap” path when `token_in == wnative` or `token_out == wnative` and config enables it
- Modify: `crates/adapters/blockchain/src/execution/client.rs`
  - ensure signer/local preflight policies allow only the expected selector+`value` combinations

**Tests:**
- Create: `crates/adapters/blockchain/tests/weth9_wrap_unwrap.rs`
  - `test_deposit_encoding_selector_d0e30db0_and_value_nonzero_required`
  - `test_withdraw_encoding_selector_2e1a7d4d_and_value_zero_required`
- Extend: `crates/adapters/blockchain/tests/pancakeswap_v2_router_calls.rs`
  - `test_swap_exact_eth_for_tokens_sets_tx_value_equal_amount_in`
  - `test_swap_exact_tokens_for_eth_sets_tx_value_zero`

Run:
- `cargo test -p nautilus-blockchain --test weth9_wrap_unwrap`
- `cargo test -p nautilus-blockchain --test pancakeswap_v2_router_calls`

---

### Milestone 7: PancakeSwap V2 router call encoding (quote + swap tx build) (2–4d)

**Goal:** implement PCS V2 router encoding for:

- `getAmountsOut`/`getAmountsIn` (eth_call)
- `swapExactTokensForTokens`/`swapTokensForExactTokens` (tx)

and define the **AMM adapter contract** used by future DEXs:

- `AmmProtocolAdapter` trait + capability flags (section 4.5)
- adapter registry/dispatch by `DexType`

**Files:**
- Create: `crates/adapters/blockchain/src/contracts/pancakeswap_v2_router.rs`
- Create: `crates/adapters/blockchain/src/execution/amm/pancakeswap_v2.rs`
- Create: `crates/adapters/blockchain/src/execution/amm/mod.rs` (traits + shared types + registry)

**Tests:**
- Unit ABI + decoding tests:
  - Create (or inline `#[cfg(test)]`): `crates/adapters/blockchain/src/contracts/pancakeswap_v2_router.rs`
    - `test_get_amounts_out_encoding_selector_d06ca61f`
    - `test_get_amounts_in_encoding_selector_1f00ca74`
    - `test_swap_exact_tokens_for_tokens_encoding_selector_38ed1739`
    - `test_swap_tokens_for_exact_tokens_encoding_selector_8803dbee`
    - `test_decode_get_amounts_out_response_u256_path`
  - Assert selector + full ABI arg order (`amount`, `path`, `to`, `deadline`) and decode correctness.
- Integration tests with mock RPC:
  - Create: `crates/adapters/blockchain/tests/pancakeswap_v2_router_calls.rs`
    - `test_quote_calls_eth_call_with_router_to_and_data`
    - `test_quote_rpc_revert_maps_to_quote_error` (captures code/message/data; section 8.5)
    - `test_quote_revert_insufficient_liquidity_maps_to_dex_error_code`
    - `test_quote_revert_insufficient_amount_maps_to_dex_error_code`
    - `test_quote_revert_identical_addresses_maps_to_dex_error_code`
    - `test_swap_tx_build_uses_min_out_deadline_path_recipient`
  - Assert call data correctness and typed error mapping.
- Adapter contract tests:
  - Create: `crates/adapters/blockchain/tests/amm_adapter_contract.rs`
    - `test_pancakeswap_v2_adapter_capabilities_match_mvp`
      - assert `supports_recipient_override == false` and `swap_call_returns_amounts == true` for classic PCS V2 exact-in/out
    - `test_pancakeswap_v2_adapter_selector_constants_match_expected`
    - `test_amm_registry_returns_adapter_for_pancakeswap_v2`
    - `test_amm_registry_unknown_dex_type_returns_error`
    - `test_amm_registry_duplicate_registration_fails_fast`

Run:
- `cargo test -p nautilus-blockchain pancakeswap_v2_router`
- `cargo test -p nautilus-blockchain --test pancakeswap_v2_router_calls`
- `cargo test -p nautilus-blockchain --test amm_adapter_contract`

---

### Milestone 7a: UniswapV2-like pair log decoding (Swap → fills) (1–2d)

**Goal:** decode PCS V2 pair `Swap` logs into protocol-agnostic `AmmFill` records so execution can emit correct `FillReport`s.

This milestone is required because existing swap event/data types are V3-specific (section 4.4).

**Files:**
- Create/Modify: `crates/adapters/blockchain/src/contracts/uniswap_v2_pair.rs`
  - add event topic constants and ABI decoding for:
    - `Swap(address,uint256,uint256,uint256,uint256,address)` (topic0 + data)
    - (optional) `Sync(uint112,uint112)`
- Modify: `crates/adapters/blockchain/src/execution/amm/pancakeswap_v2.rs`
  - implement `decode_fills_from_receipt(...)` using the V2 pair swap decoder
  - MVP invariants (fail-fast):
    - require `receipt.status == 1` and ignore/deny any `removed=true` logs (reorg artifacts)
    - require `expected_path.len() == 2` (single-hop only)
    - filter receipt logs by `Swap` topic0 **and** `log.address == expected_pool_address`
    - require exactly **one** matching pool `Swap` log (0 => error, >1 => error)
    - decode and require `Swap.to == wallet_address` (recipient invariant; fail-closed if not)
    - deterministic mapping from `(amount0In, amount1In, amount0Out, amount1Out)` + pair `token0/token1`
      into `AmmFill { token_in, token_out, amount_in, amount_out }`
    - reject “taxed”/fee-on-transfer/rebasing tokens in MVP (documented limitation; likely enforced via explicit allowlist/denylist rather than auto-detection); do not silently mis-account

**Tests:**
- Create: `crates/adapters/blockchain/tests/uniswap_v2_pair_swap_decode.rs`
  - `test_decode_swap_log_amounts_in_out_and_addresses`
  - `test_decode_swap_log_rejects_wrong_topic0`
  - `test_decode_swap_log_rejects_ambiguous_amounts`
- Create: `crates/adapters/blockchain/tests/pancakeswap_v2_receipt_decode.rs`
  - fixture receipt JSON containing a single-hop swap
  - assert decoded `AmmFill { amount_in, amount_out, token_in, token_out, log_index }`
  - `test_receipt_decode_sets_tx_hash_and_sorts_by_log_index`
  - `test_receipt_decode_rejects_if_no_swap_log_for_expected_pool`
  - `test_receipt_decode_rejects_if_multiple_swap_logs_for_expected_pool`
  - `test_receipt_decode_rejects_swap_log_from_other_pool_address`
  - `test_receipt_decode_rejects_path_len_not_two`
  - `test_receipt_decode_rejects_if_swap_to_is_not_wallet`

Run:
- `cargo test -p nautilus-blockchain --test uniswap_v2_pair_swap_decode`
- `cargo test -p nautilus-blockchain --test pancakeswap_v2_receipt_decode`

---

### Milestone 7b (Optional, post-MVP): Fee-on-transfer token support (exact-in only) (1–3d)

**Goal:** support “taxed” ERC20 tokens on **classic PCS V2 router** by using dedicated FoT functions and adjusting execution invariants.

**Key constraints (protocol reality):**

- V2 supports FoT primarily via `swapExact*SupportingFeeOnTransferTokens` variants (exact-in only).
- Exact-output swaps are generally incompatible with FoT because the router cannot know exact in/out amounts.
- This milestone is for classic router ABI (`0x10ED...` family). SmartRouter V2 has different semantics (no dedicated FoT functions; balance-delta internals).

**Plan decisions (recommended):**

- Keep MVP default: disallow FoT/rebasing (section 10.5).
- If enabling FoT:
  - Enforce **explicit allowlist-only** mode per token/pair; no auto-detection in execution hot path.
  - Allow **SELL base** (exact-in) only; fail fast on BUY base (exact-out).
  - Use router FoT function variants for token-in paths.
  - Treat quote outputs as **best-effort only** (router FoT paths do balance-delta checks at execution time; local quote precision is weaker).
  - Fill accounting in FoT mode:
    - primary: wallet-scoped `Transfer` deltas (token_in spent / token_out received),
    - optional: `balanceOf` pre/post deltas (more RPC; keep disabled by default),
    - treat pair `Swap` amounts as sanity-only (they can be gross != wallet net).
    - if transfer-delta attribution is ambiguous (multiple concurrent transfers touching wallet/token in tx), fail decode and reject reconciliation rather than emitting guessed fills.
  - Keep rebasing/reflection/blacklist/anti-bot token mechanics unsupported unless a dedicated token-profile adapter is added.
  - Explicitly keep FoT unsupported for PCS V3 adapters (preflight reject if `token_in` or `token_out` is FoT).

Primary sources:

- PCS V2 Router02 FoT entrypoints: https://raw.githubusercontent.com/pancakeswap/pancake-smart-contracts/master/projects/exchange-protocol/contracts/interfaces/IPancakeRouter02.sol
- PCS V3 pool enforces exact callback payment (`IIA`), making FoT tokens generally incompatible: https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-core/contracts/PancakeV3Pool.sol

**Files:**
- Modify: `crates/adapters/blockchain/src/contracts/pancakeswap_v2_router.rs`
  - add ABI encoding for:
    - `swapExactTokensForTokensSupportingFeeOnTransferTokens(uint256,uint256,address[],address,uint256)`
    - (if native swaps are enabled) `swapExactETHForTokensSupportingFeeOnTransferTokens(...)`
    - (if native swaps are enabled) `swapExactTokensForETHSupportingFeeOnTransferTokens(...)`
- Modify: `crates/adapters/blockchain/src/execution/amm/pancakeswap_v2.rs`
  - select FoT router functions based on token safety config/allowlist
  - tighten local preflight to prevent “silent FoT mis-accounting” when config says deny

**Tests:**
- Extend: `crates/adapters/blockchain/tests/pancakeswap_v2_router_calls.rs`
  - `test_fot_token_uses_supporting_fee_on_transfer_router_method`
  - `test_fot_token_rejects_exact_out_orders`
  - `test_fot_swap_does_not_expect_router_return_amounts`
  - `test_fot_fill_accounting_uses_wallet_transfer_deltas_not_swap_event_amounts` (fixture-based; proves gross!=net reconciliation)
  - (if PCS V3 adapter exists) `test_pancakeswap_v3_rejects_fot_tokens_pre_sign`

Run:
- `cargo test -p nautilus-blockchain --test pancakeswap_v2_router_calls`

---

### Milestone 7c (Optional, post-MVP): PancakeSwap V3 execution adapter (BSC-first: quote + swap + fills) (2–5d)

**Goal:** add PCS V3 (UniswapV3-like) swap execution as a *second* protocol adapter once the V2 signer-only path is proven.

**Recommended initial scope (keep it safe):**

- Single-pool only (Phase 1: `exactInputSingle` only; Phase 2: enable `exactOutputSingle` behind a config flag) using pool metadata (`token0/token1/fee`).
- Phase 1 implication: support **SELL base** only; reject `BUY base` orders unless `v3_enable_exact_output=true` and Phase 2 is implemented.
- ERC20-only, `tx.value == 0` (explicit wrap/unwrap remains optional as in section 3.7/3.8).
- Scope hardening: in this milestone, allow only the direct PCS V3 `SwapRouter` single-pool selectors (`exactInputSingle` / optionally `exactOutputSingle`); explicitly reject SmartRouter/UniversalRouter/multicall calldata (fail closed).
- Quotes via `QuoterV2` using normal ABI return decoding (QuoterV2 reverts internally in the callback and catches/decodes it).
- Fill decoding from **pool `Swap` logs** (not router return values), using PancakeSwap V3’s swap signature
  (it includes extra protocol-fee fields vs Uniswap V3; ignore the trailing protocol-fee words for fill calculation).

**Primary sources:**

- PCS V3 periphery router ABI (includes per-swap `deadline` fields): https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-periphery/contracts/interfaces/ISwapRouter.sol
- PCS V3 QuoterV2 (returns normally; internal callback revert is caught/decoded): https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/router/contracts/lens/QuoterV2.sol
- PCS V3 pool events (Swap signature includes protocol fee fields): https://raw.githubusercontent.com/pancakeswap/pancake-v3-contracts/main/projects/v3-core/contracts/interfaces/pool/IPancakeV3PoolEvents.sol

**Files:**
- Create: `crates/adapters/blockchain/src/contracts/uniswap_v3_swap_router.rs`
  - ABI encoding for:
    - `exactInputSingle(ExactInputSingleParams)` (exact-in)
    - `exactOutputSingle(ExactOutputSingleParams)` (exact-out)
  - (optional) also encode multi-hop forms `exactInput`/`exactOutput` but keep adapter capabilities false by default
- Create: `crates/adapters/blockchain/src/contracts/uniswap_v3_quoter_v2.rs`
  - ABI encoding for quote calls and **return decoding** into `(amountIn/amountOut, sqrtPriceAfter, ticksCrossed, gasEstimate)`
- Modify: `crates/adapters/blockchain/src/contracts/uniswap_v3_pool.rs`
  - add `token0()`, `token1()`, and `fee()` calls for pool immutables used in swap building + fill decoding
- Create: `crates/adapters/blockchain/src/execution/amm/pancakeswap_v3.rs`
  - implement `AmmProtocolAdapter` for `DexType::PancakeSwapV3`
  - build swap calldata using `uniswap_v3_swap_router` contract types
  - decode fills from receipt by filtering for **PCS V3 Swap** topic0 at `pool_address` and decoding data into:
    - `(amount0, amount1, sqrtPriceX96, liquidity, tick, protocolFeesToken0, protocolFeesToken1)`
      (ignore protocol fee fields for fill amounts)
    - Swap topic0 must be: `0x19b47279256b2a23a1665c810c8d55a1758940ee09377d4f8d26497a3577dc83`
  - (recommended) implement a shared parser for PCS V3 swap logs so this can be reused by market-data streaming later:
    - Create: `crates/adapters/blockchain/src/exchanges/parsing/pancakeswap_v3/swap.rs`
  - MVP invariants (fail-fast):
    - require exactly one matching `Swap` log for the expected pool
    - derive `token_in/token_out` by sign of `(amount0, amount1)` and pool’s `token0/token1`
    - reject `amountIn == 0` sentinel semantics (SmartRouter-style) — not applicable here
- Modify: `crates/adapters/blockchain/src/execution/amm/mod.rs`
  - register `DexType::PancakeSwapV3` adapter dispatch path
- Modify: `crates/adapters/blockchain/src/config.rs`
  - require/add `quoter_address` for V3 quoting paths (QuoterV2 for PCS V3)
  - add chain-scoped defaults for BSC mainnet/testnet router+factory+quoter where config leaves them unset
- Modify: `crates/adapters/blockchain/src/execution/client.rs`
  - enforce startup validation for BSC PCS V3 defaults:
    - `router.factory()` matches configured/default factory
    - `router.WETH9/WETH()` matches configured/default WBNB
    - fail-closed on mismatch unless explicit override flag is set

**Tests:**
- Create: `crates/adapters/blockchain/tests/uniswap_v3_quoter_v2_decode.rs`
  - `test_quoter_v2_success_return_decodes_tuple` (golden return fixture)
  - `test_quoter_v2_revert_maps_to_quote_error` (section 8.5)
- Create: `crates/adapters/blockchain/tests/pancakeswap_v3_receipt_decode.rs`
  - fixture receipt JSON containing a single-pool swap
  - `test_v3_receipt_decode_maps_signed_amounts_to_fill_amount_in_out`
  - `test_v3_receipt_decode_rejects_if_swap_log_missing_or_wrong_pool`
  - `test_v3_receipt_decode_rejects_uniswap_v3_swap_topic_for_pancake_v3`
  - `test_v3_receipt_decode_ignores_protocol_fee_fields`
- Create: `crates/adapters/blockchain/tests/pancakeswap_v3_bsc_config_defaults.rs`
  - `test_bsc_mainnet_defaults_set_router_factory_quoter`
  - `test_bsc_testnet_defaults_set_router_factory_quoter`
  - `test_startup_validation_fails_on_router_factory_mismatch`
  - `test_startup_validation_fails_on_router_wbnb_mismatch`

Run:
- `cargo test -p nautilus-blockchain --test uniswap_v3_quoter_v2_decode`
- `cargo test -p nautilus-blockchain --test pancakeswap_v3_receipt_decode`
- `cargo test -p nautilus-blockchain --test pancakeswap_v3_bsc_config_defaults`

---

### Milestone 7d (Optional, post-MVP): SmartRouter ABI guardrails + runtime validation (1–3d)

**Goal:** add explicit compatibility guards so future SmartRouter integration cannot silently reuse classic V2 assumptions.

**Files:**
- Create: `crates/adapters/blockchain/src/contracts/pancakeswap_smart_router.rs`
  - encode/decode minimal SmartRouter surface needed for validation and future execution:
    - `factoryV2()`, `factory()`, `WETH9()`, `positionManager()`, `stableSwapFactory()`, `stableSwapInfo()`
    - V2-style methods with SmartRouter selectors:
      - `swapExactTokensForTokens(uint256,uint256,address[],address)` -> `0x472b43f3`
      - `swapTokensForExactTokens(uint256,uint256,address[],address)` -> `0x42712a67`
    - deadline multicall wrapper:
      - `multicall(uint256,bytes[])` -> `0x5ae401dc`
- Modify: `crates/adapters/blockchain/src/execution/amm/mod.rs`
  - add a “router ABI mode” guard (`classic_v2_router` vs `smart_router`) to prevent selector mix-ups.

**Tests:**
- Create: `crates/adapters/blockchain/tests/pancakeswap_smart_router_abi.rs`
  - `test_smart_router_v2_selectors_do_not_match_classic_router_selectors`
  - `test_smart_router_multicall_deadline_selector_matches_expected`
  - `test_smart_router_runtime_immutable_reads_match_config_defaults`
  - `test_smart_router_rejects_sentinel_recipient_and_contract_balance_mode_by_default`
  - `test_smart_router_direct_swap_without_multicall_deadline_is_rejected`
  - `test_smart_router_erc20_only_flow_rejects_nonzero_tx_value`

Run:
- `cargo test -p nautilus-blockchain --test pancakeswap_smart_router_abi`

---

### Milestone 7e (Optional, post-MVP): StableSwap (2-pool first) adapter track (2–6d)

**Goal:** support Pancake StableSwap pools in a way that cannot be confused with constant-product pools.

**Key constraints (why this is separate):**

- StableSwap pools can be **2-pool or 3-pool**, and pricing/fill semantics differ from V2/V3.
- The SmartRouter stable surface takes `flag` arrays (see `IStableSwapRouter`) and may route through StableSwap mid-path.
- Receipt fill decoding may not be uniquely attributable to a single “pool address” in mixed routes; start with **direct stable swap** only.

**Recommended initial scope:**

- StableSwap exact-in only (`exactInputStableSwap`) for 2-pool only (enforce `flag[i] == 2`).
- No mixed routes in the first version (no “V2→Stable→V3”).
- Use receipt events from the stable pool(s) if available; if not feasible, require balance-delta accounting (harder).

**Files:**
- Create: `crates/adapters/blockchain/src/contracts/pancakeswap_stable_swap_router.rs`
  - ABI encoding for `exactInputStableSwap(...)` and `exactOutputStableSwap(...)`
  - selector constants (to prevent ABI mix-ups with V2/V3/SmartRouter):
    - `exactInputStableSwap` selector: `0xb4554231`
    - `exactOutputStableSwap` selector: `0xb4c4e555`
  - strict local preflight for `flag` semantics (reject 3-pool in first iteration)
- Modify: `crates/adapters/blockchain/src/contracts/mod.rs` (export the new StableSwap router contract helper)
- Create: `crates/adapters/blockchain/src/execution/amm/pancakeswap_stableswap.rs`
  - implement adapter skeleton + config validation + explicit `supports_*` flags
  - fill decoding strategy decision (event-based vs balance-delta) documented + tested

**Tests:**
- Create: `crates/adapters/blockchain/tests/pancakeswap_stableswap_router_calls.rs`
  - `test_stableswap_selectors_match_expected`
  - `test_stableswap_rejects_three_pool_flags_in_two_pool_mode`
  - `test_stableswap_rejects_mixed_route_in_mvp_mode`

Run:
- `cargo test -p nautilus-blockchain --test pancakeswap_stableswap_router_calls`

---

### Milestone 7f (Optional, post-MVP): Infinity UniversalRouter adapter track (commands + Permit2 + errors) (3–10d)

**Goal:** support PancakeSwap Infinity UniversalRouter swaps safely under signer-only execution.

**Why this is a separate track:**

- UniversalRouter requires **command encoding** (`bytes commands` + `bytes[] inputs`) and includes features that can hide failures (`FLAG_ALLOW_REVERT`).
- UniversalRouter deployments may expose an `execute(bytes,bytes[])` overload (no deadline). We MUST enforce the deadline-bearing selector only (see below).
- It supports **Permit2** integration:
  - pre-approved Permit2 allowance flows do **not** require typed-data signing,
  - Permit2 “permit” signature flows **do** require typed-data signing (EIP-712) and must be capability-gated.
- It uses **custom errors** heavily; some failures are nested in `ExecutionFailed(commandIndex, message)`.

**Recommended initial scope (safest path):**

- Support only “swap-only” plans with no `ALLOW_REVERT` flags and no nested `EXECUTE_SUB_PLAN`.
- Enforce a strict command allowlist: deny any command byte not in the explicitly-supported subset for this milestone.
- Recommended safe command subset (illustrative; confirm exact command byte values against upstream `Commands.sol`):
  - Permit2 transfer command(s) that use **pre-approved allowance** (no typed-data),
  - `V2_SWAP_EXACT_IN` (single hop),
  - `V3_SWAP_EXACT_IN` (single hop),
  - optional `SWEEP` to return dust.
- Explicitly deny Permit2 **permit/signature** commands until `permit_mode == Permit2Signature` and the signer supports EIP-712.
- Limit swaps to **exact-in only** initially (no exact-out, no multi-hop).
- Permit2 approach (recommended):
  - require pre-approved Permit2 allowance (ERC20 `approve(token -> Permit2)` out-of-band),
  - defer typed-data Permit2 “permit” commands until a typed-data signer route exists (`signer_capabilities.supports_eip712=true` + tests).
- Explicitly defer `INFI_SWAP` in the first iteration: it is a **second encoding layer** (planner/actions encoding from `infinity-periphery`)
  and should be treated as a separate sub-track once basic UniversalRouter swaps are production-safe.

**Phased delivery recommendation (keeps risk contained):**

1) Command model + safe subset encoding (no Permit2)  
2) Signer-only tx path for UniversalRouter with pre-approved allowances  
3) Add typed-data signer capability + Permit2 commands (one-tx permit+swap)  
4) Harden error decoding/observability (`ExecutionFailed` nesting, opcode attribution)  
5) Add `INFI_SWAP` planner integration (Infinity pools)

**Files:**
- Create: `crates/adapters/blockchain/src/contracts/pancakeswap_infinity_universal_router.rs`
  - ABI encoding for UniversalRouter `execute(bytes,bytes[],uint256 deadline)`
    - **Required selector:** `0x3593564c` (`execute(bytes,bytes[],uint256)`)
    - **Reject selector:** `0x24856bc3` (`execute(bytes,bytes[])`, no deadline)
  - helper encoders for common command templates
  - strict validation:
    - reject any command bytes with `FLAG_ALLOW_REVERT` unless explicitly enabled
    - reject commands outside the allowlisted subset for the milestone
  - validation note: many UniversalRouter parameters are immutables without public getters; startup validation should rely on pinned
    deployment addresses + optional code-hash allowlist (not runtime getter parity)
- Create: `crates/adapters/blockchain/src/contracts/permit2.rs`
  - structs + typed-data domain separator helpers (if signer supports EIP-712)
- Modify: `crates/adapters/blockchain/src/contracts/mod.rs` (export UniversalRouter + Permit2 helpers)
- Modify: `crates/adapters/blockchain/src/execution/signer/*`
  - extend signer API to optionally request `sign_typed_data` (EIP-712) and return signature bytes
- Create: `crates/adapters/blockchain/src/execution/amm/pancakeswap_infinity.rs`
  - adapter that builds universal-router command plans, preflights them, signs any required typed data, then signs/broadcasts the tx
  - robust error decoding:
    - decode `ExecutionFailed(commandIndex,message)`
    - attempt nested revert decode on `message`
  - reconciliation note (required): UniversalRouter/SmartRouter router logs are not canonical fills; fill attribution must decode the underlying
    pool/pair swap logs (potentially multiple per tx) and either emit multiple fills (multi-hop) or fail closed with a structured `Decode` error.

**Tests:**
- Create: `crates/adapters/blockchain/tests/pancakeswap_universal_router_commands.rs`
  - `test_universal_router_requires_deadline_bearing_execute_selector`
  - `test_universal_router_rejects_allow_revert_flag_by_default`
  - `test_universal_router_length_mismatch_maps_to_invalid_inputs`
  - golden vector: command plan encodes to expected calldata
- Create: `crates/adapters/blockchain/tests/permit2_typed_data.rs`
  - `test_permit2_domain_and_struct_hashes_match_known_vectors`
  - `test_signer_typed_data_roundtrip` (mock signer)
  - `test_permit2_custom_errors_map_to_dex_error_codes` (selector-map fixture; Phase 2)

Run:
- `cargo test -p nautilus-blockchain --test pancakeswap_universal_router_commands`
- `cargo test -p nautilus-blockchain --test permit2_typed_data`

---

### Milestone 8: Extend `BlockchainExecutionClient` with AMM execution + reconciliation contract (3–6d)

**Goal:** implement PCS swap execution in the existing blockchain execution client while
satisfying Rust startup reconciliation contract requirements.

**Dependencies (required):** Milestones `0a`, `4`, `5`, `6`, `7`, and `7a` (execution RPC, signer, AMM adapter, and fill decoding must exist before wiring end-to-end execution).

- Accept SubmitOrder (market)
- Perform (optional) approve
- Quote, compute slippage bounds
- Build swap tx, sign, broadcast
- Monitor receipt
- Emit order events + reconciliation reports

**Risk-management delivery note (recommended):**

- Do not block early progress on “perfect error taxonomy”. After Milestones `4–7a`, implement a minimal end-to-end
  vertical slice first:
  - require pre-approved allowance (skip the approve sub-flow initially),
  - implement: quote → build swap tx → signer → sendRawTx → receipt → decode fills → emit one fill,
  - then add: approve flow + post-approve allowance verification, tx journal durability, confirmations/reorg handling,
    and expanded error mapping.

**Files (Rust-first approach):**
- Modify: `crates/adapters/blockchain/src/execution/client.rs` (implement AMM execution and report generation)
- Create: `crates/adapters/blockchain/src/execution/journal.rs` (durable tx journal + restart recovery helpers)
- Modify: `crates/adapters/blockchain/src/execution/amm/mod.rs` (adapter trait + registry + dispatch)
- Modify: `crates/adapters/blockchain/src/execution/amm/pancakeswap_v2.rs` (PCS V2 implementation)
- Modify: `crates/adapters/blockchain/src/execution/mod.rs` (export AMM modules)
- Modify: `crates/adapters/blockchain/src/config.rs` (implement section 8.4 config surface)
- Modify: `crates/adapters/blockchain/src/factories.rs` (venue routing, config propagation, no new top-level execution client type, fix execution factory type-error messages)
- Modify: `crates/adapters/blockchain/src/constants.rs` (remove hard dependency on static `BLOCKCHAIN_VENUE` for execution routing)
- Modify: `crates/adapters/blockchain/src/python/mod.rs` (ensure exec factory/config are registered)
- Modify: `crates/adapters/blockchain/src/python/config.rs` (keep PyO3 constructor in sync with config fields)
- Modify: `crates/adapters/blockchain/src/python/factories.rs` (expose execution factory metadata symmetrically to data factory)

**Key design choices:**
- Execution path remains **generic AMM** inside `BlockchainExecutionClient` with protocol adapter dispatch.
- Do not add PCS-specific public execution methods to the client surface.
- Client config includes:
  - core routing: `venue`, `chain`, `wallet_address`
  - rpc: `http_rpc_url`, `rpc_requests_per_second`
  - signer: endpoint/route/api mode/timeout/mTLS
  - AMM: `dex_type`, `router_address`, `router_abi_mode`, defaults (slippage/deadline), approval policy
  - safety/ops: confirmations, receipt polling/timeout, max inflight txs, gas fee strategy/caps
- Replace all runtime `todo!()` in `BlockchainExecutionClient` with non-panicking behavior
  (implemented or explicit unsupported/empty returns).
- `generate_mass_status` is mandatory for startup reconciliation participation.

**Implementation steps (granular):**

1. Implement config validation helpers (fail-fast):
   - `venue.is_dex()` and `venue.parse_dex()` matches `chain` + `dex_type`
   - validate all addresses (`wallet`, `router`, `factory`, `wnative`) and `eth_getCode != 0x`
   - enforce `eth_chainId()` from RPC matches `config.chain` (fail start if mismatch)
   - if `expected_genesis_hash` is configured: fetch block 0 and fail if mismatch (section 7.6)
   - if `signer_require_tls=true`: reject insecure signer endpoint schemes at startup
2. Implement per-order override parsing from `SubmitOrder.params`:
   - `amm.slippage_bps`, `amm.deadline_secs`, `amm.expected_notional`
3. Implement nonce/tx serialization policy (MVP):
   - enforce `max_inflight_txs_per_wallet = 1` and queue subsequent orders
   - deadlock guard: model `approve → swap` as a single workflow lane within that one in-flight slot (do not re-acquire the same per-wallet lock for the internal approve step)
   - persist nonce reservations + in-flight intents in a minimal tx journal for restart recovery (section 6.3)
4. Implement full swap lifecycle:
   - quote → compute bounds → optional approve → **post-approve allowance verification** → (re-quote after approve) → build swap tx → sign → broadcast → poll receipt → decode fills → emit reports
   - always compute `tx_hash` from `raw_tx_hex` after signing; treat “ambiguous broadcast” as “broadcast-by-hash” (section 6.3)
   - enforce receipt identity checks: `tx.from==wallet`, `tx.to==expected`, `nonce==expected`, `chain_id==expected`
   - reorg-safe finalization: do not terminalize until `confirmations_required` and receipt still present
5. Implement reconciliation hooks:
   - `generate_mass_status` returns deterministic, typed collections
   - `register_external_order` stores tracking state (even if no follow-up actions in MVP)

**Tests:**
- Create shared mock harness:
  - `crates/adapters/blockchain/tests/common/mock_rpc.rs`
  - `crates/adapters/blockchain/tests/common/mock_signer.rs`
  - `crates/adapters/blockchain/tests/common/fixtures.rs`
  - `crates/adapters/blockchain/tests/test_data/amm/*.json`
- Create: `crates/adapters/blockchain/tests/amm_execution_flow.rs`
  - `test_submit_market_order_happy_path_emits_fill_from_swap_log`
  - `test_submit_market_order_commission_is_zero_and_gas_is_tracked_in_tx_journal`
  - `test_submit_market_order_idempotent_receipt_processing_does_not_double_emit_fill`
  - `test_submit_market_order_duplicate_submit_is_noop_while_inflight`
  - `test_submit_market_order_duplicate_submit_is_rejected_when_terminal`
  - `test_venue_order_id_accepts_0x_prefixed_tx_hash_string` (regression: VenueOrderId constraints)
  - `test_submit_market_order_revert_receipt_rejects_order`
  - `test_submit_market_order_revert_reason_is_recovered_and_mapped_to_dex_error_code` (best-effort; section 8.5.3)
  - `test_submit_market_order_signer_403_rejects_without_broadcast`
  - `test_submit_market_order_rejects_if_signer_returns_sender_not_matching_wallet`
  - `test_submit_market_order_handles_approve_then_swap_two_receipts`
  - `test_restart_recovery_approve_mined_swap_pending_does_not_double_broadcast` (common crash window)
  - `test_submit_market_order_quote_slippage_guard_rejects_pre_broadcast`
  - `test_submit_market_order_params_override_slippage_bps_is_applied`
  - `test_submit_market_order_params_override_deadline_secs_is_applied`
  - `test_submit_market_order_rejects_if_instrument_venue_not_matching_client_venue`
  - `test_start_fails_if_rpc_chain_id_mismatch`
  - Assert emitted Nautilus events (`Accepted`/`Filled` or rejected), tx hash tracking, and operation order.
  - Assert `generate_mass_status` returns consistent order/fill/position collections (allow empty sets).
- Create: `crates/adapters/blockchain/tests/amm_slippage_math.rs`
  - `test_exact_in_min_out_rounds_down_conservatively`
  - `test_exact_out_max_in_rounds_up_conservatively`
- Create: `crates/adapters/blockchain/tests/amm_execution_retries.rs`
  - `test_receipt_poll_retries_until_confirmed`
  - `test_ws_stale_falls_back_to_receipt_watchdog_polling_and_recovers`
  - `test_execution_budget_is_not_starved_by_data_budget_under_load`
  - `test_send_raw_tx_transient_rpc_error_retries_once`
  - `test_non_retryable_rpc_error_fails_fast`
  - `test_ambiguous_broadcast_timeout_still_polls_by_computed_tx_hash`
  - `test_send_raw_tx_already_known_treated_as_accepted_and_polled_by_hash`
  - `test_send_raw_tx_returned_hash_mismatch_fails_closed`
  - `test_send_raw_tx_nonce_too_low_rejected_with_actionable_reason`
  - Assert retry boundaries and timeout behavior.
- Create: `crates/adapters/blockchain/tests/amm_execution_confirmations.rs`
  - `test_confirmations_required_delays_fill_until_threshold`
- Create: `crates/adapters/blockchain/tests/amm_execution_reorgs.rs` (can be unit-only with mocked receipt disappearance)
  - `test_receipt_disappears_before_confirmations_keeps_order_pending`
  - `test_receipt_reappears_and_confirms_emits_fill_once`
- (post-MVP) Create: `crates/adapters/blockchain/tests/amm_execution_replacement_tx.rs`
  - `test_same_nonce_replacement_tx_wins_and_no_double_fill`
  - `test_replacement_tx_gas_accounting_counts_only_mined_hash`
- Create: `crates/adapters/blockchain/tests/execution_client_no_todos.rs`
  - `test_execution_client_methods_do_not_panic_and_return_deterministic_results`
- Create: `crates/adapters/blockchain/tests/amm_error_mapping.rs`
  - `test_decode_error_string_revert_data`
  - `test_decode_panic_revert_data`
  - `test_map_common_router_revert_strings_to_dex_error_codes`

Run:
- `cargo test -p nautilus-blockchain --test amm_execution_flow`
- `cargo test -p nautilus-blockchain --test amm_slippage_math`
- `cargo test -p nautilus-blockchain --test amm_execution_retries`
- `cargo test -p nautilus-blockchain --test amm_execution_confirmations`
- `cargo test -p nautilus-blockchain --test execution_client_no_todos`
- `cargo test -p nautilus-blockchain execution::amm`

**CI (recommended):**

- Add a fast required job in `.github/workflows/build.yml` which runs:
  - the feature-flag matrix commands from section 2.4, and
  - the deterministic AMM unit tests above (no anvil).
- Add a nightly/manual job in `.github/workflows/nightly-tests.yml` which runs the ignored `amm_anvil` test when enabled.

---

### Milestone 8b: Rust periodic reconciliation scheduler (post-MVP, optional) (1–2d)

**Goal:** wire existing `ExecutionManager` periodic consistency helpers in Rust runtime.

**Files:**
- Modify: `crates/live/src/node.rs` (schedule periodic callbacks/tasks)
- Modify: `crates/live/src/runner.rs` (if needed for task wiring)
- Modify: `crates/live/src/manager.rs` (entry points already exist; add observability hooks only as needed)

**Scope:**
- Wire `check_open_orders` cadence from config (`open_check_interval_secs`).
- Wire `check_positions_consistency` cadence from config (`position_check_interval_secs`).
- Wire `check_inflight_orders` cadence (`inflight_check_interval_ms` / `inflight_threshold_ms` / retry config already exists).
- Ensure periodic check results are applied (events emitted/processed through the execution engine path, not dropped).
- Emit metrics/logs for retries, drift counts, and reconciliation latency.
- Decide and document: wire `reconciliation_startup_delay_secs` into startup reconciliation timing, or remove/deprecate the config field.

**Tests:**
- Add/extend live manager/node integration tests to verify periodic invocations and retry behavior.

Run:
- `cargo test -p nautilus-live manager`

**Optional anvil integration for Milestone 8 (not default CI in MVP):**
- Create: `crates/adapters/blockchain/tests/amm_anvil.rs`
  - Mark with `#[ignore = "requires anvil + contracts"]`.
  - Gate by env vars (`NAUTILUS_RUN_ANVIL_TESTS=1`, `ANVIL_BIN`, `PCS_TEST_ROUTER`, token addresses).
  - Scenario:
    - launch anvil (or connect to pre-launched instance),
    - deploy or configure router/pair/token fixtures,
    - execute approve + swap through signer endpoint,
    - assert on-chain balances and receipt logs align with Nautilus fill report.

Run:
- `NAUTILUS_RUN_ANVIL_TESTS=1 cargo test -p nautilus-blockchain --test amm_anvil -- --ignored`

---

### Milestone 9: Python adapter surface (user-facing configs + examples) (2–4d)

Even if core execution is Rust, Nautilus users expect a Python adapter package for usability.

**Files:**
- Create: `nautilus_trader/adapters/pancakeswap/__init__.py`
- Create: `nautilus_trader/adapters/pancakeswap/constants.py`
- Create: `nautilus_trader/adapters/pancakeswap/config.py`
- Create: `nautilus_trader/adapters/pancakeswap/factories.py` (thin wrapper calling PyO3 factories)
- Create: `examples/live/pancakeswap/pancakeswap_v2_swap_tester.py`
- Create: `docs/integrations/pancakeswap.md`

**Expected user experience:**
- User imports `PancakeSwapV2ExecClientConfig` (Python wrapper), provides signer endpoint + router + pool list, and registers a `BlockchainExecutionClientFactory` with `TradingNode`.
- Under the hood, the wrapper builds a `BlockchainExecutionClientConfig` with:
  - `venue="Bsc:PancakeSwapV2"`
  - `chain=Bsc` (or `BscTestnet`)
  - `dex_type=PancakeSwapV2`
  - signer + router + approval + gas policy fields from section 8.4
- Router/factory/WBNB default from `chain_id` network presets, with optional explicit override in adapter config (never in strategy code).
- Wrapped-native clarity (WBNB):
  - MVP uses ERC20-only swaps, so strategies trade pools containing **WBNB** (not native BNB).
  - If the wallet holds only native BNB, user must pre-wrap to WBNB (or enable Phase 2 native-value swaps / wrap-unwap support from section 3.7/3.8 + Milestone 6b).
  - Startup validation should emit a clear actionable error when a swap would require `tx.value > 0` but `enable_native_value_swaps=false`.

**Default-address design (must implement):**
- **Single source of truth:** keep canonical per-chain PCS defaults in Rust (adapter/model layer) and *export them to Python* via PyO3.
  - Python `nautilus_trader/adapters/pancakeswap/constants.py` should be a thin layer that reads defaults from Rust exports (or is generated),
    not a duplicated constant table.
  - This prevents default-address drift across Rust/Python and makes upgrades auditable.
- Config precedence:
  1. explicit user config values (highest),
  2. per-chain PCS defaults for known chain IDs (56/97),
  3. startup derivation check via `router.factory()` and `router.WETH()`; hard-fail on mismatch unless `allow_unsafe_address_override=true`.
- Add startup validation:
  - `eth_getCode(router/factory/WBNB) != 0x`
  - `router.factory()` equals configured/default factory
  - `router.WETH()` equals configured/default WBNB (for testnet, router value is source of truth)

**Tests:**
- Python integration tests under:
  - Create: `tests/integration_tests/adapters/pancakeswap/test_factories.py`
  - Create: `tests/integration_tests/adapters/pancakeswap/test_execution.py`
  - Mirror Hyperliquid-style fixtures and mocks from:
    - `tests/integration_tests/adapters/hyperliquid/conftest.py`
    - `tests/integration_tests/adapters/hyperliquid/test_execution.py`
  - Assert:
    - `nautilus_pyo3.blockchain` exports `BlockchainExecutionClientConfig` + `BlockchainExecutionClientFactory` under `nautilus-pyo3 --features defi`
    - global PyO3 registry contains exec factory extractor for `"BLOCKCHAIN"` and config extractor for `"BlockchainExecutionClientConfig"`
    - config validation for signer/router fields
    - factory wires venue `Bsc:PancakeSwapV2` and constructs client correctly
    - submit-order errors from Rust/PyO3 path map to expected Nautilus execution events

Run: `pytest tests/integration_tests/adapters/pancakeswap -q`

---

### Milestone 10: Market data (optional for MVP; recommended for completeness) (3–7d)

Market data for AMMs has two roles:

1) **Operator/strategy visibility** (prices/volumes, “is the pool alive?”), and  
2) **Execution support** (quote freshness, slippage tuning, pre-trade sanity checks).

Because production RPC (e.g. Chainstack) is **rate-limited**, this milestone is designed to be “RPC-budget aware”.

#### Option A (recommended for scale): HyperSync/indexer-backed streaming

**Goal:** stream swap events for many pools without burning managed-RPC request budget.

**Config stance (recommended for production):**
- `BlockchainDataClientConfig.use_hypersync_for_live_data = true`
- Use Chainstack (or similar) HTTP RPC mainly for:
  - execution-path calls (submit swaps),
  - light startup validation (`eth_chainId`, `eth_getCode`),
  - bounded metadata reads (token decimals/symbol/name via Multicall3).

**Important note for PCS V2:** today the blockchain data pipeline is UniswapV3-like and does not yet parse/store
UniswapV2-like `Swap(amount0In,amount1In,amount0Out,amount1Out,...)` events (section 4.4). To make streaming PCS V2 swaps work:

- Add `PoolSwapV2` (or equivalent) model type alongside existing `PoolSwap`
- Implement V2 swap parsing in `crates/adapters/blockchain/src/exchanges/parsing/uniswap_v2/*`
- Extend the data core to batch/store/emit V2 swap events
- Ensure event capability model supports CPAMMs (Milestone 2a)

#### Option B (recommended for small universes): WS newHeads + bounded `eth_getLogs` per block

**Goal:** stream swaps for a *small* pool set (tens/low hundreds) using managed RPC safely.

**Feature-flag note:** today `crates/adapters/blockchain/src/data/*` is `--features hypersync`-gated.
Option B can initially live behind `hypersync` even if it does not use HyperSync at runtime, but the longer-term
goal is to make “RPC-only live data” buildable without hypersync once the feature flags are cleaned up (Milestone 0a).

**Key idea:** subscribe to `newHeads` over WebSocket (push), and on each new block fetch logs for that block only
(pull), chunking by address count and enforcing hard caps.

**Why this is Chainstack-friendly:**
- Predictable request volume: ~`ceil(num_pools / chunk_size)` `eth_getLogs` calls per block.
- No historical-range `eth_getLogs` scans on the hot path.

**Hard limits (required, fail closed):**
- `max_pools_for_rpc_streaming` (default conservative; e.g., 100)
- `get_logs_address_chunk_size` (e.g., 50–200 depending on provider)
- `process_head_lag_blocks` (e.g., 1–2) to reduce reorg churn for live logs (fetch `head-lag` instead of `head`)
- `max_catchup_blocks` (e.g., 100) and `max_get_logs_blocks_per_request` (e.g., 5)
  - if WS disconnect gap exceeds cap: stop streaming and instruct operator to use hypersync/backfill tooling.
- Adaptive splitting rule: if `eth_getLogs` fails due to provider result/payload limits, shrink `get_logs_address_chunk_size`
  and/or range and retry (bounded attempts). If still failing, stop streaming and surface `RATE_LIMITED`/provider-limit telemetry.

**Files:**
- Modify: `crates/adapters/blockchain/src/rpc/types.rs`
  - extend RPC subscription support to include `newHeads` (already) plus block-number extraction helpers
- Create: `crates/adapters/blockchain/src/data/live/rpc_logs.rs`
  - `RpcLogStreamer` that:
    - tracks last seen block,
    - on each new head calls `eth_getLogs` for swap topic0 filtered by known pool addresses (chunked),
      - prefer `blockHash`-anchored filters when the provider supports it (reduces reorg ambiguity),
      - otherwise use `fromBlock=toBlock=head_number` and store `(block_number, block_hash)` to detect hash changes.
    - uses a deterministic dedupe key for emitted events:
      - recommended: `(block_hash, tx_hash, log_index)` (fallback: `(block_number, tx_hash, log_index)` if hash unavailable)
    - defines reorg replay rules:
      - if a previously processed block number is later observed with a different hash, replay that block’s logs and emit `REORG_DETECTED`
        telemetry; downstream must treat reorged events as removed/invalidated where applicable.
    - emits decoded swap events into the existing pipeline
- Modify: `crates/adapters/blockchain/src/data/client.rs`
  - add a `LiveDataSource` mode:
    - `Hypersync` (existing)
    - `RpcLogsPerBlock` (new; bounded)
    - `None` (no streaming)

**Tests:**
- Create: `crates/adapters/blockchain/tests/rpc_log_streamer.rs`
  - `test_log_streamer_chunks_addresses_and_calls_get_logs_once_per_block`
  - `test_log_streamer_enforces_max_catchup_blocks_and_fails_closed`
  - `test_log_streamer_does_not_call_get_logs_when_no_pools_subscribed`
  - assert call counts against a mock RPC server (protects managed-RPC budgets)

#### Option C (MVP-minimal): execution-only quotes, no streaming

**Goal:** ship PCS execution without any live data streaming.

- Only quote at execution time (`eth_call` to router/pair/quoter)
- No subscription footprint; lowest operational complexity and minimal RPC usage

**Plan rule:** MVP can ship with Option C, but production deployments that require monitoring or analytics should adopt
Option A (preferred) or Option B (small universe).

---

### Milestone 10a (Optional): PCS V3 on BSC live-streaming completion (HyperSync-first, RPC optional) (2–5d)

**Goal:** ensure PCS V3 pools on BSC emit live data reliably when subscriptions are enabled.

**Files:**
- Modify: `crates/adapters/blockchain/src/data/client.rs`
  - remove hard `unwrap()` on event signatures in live block fan-out path
  - propagate optional signatures safely (skip unsupported event types)
- Modify: `crates/adapters/blockchain/src/hypersync/client.rs`
  - extend `process_block_dex_contract_events(...)` to support optional collect/flash topic parsing (or explicitly document unsupported live events)
- Modify: `crates/adapters/blockchain/src/data/subscription.rs`
  - align API with optional event signatures so only actually-supported topics are tracked
- Modify: `crates/adapters/blockchain/src/data/core.rs`
  - register only supported signatures for each DEX definition (including PCS V3 on BSC)
- (Optional RPC path) Modify:
  - `crates/adapters/blockchain/src/rpc/chains/bsc.rs`
  - `crates/adapters/blockchain/src/rpc/mod.rs`
  - `crates/adapters/blockchain/src/data/core.rs` (`initialize_rpc_client`)

**Acceptance criteria:**
- Subscribing to `PoolSwaps` for `Bsc:PancakeSwapV3` emits live swap events.
- Subscribing to liquidity updates emits mint/burn updates.
- If collect/flash are declared in PCS V3 definition, subscriptions either emit them or fail explicitly with a deterministic “unsupported” error.

**Tests:**
- Create: `crates/adapters/blockchain/tests/pancakeswap_v3_bsc_streaming.rs`
  - `test_bsc_pcs_v3_swap_subscription_emits_pool_swap`
  - `test_bsc_pcs_v3_liquidity_subscription_emits_mint_burn`
  - `test_missing_optional_signatures_do_not_panic_or_unwrap`

Run:
- `cargo test -p nautilus-blockchain --features hypersync --test pancakeswap_v3_bsc_streaming`

---

**Operational note (important):**

Even when streaming is enabled, do not let market data degrade execution reliability:
- keep data-streaming RPC calls behind a lower-priority budget queue,
- pause/slow streaming automatically when `429`/rate-limit signals increase,
- never increase receipt polling frequency above block cadence unless explicitly configured.

---

## 10) Security, operational concerns, and “gotchas”

### 10.1 Never hardcode addresses in strategy code

Router/factory/quoter addresses must be config, validated at startup:

- `eth_getCode(router) != 0x`
- optional static calls to check interface
- Adapter-level defaults for PCS V2 (section 3.6) keyed by chain id; strategy code references only adapter config.
- For chain id 97, if provided WBNB differs from `router.WETH()`, fail fast with explicit error showing both addresses.

### 10.2 Policy metadata must be correct

Signer enforcement is only as good as the fields we send:

- deadline must be absolute epoch seconds, not relative
- slippage bps must match how minOut/maxIn are computed
- `to` must be either router (swap) or token contract (approve)

### 10.3 Token decimals > 16

Nautilus numeric types cap at 16 decimals.

Implementation must:

- convert `Quantity`/`Money` to on-chain integer units using full token decimals
- keep full decimals in metadata and avoid rounding surprises

### 10.4 Reorgs and receipt finality

MVP can treat 1 confirmation as final, but add config:

- `confirmations_required` (default 1; allow >1)

### 10.5 Non-standard ERC20 behavior (fee-on-transfer / rebasing)

PCS V2 can be used with “taxed” tokens, but **MVP should not**:

- fee-on-transfer tokens can cause `Swap` event amounts to diverge from the recipient’s net received amount
- rebasing tokens can break balance-delta assumptions

MVP rule: disallow these tokens (documented constraint). Phase 2 options:

- use router `SupportingFeeOnTransferTokens` variants where applicable
- compute outputs from recipient `Transfer` deltas instead of swap event amounts
- require explicit allowlist + token risk profile metadata before enabling FoT paths in production
- treat FoT quotes as conservative estimates and prefer tighter max-notional/risk caps
- fail closed when transfer delta attribution is ambiguous (never emit synthetic/guessed fills)
- treat PCS V3 FoT as unsupported by default (V3 pool swap enforces exact callback payment; preflight reject unless explicitly validated)

### 10.6 Signer fail-closed controls for OSS limitations

Because OSS signer-server currently ignores unknown fields and does not enforce some invariants, Nautilus must explicitly treat signer policy as *necessary but insufficient*.

Required security controls to plan and test:

- **no policy-only trust:** enforce local checks for slippage/deadline/function/recipient/value invariants before signing
- **no silent coercions:** reject decimal-like `value` strings and any implicit legacy→EIP1559 fee conversion unless explicitly configured
- **no zero-fee surprises:** reject requests that would sign with zero `maxFeePerGas`/`maxPriorityFeePerGas` in execution mode
- **no stale deadline signing:** reject past/negative deadlines even if signer would allow them
- **no blind signer response trust:** decode and verify returned `raw_tx_hex` matches intended request exactly
- **no unsafe overrides:** keep signer/security knobs non-overrideable at per-order level

### 10.7 Platform wiring checks (non-PCS but can block PCS)

PCS execution depends on Nautilus’ live wiring behaving as expected. Verify and, if necessary, plan follow-up fixes:

- Execution routing: ensure `TradingNode` / `LiveNodeBuilder` registers execution clients such that
  `ExecutionEngine` routes by `Venue` (DEX venues must match exactly; do not rely on a default client).
- Startup reconciliation: ensure startup reconciliation is invoked and any config knobs like
  `reconciliation_startup_delay_secs` are either wired or removed to avoid a “config lies” situation.
- Periodic reconciliation: ensure periodic check results are applied/emitted (Milestone 8b).

### 10.8 MEV / sandwich risk / quote freshness (AMM execution reality)

AMM swaps executed via the public mempool are exposed to MEV (sandwiching) and rapid price movement.
This plan’s safety model is:

- **slippage bounds are mandatory**: `amountOutMin` / `amountInMax` are computed from a fresh quote and capped by policy.
- **deadlines are short**: enforce min/max TTL and reject stale deadlines.
- **re-quote after any delay**: if an `approve` is required, re-quote after the approve receipt before building the swap tx.
- **optional pre-broadcast simulation**: (future) perform a best-effort `eth_call` of the swap at the latest block to detect obvious reverts.
- (future) consider private relay / private RPC submission to reduce mempool exposure if required by strategies.

### 10.9 Operational runbook (minimum)

Document expected behavior for common operational failures:

- **Signer down / 5xx**: order is rejected fast (non-retryable) or retried per bounded policy; never falls back to local signing.
- **RPC down / transient**: bounded retries; if ambiguous broadcast, poll by computed `tx_hash` instead of re-signing.
- **Stuck tx**: in MVP, wait until `receipt_timeout_secs` and then mark as rejected with actionable reason; replacement policy is post-MVP.
- **Receipt missing / reorg**: do not finalize until `confirmations_required`; if receipt disappears before threshold, keep pending and continue polling.
- **Decode failure on success receipt**: treat as execution error requiring investigation; do not emit synthetic fills on the normal success path.

---

## 11) Alternatives considered (and why not chosen for MVP)

### Option 1 (Recommended): Rust-first AMM execution built on blockchain adapter

Pros:
- consistent with adapter guide (`docs/developer_guide/adapters.md`)
- shares DeFi types, RPC stack, and future pool discovery/data
- best path to “framework for more DEX”

Cons:
- more upfront Rust work (signer + tx broadcast + ABI encoding)

### Option 2: Python-only PCS adapter using `web3.py`

Pros:
- fastest initial prototype; easiest to port chainsaw code verbatim

Cons:
- diverges from existing Rust DeFi direction in Nautilus
- harder to reuse for other DEXs without performance/complexity debt

If you choose Option 2 anyway, keep the same conceptual layering (RPC client + signer client + protocol adapters) so a later Rust port is straightforward.

### Option 3: Use PancakeSwap SmartRouter/UniversalRouter as the primary execution backend

Pros:
- Future-proof routing surface (V2 + V3 + StableSwap) behind one contract (SmartRouter), and/or a single “universal router” entrypoint (Infinity).
- Potentially fewer transactions via multicall/self-permit patterns (when signer supports typed-data signing).

Cons:
- ABI is **not** the classic V2 router ABI (no deadlines on some paths, struct calldata for V3, StableSwap flags).
- Revert/error semantics can be less human-friendly (missing revert strings, custom errors).
- Requires substantially more protocol-specific decoding and likely EIP-712 signing support (Permit2/self-permit) to realize the UX benefits.

MVP recommendation remains: integrate classic PCS V2 router first, then add SmartRouter/Infinity once the AMM framework and signer capabilities are proven in production.

## Progress Log

- 2026-03-05 - PR-preflight (`prep/ignore-worktrees`, head SHA `3aa099213f71786d288c2338590592777c43d908`) - status: open
  - Added `.worktrees/` to `.gitignore` to satisfy required worktree hygiene guardrail.
  - No codepath or runtime behavior changes.
  - Tests run: none (docs-only `.gitignore` change).

- 2026-03-05 - PR0 (`pr0/pcs-plan-doc`, head SHA `eec2a83595ad8975f81d51f6366e47e16233b68b`) - status: open
  - Landed `docs/plans/2026-03-04-pcs-integration.md` from `remotes/local/plan/pcs-integration` onto `main` lineage.
  - Appended required tracking sections (`Progress Log`, `Deviations / Decisions`, `Known Issues / Follow-ups`) without changing milestone content.
  - Tests run: none (docs-only).

- 2026-03-05 - PR0 (`pr0/pcs-plan-doc`, head SHA `3edac0c621c59c1db0c4bc2b8d354ed17d8355fb`) - status: open
  - Updated PR0 head SHA after appending mandatory plan tracking sections.
  - Tests run: none (docs-only).

## Deviations / Decisions

- 2026-03-05 - Bootstrap decision: used a dedicated temporary external worktree for PR-preflight because `.worktrees/` was not yet ignored on `origin/main`; this avoids polluting repo status while adding the required ignore rule.

## Known Issues / Follow-ups

- Until PR-preflight is merged, branches created directly from `origin/main` will not inherit `.worktrees/` ignore and may show `.worktrees/` as untracked in that base checkout.
