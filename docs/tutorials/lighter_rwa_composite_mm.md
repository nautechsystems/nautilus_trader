# Composite Market Making on Lighter RWA with Databento US Equities NVDA

This tutorial runs the shipped [`CompositeMarketMaker`][composite-market-maker] strategy on Lighter's
`NVDA-PERP.LIGHTER` RWA market using Databento `NVDA.EQUS` quotes as an external
signal. The strategy quotes one post-only bid and one post-only ask around the
Lighter mid, then shifts both sides from a normalized Databento residual and the
current Lighter inventory.

The setup uses a Rust [`LiveNode`][live-node], while the strategy itself runs as the native
Rust `CompositeMarketMaker` strategy.

## Introduction

Lighter lists real-world asset (RWA) perpetuals that trade continuously, including
single-name equity markets. See Lighter's [RWA docs] and [market specifications]
for current venue details. Databento's [US Equities][Databento US Equities]
datasets provide US equity top-of-book data for `NVDA`, with `mbp-1` available
through the Nautilus Databento adapter.

`CompositeMarketMaker` is a small two-input market maker:

- The **target instrument** is the Lighter market to quote: `NVDA-PERP.LIGHTER`.
- The **signal instrument** is the Databento reference feed: `NVDA.EQUS`.
- The **anchor** is the Lighter mid.
- The **signal residual** is `(databento_mid / baseline) - 1.0`.
- The **quote shift** is `signal_skew_factor * residual - inventory_skew_factor * net_position`.

With no configured baseline, the strategy captures the first observed `NVDA.EQUS`
mid as the reference price. The residual starts at zero and measures NVDA's move
from that first signal mid, not the Lighter/Databento basis. Set the
`SIGNAL_BASELINE` constant in the example source to pin the reference price for
deterministic runs.

In this setup, the Lighter BBO remains the spread anchor. Databento moves the
quote center up or down through the normalized residual.

```mermaid
flowchart LR
    subgraph Databento ["Databento data client"]
        DQ["NVDA.EQUS QuoteTick<br/>dataset = EQUS.PLUS<br/>schema = mbp-1"]
        DS["signal_mid = (bid + ask) / 2"]
        DR["residual = signal_mid / baseline - 1"]
    end

    subgraph Lighter ["Lighter data + execution clients"]
        LQ["NVDA-PERP.LIGHTER QuoteTick"]
        LM["anchor = (bid + ask) / 2"]
        EX["Post-only limit orders"]
    end

    subgraph Strategy ["CompositeMarketMaker"]
        TH{{"no target orders OR anchor/signal impact<br/>>= requote_threshold_bps"}}
        CA["cancel_all_orders()"]
        SK["shift = signal_skew - inventory_skew"]
        QU["bid = anchor - half_spread + shift<br/>ask = anchor + half_spread + shift"]
        PO["submit post-only bid/ask"]
    end

    DQ --> DS --> DR --> SK
    LQ --> LM --> TH
    TH -->|yes| CA --> SK --> QU --> PO --> EX
    TH -->|no| LQ
```

The focus is the adapter wiring: one engine consumes a direct US equity feed and
a crypto-native RWA venue, while order lifecycle, inventory, and quote state stay
inside the same event-driven runtime.

## Prerequisites

- A Rust toolchain (MSRV 1.96.0 or newer).
- A Cargo project with the Nautilus, Lighter, and Databento crates as
  dependencies (see [Project setup](#project-setup)).
- Python 3.12+ to regenerate the rendered panels.
- A Databento API key with live access to Databento US Equities Plus
  (`EQUS.PLUS`) for the bundled `NVDA.EQUS` route.
- Lighter API credentials (numeric account index, API key index, and API secret)
  for the configured environment (testnet by default), required only to connect
  and submit orders.
- The Lighter integration guide: [Lighter](../integrations/lighter.md).
- The Databento integration guide: [Databento](../integrations/databento.md).

The example reads credentials from environment variables and keeps the strategy
parameters as editable Rust constants. It defaults to
`LighterEnvironment::Testnet`, so set the testnet Lighter credentials:

```bash
export DATABENTO_API_KEY="your-databento-api-key"
export LIGHTER_TESTNET_ACCOUNT_INDEX="123456"
export LIGHTER_TESTNET_API_KEY_INDEX="0"
export LIGHTER_TESTNET_API_SECRET="your-lighter-api-secret"
```

For mainnet, change `LIGHTER_ENVIRONMENT` in the source to
`LighterEnvironment::Mainnet` and use the mainnet `LIGHTER_*` credential
variables described in the integration guide. Set `DATABENTO_API_KEY` before
running the example.

## Project setup

The strategy, node, and adapters ship as crates, so you can depend on them from
your own Cargo project rather than working inside a NautilusTrader checkout. Add
the following to your `Cargo.toml`, pointing every Nautilus dependency at the
same `develop` git source so the crates resolve to one consistent version:

```toml
[dependencies]
nautilus-common = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["live"] }
nautilus-core = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop" }
nautilus-databento = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["high-precision", "live"] }
nautilus-lighter = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["examples", "high-precision"] }
nautilus-live = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["node"] }
nautilus-model = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["high-precision"] }
nautilus-trading = { git = "https://github.com/nautechsystems/nautilus_trader.git", branch = "develop", features = ["examples", "high-precision"] }

tokio = { version = "1", features = ["full"] }
```

The `examples` feature on `nautilus-trading` exposes the `CompositeMarketMaker`
strategy, and `high-precision` is required for Lighter's crypto-native pricing.
For the general crate layout, feature flags, and the crates.io alternative to
the git source, see the Rust [project setup guide][project-setup].

The Databento client also needs a publishers file that maps venues to datasets.
Download [`publishers.json`][databento-publishers] from the Databento adapter
crate and point `publishers_filepath` at your local copy. The shipped example
resolves the same file relative to the checkout, so this step only applies to
your own project.

## Why NVDA

`NVDA` is a liquid Nasdaq-listed single-name equity, and Lighter maps its RWA
perpetual to `NVDA-PERP.LIGHTER`. This pairs a licensed Databento signal with a
Lighter traded market:

| Role              | Instrument ID       | Source    | Notes                                      |
| ----------------- | ------------------- | --------- | ------------------------------------------ |
| Signal instrument | `NVDA.EQUS`         | Databento | EQUS.PLUS top‑of‑book quote updates.       |
| Target instrument | `NVDA-PERP.LIGHTER` | Lighter   | RWA perpetual traded through Lighter.      |

Subscribing to `NVDA.EQUS` requests top-of-book (`mbp-1`) quotes for `NVDA` from
Databento's `EQUS.PLUS` dataset by default, delivered as a single `QuoteTick`
stream. The adapter resolves the `EQUS` venue from a publishers file: the
example points `DatabentoLiveClientConfig` at the `publishers.json` bundled with
the Databento adapter. See [Instrument IDs and symbology][databento-symbology]
for the mapping rules.

The older Databento Equities Basic (`DBEQ.BASIC`) dataset name appears in some
grandfathered accounts and historical examples. New Databento subscriptions use
the Databento US Equities product line, so this tutorial uses the consolidated
`EQUS` venue. Treat the top-of-book feed as a licensed signal proxy for the
tutorial wiring, not as a full depth Nasdaq TotalView book.

The example starts at `trade_size=0.05`, which aligns with the Lighter NVDA
minimum base amount observed during tutorial validation. Check the
[market details endpoint] before increasing size or changing instruments.

## Session constraint

Lighter RWA markets trade continuously. `NVDA.EQUS` follows the US equity market
data session. The first live test should run during the regular cash session
(13:30-20:00 UTC, US daylight time), with special handling for holidays and
half-days.

`CompositeMarketMaker` does not include a built-in session gate or signal-age
guard. For production use, add an actor or strategy variant that cancels quotes
when the Databento signal goes stale. The tutorial example keeps this explicit
instead of hiding it in a custom strategy.

## Example node

There are two ways to run this: from a NautilusTrader checkout via the shipped
[Lighter NVDA composite market maker example][example-script] binary, or by
copying the node wiring below into a `main` in your own project that depends on
the crates from [Project setup](#project-setup).

From a checkout, with the credential variables set, the shipped binary builds
the node, registers all three clients, adds the native strategy, and exits without
connecting:

```bash
cargo run --bin lighter-nvda-composite-mm --package nautilus-tutorials --features examples
```

Databento is a multi-venue data client without a fixed venue route, so the engine
uses it as the default route for `NVDA.EQUS`. Lighter registers with the `LIGHTER`
venue route and receives `NVDA-PERP.LIGHTER` subscriptions.

The core of the setup is the three-client node plus `CompositeMarketMaker`:

```rust
let lighter_environment = LIGHTER_ENVIRONMENT;
let trader_id = TraderId::from(TRADER_ID);
let account_id = AccountId::from(ACCOUNT_ID);
let instrument_id = InstrumentId::from(INSTRUMENT_ID);
let signal_instrument_id = InstrumentId::from(SIGNAL_INSTRUMENT_ID);

let databento_api_key = get_env_var("DATABENTO_API_KEY")?;
let databento_config =
    DatabentoLiveClientConfig::new(databento_api_key, publishers_filepath, true, true);
let lighter_data_config = LighterDataClientConfig {
    environment: lighter_environment,
    ..Default::default()
};
let lighter_exec_config = LighterExecClientConfig::builder()
    .trader_id(trader_id)
    .account_id(account_id)
    .environment(lighter_environment)
    .build();

let strategy_config =
    CompositeMarketMakerConfig::new(instrument_id, signal_instrument_id, max_position)
        .with_strategy_id(StrategyId::from("NVDA_COMPOSITE_MM-001"))
        .with_order_id_tag("001".to_string())
        .with_trade_size(trade_size)
        .with_half_spread_bps(HALF_SPREAD_BPS)
        .with_inventory_skew_factor(INVENTORY_SKEW_FACTOR)
        .with_signal_skew_factor(SIGNAL_SKEW_FACTOR)
        .with_requote_threshold_bps(REQUOTE_THRESHOLD_BPS)
        .with_on_cancel_resubmit(ON_CANCEL_RESUBMIT);

let mut node = LiveNode::builder(trader_id, Environment::Live)?
    .with_name("LIGHTER-NVDA-COMPOSITE-MM-001".to_string())
    .with_reconciliation(RUN_LIVE)
    .add_data_client(
        None,
        Box::new(DatabentoDataClientFactory::new()),
        Box::new(databento_config),
    )?
    .add_data_client(
        None,
        Box::new(LighterDataClientFactory::new()),
        Box::new(lighter_data_config),
    )?
    .add_exec_client(
        None,
        Box::new(LighterExecutionClientFactory::new()),
        Box::new(lighter_exec_config),
    )?
    .build()?;

node.add_strategy(CompositeMarketMaker::new(strategy_config))?;
```

To connect and allow order submission, edit the constants near the top of the
example source:

```rust
const RUN_LIVE: bool = true;
const ALLOW_LIVE_ORDERS: bool = true;
```

Then run the same command:

```bash
cargo run --bin lighter-nvda-composite-mm --package nautilus-tutorials --features examples
```

:::warning
This command can submit live orders. Start with the smallest accepted size on a
funded test account or a mainnet account sized for loss. Confirm the active
instrument ID, account ID, numeric account index, and Lighter credentials before
setting `RUN_LIVE` and `ALLOW_LIVE_ORDERS` to `true`.
:::

For a testnet smoke run, keep `LIGHTER_ENVIRONMENT` as
`LighterEnvironment::Testnet` and use the `LIGHTER_TESTNET_*` credential
variables. If the run is outside the Databento US Equities cash session, it can
still validate node startup, routing, Lighter data, and the order lifecycle. The
Databento residual remains zero until the first `NVDA.EQUS` quote arrives.

## Strategy parameters

| Parameter               | Value               | Description                                                    |
| ----------------------- | ------------------- | -------------------------------------------------------------- |
| `instrument_id`         | `NVDA-PERP.LIGHTER` | Lighter RWA perpetual to quote.                                |
| `signal_instrument_id`  | `NVDA.EQUS`         | Databento US Equities Plus signal feed.                        |
| `trade_size`            | `0.05`              | Size per bid or ask.                                           |
| `max_position`          | `0.20`              | Hard cap on net Lighter exposure.                              |
| `half_spread_bps`       | `25`                | Half‑spread around the Lighter anchor.                         |
| `inventory_skew_factor` | `2.0`               | Price units per unit of net position.                          |
| `signal_skew_factor`    | `55.0`              | Price units per unit of normalized Databento residual.         |
| `signal_baseline`       | First signal mid    | Optional reference price for the Databento residual.           |
| `requote_threshold_bps` | `5`                 | Anchor or signal‑impact move that triggers cancel and requote. |

With a Lighter mid of `207.00` and `half_spread_bps=25`, the unskewed half
spread is `0.5175` USD. If Databento is 30 bps above its baseline, a
`signal_skew_factor` of `55.0` shifts both sides up by `0.165` USD before
inventory skew. A long position of `0.05` with `inventory_skew_factor=2.0`
shifts both sides down by `0.10` USD.

## Requote behavior

Signal ticks update internal state but do not submit orders by themselves. Until
the first Databento quote arrives, the residual is zero. The next Lighter quote
tick reads the latest signal residual and checks the quote state. A quote cycle
occurs when:

- no target orders are open or in-flight;
- the Lighter anchor moves by at least `requote_threshold_bps`; or
- the price impact of the signal residual change clears the same threshold.

The strategy then cancels open orders, reads current net position and pending
exposure from the cache, computes one bid and one ask, drops any side that
breaches `max_position`, and submits the remaining sides as post-only limits.

## Panels

The panels below use deterministic replay data. They show the quoting mechanics
and the cash-session constraint. They are not a captured live Lighter fill trace.

![NVDA composite quote center against Databento and Lighter mids](./assets/lighter_rwa_composite_mm/panel_a_reference_overlay.png)

**Figure 1.** *Databento `NVDA.EQUS` mid, Lighter `NVDA-PERP.LIGHTER` mid,
composite bid, composite ask, and quote center.*

![Databento residual, Lighter basis, and quote-center shift](./assets/lighter_rwa_composite_mm/panel_b_signal_basis.png)

**Figure 2.** *Databento residual, Lighter basis, and quote-center shift in bps.*

![Inventory skew terms for the composite market maker](./assets/lighter_rwa_composite_mm/panel_c_inventory_skew.png)

**Figure 3.** *Net position, signal shift, inventory adjustment, and total shift
for a `0.05` NVDA trade size and `0.20` NVDA position cap.*

![Lighter continuous trading and Databento session clock](./assets/lighter_rwa_composite_mm/panel_d_session_clock.png)

**Figure 4.** *Lighter's continuous RWA market clock against the Databento US
Equities cash-session signal, with signal age after the regular session.*

## Regenerate the panels

```bash
uv sync --extra visualization
python3 docs/tutorials/assets/lighter_rwa_composite_mm/render_panels.py
```

The renderer writes four PNGs into
`docs/tutorials/assets/lighter_rwa_composite_mm/`. It uses the
`nautilus_dark` Plotly theme and deterministic replay data so docs builds do not
depend on vendor data licenses or live exchange access.

## Extensions

The next useful improvement is a signal-age gate. For example, cancel all
Lighter orders when the latest `NVDA.EQUS` quote is older than 30 seconds during
the cash session, or immediately after the cash session closes. That makes the
Databento signal an explicit operating dependency instead of an implicit one.

For a pure fair-value strategy, use this tutorial as the client wiring and write
a small variant that anchors bid/ask directly on the Databento mid, then checks
the Lighter BBO only for post-only and basis limits.

[composite-market-maker]: https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/trading/src/examples/strategies/composite_market_maker/strategy.rs
[live-node]: ../how_to/run_rust_live_trading.md
[project-setup]: ../concepts/rust.md#project-setup
[databento-symbology]: ../integrations/databento.md#instrument-ids-and-symbology
[databento-publishers]: https://github.com/nautechsystems/nautilus_trader/blob/develop/crates/adapters/databento/publishers.json
[RWA docs]: https://docs.lighter.xyz/trading/real-world-assets-rwas
[market specifications]: https://docs.lighter.xyz/trading/real-world-assets-rwas/market-specifications
[market details endpoint]: https://mainnet.zklighter.elliot.ai/api/v1/orderBookDetails
[Databento US Equities]: https://databento.com/blog/introducing-databento-us-equities
[example-script]: https://github.com/nautechsystems/nautilus_trader/blob/develop/examples/tutorials/src/bin/lighter_nvda_composite_mm.rs
