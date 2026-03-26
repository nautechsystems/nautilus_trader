# Rithmic

[Rithmic](https://www.rithmic.com) provides low-latency futures market data and order routing
across supported FCMs and exchanges. This integration supports live market data ingest, instrument
loading, historical bar requests, and live order execution through NautilusTrader.

## Examples

Live example scripts are available [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/rithmic/).

The Rithmic example set includes:

- `rithmic_data_tester.py` for a standard Nautilus `TradingNode` data-client smoke run
- `rithmic_exec_tester.py` for a standard Nautilus `TradingNode` execution smoke run
- `order_submission.py` for a low-level safe working-order submit/modify/cancel flow
- `bracket_submission.py` for a low-level native bracket smoke run
- `oco_submission.py` for a low-level native OCO smoke run

## Overview

This guide assumes a trader is setting up for both live market data feeds and trade execution.
The Rithmic adapter includes multiple components which can be used together or separately
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

## Environments

The adapter supports the following Rithmic environments:

| Environment | Config value                       | Description |
|-------------|------------------------------------|-------------|
| Demo        | `RithmicEnvironment.DEMO`          | Demo / paper trading plants. |
| Live        | `RithmicEnvironment.LIVE`          | Production trading plants. |
| Test        | `RithmicEnvironment.TEST`          | Alternate test routing when provided by your setup. |

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
export RITHMIC_SYSTEM_NAME="your_system_name"
export RITHMIC_ACCOUNT_ID="your_account"
export RITHMIC_FCM_ID="your_fcm_id"
export RITHMIC_IB_ID="your_ib_id"
```

Low-level Rust users can also override the primary and alternate WebSocket endpoints directly on
`GatewayConfig` when targeting a non-standard route or a local test harness. Regular operator
setups should continue to use the canonical `RITHMIC_*_URL` environment variables.

## Symbology

Use native futures symbols together with the exchange in the Nautilus `InstrumentId` for
unambiguous live instrument loading.

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("MNQM6.CME.RITHMIC")
```

The adapter normalizes the venue lookup symbol back to the Rithmic contract symbol (`MNQM6` in
the example above), while preserving the exchange hint for instrument loads and historical bar
requests.

## Market data capability

The current Rithmic adapter supports:

- Instrument definition loading through the provider path
- Live quote tick subscriptions
- Live trade tick subscriptions
- Historical bar requests for supported time bars

The following surfaces remain limited or unsupported:

- Order book deltas and depth are not fully implemented
- Historical quote tick and trade tick requests are not implemented
- Streaming instrument status and close updates are not provided by the venue path
- Funding, mark price, and index price feeds are not provided by the current adapter

## Execution capability

The current adapter supports:

- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- Modify, cancel, cancel-all, and batch-cancel flows
- Reconciliation with bounded execution replay plus open-order snapshot recovery

Native `SubmitOrderList` routing is supported for the following venue-native shapes:

- 3-leg brackets with a `LIMIT` entry, `LIMIT` take-profit, and `STOP_MARKET` stop-loss
- 2-leg OCO pairs

Current execution boundaries:

- The adapter is still one configured execution client per Rithmic account
- Adapter-created native brackets persist their child-ID mapping across process restart
- Venue-only child attribution for native brackets created outside the adapter remains limited by
  Rithmic metadata

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
