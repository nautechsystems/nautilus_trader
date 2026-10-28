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
Test Python margin model configuration and runtime account wiring.
"""

import asyncio
import inspect
from collections.abc import Callable
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.adapters.sandbox import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox import SandboxExecutionClientFactory
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.common import Environment
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.execution import FeeModel
from nautilus_trader.execution import FillModel
from nautilus_trader.execution import LatencyModel
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import LeveragedMarginModel
from nautilus_trader.model import MarginAccount
from nautilus_trader.model import MarginModel
from nautilus_trader.model import Money
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StandardMarginModel
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import Strategy
from tests.providers import TestInstrumentProvider


class RecordingMargin:
    """
    Record exact callback inputs and return distinct margin amounts.
    """

    def __init__(self) -> None:
        """
        Set distinct margin results and empty callback state.
        """
        self.calls = []
        self.initial = Money.from_str("123.45 USD")
        self.maintenance = Money.from_str("67.89 USD")

    def calculate_initial_margin(
        self,
        instrument: CurrencyPair,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool | None,
    ) -> Money:
        """
        Record initial margin inputs and return the configured result.
        """
        self.calls.append(
            ("initial", instrument.to_dict(), quantity, price, leverage, use_quote_for_inverse),
        )

        if isinstance(self.initial, Exception):
            raise self.initial
        return self.initial

    def calculate_maintenance_margin(
        self,
        instrument: CurrencyPair,
        quantity: Quantity,
        price: Price,
        leverage: Decimal,
        use_quote_for_inverse: bool | None,
    ) -> Money:
        """
        Record maintenance margin inputs and return the configured result.
        """
        self.calls.append(
            ("maintenance", instrument.to_dict(), quantity, price, leverage, use_quote_for_inverse),
        )

        if isinstance(self.maintenance, Exception):
            raise self.maintenance
        return self.maintenance


class CustomMargin(RecordingMargin, MarginModel):
    """
    Implement the margin protocol through the Python base class.
    """


@pytest.fixture(params=["backtest", "sandbox"])
def margin_config(request: pytest.FixtureRequest) -> Callable:
    """
    Construct either configuration with the supplied margin model.
    """
    if request.param == "backtest":
        return lambda model: BacktestVenueConfig(
            name="SIM",
            oms_type="NETTING",
            account_type="MARGIN",
            starting_balances=["100000 USD"],
            margin_model=model,
        )
    return lambda model: SandboxExecutionClientConfig(
        venue=Venue("SIM"),
        starting_balances=[Money.from_str("100000 USD")],
        margin_model=model,
    )


@pytest.mark.parametrize("model_type", [RecordingMargin, CustomMargin])
def test_margin_config_preserves_instance(margin_config: Callable, model_type: type) -> None:
    """
    Keep Python identity through configuration getters and clones.
    """
    model = model_type()
    config = margin_config(model)
    if isinstance(config, BacktestVenueConfig):
        run = BacktestRunConfig(venues=[config], data=[])
        assert run.venues[0].margin_model is model
    assert config.margin_model is model
    assert config.margin_model is config.margin_model


@pytest.mark.parametrize(
    "model",
    [
        object(),
        123,
        type("MissingMaintenance", (), {"calculate_initial_margin": lambda _: None})(),
        type(
            "NotCallable",
            (),
            {"calculate_initial_margin": 1, "calculate_maintenance_margin": 2},
        )(),
    ],
)
def test_margin_config_rejects_invalid_models(margin_config: Callable, model: object) -> None:
    """
    Reject models without both callable calculation methods.
    """
    with pytest.raises(TypeError, match=r"Cannot convert .* to MarginModel"):
        margin_config(model)


@pytest.fixture(params=["engine", "node", "sandbox"])
def margin_account_factory(request: pytest.FixtureRequest, tmp_path: Path) -> Callable:
    """
    Install a model through each public entry point and return its runtime account.
    """

    def create(model: object, account_type: AccountType = AccountType.MARGIN) -> object:
        instrument = TestInstrumentProvider.audusd_sim()
        fee = MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0))
        quote = QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
            bid_size=Quantity.from_str("100000"),
            ask_size=Quantity.from_str("100000"),
            ts_event=1000,
            ts_init=1000,
        )

        if request.param == "engine":
            engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
            request.addfinalizer(engine.dispose)
            engine.add_venue(
                venue=Venue("SIM"),
                oms_type=OmsType.NETTING,
                account_type=account_type,
                starting_balances=[Money.from_str("100000 USD")],
                default_leverage=Decimal("3.25"),
                fee_model=fee,
                margin_model=model,
            )
            engine.add_instrument(instrument)
            engine.add_data([quote])
            engine.run()
            return engine.cache.account_for_venue(Venue("SIM"))
        if request.param == "node":
            catalog = ParquetDataCatalog(str(tmp_path))
            catalog.write_instruments([instrument])
            catalog.write_quote_ticks([quote])
            config = BacktestRunConfig(
                venues=[
                    BacktestVenueConfig(
                        name="SIM",
                        oms_type="NETTING",
                        account_type=str(account_type),
                        starting_balances=["100000 USD"],
                        default_leverage=Decimal("3.25"),
                        fee_model=fee,
                        margin_model=model,
                    ),
                ],
                data=[
                    BacktestDataConfig(
                        catalog=DataCatalogConfig(path=str(tmp_path)),
                        data_type=NautilusDataType.QuoteTick,
                        instrument_id=instrument.id,
                    ),
                ],
                engine=BacktestEngineConfig(bypass_logging=True, run_analysis=False),
                dispose_on_completion=False,
            )
            node = BacktestNode([config])
            request.addfinalizer(node.dispose)
            node.run()
            if isinstance(model, RecordingMargin):
                assert node.configs[0].venues[0].margin_model is model
            return node.get_engine_cache(config.id).account_for_venue(Venue("SIM"))
        node = (
            LiveNode.builder("PY-MARGIN", TraderId("TESTER-001"), Environment.SANDBOX)
            .with_reconciliation(reconciliation=False)
            .with_timeout_connection(5)
            .with_timeout_reconciliation(0)
            .with_timeout_portfolio(0)
            .with_timeout_disconnection_secs(0)
            .with_delay_post_stop_secs(0)
            .with_delay_shutdown_secs(0)
            .add_simulated_exec_client(
                None,
                SandboxExecutionClientFactory(),
                SandboxExecutionClientConfig(
                    venue=Venue("SIM"),
                    starting_balances=[Money.from_str("100000 USD")],
                    account_type=account_type,
                    default_leverage=Decimal("3.25"),
                    fee_model=fee,
                    margin_model=model,
                ),
            )
            .build()
        )
        request.addfinalizer(node.dispose)
        cache = node.cache
        handle = node.handle()

        async def connect() -> object:
            task = asyncio.create_task(node.run_async())
            try:
                async with asyncio.timeout(10):
                    while (account := cache.account_for_venue(Venue("SIM"))) is None:
                        if task.done():
                            await task
                            raise AssertionError("Sandbox stopped before creating the account")
                        await asyncio.sleep(0.001)
                return account
            finally:
                handle.stop()
                async with asyncio.timeout(10):
                    await task

        return asyncio.run(connect())

    return create


@pytest.mark.parametrize("model_type", [RecordingMargin, CustomMargin])
@pytest.mark.parametrize("use_quote", [None, False, True])
def test_runtime_margin_callbacks(
    margin_account_factory: Callable,
    model_type: type,
    use_quote: bool | None,
) -> None:
    """
    Calculate both margins on the configured runtime account with exact inputs.
    """
    model = model_type()
    account = margin_account_factory(model)
    instrument = TestInstrumentProvider.audusd_sim()
    quantity = Quantity.from_str("12345")
    price = Price.from_str("0.81234")
    initial = account.calculate_initial_margin(instrument, quantity, price, use_quote)
    maintenance = account.calculate_maintenance_margin(instrument, quantity, price, use_quote)

    assert isinstance(account, MarginAccount)
    assert initial == Money.from_str("123.45 USD")
    assert maintenance == Money.from_str("67.89 USD")
    assert model.calls == [
        ("initial", instrument.to_dict(), quantity, price, Decimal("3.25"), use_quote),
        ("maintenance", instrument.to_dict(), quantity, price, Decimal("3.25"), use_quote),
    ]


@pytest.mark.parametrize("method", ["initial", "maintenance"])
@pytest.mark.parametrize("result", [None, 1, Decimal("1.23"), ValueError("margin callback failed")])
def test_runtime_margin_callback_errors(
    margin_account_factory: Callable,
    method: str,
    result: object,
) -> None:
    """
    Return invalid results and callback exceptions through the account error boundary.
    """
    model = CustomMargin()
    setattr(model, method, result)
    account = margin_account_factory(model)
    instrument = TestInstrumentProvider.audusd_sim()
    calculate = getattr(account, f"calculate_{method}_margin")
    detail = "margin callback failed" if isinstance(result, Exception) else "Money"
    with pytest.raises(
        ValueError,
        match=rf"Python MarginModel.calculate_{method}_margin failed:.*{detail}",
    ):
        calculate(instrument, Quantity.from_str("12345"), Price.from_str("0.81234"))
    assert len(model.calls) == 1


@pytest.mark.parametrize("model", [None, StandardMarginModel(), LeveragedMarginModel()])
def test_runtime_builtin_margin_defaults(margin_account_factory: Callable, model: object) -> None:
    """
    Preserve built-in calculations with a non-default account leverage.
    """
    account = margin_account_factory(model)
    instrument = TestInstrumentProvider.audusd_sim()
    initial = account.calculate_initial_margin(
        instrument,
        Quantity.from_str("10000"),
        Price.from_str("0.81250"),
    )
    maintenance = account.calculate_maintenance_margin(
        instrument,
        Quantity.from_str("10000"),
        Price.from_str("0.81250"),
    )
    assert initial == Money.from_str(
        "75 USD" if not isinstance(model, StandardMarginModel) else "243.75 USD",
    )
    assert maintenance == Money.from_str(
        "75 USD" if not isinstance(model, StandardMarginModel) else "243.75 USD",
    )


def test_runtime_cash_account_ignores_margin_model(margin_account_factory: Callable) -> None:
    """
    Keep cash accounts independent of the configured margin model.
    """
    model = CustomMargin()
    account = margin_account_factory(model, AccountType.CASH)
    assert account.account_type == AccountType.CASH
    assert model.calls == []


class MarginProbe(Strategy):
    """
    Reserve order margin and open a position through normal execution.
    """

    def on_start(self) -> None:
        """
        Subscribe to the instrument that triggers both orders.
        """
        self.subscribe_quotes(TestInstrumentProvider.audusd_sim().id)

    def on_quote(self, quote: QuoteTick) -> None:
        """
        Submit one resting limit order and one market order.
        """
        if quote.ts_event != 1000:
            return
        self.submit_order(
            self.order_factory.limit(
                instrument_id=quote.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_str("3000"),
                price=Price.from_str("0.70000"),
                client_order_id=ClientOrderId("MARGIN-LIMIT"),
            ),
        )
        self.submit_order(
            self.order_factory.market(
                instrument_id=quote.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_str("2000"),
                client_order_id=ClientOrderId("MARGIN-MARKET"),
            ),
        )


def test_backtest_portfolio_uses_python_margin_results() -> None:
    """
    Reserve the custom initial and maintenance amounts during normal execution.
    """
    model = CustomMargin()
    instrument = TestInstrumentProvider.audusd_sim()
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    try:
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            starting_balances=[Money.from_str("100000 USD")],
            default_leverage=Decimal("3.25"),
            fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
            margin_model=model,
        )
        engine.add_instrument(instrument)
        engine.add_strategy(MarginProbe())
        engine.add_data(
            [
                QuoteTick(
                    instrument_id=instrument.id,
                    bid_price=Price.from_str("0.80000"),
                    ask_price=Price.from_str("0.80010"),
                    bid_size=Quantity.from_str("100000"),
                    ask_size=Quantity.from_str("100000"),
                    ts_event=ts,
                    ts_init=ts,
                )
                for ts in (1000, 2000)
            ],
        )
        engine.run()
        account = engine.cache.account_for_venue(Venue("SIM"))

        assert engine.cache.order(ClientOrderId("MARGIN-LIMIT")).status == OrderStatus.ACCEPTED
        assert engine.cache.order(ClientOrderId("MARGIN-MARKET")).status == OrderStatus.FILLED
        assert account.initial_margins() == {instrument.id: Money.from_str("123.45 USD")}
        assert account.maintenance_margins() == {instrument.id: Money.from_str("67.89 USD")}
        assert {call[0] for call in model.calls} == {"initial", "maintenance"}
    finally:
        engine.dispose()


@pytest.fixture
def ordered_models() -> tuple:
    """
    Supply distinct model protocols in the public parameter order.
    """
    latency = type(
        "OrderingLatency",
        (LatencyModel,),
        {
            "get_insert_latency": lambda *_: 101,
            "get_update_latency": lambda *_: 103,
            "get_delete_latency": lambda *_: 107,
        },
    )()
    return FillModel(), FeeModel(), CustomMargin(), latency


@pytest.mark.parametrize(
    "target",
    [BacktestEngine.add_venue, BacktestVenueConfig, SandboxExecutionClientConfig],
)
def test_model_parameter_order(target: Callable) -> None:
    """
    Expose the same model parameter order on each simulation entry point.
    """
    parameters = list(inspect.signature(target).parameters)
    start = parameters.index("fill_model")
    assert parameters[start : start + 4] == [
        "fill_model",
        "fee_model",
        "margin_model",
        "latency_model",
    ]

    if "modules" in parameters:
        assert parameters[start + 4] == "modules"


@pytest.mark.parametrize("venue_type", ["backtest", "sandbox"])
def test_config_positional_model_arguments(venue_type: str, ordered_models: tuple) -> None:
    """
    Bind positional model arguments to their matching configuration properties.
    """
    if venue_type == "backtest":
        prefix = ["SIM", "NETTING", "MARGIN", ["100000 USD"], *([None] * 20)]
        config = BacktestVenueConfig(*prefix, *ordered_models)
    else:
        prefix = [Venue("SIM"), [Money.from_str("100000 USD")], *([None] * 6)]
        config = SandboxExecutionClientConfig(*prefix, *ordered_models)

    fill, fee, margin, latency = ordered_models
    assert config.fill_model is fill
    assert config.fee_model is fee
    assert config.margin_model is margin
    assert config.latency_model is latency


def test_add_venue_positional_model_arguments(ordered_models: tuple) -> None:
    """
    Carry the positional margin model into the runtime account.
    """
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    instrument = TestInstrumentProvider.audusd_sim()
    try:
        engine.add_venue(
            Venue("SIM"),
            OmsType.NETTING,
            AccountType.MARGIN,
            [Money.from_str("100000 USD")],
            None,
            None,
            None,
            *ordered_models,
        )
        engine.add_instrument(instrument)
        engine.add_data(
            [
                QuoteTick(
                    instrument_id=instrument.id,
                    bid_price=Price.from_str("0.80000"),
                    ask_price=Price.from_str("0.80010"),
                    bid_size=Quantity.from_str("100000"),
                    ask_size=Quantity.from_str("100000"),
                    ts_event=1000,
                    ts_init=1000,
                ),
            ],
        )
        engine.run()
        account = engine.cache.account_for_venue(Venue("SIM"))
        result = account.calculate_initial_margin(
            instrument,
            Quantity.from_str("12345"),
            Price.from_str("0.81234"),
        )
        assert result == Money.from_str("123.45 USD")
    finally:
        engine.dispose()
