# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.18.1
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# ## imports

# %%
from nautilus_trader.adapters.databento.data_utils import load_catalog
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.option_exercise import OptionExerciseConfig
from nautilus_trader.backtest.option_exercise import OptionExerciseModule
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


# %% [markdown]
# ## parameters

# %%
catalog_folder = "options_exercise"
catalog = load_catalog(catalog_folder)

future_symbols = ["ESH6"]
option_symbols = ["EW2F6 C7000"]
future_id = InstrumentId.from_str(f"{future_symbols[0]}.XCME")
option_id = InstrumentId.from_str(f"{option_symbols[0]}.XCME")

backtest_start_time = "2024-05-09T10:00"
end_time = "2026-01-09T21:05"

# %% [markdown]
# ## strategy


# %%
class OptionConfig(StrategyConfig, frozen=True):
    future_id: InstrumentId
    option_id: InstrumentId


class OptionStrategy(Strategy):
    """
    A simplified strategy to test option exercise.
    """

    def __init__(self, config: OptionConfig):
        super().__init__(config=config)
        self.order_submitted = False

    def on_start(self):
        self.request_instrument(self.config.option_id)
        self.request_instrument(self.config.future_id)
        self.subscribe_quote_ticks(self.config.option_id)

        self.bar_type = BarType.from_str(f"{self.config.future_id}-1-MINUTE-LAST-EXTERNAL")
        self.subscribe_bars(self.bar_type)

    def on_bar(self, bar):
        self.log.warning(
            f"Bar: {bar}, ts={unix_nanos_to_iso8601(bar.ts_init)}",
            color=LogColor.RED,
        )

    def on_quote_tick(self, tick: QuoteTick):
        if tick.instrument_id == self.config.option_id and not self.order_submitted:
            self.log.warning(f"Quote received, submitting market order for {self.config.option_id}")
            order = self.order_factory.market(
                instrument_id=self.config.option_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),
            )
            self.submit_order(order)
            self.order_submitted = True

    def on_order_filled(self, event):
        self.log.warning(f"Order filled: {event}")

    def on_position_opened(self, event):
        self.log.warning(f"Position opened: {event}")

    def on_position_closed(self, event):
        self.log.warning(f"Position closed: {event}")

    def on_position_changed(self, event):
        self.log.warning(f"Position changed: {event}")

    def on_stop(self):
        self.unsubscribe_quote_ticks(self.config.option_id)


# %% [markdown]
# ## backtest node

# %%
strategies = [
    ImportableStrategyConfig(
        strategy_path=OptionStrategy.fully_qualified_name(),
        config_path=OptionConfig.fully_qualified_name(),
        config={
            "future_id": future_id,
            "option_id": option_id,
        },
    ),
]

logging = LoggingConfig(
    log_level="WARNING",
    log_level_file="WARNING",
    log_directory=".",
    log_file_name="databento_option_exercise",
    clear_log_file=True,
)

engine_config = BacktestEngineConfig(
    logging=logging,
    strategies=strategies,
)

# BacktestRunConfig
data = [
    BacktestDataConfig(
        data_cls=QuoteTick,
        catalog_path=catalog.path,
        instrument_ids=[option_id, future_id],
    ),
    BacktestDataConfig(
        data_cls=Bar,
        catalog_path=catalog.path,
        instrument_ids=[future_id],
    ),
]

modules = [
    ImportableActorConfig(
        actor_path=OptionExerciseModule.fully_qualified_name(),
        config_path=OptionExerciseConfig.fully_qualified_name(),
        config={
            "auto_exercise_enabled": True,
        },
    ),
]

venues = [
    BacktestVenueConfig(
        name="XCME",
        oms_type="NETTING",
        account_type="MARGIN",
        base_currency="USD",
        starting_balances=["1_000_000 USD"],
        modules=modules,
    ),
]

configs = [
    BacktestRunConfig(
        engine=engine_config,
        data=data,
        venues=venues,
        start=backtest_start_time,
        end=end_time,
    ),
]

node = BacktestNode(configs=configs)

# %%
results = node.run()

# %% [markdown]
# ## backtest results

# %%
engine = node.get_engine(configs[0].id)
engine.trader.generate_order_fills_report()

# %%
engine.trader.generate_positions_report()

# %%
engine.trader.generate_account_report(Venue("XCME"))

# %%
node.dispose()
