# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Verify Python client base contracts independently of node registration.
"""

from types import SimpleNamespace

import pytest

from nautilus_trader.common import Clock
from nautilus_trader.config import DataClientConfig
from nautilus_trader.config import ExecutionClientConfig
from nautilus_trader.live.clients import DataClient
from nautilus_trader.live.clients import DataClientFactory
from nautilus_trader.live.clients import ExecutionClient
from nautilus_trader.live.clients import ExecutionClientFactory
from nautilus_trader.live.clients import _create_client
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import OmsType
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.testkit.providers import TestInstrumentProvider


@pytest.fixture
def data_client() -> DataClient:
    """
    Construct a base data client without registering it with a node.
    """
    return DataClient(name="SIM", config=DataClientConfig(), cache=None, clock=Clock.new_test())


@pytest.fixture
def execution_client() -> ExecutionClient:
    """
    Construct a base client without registering it with a node.
    """
    return ExecutionClient(
        name="SIM",
        config=ExecutionClientConfig(),
        cache=None,
        clock=Clock.new_test(),
        trader_id=TraderId("TESTER-001"),
        venue=Venue("SIM"),
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        oms_type=OmsType.NETTING,
    )


def test_client_constructors_reject_other_config_family(execution_client) -> None:
    """
    Wrong configuration families fail before client construction.
    """
    with pytest.raises(TypeError, match="Expected DataClientConfig"):
        DataClient(name="SIM", config=ExecutionClientConfig(), cache=None, clock=Clock.new_test())
    with pytest.raises(TypeError, match="Expected ExecutionClientConfig"):
        ExecutionClient(
            name="SIM",
            config=DataClientConfig(),
            cache=None,
            clock=Clock.new_test(),
            trader_id=execution_client.trader_id,
            venue=execution_client.venue,
            account_id=execution_client.account_id,
            account_type=execution_client.account_type,
            oms_type=execution_client.oms_type,
        )


@pytest.mark.asyncio
async def test_default_execution_coverage_and_commission_hooks(execution_client) -> None:
    """
    Default hooks claim their venue and bulk positions without inventing commission.
    """
    instrument = TestInstrumentProvider.audusd_sim()
    assert execution_client._handles_order_venue(Venue("SIM")) is True
    assert execution_client._handles_order_venue(Venue("OTHER")) is False
    assert execution_client._handles_order_venue(None) is False
    assert execution_client._provides_bulk_position_coverage(instrument.id) is True
    assert (
        execution_client._calculate_commission(
            instrument,
            Quantity.from_str("17"),
            Price.from_str("0.71231"),
            LiquiditySide.TAKER,
        )
        is None
    )
    assert await execution_client._generate_mass_status(23) is NotImplemented
    assert await execution_client._on_instrument(instrument) is None
    assert (
        await execution_client._register_external_order(
            ClientOrderId("O-EXTERNAL"),
            None,
            instrument.id,
            StrategyId("S-EXTERNAL"),
            137,
        )
        is None
    )


@pytest.mark.asyncio
@pytest.mark.parametrize("kind", ["modify", "cancel"])
@pytest.mark.parametrize("failure_index", [None, 1])
async def test_batch_fallback_preserves_order_and_stops_at_failure(
    execution_client,
    kind,
    failure_index,
) -> None:
    """
    Batch fallback awaits each command and propagates the first failing operation.
    """
    commands = [object(), object(), object()]
    received = []

    async def execute(command):
        received.append(command)
        if failure_index is not None and command is commands[failure_index]:
            raise RuntimeError("Venue batch operation failed")

    setattr(execution_client, f"_{kind}_order", execute)
    command = SimpleNamespace(**{"modifies" if kind == "modify" else "cancels": commands})
    if failure_index is None:
        await getattr(execution_client, f"_batch_{kind}_orders")(command)
    else:
        with pytest.raises(RuntimeError, match="Venue batch operation failed"):
            await getattr(execution_client, f"_batch_{kind}_orders")(command)

    assert received == (commands if failure_index is None else commands[:2])


def test_factory_without_inspectable_signature_receives_original_arguments() -> None:
    """
    Opaque native callables receive the same keyword contract as Python factories.
    """

    class Factory:
        create = staticmethod(dict)

    config = DataClientConfig()
    kwargs = {"name": "SIM", "config": config, "cache": object(), "clock": Clock.new_test()}
    result = _create_client(Factory, kwargs.copy())

    assert result == kwargs
    assert result["config"] is config


@pytest.mark.parametrize("callable_instance", [False, True])
def test_factory_rejects_and_closes_coroutine_from_sync_callable(callable_instance) -> None:
    """
    A synchronous-looking factory cannot leak an unawaited coroutine.
    """
    import asyncio
    import inspect

    coroutine = asyncio.sleep(0)

    class Callable:
        def __call__(self, **_kwargs: object) -> object:
            return coroutine

    class Factory:
        create = Callable() if callable_instance else staticmethod(lambda **kwargs: coroutine)

    try:
        with pytest.raises(TypeError, match="Factory create must be synchronous"):
            _create_client(Factory, {})
        assert inspect.getcoroutinestate(coroutine) == inspect.CORO_CLOSED
    finally:
        coroutine.close()


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "method",
    [
        "_connect",
        "_disconnect",
    ],
)
async def test_client_unimplemented_hooks_fail_explicitly(data_client, method) -> None:
    """
    Unsupported adapter operations cannot silently report success.
    """
    with pytest.raises(NotImplementedError):
        await getattr(DataClient, method)(data_client)


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "method",
    [
        "_subscribe",
        "_subscribe_instruments",
        "_subscribe_instrument",
        "_subscribe_book_deltas",
        "_subscribe_book_depth",
        "_subscribe_quotes",
        "_subscribe_trades",
        "_subscribe_mark_prices",
        "_subscribe_index_prices",
        "_subscribe_funding_rates",
        "_subscribe_bars",
        "_subscribe_instrument_status",
        "_subscribe_instrument_close",
        "_subscribe_option_greeks",
        "_unsubscribe",
        "_unsubscribe_instruments",
        "_unsubscribe_instrument",
        "_unsubscribe_book_deltas",
        "_unsubscribe_book_depth",
        "_unsubscribe_quotes",
        "_unsubscribe_trades",
        "_unsubscribe_mark_prices",
        "_unsubscribe_index_prices",
        "_unsubscribe_funding_rates",
        "_unsubscribe_bars",
        "_unsubscribe_instrument_status",
        "_unsubscribe_instrument_close",
        "_unsubscribe_option_greeks",
        "_request_data",
        "_request_instruments",
        "_request_instrument",
        "_request_book_snapshot",
        "_request_quotes",
        "_request_trades",
        "_request_funding_rates",
        "_request_option_chain_reference_price",
        "_request_bars",
        "_request_book_depth",
        "_request_book_deltas",
    ],
)
async def test_dataclient_unimplemented_hooks_fail_explicitly(data_client, method) -> None:
    """
    Unsupported adapter operations cannot silently report success.
    """
    with pytest.raises(NotImplementedError):
        await getattr(DataClient, method)(data_client, object())


@pytest.mark.asyncio
@pytest.mark.parametrize(
    "method",
    [
        "_submit_order",
        "_submit_order_list",
        "_modify_order",
        "_cancel_order",
        "_cancel_all_orders",
        "_query_account",
        "_query_order",
        "_generate_order_status_report",
        "_generate_order_status_reports",
        "_generate_fill_reports",
        "_generate_position_status_reports",
    ],
)
async def test_executionclient_unimplemented_hooks_fail_explicitly(
    execution_client,
    method,
) -> None:
    """
    Unsupported adapter operations cannot silently report success.
    """
    with pytest.raises(NotImplementedError):
        await getattr(ExecutionClient, method)(execution_client, object())


@pytest.mark.parametrize("factory", [DataClientFactory, ExecutionClientFactory])
def test_base_factory_requires_an_implementation(factory, execution_client, data_client) -> None:
    """
    An unimplemented factory cannot produce a registered client.
    """
    client = execution_client if factory is ExecutionClientFactory else data_client
    kwargs = {
        "name": "SIM",
        "config": client.config,
        "cache": None,
        "clock": execution_client.clock,
    }

    if factory is ExecutionClientFactory:
        kwargs["trader_id"] = execution_client.trader_id
    with pytest.raises(NotImplementedError):
        factory.create(**kwargs)
