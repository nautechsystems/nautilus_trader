# Get Started with Lighter

Lighter is available through the v2 Rust engine. You can use it from a pure Rust project, or from
Python v2 through PyO3 bindings that expose the same Rust data and execution clients to a Python
`LiveNode`.

The shortest path is to start with public data. Once data subscriptions work, add execution
credentials and then add a strategy that can submit orders.

## Choose a setup path

| Path        | Use when                                            | First step                                 |
|:------------|:----------------------------------------------------|:-------------------------------------------|
| Pure Rust   | You want a compiled app with no Python runtime.     | Copy the Rust quickstart.                  |
| Python v2   | You want Python scripts on the Rust engine.         | Run the Python v2 data tester.             |
| RWA example | You want Databento signal data and Lighter trading. | Read the composite market making tutorial. |

Start from these files:

- Rust quickstart: `examples/quickstarts/lighter-rust-data-client/`.
- Python v2 data tester: `python/examples/lighter/data_tester.py`.
- RWA tutorial: [Composite market making tutorial][lighter-rwa-composite-mm].

The Rust and Python v2 paths both use these pieces:

- `LighterDataClientConfig` selects mainnet or testnet and optional transport settings.
- `LighterExecClientConfig` adds trader/account IDs and resolves credentials.
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
let exec_config = LighterExecClientConfig::builder()
    .trader_id(trader_id)
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

For execution, set the matching environment variables before connecting:

```bash
export LIGHTER_TESTNET_ACCOUNT_INDEX="123456"
export LIGHTER_TESTNET_API_KEY_INDEX="0"
export LIGHTER_TESTNET_API_SECRET="your-lighter-api-secret"
```

Use `LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`, and `LIGHTER_API_SECRET` for mainnet.

## Python v2 starter

Python v2 uses the Rust engine through PyO3. Install a Python v2 development wheel outside a source
checkout, or build the v2 package from source before running these examples. See
[Python v2 installation][python-v2-install].

From a source checkout with Python v2 installed:

```bash
cd python
.venv/bin/python examples/lighter/data_tester.py --lighter-environment testnet
```

That command builds the node and exits. Pass `--run` to connect:

```bash
.venv/bin/python examples/lighter/data_tester.py \
    --lighter-environment testnet \
    --instrument BTC-PERP.LIGHTER \
    --run
```

The Python script mirrors the Rust setup:

```python
builder = LiveNode.builder(
    "LIGHTER-DATA-TESTER-001",
    TraderId.from_str("TESTER-001"),
    Environment.LIVE,
).add_data_client(
    None,
    LighterDataClientFactory(),
    LighterDataClientConfig(environment=LighterEnvironment.TESTNET),
)
```

Use the execution tester only after the data tester works:

```bash
.venv/bin/python examples/lighter/exec_tester.py \
    --lighter-environment testnet \
    --instrument DOGE-PERP.LIGHTER
```

Like the data tester, that command builds the node and exits. Pass `--run` to connect in dry-run
mode, then add `--live-orders` to submit real orders.

## Move to a strategy

The starter paths prove client wiring, subscriptions, and credential lookup. The next step is to
replace the tester with a strategy:

- Use [Write a Strategy (Rust)](write_rust_strategy.md) for a pure Rust strategy.
- Use `python/examples/lighter/nvda_composite_mm.py` for Python v2 node wiring with the built-in
  Rust `CompositeMarketMaker` strategy.
- Use [Composite market making on Lighter RWA][lighter-rwa-composite-mm] when you need the full
  Databento signal setup.

:::warning
Rust execution examples can submit live orders when you set `DRY_RUN` to `false`. Python execution
examples can submit live orders when you pass `--run --live-orders`. Start on testnet or use the
smallest accepted size, and confirm the instrument, environment, account index, API key index, and
private key before you run.
:::

For emergency cleanup, `cargo run --bin lighter-flatten -p nautilus-lighter` cancels open orders
and closes positions for the configured Lighter account. Review it before use because it scans the
account, can take several minutes under the standard 60 req/min quota, and affects more than one
strategy or market when the account has broader exposure.

[lighter-rwa-composite-mm]: ../tutorials/lighter_rwa_composite_mm.md
[python-v2-install]: ../getting_started/installation.md#python-v2-development-wheels
