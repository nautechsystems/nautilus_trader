# Python Adapter Interface

Use this guide to implement a custom live adapter in a separate Python package, optionally with
its own Rust/PyO3 extension. It covers the client interface, registration, and runtime contracts.
For the platform overview, see [Adapters](../concepts/adapters.md). For in-tree Rust adapters, see
[the Rust adapter guide](adapters.md).

Custom clients run on the node's Python event loop and exchange typed objects with the Rust engines
through PyO3. Subclass a client from `nautilus_trader.live.clients`:

| Base class         | Responsibility                                                       |
| ------------------ | -------------------------------------------------------------------- |
| `DataClient`       | Data subscriptions and historical requests.                          |
| `MarketDataClient` | Naming specialization of `DataClient`; identical hooks and behavior. |
| `ExecutionClient`  | Order commands, account updates, and reconciliation reports.         |

Start with the [deterministic adapter template](../../examples/live/_template/README.md).
The [independent Rust/PyO3 package section](#independent-rustpyo3-packages) shows how an extension
uses the same interface.

## Registration and configuration

### Define a factory

Subclass `DataClientFactory` or `ExecutionClientFactory` from `nautilus_trader.live.clients`.
Register either the subclass with a static `create` method or an instance of it.

Data factories implement `create(*, name, config, cache, clock)`. Execution factories also receive
`trader_id`. The factory returns the appropriate client subclass **synchronously**. The client retains
the supplied config and cache objects. Before accepting it, the node validates its registration name
and execution identity.

Run this example from the repository root to register the template's data client:

```python
from nautilus_trader.config import DataClientConfig
from nautilus_trader.live import LiveNodeBuilder
from nautilus_trader.live.clients import DataClientFactory
from examples.live._template.constants import VENUE
from examples.live._template.data import TemplateDataClient
from examples.live._template.providers import TemplateInstrumentProvider


class ExampleFactory(DataClientFactory):
    @staticmethod
    def create(*, name, config, cache, clock):
        return TemplateDataClient(
            name=name,
            config=config,
            cache=cache,
            clock=clock,
            venue=VENUE,
            instrument_provider=TemplateInstrumentProvider(config.instrument_provider),
        )


builder = LiveNodeBuilder.from_config("example")
builder = builder.add_data_client("TEMPLATE", ExampleFactory, DataClientConfig())
```

The template also supplies an execution client and its factory.
Keep node configuration and strategies outside the adapter package.

### Register clients with the node

For **direct registration**, use `LiveNodeBuilder.add_data_client(name, factory, config)` or
`add_exec_client(...)`. An explicit fourth `routing` argument overrides `config.routing`.

For **configuration-based registration**:

1. Put name-to-config mappings in `LiveNodeConfig(data_clients=..., exec_clients=...)`.
1. Supply `data_factories` and `exec_factories` to `LiveNode.build(name, config, ...)` or
   `LiveNodeBuilder.from_config(name, config, ...)`.

Factory lookup checks the complete client name, then its prefix before the first hyphen. A supplied
factory takes precedence over an importable config's factory descriptor. Venue and default routing
use each config's `RoutingConfig` through the same builder path as direct registration.

Both registration paths preserve these rules:

- **Native and custom clients can share a node.** Registration distinguishes native factories from
  Python factories; custom configs do not pass through a native config downcast.
- **Duplicate names fail.** Each registered client needs a distinct name.
- **Failed builds preserve Python client registrations.** The node disposes clients from the failed
  attempt, then a retry creates fresh client instances.

### Define adapter configuration

Subclass `DataClientConfig` or `ExecutionClientConfig` for adapter fields. The inherited native
constructor handles common fields. Declare adapter fields as keyword arguments in a Python
`__init__` and accept `**kwargs` for common fields. The factory receives the original subclass instance.

Import `ImportableConfig` and `ImportableFactoryConfig` from `nautilus_trader.config` when configs
or factories need to be loaded by import path.

| Operation                                | Result                                                  |
| ---------------------------------------- | ------------------------------------------------------- |
| `config.dict()`                          | Config fields with their Python values retained.        |
| `config.json()`                          | JSON bytes.                                             |
| `config.to_importable(factory)`          | `ImportableConfig` with a `module:qualified_name` path. |
| `ImportableFactoryConfig(path).create()` | Factory imported and constructed without arguments.     |

`ImportableConfig.json()`, `parse()`, and `create()` provide an importable round trip.
Classes defined inside functions are not importable.

### Serialize configuration

Keep runtime resources out of config objects. Instance fields, including underscore-prefixed fields,
participate in serialization; `ClassVar` metadata does not.

JSON serialization supports:

- Native JSON values.
- Common routing and instrument-provider configs.
- `Decimal` values encoded as strings.
- Domain values with `from_str`.

Annotate adapter fields to restore domain values and decimals, including values inside lists,
dictionaries, and optional types. `Any` retains the decoded JSON value. Unsupported values raise
`TypeError`.

:::warning[Serialized credentials]
Serializing a config also serializes any credentials it contains. Protect the serialized result.
:::

## Startup, scheduling, and shutdown

### Bind resources at startup

Constructors and factories **must not start tasks, capture an event loop, or open network resources**.
`client.loop` is `None` during construction. A v1-style factory that declares a `loop` parameter
receives `None`.

Startup binds every custom client to the actual running loop before `_connect`. Create loop-dependent
sessions, locks, and network clients inside that async hook.

### Choose a launch mode

| Launch mode                      | Event loop                                        | Signal ownership                           |
| -------------------------------- | ------------------------------------------------- | ------------------------------------------ |
| `node.run()` with custom clients | Creates and drives asyncio on the calling thread. | Restores signal handlers after completion. |
| `await node.run_async()`         | Uses the host's running loop.                     | Host application owns its signals.         |

Both modes preserve the Rust node's startup, reconciliation, maintenance, and shutdown path.
Native-only `run()` retains its native execution path.

Calling blocking `run()` from an active event loop raises before consuming the node. Use
`run_async()` inside an existing loop.

:::warning[Database cache backing]
Redis/PostgreSQL cache backing is unsupported with custom Python clients in either launch mode.
Both modes drive asyncio, where the backing's blocking worker calls would stall the event loop.
Native-only nodes can use database backing with `run()`.
:::

### Schedule background work

Use `self.create_task(coroutine, name)` for adapter background work. The runtime:

- Retains the client and every task until terminal result retrieval.
- Defers the first coroutine poll, including with an eager task factory.
- Logs unawaited operation failures with the client and operation name.

Tasks created directly through asyncio or an external library remain that code's responsibility.
Join them during `_disconnect`.

### Process commands in order

Commands and subscription changes share a **FIFO queue** with capacity for **1,024 waiting operations
per client**. A full queue rejects further admission. An operation failure is logged before the next
queued operation runs.

Historical requests and awaited reconciliation operations run separately, so network waits do not
block command admission. Keep each command hook bounded: it delays later commands on that client.

### Disconnect and drain tasks

The node's post-stop grace period precedes disconnection, allowing strategy shutdown cancellations
to enter the normal command path. Disconnection then:

1. Closes admission and discards queued commands with diagnostics.
1. Cancels the active command.
1. Calls `_disconnect`, where the adapter closes network resources and joins adapter-owned work.

The node bounds disconnection by its configured timeout. The runtime requests cancellation
**at most once per task** and shields tasks while draining them, so repeated supervisor cancellation
does not interrupt asynchronous cleanup. A deadline reports incomplete cleanup rather than issuing
another cancellation. Independently cancelled commands allow the next queued operation to run while
admission remains open.

:::warning[Cancellation is not completion]
Resistant tasks remain supervised, and incomplete cleanup is reported, including when a host closes
the loop too early. Finish awaiting the node before closing a host loop.
:::

Disposal invalidates output and cache views, so late work cannot emit into a subsequent node.
A client instance and its bound provider belong to **one node run**.

## Read-only cache and typed output

### Inspect core state

Factories receive `ClientCache`, a **read-only view** of the owning core cache. Instruments, orders,
accounts, positions, books, and query collections are owned snapshots. Mutating a returned object
does not change core state. No mutable cache, engine, or raw dispatch handle is passed to an adapter.

Cache access raises a Python exception when it occurs:

- On another thread.
- After disposal.
- During an incompatible core borrow.

The read methods include:

| State                  | Methods                                                               |
| ---------------------- | --------------------------------------------------------------------- |
| Instruments            | `instrument`, `instruments`, `instrument_ids`                         |
| Quotes and books       | `quote`, `order_book`                                                 |
| Accounts and positions | `account`, `position`, `positions_open`                               |
| Orders                 | `order`, `order_list`, `orders`, `orders_open`, `orders_inflight`     |
| Order queries          | `orders_open_count`, `client_order_ids_open`, order identity mappings |
| Stored bytes           | `get`                                                                 |

Use `quote(instrument_id, index=0)` for quote history. `get` returns a list of byte values, consistent
with the v2 cache binding.

### Send typed output

Client output methods **enqueue typed values for core processing**. The core remains the sole writer
of trading state.

| Output                 | Client methods                                                |
| ---------------------- | ------------------------------------------------------------- |
| Instrument definitions | `_handle_instrument`                                          |
| Streaming data         | `_handle_data`                                                |
| Historical responses   | `_handle_response`                                            |
| Execution events       | `generate_account_state`, `generate_order_*`, `_handle_event` |
| Reconciliation reports | `_handle_report`                                              |

`_handle_data` accepts these types from `nautilus_trader.model`:

- **Quotes, trades, and bars**: `QuoteTick`, `TradeTick`, `Bar`.
- **Order books**: `OrderBookDelta`, `OrderBookDeltas`, `OrderBookDepth10`.
- **Prices and funding**: `MarkPriceUpdate`, `IndexPriceUpdate`, `FundingRateUpdate`.
- **Options and instrument events**: `OptionGreeks`, `InstrumentStatus`, `InstrumentClose`.
- **Custom data**: `CustomData`.

`_handle_report` accepts `OrderStatusReport`, `FillReport`, `PositionStatusReport`, or
`ExecutionMassStatus` from `nautilus_trader.model`. Its optional `fills` list accompanies an
`OrderStatusReport` only. Report account identity, mass-report client/venue identity, and nested
report ownership must match the registered client.

## Data client hooks

### Subscriptions and requests

Each hook is async and receives an owned, frozen command or request from `nautilus_trader.live`.
Nested params are copied. Override the hooks for the venue's supported capabilities; unimplemented
hooks raise `NotImplementedError`. Track venue subscription acknowledgments and reconnect replay
in adapter-local state.

| Family            | Subscribe/unsubscribe suffix            | Historical request suffix                    |
| ----------------- | --------------------------------------- | -------------------------------------------- |
| Custom data       | No suffix                               | `data`                                       |
| Instruments       | `instruments`, `instrument`             | `instruments`, `instrument`                  |
| Order books       | `book_deltas`, `book_depth10`           | `book_snapshot`, `book_deltas`, `book_depth` |
| Quotes and trades | `quotes`, `trades`                      | `quotes`, `trades`                           |
| Reference prices  | `mark_prices`, `index_prices`           | *Not supported*                              |
| Funding           | `funding_rates`                         | `funding_rates`                              |
| Bars              | `bars`                                  | `bars`                                       |
| Instrument events | `instrument_status`, `instrument_close` | *Not supported*                              |
| Options           | `option_greeks`                         | `option_chain_reference_price`               |

For example, quote subscriptions call `_subscribe_quotes(command)` and
`_unsubscribe_quotes(command)`; quote history calls `_request_quotes(request)`.

### Historical responses

Preserve request IDs, params, time bounds, and payload identity when constructing the matching typed
response. Response classes in `nautilus_trader.live` are:

- **Custom data and instruments**: `CustomDataResponse`, `InstrumentResponse`, `InstrumentsResponse`.
- **Order books**: `BookResponse`, `BookDeltasResponse`, `BookDepthResponse`.
- **Quotes and trades**: `QuotesResponse`, `TradesResponse`.
- **Funding and bars**: `FundingRatesResponse`, `BarsResponse`.
- **Options**: `OptionChainReferencePriceResponse`.

An **empty response still completes the adapter's response path**. Windowed requests expose UTC
`start`/`end` datetimes and exact `start_ns`/`end_ns`; response bounds use integer nanoseconds.
`RequestBookSnapshot` and `RequestOptionChainReferencePrice` have no time-window fields.

### Instrument providers

`InstrumentProvider` stores adapter-local instruments and currencies. Override `load_all_async`,
`load_ids_async`, or `load_async`. `initialize(reload=False)` uses `InstrumentProviderConfig` and
retries after a failed load.

The synchronous loading methods depend on the provider's binding:

- **Bound to a client**: schedule supervised work.
- **Unbound, outside an active event loop**: run the load synchronously.
- **Unbound, inside an active event loop**: await the async method instead.

Send loaded instruments through the client output to populate the core cache.

## Execution client hooks

### Construct the client

Pass the factory's `name`, `config`, `cache`, `clock`, and `trader_id` to `ExecutionClient`.
Execution clients also require these non-optional values from `nautilus_trader.model`:

| Argument       | Type          |
| -------------- | ------------- |
| `venue`        | `Venue`       |
| `account_id`   | `AccountId`   |
| `account_type` | `AccountType` |
| `oms_type`     | `OmsType`     |

`base_currency` and `instrument_provider` are optional. Data clients can use `venue=None`.

### Generate reconciliation reports

Implement the reconciliation hooks:

- `_generate_order_status_report`
- `_generate_order_status_reports`
- `_generate_fill_reports`
- `_generate_position_status_reports`

Return typed reports, lists, or the optional result prescribed by the method.
`_generate_mass_status(lookback_mins)` defaults to native composition of the bulk methods using the
owning node's clock; an override returns an `ExecutionMassStatus`.
**Propagated report failures fail startup reconciliation.**

### Handle order commands

Override the hooks supported by the venue:

- **Submit**: `_submit_order`, `_submit_order_list`.
- **Modify**: `_modify_order`.
- **Cancel**: `_cancel_order`, `_cancel_all_orders`.
- **Query**: `_query_account`, `_query_order`.

`_batch_modify_orders` and `_batch_cancel_orders` default to ordered calls of the corresponding
single-order hook. Override them for a venue batch endpoint.

Commands preserve native identifiers and params, correlation/causation IDs, timestamps, and nested
orders or batch members.

### Make synchronous decisions and receive notifications

`_handles_order_venue`, `_provides_bulk_position_coverage`, and `_calculate_commission` are synchronous
decision hooks. They **must return promptly without I/O**. Commission returns `Money` or `None` to use
the core fallback. `position_reconciliation_tolerance` accepts a nonnegative `Decimal` at construction.

`_on_instrument` and `_register_external_order` are async notifications admitted to the command queue.
Registration timestamps come from reconciliation metadata, not necessarily the order's initialization event.

## Migration from v1

The interface restores live adapter capabilities using v2 names and ownership rules. It does not
make an unchanged Cython adapter source-compatible.

| V1 surface                          | V2 replacement                      | Migration detail                            |
| ----------------------------------- | ----------------------------------- | ------------------------------------------- |
| `LiveDataClient`                    | `DataClient`                        | Import from `nautilus_trader.live.clients`. |
| `LiveMarketDataClient`              | `MarketDataClient`                  | Same module.                                |
| `LiveExecutionClient`               | `ExecutionClient`                   | Same module.                                |
| `quote_ticks` / `trade_ticks` hooks | `quotes` / `trades` hooks           | Typed v2 commands.                          |
| `order_book_*` hooks                | `book_*` hooks                      | Depth subscription uses `book_depth10`.     |
| `_request`                          | `_request_data`                     | Custom-data request.                        |
| `_handle_*` history methods         | `_handle_response(typed_response)`  | Preserve correlation and bounds.            |
| `_send_*` execution methods         | `_handle_event` / `_handle_report`  | Validates owner identity.                   |
| Cache writes                        | Queued instruments and events       | Core applies changes.                       |
| Subscription tracking methods       | Adapter-local subscription state    | Update from venue acknowledgments.          |
| Constructor event loop              | `client.loop` after startup binding | Create resources in `_connect`.             |
| `cancel_pending_tasks`              | Supervised node disconnection       | Cancellation is not terminal completion.    |
| `run_after_delay`                   | Coroutine with `asyncio.sleep`      | Schedule through `create_task`.             |

### Remaining differences and limitations

- **Forward prices**: use `RequestOptionChainReferencePrice` and its matching response for a v2 option series.
- **DeFi**: native block/pool subscriptions and pool snapshots are outside this Python interface.
- **Historical requests**: the [known migration limitations](../../MIGRATION_V2.md#known-limitations)
  for request joining/completion still apply.
- **Custom publication and subscription**: the same migration limitations apply. Clients emit
  `CustomData` through typed data output; they do not expose component `publish_message` or topic
  subscription methods.
- **Database cache backing**: unsupported with custom Python clients in either launch mode. See
  [startup](#startup-scheduling-and-shutdown) and [hosted event loops](../concepts/live.md#hosted-event-loops).
- **Revised bars**: config retains `handle_revised_bars`, but the v2 core lacks the v1 bar revision
  marker and revision overwrite behavior.
- **Networking**: this interface does not restore removed HTTP/WebSocket bindings.

## Independent Rust/PyO3 packages

An adapter can ship its venue implementation in a separate Rust/PyO3 extension. Its Python facade
subclasses the same client bases and registers through the same factories as a pure Python adapter.
Compile the extension against PyO3 and import NautilusTrader model classes from the installed wheel.

:::warning[Use the installed wheel's model classes]
All exchanged domain values must be instances of those classes. Do not link a second set of Nautilus
model pyclasses or exchange native Rust trait objects across extension modules. The integration
boundary is the Python protocol, not a shared Rust ABI.
:::

### Package layout and dependencies

Keep the adapter in its own project, with this layout:

```text
external-adapter/
  Cargo.toml
  pyproject.toml
  src/lib.rs
  external_adapter/__init__.py
```

Configure the library as a Python extension. For example, `Cargo.toml` can contain:

```toml
[package]
name = "external-adapter"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
name = "_backend"
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "=0.29.2", features = ["extension-module"] }

[workspace]
```

The empty workspace keeps Cargo resolution independent if the project sits beneath another Cargo
workspace. Keep the adapter's dependency lockfile and audit policy in its own project.

Configure the Python package in `pyproject.toml`:

```toml
[build-system]
requires = ["maturin==1.15.0"]
build-backend = "maturin"

[project]
name = "external-adapter"
version = "0.1.0"
requires-python = ">=3.12,<3.15"
dependencies = ["nautilus-trader"]

[tool.maturin]
module-name = "external_adapter._backend"
python-source = "."
```

These build versions match the demonstrated PyO3 boundary. Set the NautilusTrader dependency range
to the releases covered by your adapter's tests.

### Exchange wheel-owned objects

Use `Bound<'_, PyAny>` for incoming commands and `Py<PyAny>` for objects returned to Python. Import
classes while attached to Python, read typed command attributes, and construct output with those
imported classes. This method illustrates the conversion inside a `#[pymethods]` implementation:

```rust
fn quote(&self, py: Python<'_>, command: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let model = py.import("nautilus_trader.model")?;
    let values = PyDict::new(py);
    values.set_item("instrument_id", command.getattr("instrument_id")?)?;
    for (name, value) in [("bid_price", "1.12345"), ("ask_price", "1.12349")] {
        values.set_item(
            name,
            model.getattr("Price")?.call_method1("from_str", (value,))?,
        )?;
    }
    for (name, value) in [("bid_size", "17000"), ("ask_size", "23000")] {
        values.set_item(
            name,
            model.getattr("Quantity")?.call_method1("from_str", (value,))?,
        )?;
    }
    values.set_item("ts_event", 19)?;
    values.set_item("ts_init", 29)?;
    Ok(model.getattr("QuoteTick")?.call((), Some(&values))?.unbind())
}
```

Import `pyo3::{prelude::*, types::PyDict}` and expose the containing class from a `#[pymodule]`
function named `_backend`. The example values are deterministic; a venue implementation supplies
its received prices, sizes, and nanosecond timestamps. Construct prices, quantities, and money
from exact decimal values or strings without a floating-point round trip.

### Delegate from Python client hooks

The Python facade imports the backend and delegates from its client hooks. For a backend class
named `Backend` with the method above, the quote hook is:

```python
from external_adapter._backend import Backend


async def _subscribe_quotes(self, command):
    self._handle_data(Backend().quote(command))
```

Place that hook on a `MarketDataClient` subclass. Publish the instrument definition before its
market data. Implement connection and disconnection hooks, and supply a `DataClientFactory` that
constructs the subclass using the node-provided name, config, cache, and clock. The
[Python template](../../examples/live/_template/README.md) demonstrates those hooks and
factory registration. A backend with persistent venue state should be retained by its client.

Execution adapters follow the same pattern with `ExecutionClient` and `ExecutionClientFactory`.
Read `command.order` and emit submission, acceptance, rejection, and fill events through the
client's `generate_*` methods. Return the documented report types from reconciliation hooks.
Preserve client/account identity, request correlation, exact commission values, and event order;
calling from Rust does not change these contracts.

### Lifecycle and cache ownership

Keep Python calls and cache access on the node's owner thread and event loop. The GIL permits Python
object access; it does not grant another thread permission to use the node's cache or output
capability. The supplied cache is read-only, and returned mutable snapshots do not mutate the core.
Send typed output through the client so the synchronous core applies state changes.

The extension follows the same lifecycle as a Python adapter:

- **Construction**: retain configuration without starting network work.
- **Connection**: create loop-bound resources in `_connect`.
- **Background work**: schedule Python tasks through `self.create_task`.
- **Disconnection**: close resources in `_disconnect`. Stop any Rust workers or network tasks owned
  by the extension and retrieve their terminal results.

Do not retain usable cache/output capabilities beyond node teardown. Propagate failures as Python
exceptions so the client runtime can supervise them.

### Build and verify installed wheels

Install maturin into a compatible CPython build interpreter first. If Cargo would discover a different
interpreter, select the build interpreter with `PYO3_PYTHON`. From the adapter's own project, build a
wheel with bounded Cargo concurrency:

```bash
CARGO_BUILD_JOBS=4 python -m maturin build --out dist
```

Install the adapter wheel and a standard, non-editable NautilusTrader wheel into a fresh environment.
Use absolute wheel paths when running these commands from outside both source trees:

```bash
python -m venv /tmp/adapter-check
/tmp/adapter-check/bin/python -m pip install /absolute/path/nautilus_trader.whl /absolute/path/adapter.whl
/tmp/adapter-check/bin/python -I /absolute/path/verify_adapter.py
```

Replace the wheel placeholders with the complete generated wheel filenames. On Windows, use the
virtual environment's `Scripts/python.exe` path.

Your verifier should check:

- **Import isolation**: `nautilus_trader`, the adapter package, and its backend resolve beneath
  `sys.prefix`. Editable imports can hide packaging or extension-identity defects.
- **Launch modes**: both `node.run()` and hosted `run_async()` work.
- **Data delivery**: a quote reaches the strategy and core cache.
- **Reconciliation**: reconciliation completes.
- **Execution**: a submitted order produces the expected fill quantity, price, commission, and final
  cached order state.
- **Shutdown**: resources close and further adapter output is prevented.

Run this installed-wheel check when changing the adapter protocol or the extension's supported
NautilusTrader versions.
