# Python Adapter Template

Use this package as the starting layout for an out-of-tree adapter. Its data and execution clients
provide deterministic behavior without network access: they load a currency pair, publish a quote,
reconcile an account, and simulate market fills with a fixed commission of `0.03 USD`.

## Module layout

- [constants.py](constants.py): Shared venue identity, instrument ID, and unsupported-operation message.
- [providers.py](providers.py): Instrument loading and storage.
- [data.py](data.py): Market data subscriptions and historical request hooks.
- [execution.py](execution.py): Account state, reconciliation, and order execution hooks.
- [factories.py](factories.py): Client and provider construction from node configuration.

Replace the data and execution hooks with venue operations. Keep constructors free of tasks and
network resources, schedule background work through `self.create_task`, and close resources in
`_disconnect`. Use the supplied read-only cache and emit typed output for the core to process.
The [interface reference](../../../docs/developer_guide/python_adapters.md) describes the full
contract and migration from v1.

## Register the adapter

Keep node configuration and strategies outside the adapter package. From the repository root, the
following registers both factories and enables instrument loading:

```python
from examples.live._template.factories import TemplateDataClientFactory
from examples.live._template.factories import TemplateExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import DataClientConfig
from nautilus_trader.config import ExecutionClientConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveNodeConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.model import TraderId

config = LiveNodeConfig(
    environment=Environment.SANDBOX,
    trader_id=TraderId("TEMPLATE-001"),
    data_clients={
        "TEMPLATE": DataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={"TEMPLATE": ExecutionClientConfig()},
)
node = LiveNode.build(
    "PYTHON-TEMPLATE",
    config,
    data_factories={"TEMPLATE": TemplateDataClientFactory},
    exec_factories={"TEMPLATE": TemplateExecutionClientFactory},
)
```

Add a strategy before running the node. The [sandbox execution example](../sandbox/exec_tester.py)
shows how to configure the built-in `ExecTester`; enable only the operations your adapter supports.
The [template integration tests](../../../python/tests/integration/test_python_adapter_template.py)
exercise factory registration, a market fill, and shutdown on owned and hosted event loops.

## Implement an independent Rust/PyO3 package

The [independent package guide](../../../docs/developer_guide/python_adapters.md#independent-rustpyo3-packages)
describes package layout, Python object exchange, lifecycle ownership, and installed-wheel
validation for adapters developed outside this repository.
