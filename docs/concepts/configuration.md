# Configuration

NautilusTrader uses typed configuration objects for data clients, execution clients, engines, and
strategies. Higher-level configs compose these component configs. For example, `LiveNodeConfig`
owns the node's core component settings; register adapter clients through `LiveNode.builder(...)`.
Adapters keep separate data and execution client configs when their capabilities or credentials
differ.

## Design principles

### Concrete fields carry resolved values

Rust config fields normally carry concrete values when the component requires a resolved setting.
For example, adapter timeouts, retry counts, backoff delays, and heartbeat intervals often use
plain types such as `u64` or `u32`. Construction resolves these fields before the component starts,
so downstream code can consume them without repeating defaulting logic.

### Option semantics are field-specific

In a stored Rust config, `Option<T>` contains either `Some(value)` or `None`. The stored value does
not record whether a caller omitted an input. A component may interpret `None` as disabling a
feature, leaving a lookback window unbounded, falling back to the runtime environment, or applying
an internal default. The field documentation defines its meaning.

This distinction makes config semantics visible in the type. A plain `u64` always has a value, while
the code consuming an `Option<u64>` handles the absent case.

### Defaults are type-specific

Rust config types define defaults through `#[builder(default = value)]` annotations, a custom
`Default` implementation, or both. PyO3 constructors generally resolve omitted concrete parameters
from the Rust `Default` implementation instead of maintaining separate Python defaults.

Container-level `#[serde(default)]` on a config struct fills its missing serialized fields from that
config's `Default` implementation. Field-level `#[serde(default)]` instead uses the field type's
default, unless the attribute names another function.

`Type::default()` and `Type::builder().build()` are separate construction paths. A custom `Default`
implementation may delegate part of its construction to the builder, but this is type-specific. Do
not assume that the two paths are interchangeable unless the implementation or documentation
guarantees it.

### Unknown-field handling depends on the construction path

Rust deserialization and Python constructor binding enforce unknown fields independently.
`BybitDataClientConfig`, for example, uses `#[serde(deny_unknown_fields)]` and rejects extra
serialized keys. A Rust type without that attribute may accept them. Fixed Python config
constructors raise `TypeError` for unsupported keywords. `DataActorConfig`, `StrategyConfig`, and
`ExecutionAlgorithmConfig` accept additional keywords for Python subclasses. Do not infer one
construction path's strictness from another.

## Python configs

Import core config types from `nautilus_trader.config`. Import adapter configs from the adapter's
public module, such as `nautilus_trader.adapters.bybit`.

Most runtime config classes are PyO3 wrappers around Rust config structs. In a Python constructor,
omitting a parameter whose signature default is `None` is equivalent to passing `None` explicitly.
The wrapper then either selects the Rust default or preserves an absent optional value, depending on
the field. Check the field documentation rather than inferring its behavior from the Python
annotation.

Properties expose selected config values. Configs that hold secrets can omit their values or expose
only presence checks; consult the config API before displaying or logging a config. Mutability is
type-specific: many configs expose only read-only getters, while extensible component configs and
some adapter configs expose documented setters. `DataActorConfig`, `StrategyConfig`, and
`ExecutionAlgorithmConfig` also accept extra fields for Python subclasses. Python-owned analysis
configs retain their documented dataclass behavior.

```python
from nautilus_trader.adapters.bybit import BybitDataClientConfig

omitted = BybitDataClientConfig()
explicit_none = BybitDataClientConfig(
    http_timeout_secs=None,
    base_url_http=None,
)
assert omitted.http_timeout_secs == explicit_none.http_timeout_secs == 60
assert omitted.base_url_http is explicit_none.base_url_http is None

# Override the timeout
config = BybitDataClientConfig(http_timeout_secs=30)

# Read the resolved value
assert config.http_timeout_secs == 30
```

When a wrapper maps `None` to a non-`None` Rust default, Python cannot use that parameter to store
Rust `None`. For example, passing `instrument_status_poll_secs=None` to `BybitDataClientConfig`
retains its 60-second default. Rust callers can set `instrument_poll_interval_secs` to `None` to
disable periodic instrument and status polling.

## Rust configs

Many Rust config structs derive [`bon::Builder`](https://bon-rs.com), which generates a type-safe
builder with compile-time checks for required fields. A builder can omit fields that declare a
builder default.

Use the construction style documented for the config type. For `DataEngineConfig`, the builder and
struct update forms below both enable delta buffering and retain the declared defaults for other
fields:

```rust
use nautilus_data::engine::config::DataEngineConfig;

let with_builder = DataEngineConfig::builder()
    .buffer_deltas(true)
    .build();

let with_struct_update = DataEngineConfig {
    buffer_deltas: true,
    ..Default::default()
};
```

Use `DataEngineConfig::default()` when no fields need an override.

## Adapter config fields

Names recur across adapter configs, but their types and defaults depend on the adapter and client.
For example, `BybitDataClientConfig` defines the fields below. The `Default` column shows the values
from `BybitDataClientConfig::default()`:

| Rust field                      | Rust type     | Default    | Purpose                        |
| ------------------------------- | ------------- | ---------- | ------------------------------ |
| `http_timeout_secs`             | `u64`         | `60`       | REST request timeout.          |
| `max_retries`                   | `u32`         | `3`        | Maximum retry attempts.        |
| `retry_delay_initial_ms`        | `u64`         | `1_000`    | Initial backoff delay.         |
| `retry_delay_max_ms`            | `u64`         | `10_000`   | Maximum backoff delay.         |
| `heartbeat_interval_secs`       | `u64`         | `20`       | WebSocket keepalive interval.  |
| `recv_window_ms`                | `u64`         | `5_000`    | Signed request expiry window.  |
| `instrument_poll_interval_secs` | `Option<u64>` | `Some(60)` | Instrument and status polling. |

Python exposes `instrument_poll_interval_secs` as `instrument_status_poll_secs`.

`BybitDataClientConfig::builder().build()` instead leaves
`instrument_poll_interval_secs` as `None`, which disables periodic instrument and status polling.
This is one case where the type's default and builder paths differ.

Adapter-specific fields such as rate limits, polling intervals, and margin modes are documented in
the [integration guides](../integrations/index.md).

## Engine configs

Engine configs use the same typed-field approach. In `LiveExecEngineConfig`, fields such as
`reconciliation`, `inflight_check_interval_ms`, and `open_check_threshold_ms` have concrete defaults:

| Field                        | Default | Purpose                                                |
| ---------------------------- | ------- | ------------------------------------------------------ |
| `reconciliation`             | `True`  | Run reconciliation during startup.                     |
| `inflight_check_interval_ms` | `2_000` | Check whether in-flight orders exceed their threshold. |
| `open_check_threshold_ms`    | `5_000` | Wait before acting on an open-order discrepancy.       |

Optional fields such as `open_check_interval_secs` and `position_check_interval_secs` enable or
disable their periodic checks:

```python
from nautilus_trader.config import LiveExecEngineConfig

config = LiveExecEngineConfig(
    open_check_interval_secs=30.0,  # Enable open order polling
    open_check_lookback_mins=60,  # Look back 60 minutes
)

assert config.open_check_interval_secs == 30.0
assert config.open_check_lookback_mins == 60
assert config.position_check_interval_secs is None  # Disabled by default
```

After the live node completes startup, an available execution client lets this config schedule
open-order report requests every 30 seconds and limit each request to the previous 60 minutes. It
does not schedule periodic position report requests. Supplied periodic intervals must be positive,
finite values of at least one nanosecond.

The separate `reconciliation` field enables startup reconciliation and defaults to `True`; the
interval fields control periodic checks independently. When startup reconciliation is enabled,
`reconciliation_startup_delay_secs` also delays the first periodic check after startup.

For the full set of live engine options, see
[ExecutionEngine configuration](../how_to/configure_live_trading.md#executionengine-configuration).
