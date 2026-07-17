# Hyperliquid asset identity

## Wire coin formats

Hyperliquid uses a `coin` string field across all its API responses (HTTP and
WebSocket). The same field carries different formats depending on the asset type:

| Format | Example | Product type | Distinguishable? |
|--------|---------|--------------|------------------|
| Plain name | `BTC`, `ETH`, `HYPE` | Perp **or** Spot | No — ambiguous |
| `prefix:Name` | `xyz:TSLA`, `cash:NVDA` | HIP-3 builder perp | Yes — colon prefix |
| `#N` | `#970` | HIP-4 outcome coin | Yes — `#` prefix |
| `+N` | `+970` | HIP-4 outcome token | Yes — `+` prefix |
| `@N` | `@107` | Spot fill identifier | Yes — `@` prefix |
| `vntls:Name` | `vntls:vCURSOR` | Vault LP token | Yes — `vntls:` prefix |

### The ambiguity problem

Plain names like `HYPE` are used by both perps (`HYPE-USD-PERP`) and spot
(`HYPE-USDC-SPOT`). The wire protocol does not distinguish them — the only
way to know is which **endpoint** the data came from:

- `clearinghouseState.assetPositions[].position.coin` → always **Perp** (includes builder perps)
- `spotClearinghouseState.balances[].coin` → always **Spot** context (Outcome/Vault detected by prefix)
- `userFills[].coin` → **Perp** for plain coins; spot fills use `@N` format
- `openOrders[].coin` → **Perp** for plain coins; spot uses `@N`
- WebSocket `trades.coin` → determined by subscription (perp or spot channel)

### Detection rules (from wire coin alone)

```text
coin.starts_with('#') || coin.starts_with('+')  → Outcome
coin.starts_with("vntls:")                       → Vault (Spot)
coin.contains(':') && !coin.starts_with("vntls") → HIP-3 builder perp
coin.starts_with('@')                            → Spot fill format
else                                             → AMBIGUOUS (Perp or Spot)
```

When a coin is ambiguous, callers **must** provide the product type from context
(the endpoint, the subscription channel, or an explicit user parameter).

## Asset identity mappings

Each asset has multiple representations used in different contexts:

```text
HyperliquidAssetId (numeric, used on order wire)
    ↕
coin string (used in all API responses)
    ↕
InstrumentId (Nautilus internal, e.g. "BTC-USD-PERP.HYPERLIQUID")
```

### Numeric ID ranges

| Range | Kind | Example |
|-------|------|---------|
| `0..10_000` | Standard perp | BTC = 0, ETH = 1 |
| `10_000..100_000` | Spot (`10_000 + pair_index`) | HYPE-USDC = 10_007 |
| `100_000..100_000_000` | HIP-3 builder perp (`100_000 + dex*10_000 + idx`) | xyz:TSLA = 110_001 |
| `100_000_000+` | HIP-4 outcome (`100_000_000 + 10*outcome + side`) | #970 = 100_000_970 |
| `u32::MAX` | Vault LP token (sentinel, not on wire) | — |

### InstrumentId format

| Kind | Symbol pattern | Example |
|------|---------------|---------|
| Perp | `{COIN}-USD-PERP` | `BTC-USD-PERP.HYPERLIQUID` |
| Builder perp | `{coin}-USD-PERP` | `xyz:TSLA-USD-PERP.HYPERLIQUID` |
| Spot | `{BASE}-{QUOTE}-SPOT` | `HYPE-USDC-SPOT.HYPERLIQUID` |
| Outcome | `{N}-{YES\|NO}-OUTCOME` | `97-YES-OUTCOME.HYPERLIQUID` |
| Vault | `{coin}-USDC-SPOT` | `vntls:vCURSOR-USDC-SPOT.HYPERLIQUID` |

## Where coin strings appear

### HTTP endpoints

| Endpoint | Field | Always one type? | Format |
|----------|-------|-----------------|--------|
| `clearinghouseState` | `assetPositions[].position.coin` | Yes: Perp | Plain or `prefix:Name` |
| `spotClearinghouseState` | `balances[].coin` | Yes: Spot context | Plain=Spot, `#N`/`+N`=Outcome, `vntls:X`=Vault |
| `userFills` | `[].coin` | Deterministic | Plain=Perp, `@N`=Spot, `:`=BuilderPerp |
| `frontendOpenOrders` | `[].coin` | Deterministic | Plain=Perp, `@N`=Spot, `:`=BuilderPerp |
| `recentTrades` | `[].coin` | Yes: matches request | Plain or `prefix:Name` |
| `l2Book` | `coin` | Yes: matches request | Plain or `prefix:Name` |
| `metaAndAssetCtxs` | `universe[].name` | Yes: Perp | Plain |
| `spotMeta` | `tokens[].name` | Yes: Spot base | Plain |
| `allPerpMetas` | `[dex].universe[].name` | Yes: Builder perp | `prefix:Name` |

### WebSocket channels

| Channel | Field | Always one type? | Format |
|---------|-------|-----------------|--------|
| `trades` | `data[].coin` | Yes: matches subscription | Plain or `prefix:Name` |
| `l2Book` | `data.coin` | Yes: matches subscription | Plain or `prefix:Name` |
| `user:fills` | `data.fills[].coin` | Deterministic | Plain=Perp, `@N`=Spot, `:`=BuilderPerp |
| `user:orderUpdates` | `data[].order.coin` | Deterministic | Plain=Perp, `@N`=Spot, `:`=BuilderPerp |
| `allMids` | `data.mids{coin}` | Yes: Perp | Plain (all perp coins) |
| `activeAssetCtx` | `data.coin` | Yes: matches subscription | Plain |
| `bbo` | `data.coin` | Yes: matches subscription | Plain |

## Spot fill coin format (`@N`)

Spot fills on WebSocket use `@{pair_index}` instead of the base token name.
The pair index is derived from `spotMeta.universe` ordering. This format is
only seen in `user:fills` for spot trades.

Mapping: `@107` → pair_index=107 → asset_id = 10_107 → find the spot instrument.

## The unified approach

### Design principles

1. All code uses `HyperliquidProduct` and `HyperliquidProductType`.
2. **All call sites must provide `HyperliquidProductType`.** No inference, no guessing, no fallback chains.
3. **All coin string logic lives in `asset.rs`.** No `starts_with` checks scattered elsewhere.
4. **The registry requires `HyperliquidProductType` for lookup.** There is no untyped `resolve(coin)`.
5. **`BuilderPerp` is distinguishable from `Perp` on the wire** (colon prefix). Callers in mixed endpoints detect it from the coin format, not by guessing — this is not a fallback chain, it's deterministic parsing.

### Types

- **`HyperliquidAssetId`** — numeric wire index (u32)
- **`HyperliquidProduct`** — full metadata enum (carries `pair_index`, `dex_index`, `outcome_index`, `side`, `side_label`, `quote`)
- **`HyperliquidProductType`** — lightweight discriminant enum (no data, just the type tag)
- **`HyperliquidAsset`** — the unified struct (`id` + `coin` + `product`)
- **`HyperliquidAssetRegistry`** — bidirectional index

```rust
/// Lightweight asset type discriminant — no metadata, just classification.
/// Used as a lookup key and caller-provided context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HyperliquidProductType {
    Perp,
    Spot,
    BuilderPerp,
    Outcome,
    Vault,
}

impl From<&HyperliquidProduct> for HyperliquidProductType { ... }

impl HyperliquidProductType {
    /// Deterministic detection from unambiguous coin prefixes.
    /// Returns `None` for plain names (AMBIGUOUS — caller must supply context).
    pub fn from_coin(coin: &str) -> Option<Self> {
        if coin.starts_with('#') || coin.starts_with('+') {
            Some(Self::Outcome)
        } else if coin.starts_with("vntls:") {
            Some(Self::Vault)
        } else if coin.starts_with('@') {
            Some(Self::Spot)  // spot fill format
        } else if coin.contains(':') {
            Some(Self::BuilderPerp)
        } else {
            None  // plain name — could be Perp or Spot
        }
    }
}
```

### `HyperliquidAsset` (in `asset.rs`)

Single type that connects all representations:
- `id: HyperliquidAssetId` — numeric wire index
- `coin: Ustr` — wire coin string
- `product: HyperliquidProduct` — carries product-specific metadata

**Rename**: The current field name `kind: AssetKind` in code becomes `product: HyperliquidProduct`.

Coin format detection (prefix parsing) is internal to `asset.rs` and used only
for **unambiguous** prefixes (`#`, `+`, `vntls:`, `@`, `:`). The registry does
not guess between `Perp` and `Spot` for plain coins — callers must provide
`HyperliquidProductType` from endpoint context. For coins with distinguishable
prefixes, callers can use `HyperliquidProductType::from_coin(coin)` which returns
`None` for ambiguous plain names (forcing the caller to supply context).

### `HyperliquidAssetRegistry`

Keyed by `(coin, HyperliquidProductType)` internally:

```rust
pub fn resolve(&self, coin: &Ustr, class: HyperliquidProductType) -> Option<&HyperliquidAsset>
pub fn get_by_id(&self, id: HyperliquidAssetId) -> Option<&HyperliquidAsset>
pub fn get_by_instrument_id(&self, id: &InstrumentId) -> Option<&HyperliquidAsset>
```

No overloaded `resolve()` without class. Callers always state what they want.

### Caller responsibility

Every place that handles a coin string must know which class it is:

| Source | How caller knows the class |
|--------|---------------------------|
| `clearinghouseState.assetPositions` | Always `Perp` (includes builder perps — colon prefix distinguishes) |
| `spotClearinghouseState.balances` | Always `Spot` context; Outcome/Vault detected by `#`/`+`/`vntls:` prefix |
| `userFills` | Plain coins → `Perp`; `@N` format → `Spot`; colon prefix → `BuilderPerp` |
| `openOrders` | Plain coins → `Perp`; `@N` → `Spot`; colon prefix → `BuilderPerp` |
| WebSocket `trades` subscription | Subscription was for a known instrument → class already known |
| WebSocket `user:fills` | Same rules as HTTP fills |
| `l2Book` / `bbo` response | Was requested for a known instrument |

## Call sites to change

### `get_or_create_instrument` signature

```rust
// FROM:
fn get_or_create_instrument(&self, coin: &Ustr, product_type: Option<HyperliquidProductType>)

// TO:
fn get_or_create_instrument(&self, coin: &Ustr, product_type: HyperliquidProductType)
```

### Currently pass `None` — MUST change to explicit type

| File | Line | Context | Should become |
|------|------|---------|---------------|
| `client.rs` | 2135 | `request_order_status_reports` — open orders | `Perp` for plain; detect by prefix otherwise |
| `client.rs` | 2207 | `request_order_status_report_by_oid` | `Perp` for plain; detect by prefix otherwise |
| `client.rs` | 2240 | Same function, fallback path | Same as above |
| `client.rs` | 2332 | `request_order_status_report_by_client_order_id` | Same |
| `client.rs` | 2389 | `request_fill_reports` — user fills | `Perp` for plain; `@N` → `Spot`; colon → `BuilderPerp` |
| `client.rs` | 2482 | `request_position_status_reports` — perp positions | Always `Perp` |

### Currently pass `Some(pt)` — remove the `Some` wrapper

| File | Line | Context |
|------|------|---------|
| `client.rs` | 1701 | `parse_participant_profile` closure |
| `client.rs` | 2644 | Spot balance parsing (already has explicit `product_type`) |

### Currently pass `Option` from `from_symbol().ok()` — make non-optional

| File | Line | Context | Change |
|------|------|---------|--------|
| `client.rs` | 2694 | `build_submit_order_report` | `from_symbol().unwrap()` or handle error |
| `client.rs` | 2704 | Same — passes to `get_or_create_instrument` | Remove `Option` |
| `client.rs` | 2794 | `build_modify_order_report` | Same |
| `client.rs` | 2799 | Same — passes to `get_or_create_instrument` | Remove `Option` |
| `client.rs` | 3282 | `info_asset` | Same |
| `client.rs` | 3288 | Same — passes to `get_or_create_instrument` | Remove `Option` |

### `parse_participant_profile` closure signature

| File | Line | Current | Target |
|------|------|---------|--------|
| `parse.rs` | 1356 | `F: FnMut(&Ustr, Option<HyperliquidProductType>)` | `F: FnMut(&Ustr, HyperliquidProductType)` |
| `parse.rs` | 1364 | `resolve_instrument(&coin, Some(Perp))` | `resolve_instrument(&coin, Perp)` |
| `parse.rs` | 1378 | Spot balance detection | Already explicit, just remove `Some` |
| `client.rs` | 1701 | Closure call site | Match new signature |

### `execution.rs`

| File | Line | Context | Change |
|------|------|---------|--------|
| `execution.rs` | 61 | Import | No change |
| `execution.rs` | 2218 | `from_symbol(symbol)` | Extend to handle `BuilderPerp`/`Vault` |
| `execution.rs` | 2240 | `if product_type == Outcome` | No change |

### `AssetKind` → `HyperliquidProduct` rename

The current `AssetKind` enum in `asset.rs` becomes `HyperliquidProduct`. All
field references change from `kind` to `product`.

### `HyperliquidProductType` enum extension

```rust
// FROM (common/enums.rs:1000):
pub enum HyperliquidProductType { Perp, Spot, Outcome }

// TO:
pub enum HyperliquidProductType { Perp, Spot, BuilderPerp, Outcome, Vault }
```

Note: `BuilderPerp` is a wire-distinguishable type (colon prefix), not a
caller guess. It's unambiguous from the coin string alone, unlike plain `Perp`
vs `Spot`. The registry stores them as separate entries.

### Test call sites

| File | Line | Change |
|------|------|--------|
| `client.rs` | 3997 | `Some(Spot)` → `Spot` |
| `client.rs` | 4006 | `None` → `Vault` |
| `client.rs` | 4071 | `Some(Outcome)` → `Outcome` |
| `client.rs` | 4075 | `None` → explicit type |
| `client.rs` | 4082 | `None` → `Outcome` |
