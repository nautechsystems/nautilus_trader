# Get Started with Lighter

Lighter is available through the Rust engine. You can use it from a pure Rust project, or from
Python through PyO3 bindings that expose the same Rust data and execution clients to a Python
`LiveNode`.

The shortest path is to start with public data. Once data subscriptions work, add execution
credentials and then add a strategy that can submit orders.

## Choose a setup path

| Path        | Use when                                            | First step                                 |
| :---------- | :-------------------------------------------------- | :----------------------------------------- |
| Pure Rust   | You want a compiled app with no Python runtime.     | Copy the Rust quickstart.                  |
| Python      | You want Python scripts on the Rust engine.         | Run the Python data tester.                |
| RWA example | You want Databento signal data and Lighter trading. | Read the composite market making tutorial. |

Start from these files:

- Rust quickstart: `examples/quickstarts/lighter-rust-data-client/`.
- Python data tester: `examples/live/lighter/data_tester.py`.
- RWA tutorial: [Composite market making tutorial][lighter-rwa-composite-mm].

The Rust and Python paths both use these pieces:

- `LighterDataClientConfig` selects the Lighter or Robinhood deployment, mainnet or testnet, an
  optional custom venue, and optional transport settings.
- `LighterExecutionClientConfig` adds the account ID and resolves credentials. Its account issuer
  must match the resolved venue.
- `LighterDataClientFactory` and `LighterExecutionClientFactory` register clients with `LiveNode`.
- `DataTester` and `ExecTester` provide smoke-test actors before you write a custom strategy.

## Pure Rust starter

Copy the quickstart into your own workspace:

```bash
cp -R examples/quickstarts/lighter-rust-data-client ~/lighter-rust-data-client
cd ~/lighter-rust-data-client
cargo run
```

This builds a `LiveNode`, registers the Lighter data client, adds a `DataTester`, and connects to
testnet public streams. Stop it with Ctrl+C.

The core setup uses builders, which fill in optional defaults for you:

```rust
let data_config = LighterDataClientConfig::builder()
    .environment(LighterEnvironment::Testnet)
    .build();

let mut node = LiveNode::builder(trader_id, Environment::Live)?
    .with_name("LIGHTER-DATA-STARTER-001".to_string())
    .add_data_client(
        None,
        Box::new(LighterDataClientFactory::new()),
        Box::new(data_config),
    )?
    .build()?;
```

After the data path works, add an execution client to the builder before calling `.build()`:

```rust
let exec_config = LighterExecutionClientConfig::builder()
    .account_id(account_id)
    .environment(LighterEnvironment::Testnet)
    .build();

let mut node = LiveNode::builder(trader_id, Environment::Live)?
    .with_name("LIGHTER-EXEC-STARTER-001".to_string())
    .add_data_client(
        None,
        Box::new(LighterDataClientFactory::new()),
        Box::new(data_config),
    )?
    .add_exec_client(
        None,
        Box::new(LighterExecutionClientFactory::new()),
        Box::new(exec_config),
    )?
    .build()?;
```

For execution, follow the
[account and API key setup](../integrations/lighter.md#account-and-api-key-setup), then set the
matching environment variables before connecting:

```bash
export LIGHTER_TESTNET_ACCOUNT_INDEX="123456"
export LIGHTER_TESTNET_API_KEY_INDEX="4"
export LIGHTER_TESTNET_API_SECRET="your-lighter-api-secret"
```

The deployment and environment select the credential namespace:

| Deployment | Environment | Credential prefix             |
| ---------- | ----------- | ----------------------------- |
| Lighter    | Mainnet     | `LIGHTER_*`                   |
| Lighter    | Testnet     | `LIGHTER_TESTNET_*`           |
| Robinhood  | Mainnet     | `LIGHTER_ROBINHOOD_*`         |
| Robinhood  | Testnet     | `LIGHTER_ROBINHOOD_TESTNET_*` |

Each namespace supplies `ACCOUNT_INDEX`, `API_KEY_INDEX`, and `API_SECRET`.

## Python starter

Python uses the Rust engine through PyO3. Install a Python development wheel outside a source
checkout, or build the package from source before running these examples. See
[Python installation][python-install].

From the repository root with Python installed:

```bash
uv run --project python --no-sync python examples/live/lighter/data_tester.py
```

The script connects to Lighter Testnet immediately and starts streaming. The deployment,
environment, and instrument are module-level constants at the top of the file.

The Python script mirrors the Rust setup:

```python
builder = LiveNode.builder(
    "LIGHTER-DATA-TESTER-001",
    TraderId.from_str("TESTER-001"),
    Environment.LIVE,
).add_data_client(
    VENUE,
    LighterDataClientFactory(),
    LighterDataClientConfig(
        environment=LighterEnvironment.TESTNET,
        deployment=LighterDeployment.LIGHTER,
    ),
)
```

Use the execution tester only after the data tester works:

```bash
uv run --project python --no-sync python examples/live/lighter/exec_tester.py
```

The execution tester also connects immediately, and it places real orders by default
(`dry_run=False`, with a warning at the top of the module). The default environment is testnet;
set `LIGHTER_DEPLOYMENT` and `LIGHTER_ENVIRONMENT` to select the target deployment and environment.

## Move to a strategy

The starter paths prove client wiring, subscriptions, and credential lookup. The next step is to
replace the tester with a strategy:

- Use [Write a Strategy (Rust)](write_rust_strategy.md) for a pure Rust strategy.
- Use `examples/live/lighter/nvda_composite_mm.py` for Python node wiring with the built-in
  Rust `CompositeMarketMaker` strategy.
- Use [Composite market making on Lighter RWA][lighter-rwa-composite-mm] when you need the full
  Databento signal setup.

:::warning
The Rust execution example ships with `DRY_RUN = false` and can submit live orders as soon as you
run it. Set `DRY_RUN` to `true` to connect without order submission. Python execution examples also
submit live orders as soon as you run them. Start on testnet or use the smallest accepted size, and
confirm the instrument, deployment, environment, account index, API key index, and private key
before you run.
:::

For emergency cleanup, `cargo run --bin lighter-flatten -p nautilus-lighter` cancels open orders
and closes positions for the selected deployment account. Review it before use because it scans the
account, can take several minutes under the standard 60 req/min quota, and affects more than one
strategy or market when the account has broader exposure. Set `LIGHTER_DEPLOYMENT` to `lighter` or
`robinhood` and `LIGHTER_ENVIRONMENT` to `mainnet` or `testnet`; omitted selectors default to
Lighter Mainnet and select the matching credential namespace.

[lighter-rwa-composite-mm]: ../tutorials/lighter_rwa_composite_mm.md
[python-install]: ../getting_started/installation.md#development-wheels
