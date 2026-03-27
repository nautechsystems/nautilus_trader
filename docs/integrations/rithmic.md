# Rithmic

[Rithmic](https://www.rithmic.com) provides low-latency futures market data and order routing
across supported FCMs and exchanges. This integration supports live market data ingest, instrument
loading, historical bar requests, and live order execution through NautilusTrader.

:::warning
**Alpha status.** The current Rithmic adapter is alpha software. It is still under active testing and development
and should not be used for live trading.
:::

## Overview

This guide assumes a trader is setting up for both live market data feeds and trade execution.
The Rithmic adapter includes multiple components, which can be used together or separately
depending on the use case.

- `RithmicGateway`: Low-level gateway connectivity to the Rithmic plants.
- `RithmicInstrumentProvider`: Instrument parsing and loading functionality.
- `RithmicDataClient`: Low-level Rust market-data client.
- `RithmicExecutionClient`: Low-level Rust execution client.
- `RithmicLiveDataClient`: Nautilus `LiveMarketDataClient` implementation.
- `RithmicLiveExecutionClient`: Nautilus `LiveExecutionClient` implementation.
- `RithmicLiveDataClientFactory`: Factory for Rithmic data clients.
- `RithmicLiveExecClientFactory`: Factory for Rithmic execution clients.

:::note
Most users will define a live trading node configuration and will not need to work with
the lower-level components directly.
:::

## Examples

Live example scripts are available [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/rithmic/).

The current Rithmic example set includes:

- `rithmic_data_tester.py` for a standard Nautilus `TradingNode` data-client smoke run.
- `rithmic_exec_tester.py` for a standard Nautilus `TradingNode` execution smoke run.
- `rithmic_ema_cross.py` for a full live `TradingNode` EMA-cross strategy on resolved front-month futures with internal bars.
- `notebooks/rithmic_live_strategy_sandbox.py` for a live `TradingNode` quote/trade/internal-bar sandbox.
- `notebooks/rithmic_backtest_strategy_sandbox.py` for historical 1-minute bar download plus a local EMA backtest.
- `order_submission.py` for a low-level safe working-order submit/modify/cancel flow.
- `bracket_submission.py` for a low-level native bracket smoke run.
- `oco_submission.py` for a low-level native OCO smoke run.

## Products

The current adapter is futures-focused.

| Product Type | Supported | Notes |
|--------------|-----------|-------|
| Futures market data | ✓ | Quote ticks, trade ticks, instrument definitions, and historical bars. |
| Futures execution | ✓ | Live order submission, reconciliation, native venue brackets, and OCO. |
| Spot / cash products | - | Not exposed through the current adapter surface. |
| Options workflows | Limited | The adapter does not currently provide a complete options-specific operator guide or examples. |

## Environments

The adapter supports the following Rithmic environments:

| Environment | Config value | Description |
|-------------|--------------|-------------|
| Demo | `RithmicEnvironment.DEMO` | Demo / paper trading plants. |
| Live | `RithmicEnvironment.LIVE` | Production trading plants. |
| Test | `RithmicEnvironment.TEST` | Alternate test routing when provided by your setup. |

## Symbology

### Contract symbology

Use native futures symbols together with the exchange in the Nautilus `InstrumentId` for
unambiguous live instrument loading.

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("MNQM6.CME.RITHMIC")
```

The adapter normalizes the venue lookup symbol back to the Rithmic contract symbol (`MNQM6` in
the example above), while preserving the exchange hint for instrument loads and historical bar
requests.

### Live front-month workflow

The current adapter does **not** transparently rewrite a root alias such as `MNQ.CME.RITHMIC`
into the active front-month contract inside live subscriptions or order submission calls.

The supported live workflow today is:

1. Start with a product root and exchange, such as `MNQ` and `CME`.
2. Resolve the active contract through `load_front_month_async(...)`.
3. Build the actual live `InstrumentId`, such as `MNQM6.CME.RITHMIC`.
4. Use that resolved contract ID for live subscriptions, bar requests, and order submission.

The notebook helpers under `examples/live/rithmic/notebooks/` follow this pattern.

```python
from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic.bindings import RithmicGateway
from nautilus_trader.adapters.rithmic.bindings import (
    RithmicInstrumentProvider as BindingInstrumentProvider,
)
from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.config import to_binding_environment
from nautilus_trader.model.identifiers import InstrumentId


async def resolve_front_month_instrument_id(
    profile: str,
    product: str,
    exchange: str,
) -> InstrumentId:
    config = RithmicDataClientConfig.from_env(profile)
    gateway = RithmicGateway(
        environment=to_binding_environment(config.environment),
        username=config.username,
        password=config.password,
        system_name=config.system_name,
        app_name=config.app_name,
        app_version=config.app_version,
        fcm_id=config.fcm_id or "",
        ib_id=config.ib_id or "",
        account_id="",
        enable_ticker=True,
        enable_order=False,
        enable_pnl=False,
        enable_history=False,
    )
    provider = BindingInstrumentProvider(gateway)

    await gateway.connect()
    try:
        contract = await provider.load_front_month_async(product, exchange)
        resolved_exchange = getattr(contract, "exchange", None) or exchange
        return InstrumentId.from_str(f"{contract.symbol}.{resolved_exchange}.{RITHMIC}")
    finally:
        await gateway.disconnect()
```

If you prefer operator shorthand such as `MNQ.CME.RITHMIC`, parse the root and exchange first,
resolve the front month, then pass the resolved contract ID into the live node or strategy.

### Backtest symbology

Backtest and catalog flows are intentionally separate from live front-month resolution. Use the
instrument IDs that exist in your local parquet/catalog data. If your local Rithmic dataset is
stored under a chosen backtest symbol workflow, keep using that dataset directly rather than
expecting the live adapter to rewrite it.

### Futures month codes

Rithmic futures symbols use standard month codes:

- `F` = January
- `G` = February
- `H` = March
- `J` = April
- `K` = May
- `M` = June
- `N` = July
- `Q` = August
- `U` = September
- `V` = October
- `X` = November
- `Z` = December

### Common exchange hints

The provider recognizes exchange hints in either filters or symbology suffixes. Common examples:

- `CME`
- `CBOT`
- `NYMEX`
- `COMEX`
- `ICE`
- `ICE_US`
- `EUREX`
- `MGEX`

## Market data capability

### Data surfaces

| Capability | Status | Notes |
|------------|--------|-------|
| Instrument definition loading | ✓ | Via `RithmicInstrumentProvider` and the live data client provider path. |
| Live quote ticks | ✓ | Subscribes to the Rithmic ticker plant. |
| Live trade ticks | ✓ | Subscribes to the Rithmic ticker plant. |
| Historical bars | ✓ | Time bars plus `1-TICK` replay via the history plant. Native `N-TICK` replay exists in the Rithmic protocol but is not yet exposed by the current `rithmic-rs` request helper used here. |
| Live external bar subscriptions | ✓ | Time bars and tick bars via the history plant when `enable_history=True`. |
| Internal bars | ✓ | Still the simplest live strategy pattern: subscribe to ticks and consolidate inside Nautilus. |
| Historical quote ticks | - | Not exposed through the current Rithmic API path used by this adapter. |
| Historical trade ticks | - | Not exposed through the current Rithmic API path used by this adapter. |
| Order book deltas / depth | Limited | Adapter hooks exist, but full depth support is not complete. |
| Instrument status / close updates | - | No streaming venue path is exposed. |
| Funding, mark price, index price feeds | - | Not provided by the current adapter. |

### Historical bar requests

Historical bar requests require the history plant to be enabled on the data client:

```python
data_config = RithmicDataClientConfig(
    ...,
    enable_history=True,
)
```

If `enable_history=False`, the live node can still stream quotes and trades, but both
`request_bars()` and live external `subscribe_bars()` calls will be rejected. This is useful for
live-only nodes that do not need the history plant.

:::warning
Rithmic historical API usage is plan-limited. On basic Rithmic plans, historical downloads are
typically capped at **20 GB per month**. Rithmic sends warning emails to the account's registered
email address when API usage approaches that limit or when their access rules are being breached.
Do not ignore those emails. Temporary restrictions can be applied automatically if usage continues
after warnings are sent.

If you are downloading large windows, prefer smaller batched requests and monitor the registered
email inbox for notices from Rithmic.
:::

Current historical external bar limits:

- only `EXTERNAL` bars are supported
- only `LAST` price bars are supported
- supported time aggregations are `SECOND`, `MINUTE`, `DAY`, and `WEEK`
- supported historical tick aggregation is currently `1-TICK` only

The `1-TICK` limit is intentional. The adapter does not locally re-aggregate raw ticks into larger
tick bars outside Nautilus aggregators. The Rithmic protocol defines native tick-bar replay, but
the current `rithmic-rs` history request helper still hardcodes the replay specifier to `1`, so the
adapter rejects `>1-TICK` historical requests instead of faking them.

Planned completion:

- once upstream `rithmic-rs` exposes native parameterized tick-bar replay, the adapter should pass
  historical `N-TICK` replay through directly
- no adapter-side tick-bar re-aggregation is planned as part of that follow-up

Rithmic history requests can also be truncated venue-side. If a response returns a round-number bar
count such as `10000`, or the returned bars do not cover the requested window, retry with smaller
time windows. Rithmic documents a `request_key`/resume flow for this case, but the adapter does not
yet auto-resume history requests.

### Live external bars

The adapter now supports venue-fed live time bars and tick bars through Nautilus
`subscribe_bars()`, with the same history-plant dependency as historical bar requests.

Current live external bar limits:

- only `EXTERNAL` bars are supported
- only `LAST` price bars are supported
- supported aggregations are `SECOND`, `MINUTE`, `DAY`, `WEEK`, and `TICK`

Example:

```python
from nautilus_trader.model.data import BarType


bar_type = BarType.from_str("MNQM6.RITHMIC-1-MINUTE-LAST-EXTERNAL")
strategy.subscribe_bars(bar_type, params={"exchange": "CME"})
```

Tick-bar example:

```python
from nautilus_trader.model.data import BarType


bar_type = BarType.from_str("MNQM6.RITHMIC-233-TICK-LAST-EXTERNAL")
strategy.subscribe_bars(bar_type, params={"exchange": "CME"})
```

Use this path when you specifically want venue-fed candles. For many live strategies, internal bars
from quote/trade ticks are still the more robust default because they do not depend on the history
plant being enabled or permissioned on the venue side.

### Live strategy pattern

For live strategies, the recommended pattern is:

1. Resolve the active contract first.
2. Subscribe to quote ticks and trade ticks for that contract.
3. Choose one of:
   - use Nautilus internal aggregation to build bars locally
   - subscribe to live external `LAST` bars with `enable_history=True`

This is the pattern used in `examples/live/rithmic/notebooks/rithmic_live_strategy_sandbox.py`.

## Execution capability

### Order types

| Order Type | Supported | Notes |
|------------|-----------|-------|
| `MARKET` | ✓ | Supported for direct order submission. |
| `LIMIT` | ✓ | Supported for direct order submission and native bracket entry. |
| `STOP_MARKET` | ✓ | Supported for direct order submission and native bracket stop legs. |
| `STOP_LIMIT` | ✓ | Supported for direct order submission. |

### Time in force

| Time in Force | Supported |
|---------------|-----------|
| `DAY` | ✓ |
| `GTC` | ✓ |
| `IOC` | ✓ |
| `FOK` | ✓ |

### Order and reconciliation flows

| Capability | Status | Notes |
|------------|--------|-------|
| Submit order | ✓ | Single-order submission through the execution client. |
| Modify order | ✓ | Venue order ID required once the order is working. |
| Cancel order | ✓ | Venue order ID required once the order is working. |
| Cancel all orders | ✓ | Cancels all open orders for the configured account connection. |
| Batch cancel | ✓ | Supported through `BatchCancelOrders`. |
| Execution replay | ✓ | Bounded replay on connect for recent order/fill state. |
| Open-order snapshot recovery | ✓ | Reconcile active working orders on connect. |
| Account / PnL snapshots | ✓ | Primary account balances and positions are rebuilt from the PnL plant. |
| Shared multi-account fan-out | - | Current adapter remains one execution client per configured Rithmic account. |

### Native `SubmitOrderList` routing

The adapter currently supports venue-native order-list routing for the following shapes:

- 3-leg brackets with one `LIMIT` entry, one `LIMIT` take-profit, and one `STOP_MARKET` stop-loss.
- 2-leg OCO pairs.

General sequential `SubmitOrderList` fallback is intentionally not used. Unsupported list shapes
should be decomposed by the strategy or submitted as individual orders.

### Current execution boundaries

- Adapter-created native brackets persist their child-ID mapping across process restart.
- Reconnect logic can warn about active venue-native bracket parents that were not created and
  persisted locally.
- Venue-only child attribution for native brackets created outside the adapter remains limited by
  available Rithmic metadata.

## Configuration

The adapter reads canonical `RITHMIC_*` environment variables and also supports profile-scoped
overrides through `RITHMIC_{PROFILE}_*`, with the profile-specific values checked first.

Required environment variables:

- `RITHMIC_USERNAME`
- `RITHMIC_PASSWORD`
- `RITHMIC_SYSTEM_NAME`
- `RITHMIC_ACCOUNT_ID` for execution clients

Optional environment variables:

- `RITHMIC_PROFILE`
- `RITHMIC_ENV`
- `RITHMIC_FCM_ID`
- `RITHMIC_IB_ID`
- `RITHMIC_APP_NAME`
- `RITHMIC_APP_VERSION`
- `RITHMIC_EXECUTION_REPLAY_LOOKBACK_SECS`
- `RITHMIC_NATIVE_BRACKET_STATE_PATH`

Example shell setup:

```bash
export RITHMIC_ENV=demo
export RITHMIC_USERNAME="your_username"
export RITHMIC_PASSWORD="your_password"
export RITHMIC_SYSTEM_NAME="your_system_name"  # Exact System value from RTrader Pro > File > User Profile
export RITHMIC_ACCOUNT_ID="your_account"
export RITHMIC_FCM_ID="your_fcm_id"            # Exact FCM value from RTrader Pro > File > User Profile
export RITHMIC_IB_ID="your_ib_id"              # Exact IB value from RTrader Pro > File > User Profile
```

`RITHMIC_PROFILE` is only a local environment-variable namespace. The actual broker-facing values
are `RITHMIC_*_SYSTEM_NAME`, `RITHMIC_*_FCM_ID`, and `RITHMIC_*_IB_ID`.

:::note
Do not guess the Rithmic `System`, `FCM`, or `IB` values from the broker or prop-firm brand
name alone. Some connections use `paper_trading`, Apex uses `Apex`, and other Rithmic brokers may
use non-obvious identifiers even on standard demo or live accounts.

To find the correct values, sign in to the RTrader Pro desktop application and open
`File > User Profile`, then copy `System`, `FCM`, and `IB` exactly as shown. Treat them as
case-sensitive.
:::

### Data client configuration

The most important data-client options are:

- `enable_history`: enable this only when the node needs historical bar requests.
- `instrument_provider.load_all`: preload the full instrument snapshot on connect.
- `instrument_provider.load_ids`: preload a selected live contract set.
- `instrument_provider.filters`: usually include the target futures exchange, such as `{"exchange": "CME"}`.

### Execution client configuration

The most important execution-client options are:

- `account_id`: the Rithmic account this execution client is allowed to control.
- `execution_replay_lookback_secs`: bounded replay window used during reconnect reconciliation.
- `native_bracket_state_path`: optional override for the local persistence file used to recover
  adapter-created native bracket child IDs across restart.

Low-level Rust users can also override the primary and alternate WebSocket endpoints directly on
`GatewayConfig` when targeting a non-standard route or a local test harness. Regular operator
setups should continue to use the canonical `RITHMIC_*_URL` environment variables.

## Trading node example

```python
import os

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic import RithmicLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode

profile = os.environ.get("RITHMIC_PROFILE")
provider = InstrumentProviderConfig(load_all=False, filters={"exchange": "CME"})

base_data = RithmicDataClientConfig.from_env(profile)
base_exec = RithmicExecClientConfig.from_env(profile)

data_config = RithmicDataClientConfig(
    environment=base_data.environment,
    username=base_data.username,
    password=base_data.password,
    system_name=base_data.system_name,
    app_name=base_data.app_name,
    app_version=base_data.app_version,
    fcm_id=base_data.fcm_id,
    ib_id=base_data.ib_id,
    instrument_provider=provider,
)
exec_config = RithmicExecClientConfig(
    environment=base_exec.environment,
    username=base_exec.username,
    password=base_exec.password,
    system_name=base_exec.system_name,
    account_id=base_exec.account_id,
    app_name=base_exec.app_name,
    app_version=base_exec.app_version,
    fcm_id=base_exec.fcm_id,
    ib_id=base_exec.ib_id,
    execution_replay_lookback_secs=base_exec.execution_replay_lookback_secs,
    native_bracket_state_path=base_exec.native_bracket_state_path,
    instrument_provider=provider,
)

node = TradingNode(
    config=TradingNodeConfig(
        logging=LoggingConfig(log_level="INFO", use_pyo3=True),
        data_clients={RITHMIC: data_config},
        exec_clients={RITHMIC: exec_config},
    ),
)
node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
node.add_exec_client_factory(RITHMIC, RithmicLiveExecClientFactory)
node.build()
```

See the `examples/live/rithmic/` scripts for complete runnable node and low-level smoke examples.
