# Migrate from v1 to v2

NautilusTrader v2 is the Rust core and PyO3 Python package under `python/`. It becomes the primary
Python path when `develop` switches to that package. Until that cutover, the main distribution and
general documentation still describe the legacy v1 Cython package. Use this guide to test or prepare
a migration without mixing the two environments.

After cutover, v1 moves to the `develop_v1` branch for approximately three months of critical
security backports. It does not receive new feature or parity work.

The v1 and v2 packages both install and import as `nautilus_trader`. Use separate virtual
environments while migrating. Do not install both versions into one environment.

## Install v2

Install a release candidate from PyPI in a fresh environment:

```bash
uv venv --python 3.14
source .venv/bin/activate
uv pip install --pre nautilus_trader
```

Run this block outside a NautilusTrader source checkout. The repository's `exclude-newer` uv policy
can filter out newly published release-candidate wheels. Inside a source checkout, use the source
build below.

Inside a source checkout, build the package from `python/` into its dedicated `python/.venv`:

```bash
make build-debug-v2
cd python
.venv/bin/python -c 'import nautilus_trader; print(nautilus_trader.__version__)'
```

The root `.venv` and root `pyproject.toml` belong to the legacy v1 build during the transition.
See [Installation](docs/getting_started/installation.md) for platform support and package-index options.

## Port Python code

The strategy lifecycle and common data, order, risk, portfolio, backtest, and live workflows remain
available. Update imports and configuration to the new module paths:

| v1 path                                                        | v2 path                                                   |
|----------------------------------------------------------------|-----------------------------------------------------------|
| `nautilus_trader.backtest.engine.BacktestEngine`               | `nautilus_trader.backtest.BacktestEngine`                 |
| `nautilus_trader.backtest.node.BacktestNode`                   | `nautilus_trader.backtest.BacktestNode`                   |
| `nautilus_trader.live.node.TradingNode`                        | `nautilus_trader.live.LiveNode`                           |
| `nautilus_trader.config.StrategyConfig`                        | `nautilus_trader.trading.StrategyConfig`                  |
| Adapter classes from `nautilus_trader.adapters.<venue>.config` | Rust/PyO3 classes from `nautilus_trader.adapters.<venue>` |

In general, v1 types from `nautilus_trader.config` move beside the runtime that owns them. For
example, v2 exposes `BacktestRunConfig` from `nautilus_trader.backtest` and `PortfolioConfig` from
`nautilus_trader.portfolio`.

Use the generated type stubs in `python/nautilus_trader/` as the exact Python contract. The
[Python v2 examples][python-v2-examples] show current live-node builders, adapter factories,
strategies, actors, and data/execution testers.

Python v2 strategies still subclass `Strategy` and override lifecycle or data callbacks:

```python
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class MyStrategyConfig(StrategyConfig):
    pass


class MyStrategy(Strategy):
    def on_start(self) -> None:
        pass
```

v1-style annotated custom fields on a `StrategyConfig` subclass do not carry over. When a v2
subclass adds custom fields, remove their keyword arguments in `__new__` before the PyO3 base
validates them, then assign the fields in `__init__`. See the
[v2 strategy config example][python-v2-strategy-config].

Port one workflow at a time and verify the generated stub before replacing a v1 convenience method.
Do not assume that a v1 adapter config field also exists on its v2 Rust config.

## Accepted contract differences

The cutover accepts these differences from v1:

- Custom data flows as native `CustomData` without the v1 wrapper semantics.
- v2 caches `OptionGreeks` for option fee calculation; this is an extension rather than v1 parity.
- `Bar.is_revision` is not exposed on the v2 Python surface. Do not depend on it during migration.
- A direct `Position.apply` fill that crosses zero resets the open entry price to the flipping fill.
  v1 retains the old side's entry price; the v2 behavior is the go-forward contract.
- `PortfolioConfig.use_mark_prices` defaults to `true`; v1 defaulted to `false`. Set it to `false` to
  skip mark prices.
- Catalog order-event data written before `activation_price` and `OrderFilled.info` were added cannot
  be read by the new schema. Regenerate or migrate that data before upgrading a catalog in place.

## Deferred limits

These gaps do not block the supported cutover workflows, but they can affect a migration:

- Python request callback, join, and pending-request convenience semantics are not complete.
- Direct Python injection of Redis cache databases and external message-bus backing factories into
  `LiveNode` is not exposed. The backing implementations remain available to Rust builders.
- SQL cache position and synthetic loads, actor and strategy state persistence, and heartbeat remain
  incomplete. The audited restart workflow uses the Redis backing through Rust builders; Python
  `LiveNode` configuration cannot select that backing yet.
- Serialized order and position snapshot publishing to external message-bus topics remains deferred.
- The v2 `BacktestNode` does not yet wire the v1 `StreamingConfig` and `DataCatalogConfig` iterator
  workflow.
- Instrument-provider filter dictionaries are not a common v2 adapter contract. Hyperliquid v2
  loads its configured instrument universe and does not accept the v1 `instrument_provider` field.
  Check each adapter's Rust/PyO3 config rather than copying v1 provider examples.
- The published quickstart and backtesting tutorials still use v1 imports and configuration. Use the
  generated v2 stubs, the [v2 backtest acceptance tests][python-v2-backtest-tests] for backtesting,
  and [Python v2 examples][python-v2-examples] for live and adapter workflows while those tutorials
  are ported.

The [v2 roadmap][v2-roadmap] tracks the wider post-cutover surface. Release-specific breaking
changes remain in [RELEASES.md][release-notes].

[python-v2-backtest-tests]: python/tests/acceptance/test_backtest.py
[python-v2-examples]: python/examples/
[python-v2-strategy-config]: python/tests/strategies/ema_cross.py
[release-notes]: RELEASES.md
[v2-roadmap]: https://github.com/nautechsystems/nautilus_trader/issues/4042
