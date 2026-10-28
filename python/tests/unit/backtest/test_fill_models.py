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
Test custom fill models through configuration and backtest execution.
"""

from collections.abc import Callable
from decimal import Decimal
from pathlib import Path

import pytest

from nautilus_trader.adapters.sandbox import SandboxExecutionClientConfig
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.execution import FillModel
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import CurrencyPair
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import Money
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import Strategy
from tests.providers import TestInstrumentProvider


class DuckFill:
    """
    Retain callback state without inheriting the fill base class.
    """

    def __init__(self) -> None:
        """
        Start the slippage callback counter.
        """
        self.slips = 0

    def is_limit_filled(self) -> bool:
        """
        Allow fills at the limit.
        """
        return True

    def is_slipped(self) -> bool:
        """
        Count calls and disable slippage.
        """
        self.slips += 1
        return False


class CustomFill(FillModel):
    """
    Retain liquidity callback inputs on the supplied model.
    """

    def __init__(self, fails: bool = False) -> None:
        """
        Set the callback failure mode.
        """
        self.fails = fails
        self.calls = []

    def get_orderbook_for_fill_simulation(
        self,
        instrument: CurrencyPair,
        order: MarketOrder,
        best_bid: Price | None,
        best_ask: Price | None,
    ) -> None:
        """
        Record liquidity inputs and optionally raise a callback error.
        """
        self.calls.append((instrument.id, order.client_order_id, best_bid, best_ask))
        if self.fails:
            raise ValueError("fill callback failed")


class FillProbe(Strategy):
    """
    Submit a market order after the first quote.
    """

    def __init__(self, instrument_id: InstrumentId) -> None:
        """
        Set the instrument and submission state.
        """
        super().__init__()
        self.instrument_id = instrument_id
        self.sent = False

    def on_start(self) -> None:
        """
        Subscribe to the quote that starts the probe.
        """
        self.subscribe_quotes(self.instrument_id)

    def on_quote(self, _quote: QuoteTick) -> None:
        """
        Submit one order when prices are available.
        """
        if self.sent:
            return
        self.sent = True
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_str("10000"),
            client_order_id=ClientOrderId("PY-FILL-001"),
        )
        self.submit_order(order)


@pytest.fixture(params=["backtest", "sandbox"])
def fill_config(
    request: pytest.FixtureRequest,
) -> Callable[[object], BacktestVenueConfig | SandboxExecutionClientConfig]:
    """
    Construct either configuration with the same supplied model.
    """
    if request.param == "backtest":
        return lambda model: BacktestVenueConfig(
            name="SIM",
            oms_type="NETTING",
            account_type="MARGIN",
            starting_balances=["100000 USD"],
            fill_model=model,
        )
    return lambda model: SandboxExecutionClientConfig(
        venue=Venue("SIM"),
        starting_balances=[Money.from_str("100000 USD")],
        fill_model=model,
    )


@pytest.mark.parametrize("model_type", [FillModel, CustomFill, DuckFill])
def test_fill_config_preserves_python_instance(
    fill_config: Callable[[object], BacktestVenueConfig | SandboxExecutionClientConfig],
    model_type: type[FillModel] | type[DuckFill],
) -> None:
    """
    Accept base defaults, subclasses, and duck types without copying their state.
    """
    model = model_type()
    config = fill_config(model)
    if isinstance(config, BacktestVenueConfig):
        run = BacktestRunConfig(venues=[config], data=[])
        assert run.venues[0].fill_model is model
    assert config.fill_model is model
    assert config.fill_model is config.fill_model


@pytest.mark.parametrize(
    "model",
    [object(), 123, type("MissingSlip", (), {"is_limit_filled": lambda _: True})()],
)
def test_fill_config_rejects_invalid_objects(
    fill_config: Callable[[object], BacktestVenueConfig | SandboxExecutionClientConfig],
    model: object,
) -> None:
    """
    Reject objects which lack the required fill protocol.
    """
    with pytest.raises(TypeError, match=r"Cannot convert .* to FillModel"):
        fill_config(model)


@pytest.mark.parametrize(
    ("model_type", "fails"),
    [(FillModel, False), (DuckFill, False), (CustomFill, False), (CustomFill, True)],
)
def test_node_dispatches_configured_python_fill_model(
    tmp_path: Path,
    model_type: type[FillModel] | type[DuckFill],
    fails: bool,
) -> None:
    """
    Carry configuration clones into matching and retain the original callback state.
    """
    model = CustomFill(fails=fails) if model_type is CustomFill else model_type()
    instrument = TestInstrumentProvider.audusd_sim()
    quote = QuoteTick(
        instrument_id=instrument.id,
        bid_price=Price.from_str("0.70000"),
        ask_price=Price.from_str("0.70010"),
        bid_size=Quantity.from_str("100000"),
        ask_size=Quantity.from_str("100000"),
        ts_event=1000,
        ts_init=1000,
    )
    catalog = ParquetDataCatalog(str(tmp_path))
    catalog.write_instruments([instrument])
    catalog.write_quote_ticks([quote])
    venue = BacktestVenueConfig(
        name="SIM",
        oms_type="NETTING",
        account_type="MARGIN",
        starting_balances=["100000 USD"],
        fill_model=model,
        fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
    )
    run = BacktestRunConfig(
        venues=[venue],
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
    node = BacktestNode([run])
    try:
        node.build()
        node.add_strategy(run.id, FillProbe(instrument.id))
        node.run()
        order = node.get_engine_cache(run.id).order(ClientOrderId("PY-FILL-001"))
        assert order.status == (OrderStatus.SUBMITTED if fails else OrderStatus.FILLED)
        assert order.filled_qty == Quantity.from_str("0" if fails else "10000")
        if not fails:
            assert order.avg_px == Decimal("0.70010")
        if model_type is CustomFill:
            assert model.calls == [
                (
                    instrument.id,
                    ClientOrderId("PY-FILL-001"),
                    quote.bid_price,
                    quote.ask_price,
                ),
            ]
        if model_type is DuckFill:
            assert model.slips == 1
        assert node.configs[0].venues[0].fill_model is model
    finally:
        node.dispose()
